//! ModelProvider trait and supporting types.
//!
//! `ModelProvider`, `ProviderHealth`, `ProviderTier`, and `TaskType` moved to
//! `cronus-contract` — the trait signature they
//! form is the seam concrete provider backends implement. `ProviderError`,
//! `RoutingRequest`, and `RouteDecision` stay here: they are router-internal,
//! never part of the `ModelProvider` trait itself.

pub use cronus_contract::{ModelProvider, ProviderHealth, ProviderTier, TaskType};

/// Error returned by a provider when routing fails.
#[derive(Debug)]
pub enum ProviderError {
    Unavailable(String),
    ContextOverflow { required: u32, available: u32 },
    QuotaExhausted,
    RateLimit,
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderError::Unavailable(r) => write!(f, "provider unavailable: {r}"),
            ProviderError::ContextOverflow {
                required,
                available,
            } => {
                write!(f, "context overflow: need {required}, have {available}")
            }
            ProviderError::QuotaExhausted => write!(f, "quota exhausted"),
            ProviderError::RateLimit => write!(f, "rate limited"),
        }
    }
}

impl std::error::Error for ProviderError {}

/// A routing request passed to the pool.
#[derive(Debug, Clone)]
pub struct RoutingRequest {
    /// Hash of the prompt for semantic cache lookup.
    pub prompt_hash: u64,
    /// Number of tokens required in context.
    pub required_context: u32,
    /// Task category for scoring.
    pub task_type: TaskType,
}

/// The selected provider and routing metadata.
#[derive(Debug, Clone)]
pub struct RouteDecision {
    pub provider_id: String,
    pub score: f64,
    pub via_lkgp: bool,
    pub via_bandit: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockProvider {
        pub id: &'static str,
        pub health: ProviderHealth,
        pub ctx: u32,
        pub cost: f64,
        pub latency: u64,
        pub tier: ProviderTier,
    }

    impl ModelProvider for MockProvider {
        fn id(&self) -> &str {
            self.id
        }
        fn health(&self) -> ProviderHealth {
            self.health
        }
        fn context_window(&self) -> u32 {
            self.ctx
        }
        fn cost_per_1k_tokens(&self) -> f64 {
            self.cost
        }
        fn latency_p50_ms(&self) -> u64 {
            self.latency
        }
        fn tier(&self) -> ProviderTier {
            self.tier
        }
        fn task_fit(&self, _task: TaskType) -> f64 {
            0.8
        }
    }

    #[test]
    fn provider_tier_ordering() {
        assert!(ProviderTier::Local < ProviderTier::Economy);
        assert!(ProviderTier::Economy < ProviderTier::Standard);
        assert!(ProviderTier::Standard < ProviderTier::Premium);
    }

    #[test]
    fn mock_provider_contract() {
        let p = MockProvider {
            id: "test",
            health: ProviderHealth::Healthy,
            ctx: 128_000,
            cost: 0.01,
            latency: 300,
            tier: ProviderTier::Standard,
        };
        assert_eq!(p.id(), "test");
        assert_eq!(p.health(), ProviderHealth::Healthy);
        assert_eq!(p.context_window(), 128_000);
    }
}
