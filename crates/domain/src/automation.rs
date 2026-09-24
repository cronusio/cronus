//! Automation pipeline engine — the runtime behind implicit (`@ON:`) and explicit
//! (canvas) automation, one engine for both.
//!
//! Foundation: the node taxonomy, the deduplication window, payload
//! isolation, scoped state over a volatile/durable backend registry,
//! the control plane separate from the data plane, and
//! in-graph lifecycle observers with scoped-precedes-catch-all routing.
//! Full topological execution + action dispatch to subsystems is the documented
//! seam; `action` nodes delegate to kanban/orchestration/inbox in production.

use std::collections::{HashMap, HashSet};

/// A node in the pipeline DAG.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    Trigger,
    Filter,
    Transform,
    Branch,
    Action,
    Delay,
    Aggregate,
    Loop,
    Subpipeline,
    Observer,
}

impl NodeType {
    /// `transform` is pure and stateless: it declares no scoped state and is
    /// therefore freely retryable.
    pub fn is_pure(self) -> bool {
        matches!(self, NodeType::Transform)
    }
}

/// The deduplication window: within the window, an event activates at most
/// one run per trigger definition. Distinct triggers fire independently.
#[derive(Debug, Default)]
pub struct DedupWindow {
    window_ms: u64,
    /// (trigger_id, event_key) -> last-fired timestamp.
    seen: HashMap<(String, String), u64>,
}

impl DedupWindow {
    pub fn new(window_ms: u64) -> Self {
        DedupWindow {
            window_ms,
            seen: HashMap::new(),
        }
    }

    /// Admit an event for a trigger. Returns `true` if it should fire, `false` if
    /// suppressed as a duplicate within the window.
    pub fn admit(&mut self, trigger_id: &str, event_key: &str, now: u64) -> bool {
        let key = (trigger_id.to_string(), event_key.to_string());
        if let Some(&last) = self.seen.get(&key)
            && now.saturating_sub(last) < self.window_ms
        {
            return false;
        }
        self.seen.insert(key, now);
        true
    }
}

/// Content classes forbidden in an event payload. Descriptors are permitted;
/// verbatim content is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExcludedContent {
    RawUserText,
    SessionContext,
    Credential,
    MemoryStoreContents,
}

/// Validate an event payload's fields against the exclusion set. A field whose
/// value carries an excluded content class rejects the payload before propagation.
pub fn validate_payload(fields: &[(&str, ContentClass)]) -> Result<(), ExcludedContent> {
    for (_name, class) in fields {
        if let ContentClass::Excluded(kind) = class {
            return Err(*kind);
        }
    }
    Ok(())
}

/// The classification of a payload field's content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentClass {
    /// A descriptor — field name, type, count. Permitted.
    Descriptor,
    /// Verbatim content of an excluded class. Forbidden.
    Excluded(ExcludedContent),
}

/// The persistence backend for a scoped store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// In-process; lost on restart — caches, within-run scratch.
    Volatile,
    /// Survives restart — baselines, dedup horizons, digests.
    Durable,
}

/// The scope of automation state: node-private is the default;
/// pipeline-shared is visible to every node of one pipeline definition.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Scope {
    NodePrivate(String),
    PipelineShared(String),
}

/// A scoped key/value store over a named backend. Office-scoped, schema-bounded,
/// individually resettable. Not the office memory store.
#[derive(Debug)]
pub struct ScopeStore {
    default_backend: Backend,
    backends: HashMap<Scope, Backend>,
    values: HashMap<(Scope, String), String>,
}

impl ScopeStore {
    pub fn new(default_backend: Backend) -> Self {
        ScopeStore {
            default_backend,
            backends: HashMap::new(),
            values: HashMap::new(),
        }
    }

    /// Override the backend for a specific scope (per-scope override over default).
    pub fn set_backend(&mut self, scope: Scope, backend: Backend) {
        self.backends.insert(scope, backend);
    }

    pub fn backend_of(&self, scope: &Scope) -> Backend {
        self.backends
            .get(scope)
            .copied()
            .unwrap_or(self.default_backend)
    }

    pub fn set(&mut self, scope: Scope, key: &str, value: &str) {
        self.values
            .insert((scope, key.to_string()), value.to_string());
    }

    pub fn get(&self, scope: &Scope, key: &str) -> Option<&str> {
        self.values
            .get(&(scope.clone(), key.to_string()))
            .map(String::as_str)
    }

    /// Reset one scope's state (individually resettable).
    pub fn reset(&mut self, scope: &Scope) {
        self.values.retain(|(s, _), _| s != scope);
    }

    /// Simulate a restart: volatile-backed scopes are lost; durable survive.
    pub fn restart(&mut self) {
        let volatile: HashSet<Scope> = self
            .values
            .keys()
            .map(|(s, _)| s.clone())
            .filter(|s| self.backend_of(s) == Backend::Volatile)
            .collect();
        self.values.retain(|(s, _), _| !volatile.contains(s));
    }
}

/// A control-plane verb. Control edges carry these; data edges never do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlVerb {
    Enable,
    Disable,
    Trigger,
}

/// The control graph — pipeline enable/disable/trigger state, separate from the
/// data plane. A control edge never carries an event payload.
#[derive(Debug, Default)]
pub struct ControlGraph {
    disabled: HashSet<String>,
    manual_fires: Vec<String>,
}

impl ControlGraph {
    pub fn new() -> Self {
        ControlGraph::default()
    }

    pub fn apply(&mut self, target: &str, verb: ControlVerb) {
        match verb {
            ControlVerb::Enable => {
                self.disabled.remove(target);
            }
            ControlVerb::Disable => {
                self.disabled.insert(target.to_string());
            }
            ControlVerb::Trigger => self.manual_fires.push(target.to_string()),
        }
    }

    pub fn is_enabled(&self, target: &str) -> bool {
        !self.disabled.contains(target)
    }

    pub fn take_manual_fires(&mut self) -> Vec<String> {
        std::mem::take(&mut self.manual_fires)
    }
}

/// A lifecycle-observer kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObserverKind {
    Error,
    Status,
    Completion,
}

/// An observer subscription: a kind + a scope of covered nodes, or the
/// catch-all *unhandled* set.
#[derive(Debug, Clone)]
pub struct Observer {
    pub id: String,
    pub kind: ObserverKind,
    /// Explicit node set; empty = catch-all (*unhandled*).
    pub scope: HashSet<String>,
}

impl Observer {
    pub fn is_catch_all(&self) -> bool {
        self.scope.is_empty()
    }
}

/// Route a node's lifecycle event to an observer: a scoped observer covering
/// the node takes precedence over a catch-all one. Returns the chosen observer id,
/// or `None` if no observer handles it (stop-on-failure then stands).
pub fn route_observer<'a>(
    observers: &'a [Observer],
    node: &str,
    kind: ObserverKind,
) -> Option<&'a str> {
    let of_kind = || observers.iter().filter(move |o| o.kind == kind);
    // 1. a scoped observer covering the node wins
    if let Some(o) = of_kind().find(|o| o.scope.contains(node)) {
        return Some(&o.id);
    }
    // 2. else a catch-all observer of the same kind
    if let Some(o) = of_kind().find(|o| o.is_catch_all()) {
        return Some(&o.id);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_is_pure_others_may_be_stateful() {
        // Transform declares no state; a stateful node may.
        assert!(NodeType::Transform.is_pure());
        assert!(!NodeType::Aggregate.is_pure());
        assert!(!NodeType::Action.is_pure());
    }

    #[test]
    fn dedup_suppresses_duplicate_but_allows_distinct_triggers() {
        // Same (trigger, event) within window suppressed; distinct fire.
        let mut w = DedupWindow::new(1000);
        assert!(w.admit("t1", "evt-a", 0));
        assert!(!w.admit("t1", "evt-a", 500)); // duplicate within window
        assert!(w.admit("t1", "evt-a", 1200)); // window elapsed -> fires
        assert!(w.admit("t2", "evt-a", 500)); // distinct trigger fires independently
    }

    #[test]
    fn payload_rejects_excluded_content() {
        // Descriptors pass; verbatim excluded content is rejected.
        let ok = validate_payload(&[
            ("event_type", ContentClass::Descriptor),
            ("count", ContentClass::Descriptor),
        ]);
        assert!(ok.is_ok());

        let bad =
            validate_payload(&[("body", ContentClass::Excluded(ExcludedContent::RawUserText))]);
        assert_eq!(bad, Err(ExcludedContent::RawUserText));
    }

    #[test]
    fn scoped_state_durable_survives_restart_volatile_does_not() {
        // Volatile lost on restart; durable survives; per-scope override.
        let mut store = ScopeStore::new(Backend::Volatile);
        let cache = Scope::NodePrivate("dedup-cache".into());
        let baseline = Scope::PipelineShared("baseline".into());
        store.set_backend(baseline.clone(), Backend::Durable);

        store.set(cache.clone(), "k", "v");
        store.set(baseline.clone(), "seen", "42");
        store.restart();

        assert_eq!(store.get(&cache, "k"), None); // volatile lost
        assert_eq!(store.get(&baseline, "seen"), Some("42")); // durable survives
    }

    #[test]
    fn scope_reset_is_individual() {
        let mut store = ScopeStore::new(Backend::Durable);
        let a = Scope::NodePrivate("a".into());
        let b = Scope::NodePrivate("b".into());
        store.set(a.clone(), "k", "1");
        store.set(b.clone(), "k", "2");
        store.reset(&a);
        assert_eq!(store.get(&a, "k"), None);
        assert_eq!(store.get(&b, "k"), Some("2"));
    }

    #[test]
    fn control_plane_governs_enabled_state_separately() {
        // Control verbs change enabled state / fire; distinct from data flow.
        let mut cg = ControlGraph::new();
        assert!(cg.is_enabled("p1"));
        cg.apply("p1", ControlVerb::Disable);
        assert!(!cg.is_enabled("p1"));
        cg.apply("p1", ControlVerb::Enable);
        assert!(cg.is_enabled("p1"));
        cg.apply("p2", ControlVerb::Trigger);
        assert_eq!(cg.take_manual_fires(), vec!["p2".to_string()]);
    }

    #[test]
    fn observer_scoped_precedes_catch_all_else_unhandled() {
        // Scoped covering observer wins; else catch-all; else None.
        let scoped = Observer {
            id: "scoped-err".into(),
            kind: ObserverKind::Error,
            scope: HashSet::from(["node-a".to_string()]),
        };
        let catch_all = Observer {
            id: "catch-all-err".into(),
            kind: ObserverKind::Error,
            scope: HashSet::new(),
        };
        let observers = vec![scoped, catch_all];

        assert_eq!(
            route_observer(&observers, "node-a", ObserverKind::Error),
            Some("scoped-err")
        );
        assert_eq!(
            route_observer(&observers, "node-b", ObserverKind::Error),
            Some("catch-all-err")
        );
        // No completion observer exists -> unhandled (stop-on-failure stands).
        assert_eq!(
            route_observer(&observers, "node-a", ObserverKind::Completion),
            None
        );
    }
}
