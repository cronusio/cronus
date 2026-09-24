//! Model router — selects the best provider for a request.
//!
//! Routing applies a 9-factor score, circuit breakers, LKGP fallback,
//! and a deterministic 5% bandit exploration policy.

pub mod cache;
pub mod circuit;
pub mod hardware;
pub mod provider;
pub mod recovery;
pub mod scoring;

use std::sync::{Mutex, MutexGuard, PoisonError};

use cache::SemanticCache;
use circuit::{CircuitBreaker, CircuitState};
use provider::{ModelProvider, ProviderHealth, RouteDecision, RoutingRequest};
use scoring::{ModePack, score};

pub use hardware::FitLevel;
pub use provider::{ProviderError, ProviderTier, TaskType};
pub use scoring::ModePack as RouterMode;

// ── Error ─────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum RouterError {
    NoProvidersAvailable,
    AllCircuitsBroken { lkgp_id: Option<String> },
}

impl std::fmt::Display for RouterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RouterError::NoProvidersAvailable => write!(f, "no model providers registered"),
            RouterError::AllCircuitsBroken { lkgp_id } => {
                if let Some(id) = lkgp_id {
                    write!(f, "all provider circuits open; LKGP fallback: {id}")
                } else {
                    write!(f, "all provider circuits open; no LKGP available")
                }
            }
        }
    }
}

impl std::error::Error for RouterError {}

// ── RouterPool ────────────────────────────────────────────────────────────────

/// One in every `BANDIT_INTERVAL` route calls uses bandit exploration.
const BANDIT_INTERVAL: u64 = 20;

struct ProviderEntry {
    provider: Box<dyn ModelProvider>,
    circuit: CircuitBreaker,
}

struct PoolInner {
    entries: Vec<ProviderEntry>,
    mode: ModePack,
    lkgp: Option<String>,
    max_cost: f64,
    max_latency_ms: u64,
    /// Per-pool exploration counter (every `BANDIT_INTERVAL`-th call explores).
    /// Instance-scoped, not a process-global `static`: a global counter coupled
    /// unrelated pools (and unrelated tests) so exploration cadence depended on
    /// how many calls every *other* pool in the process had made. It is stepped
    /// under the same lock as the rest of the routing decision.
    bandit_counter: u64,
}

pub struct RouterPool {
    inner: Mutex<PoolInner>,
    cache: SemanticCache,
}

impl RouterPool {
    pub fn new(mode: ModePack) -> Self {
        RouterPool {
            inner: Mutex::new(PoolInner {
                entries: Vec::new(),
                mode,
                lkgp: None,
                max_cost: 1.0,
                max_latency_ms: 5000,
                bandit_counter: 0,
            }),
            cache: SemanticCache::new(),
        }
    }

    /// Lock the pool, recovering from a poisoned lock.
    ///
    /// Poisoning only records that another holder panicked — most plausibly a
    /// provider's own `health()` or `score` input, which run under this lock.
    /// Every field of the pool is valid on its own (a provider list, a mode, a
    /// last-known-good id, two counters), and a circuit breaker that missed one
    /// update merely re-learns it, so recovering keeps routing available
    /// rather than failing every later call after a single provider panic.
    fn lock(&self) -> MutexGuard<'_, PoolInner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Register a provider. Uses default circuit breaker (threshold=5, cooldown=60s).
    pub fn register(&self, provider: Box<dyn ModelProvider>) {
        let mut inner = self.lock();
        inner.entries.push(ProviderEntry {
            provider,
            circuit: CircuitBreaker::default(),
        });
    }

    /// Register a provider with a custom circuit breaker.
    pub fn register_with_circuit(&self, provider: Box<dyn ModelProvider>, circuit: CircuitBreaker) {
        let mut inner = self.lock();
        inner.entries.push(ProviderEntry { provider, circuit });
    }

    pub fn set_mode(&self, mode: ModePack) {
        self.lock().mode = mode;
    }

    /// Record a successful call to a provider — resets its circuit.
    pub fn record_success(&self, provider_id: &str) {
        let mut inner = self.lock();
        for entry in &mut inner.entries {
            if entry.provider.id() == provider_id {
                entry.circuit.record_success();
                inner.lkgp = Some(provider_id.to_owned());
                return;
            }
        }
    }

    /// Record a failed call to a provider — may open its circuit.
    pub fn record_failure(&self, provider_id: &str) {
        let mut inner = self.lock();
        for entry in &mut inner.entries {
            if entry.provider.id() == provider_id {
                entry.circuit.record_failure();
                return;
            }
        }
    }

    /// Return the current LKGP (Last Known Good Provider) ID, if any.
    pub fn lkgp(&self) -> Option<String> {
        self.lock().lkgp.clone()
    }

    /// Return the current circuit state for a registered provider.
    pub fn circuit_state(&self, provider_id: &str) -> Option<circuit::CircuitState> {
        let inner = self.lock();
        inner
            .entries
            .iter()
            .find(|e| e.provider.id() == provider_id)
            .map(|e| e.circuit.state())
    }

    /// Select the best provider for a request.
    ///
    /// Returns an error if all circuits are open (includes LKGP ID if known).
    pub fn route(&self, req: &RoutingRequest) -> Result<RouteDecision, RouterError> {
        // Semantic cache lookup (always misses — stub)
        if let Some(cached) = self.cache.lookup(req.prompt_hash) {
            return Ok(cached);
        }

        let mut inner = self.lock();

        if inner.entries.is_empty() {
            return Err(RouterError::NoProvidersAvailable);
        }

        // 5% bandit exploration — explore a healthy provider that still fits
        // the request. The fit gate matters: without it, exploration could
        // pick a provider whose context window is too small for the request
        // (the scored path already excludes those via `FitLevel::TooTight`),
        // so bandit selection would violate the same feasibility invariant the
        // scored path upholds. Use `state()` (&self) rather than `is_open()`
        // (&mut self) because the `find` predicate only gets a shared reference.
        // Increment first, then test: the 20th/40th/… call of *this* pool
        // explores, never the very first. (A count-then-increment scheme would
        // fire on call 0, so a single-call routing test would always land on
        // the bandit path — the accidental coupling the old global counter hid
        // only because unrelated calls kept it away from a multiple of 20.)
        inner.bandit_counter = inner.bandit_counter.wrapping_add(1);
        let use_bandit = inner.bandit_counter.is_multiple_of(BANDIT_INTERVAL);
        if use_bandit
            && let Some(entry) = inner.entries.iter_mut().find(|e| {
                e.circuit.state() != CircuitState::Open
                    && e.provider.health() != ProviderHealth::Unavailable
                    && FitLevel::evaluate(req.required_context, e.provider.context_window())
                        != FitLevel::TooTight
            })
        {
            return Ok(RouteDecision {
                provider_id: entry.provider.id().to_owned(),
                score: 0.0,
                via_lkgp: false,
                via_bandit: true,
            });
        }

        // Score all healthy providers
        let mode = inner.mode;
        let max_cost = inner.max_cost;
        let max_latency = inner.max_latency_ms;

        let mut best_id: Option<String> = None;
        let mut best_score = f64::NEG_INFINITY;

        for entry in inner.entries.iter_mut() {
            if entry.circuit.is_open() {
                continue;
            }
            let s = score(entry.provider.as_ref(), req, mode, max_cost, max_latency);
            if s > best_score {
                best_score = s;
                best_id = Some(entry.provider.id().to_owned());
            }
        }

        if let Some(id) = best_id {
            return Ok(RouteDecision {
                provider_id: id,
                score: best_score,
                via_lkgp: false,
                via_bandit: false,
            });
        }

        // All circuits open — LKGP fallback
        let lkgp = inner.lkgp.clone();
        Err(RouterError::AllCircuitsBroken { lkgp_id: lkgp })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use provider::{ProviderHealth, ProviderTier, TaskType};

    struct TestProvider {
        id: &'static str,
        health: ProviderHealth,
    }

    impl ModelProvider for TestProvider {
        fn id(&self) -> &str {
            self.id
        }
        fn health(&self) -> ProviderHealth {
            self.health
        }
        fn context_window(&self) -> u32 {
            128_000
        }
        fn cost_per_1k_tokens(&self) -> f64 {
            0.01
        }
        fn latency_p50_ms(&self) -> u64 {
            300
        }
        fn tier(&self) -> ProviderTier {
            ProviderTier::Standard
        }
        fn task_fit(&self, _: TaskType) -> f64 {
            0.8
        }
    }

    fn req() -> RoutingRequest {
        RoutingRequest {
            prompt_hash: 42,
            required_context: 1000,
            task_type: TaskType::Chat,
        }
    }

    #[test]
    fn routes_to_healthy_provider() {
        let pool = RouterPool::new(ModePack::Quality);
        pool.register(Box::new(TestProvider {
            id: "p1",
            health: ProviderHealth::Healthy,
        }));

        let decision = pool.route(&req()).expect("must route");
        assert_eq!(decision.provider_id, "p1");
    }

    #[test]
    fn no_providers_returns_error() {
        let pool = RouterPool::new(ModePack::Quality);
        assert!(matches!(
            pool.route(&req()),
            Err(RouterError::NoProvidersAvailable)
        ));
    }

    #[test]
    fn records_success_updates_lkgp() {
        let pool = RouterPool::new(ModePack::Quality);
        pool.register(Box::new(TestProvider {
            id: "p1",
            health: ProviderHealth::Healthy,
        }));
        pool.route(&req()).unwrap();
        pool.record_success("p1");
        let inner = pool.inner.lock().unwrap();
        assert_eq!(inner.lkgp.as_deref(), Some("p1"));
    }

    #[test]
    fn open_circuit_skips_provider() {
        let pool = RouterPool::new(ModePack::Quality);
        let cb = CircuitBreaker::new(1, 3600); // 1-hour cooldown — stays Open
        let mut cb_open = cb;
        cb_open.record_failure(); // opens it
        pool.register_with_circuit(
            Box::new(TestProvider {
                id: "broken",
                health: ProviderHealth::Healthy,
            }),
            cb_open,
        );
        pool.register(Box::new(TestProvider {
            id: "ok",
            health: ProviderHealth::Healthy,
        }));

        let decision = pool.route(&req()).expect("must route via fallback");
        assert_eq!(decision.provider_id, "ok");
    }

    /// A provider that panics inside `health()` does so while the pool lock is
    /// held; that must not make every later call on the pool panic too.
    #[test]
    fn pool_survives_a_provider_panicking_under_the_lock() {
        struct PanickingProvider;
        impl ModelProvider for PanickingProvider {
            fn id(&self) -> &str {
                "boom"
            }
            fn health(&self) -> ProviderHealth {
                panic!("simulated provider panic while the pool lock is held")
            }
            fn context_window(&self) -> u32 {
                128_000
            }
            fn cost_per_1k_tokens(&self) -> f64 {
                0.01
            }
            fn latency_p50_ms(&self) -> u64 {
                300
            }
            fn tier(&self) -> ProviderTier {
                ProviderTier::Standard
            }
            fn task_fit(&self, _: TaskType) -> f64 {
                0.8
            }
        }

        let pool = RouterPool::new(ModePack::Quality);
        pool.register(Box::new(PanickingProvider));
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = pool.route(&req());
        }));
        assert!(panicked.is_err(), "the provider must have panicked");
        assert!(pool.inner.is_poisoned(), "the test must actually poison");

        // Every entry point still works on the recovered pool.
        pool.set_mode(ModePack::Quality);
        assert_eq!(pool.lkgp(), None);
        pool.record_failure("boom");
        assert!(pool.circuit_state("boom").is_some());
        pool.register(Box::new(TestProvider {
            id: "healthy",
            health: ProviderHealth::Healthy,
        }));
        assert_eq!(pool.circuit_state("healthy"), Some(CircuitState::Closed));
    }
}
