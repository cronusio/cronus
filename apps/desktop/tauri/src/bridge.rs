//! Shell ↔ core IPC bridge.
//!
//! Two command families cross this seam. `capability_version`/`capability_status`
//! are the shell's own bespoke, `Locus::Installation`-adjacent bridge methods —
//! `core:version`/`core:status` are not projectable through the shared registry
//! (the same disclosed exception the terminal UI's own composition carries) —
//! and stay masked through `Bridge::mask`'s local redaction call exactly as
//! before. `capability_catalog`/`capability_invoke` are the generic pair: the
//! shell composes the same `InvocableRegistry`/`Dispatcher` the CLI and TUI
//! compose through `cronus_core::invocable_bootstrap::bootstrap` — no private
//! path — and every dispatched `Outcome` is masked by the dispatcher's own
//! boundary (`Dispatcher::set_secrets`), not by a second, bridge-local
//! redaction call. The shell only marshals: no domain logic, no re-implemented
//! dispatch.

use std::collections::HashMap;

use cronus_contract::{
    ArgValue, ArgValues, Dispatched, Invocable, InvocableId, Invocation, Outcome, Surface,
};
use cronus_core::invocable::{Dispatcher, InvocableRegistry};
use cronus_core::{Capabilities, Engine};

use crate::settings::{SettingsStore, ShellSettings, ShellSettingsPatch};

/// Bridge over a core handle, the secret values to mask in any output that
/// crosses the IPC boundary, and the shared invocable registry/dispatcher
/// pair every generic call runs through.
pub struct Bridge<C: Capabilities> {
    core: C,
    secrets: Vec<String>,
    registry: InvocableRegistry,
    dispatcher: Dispatcher,
}

impl<C: Capabilities> Bridge<C> {
    /// Wrap a core handle, the secret values to mask in bridged output, and
    /// an already-composed registry/dispatcher pair. `secrets` is applied to
    /// the dispatcher here too (`Dispatcher::set_secrets`) — one list feeds
    /// both masking mechanisms, rather than a caller having to keep two
    /// copies in step.
    pub fn new(
        core: C,
        secrets: Vec<String>,
        registry: InvocableRegistry,
        mut dispatcher: Dispatcher,
    ) -> Self {
        dispatcher.set_secrets(secrets.clone());
        Self {
            core,
            secrets,
            registry,
            dispatcher,
        }
    }

    fn mask(&self, raw: &str) -> String {
        let secret_refs: Vec<&str> = self.secrets.iter().map(String::as_str).collect();
        cronus_core::redact::redact(raw, &secret_refs)
    }

    /// Core/product version, masked like every bridged value.
    pub fn version(&self) -> String {
        self.mask(self.core.version())
    }

    /// Core status line, masked before it crosses to the WebView.
    pub fn status(&self) -> String {
        self.mask(&self.core.status())
    }

    /// Every descriptor a frontend may project: the same predicate
    /// every surface's own catalog is filtered through, not a second
    /// hand-rolled list local to this shell.
    pub fn catalog(&self) -> Vec<Invocable> {
        self.registry
            .all()
            .filter(|invocable| invocable.is_projected())
            .cloned()
            .collect()
    }

    /// Dispatch one call through the shared boundary. `Ok(None)` is the
    /// registry's real `Unknown` answer — never fabricated as a
    /// failure; the caller's own catalog refresh on that answer is this
    /// task's own sibling concern, not this method's. The caller identity
    /// is asserted here as [`Surface::Desktop`], never accepted from the
    /// wire — the executable face marshals a payload, not an identity claim
    /// (the same reasoning, extended from shape to identity).
    pub fn invoke(
        &self,
        id: String,
        args: HashMap<String, ArgValue>,
    ) -> Result<Option<Outcome>, String> {
        let id = InvocableId::new(id).map_err(|err| err.to_string())?;
        let mut values = ArgValues::new();
        for (name, value) in args {
            values.insert(name, value);
        }
        let invocation = Invocation {
            id,
            args: values,
            caller: Surface::Desktop,
        };
        match self.dispatcher.dispatch(&self.registry, &invocation) {
            Dispatched::Unknown => Ok(None),
            Dispatched::Ran(outcome) => Ok(Some(outcome)),
        }
    }
}

/// The production bridge the shell manages as Tauri state.
pub type CoreBridge = Bridge<Engine>;

/// Construct the production bridge over an embedded engine.
///
/// Two `Engine` instances, not one shared: `Engine` is currently a
/// zero-state placeholder (`no subsystems loaded yet`) that composition
/// consumes by value, so the version/status handle and the registry's own
/// composition each get their own — the same shape the TUI's own `run()`
/// already establishes for the identical reason. The secret list starts
/// empty; population from the core secret store is deferred until the core
/// exposes it (same pending note as every other surface's composition).
pub fn core_bridge() -> CoreBridge {
    let (registry, dispatcher) = cronus_core::invocable_bootstrap::bootstrap(Engine::new());
    Bridge::new(Engine::new(), Vec::new(), registry, dispatcher)
}

/// IPC: `capability_version` — the core version string.
#[tauri::command]
pub fn capability_version(bridge: tauri::State<'_, CoreBridge>) -> String {
    bridge.version()
}

/// IPC: `capability_status` — the core status line.
#[tauri::command]
pub fn capability_status(bridge: tauri::State<'_, CoreBridge>) -> String {
    bridge.status()
}

/// IPC: `capability_catalog` — every descriptor this shell may project,
/// sourced from the shared registry rather than a hand-written list.
#[tauri::command]
pub fn capability_catalog(bridge: tauri::State<'_, CoreBridge>) -> Vec<Invocable> {
    bridge.catalog()
}

/// IPC: `capability_invoke` — dispatch one call through the shared
/// registry/dispatcher boundary. `Ok(None)` means the id named nothing the
/// registry currently knows (the `Unknown` answer) — the caller's job is to
/// refresh its catalog, not render a failure.
#[tauri::command]
pub fn capability_invoke(
    bridge: tauri::State<'_, CoreBridge>,
    id: String,
    args: HashMap<String, ArgValue>,
) -> Result<Option<Outcome>, String> {
    bridge.invoke(id, args)
}

/// IPC: `capability_settings_get` — the shell-facing settings slice (theme,
/// colour scheme, the opaque layout blob, the user keymap layer). Host-owned
/// configuration: marshalling, not logic (admission rule, host-owned facility).
#[tauri::command]
pub fn capability_settings_get(store: tauri::State<'_, SettingsStore>) -> ShellSettings {
    store.shell_settings()
}

/// IPC: `capability_settings_set` — apply a partial update and persist it.
#[tauri::command]
pub fn capability_settings_set(
    store: tauri::State<'_, SettingsStore>,
    patch: ShellSettingsPatch,
) -> Result<(), String> {
    store.update_shell(patch).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use cronus_contract::{Locus, OutcomeValue, Stability};
    use cronus_core::invocable::Registrant;

    use super::*;

    /// Capability stub whose status embeds a known secret value.
    struct StubCore {
        status: String,
    }

    impl Capabilities for StubCore {
        fn version(&self) -> &str {
            "9.9.9-test"
        }

        fn status(&self) -> String {
            self.status.clone()
        }
    }

    fn empty_registry_and_dispatcher() -> (InvocableRegistry, Dispatcher) {
        (InvocableRegistry::new(), Dispatcher::new())
    }

    #[test]
    fn version_passes_through_the_core_value() {
        let (registry, dispatcher) = empty_registry_and_dispatcher();
        let bridge = Bridge::new(
            StubCore {
                status: String::new(),
            },
            Vec::new(),
            registry,
            dispatcher,
        );
        assert_eq!(bridge.version(), "9.9.9-test");
    }

    #[test]
    fn status_masks_known_secrets_via_core_redaction() {
        let (registry, dispatcher) = empty_registry_and_dispatcher();
        let bridge = Bridge::new(
            StubCore {
                status: "token=sk-LIVE-777 ready".into(),
            },
            vec!["sk-LIVE-777".into()],
            registry,
            dispatcher,
        );
        let out = bridge.status();
        assert!(!out.contains("sk-LIVE-777"), "secret must not cross IPC");
        assert!(out.contains(cronus_core::redact::MASK));
        assert!(out.contains("ready"), "non-secret content preserved");
    }

    #[test]
    fn production_bridge_reports_the_embedded_core_status() {
        let bridge = core_bridge();
        assert!(bridge.status().contains(bridge.version().as_str()));
    }

    /// A real, test-registered `core:test.probe` invocable, one required
    /// argument, whose handler echoes a known secret in its result — proves
    /// `invoke()` reaches it through the shared door, and that the returned
    /// `Outcome` is masked by the dispatcher's own boundary (`set_secrets`),
    /// exactly as `status_masks_known_secrets_via_core_redaction` already
    /// proves for `status()` through the bridge's own local `mask`.
    fn registry_with_probe() -> (InvocableRegistry, Dispatcher) {
        let mut registry = InvocableRegistry::new();
        let mut dispatcher = Dispatcher::new();
        let id = InvocableId::new("core:test.probe").expect("well-formed test id");
        let invocable = Invocable {
            id: id.clone(),
            name: "Probe",
            summary: "Test-only fixture invocable",
            group: "test",
            locus: Locus::Semantic,
            binders: Vec::new(),
            stability: Stability::Shipped,
            journal_raw_input: true,
        };
        registry
            .register(&Registrant::core(), invocable)
            .expect("a test fixture registers cleanly");
        dispatcher.attach(
            id,
            Arc::new(|_args| {
                Outcome::Value(OutcomeValue::Text("token=sk-LIVE-777 ready".to_string()))
            }),
        );
        (registry, dispatcher)
    }

    #[test]
    fn invoke_dispatches_a_registered_fixture_and_masks_its_outcome() {
        let (registry, dispatcher) = registry_with_probe();
        let bridge = Bridge::new(
            StubCore {
                status: String::new(),
            },
            vec!["sk-LIVE-777".into()],
            registry,
            dispatcher,
        );
        let outcome = bridge
            .invoke("core:test.probe".to_string(), HashMap::new())
            .expect("well-formed id")
            .expect("a registered invocable must run, not answer Unknown");
        match outcome {
            Outcome::Value(OutcomeValue::Text(text)) => {
                assert!(!text.contains("sk-LIVE-777"), "secret must not cross IPC");
                assert!(text.contains(cronus_core::redact::MASK));
                assert!(text.contains("ready"), "non-secret content preserved");
            }
            other => panic!("expected a masked Text outcome, got {other:?}"),
        }
    }

    #[test]
    fn invoke_answers_none_for_an_id_the_registry_does_not_know() {
        let (registry, dispatcher) = empty_registry_and_dispatcher();
        let bridge = Bridge::new(
            StubCore {
                status: String::new(),
            },
            Vec::new(),
            registry,
            dispatcher,
        );
        let outcome = bridge
            .invoke("core:test.probe".to_string(), HashMap::new())
            .expect("well-formed id");
        assert!(
            outcome.is_none(),
            "an unresolved id must answer the registry's real Unknown, not a fabricated Outcome"
        );
    }

    #[test]
    fn invoke_refuses_a_malformed_id_before_reaching_the_dispatcher() {
        let (registry, dispatcher) = empty_registry_and_dispatcher();
        let bridge = Bridge::new(
            StubCore {
                status: String::new(),
            },
            Vec::new(),
            registry,
            dispatcher,
        );
        assert!(
            bridge
                .invoke("not-a-qualified-id".to_string(), HashMap::new())
                .is_err()
        );
    }

    #[test]
    fn catalog_reports_only_projectable_descriptors() {
        let mut registry = InvocableRegistry::new();
        let mut dispatcher = Dispatcher::new();
        let projectable = InvocableId::new("core:test.probe").expect("well-formed test id");
        let not_projectable =
            InvocableId::new("core:test.installation-only").expect("well-formed test id");
        registry
            .register(
                &Registrant::core(),
                Invocable {
                    id: projectable.clone(),
                    name: "Probe",
                    summary: "Projectable fixture",
                    group: "test",
                    locus: Locus::Semantic,
                    binders: Vec::new(),
                    stability: Stability::Shipped,
                    journal_raw_input: true,
                },
            )
            .expect("registers cleanly");
        registry
            .register(
                &Registrant::core(),
                Invocable {
                    id: not_projectable.clone(),
                    name: "Installation-only fixture",
                    summary: "Never projected to a session-scoped surface",
                    group: "test",
                    locus: Locus::Installation,
                    binders: Vec::new(),
                    stability: Stability::Shipped,
                    journal_raw_input: true,
                },
            )
            .expect("registers cleanly");
        dispatcher.attach(
            projectable.clone(),
            Arc::new(|_args| Outcome::Value(OutcomeValue::Empty)),
        );
        let bridge = Bridge::new(
            StubCore {
                status: String::new(),
            },
            Vec::new(),
            registry,
            dispatcher,
        );
        let ids: Vec<InvocableId> = bridge.catalog().into_iter().map(|inv| inv.id).collect();
        assert!(ids.contains(&projectable));
        assert!(!ids.contains(&not_projectable));
    }

    // Command *registration* is verified at compile time: `run()` passes
    // every command through `tauri::generate_handler!`, so a renamed or
    // mis-typed command fails the build. A mock-runtime IPC round-trip
    // (tauri "test" feature) is not used here: on windows-gnu it makes the
    // test binary fail to load (STATUS_ENTRYPOINT_NOT_FOUND via WebView2Loader
    // imports).
}
