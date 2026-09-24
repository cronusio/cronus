use std::sync::Arc;

use cronus_contract::{Binder, BinderKind, Invocable, Locus, Outcome, OutcomeValue, Stability};
use cronus_domain::invocable::{Dispatcher, InvocableRegistry, Registrant};

use super::{core_id, text_arg};
use crate::memory::{MemoryEntry, MemoryKind, MemorySource, MemoryStore};

pub(super) fn register(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    register_store(registry, dispatcher);
    register_search(registry, dispatcher);
    register_forget(registry, dispatcher);
}

/// On-disk location of the memory database. A memory entry is notes about a
/// specific project — resolves against the current workspace, the
/// same resolution `board.rs` uses for the kanban store.
fn memory_db_path() -> std::path::PathBuf {
    cronus_domain::paths::resolve_workspace_root()
        .join("memory")
        .join("memory.db")
}

/// Open the **persistent** memory store. Every `memory` verb used to open a
/// fresh `open_in_memory()` database, so a `store` wrote into a database that
/// was dropped when the handler returned and the next `search`/`forget` saw an
/// empty one — the entry, and the id `store` handed back, were unreachable.
/// One file, opened by every verb, is what makes the round-trip
/// work.
fn open_memory_store() -> Result<MemoryStore, String> {
    let path = memory_db_path();
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return Err(format!("cannot create memory directory: {e}"));
    }
    MemoryStore::open(&path).map_err(|e| e.to_string())
}

fn register_store(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("memory.store");
    let invocable = Invocable {
        id: id.clone(),
        name: "Store",
        // "key-value" promised single-value-per-key uniqueness this
        // verb never delivered — MemoryEntry has no key field to be unique
        // on (only `title`/`body`), and every call adds a genuinely new,
        // separately timestamped entry, never replacing a same-titled one.
        // That is a deliberate property of the richer domain model (trust
        // scoring, decay, `superseded_at`-based history) this thin CLI
        // binding does not attempt to collapse into overwrite-in-place —
        // a real title-keyed supersession policy belongs to that model's
        // own consolidation pass (store-local's `consolidate` module),
        // not to a silent side effect of this handler. Named honestly
        // instead: this adds an entry, it does not set a value.
        summary: "Add a titled memory entry (does not replace a same-titled one — see \
                   `memory search`).",
        group: "memory",
        locus: Locus::Semantic,
        binders: vec![
            Binder {
                name: "key",
                kind: BinderKind::Text,
                optional: false,
            },
            Binder {
                name: "value",
                kind: BinderKind::Text,
                optional: false,
            },
        ],
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry
        .register(&Registrant::core(), invocable)
        .expect("core:memory.store registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(|args| {
            let key = text_arg(args, "key");
            let value = text_arg(args, "value");
            let store = match open_memory_store() {
                Ok(store) => store,
                Err(reason) => return Outcome::Unavailable { reason },
            };
            let entry = MemoryEntry::new(
                MemoryKind::ProjectContext,
                MemorySource::System,
                key.to_string(),
                value.to_string(),
            );
            match store.add(entry) {
                Ok(entry_id) => Outcome::Value(OutcomeValue::Record(vec![(
                    "id".to_string(),
                    OutcomeValue::Text(entry_id.to_string()),
                )])),
                Err(e) => Outcome::Unavailable {
                    reason: e.to_string(),
                },
            }
        }),
    );
}

fn register_search(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("memory.search");
    let invocable = Invocable {
        id: id.clone(),
        name: "Search",
        summary: "Search memory entries by keyword.",
        group: "memory",
        locus: Locus::Semantic,
        binders: vec![Binder {
            name: "query",
            kind: BinderKind::Text,
            optional: false,
        }],
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry
        .register(&Registrant::core(), invocable)
        .expect("core:memory.search registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(|args| {
            let query = text_arg(args, "query");
            let store = match open_memory_store() {
                Ok(store) => store,
                Err(reason) => return Outcome::Unavailable { reason },
            };
            // A zero-item list and a genuinely empty result are different
            // facts (§4.5's zero-count-list fixture guards exactly this):
            // a completed search that matched nothing is still a List, not
            // Empty — Empty would claim the search itself produced nothing
            // to report, which is not what a clean zero-match search means.
            match store.search_fts(query, 10) {
                Ok(entries) => Outcome::Value(OutcomeValue::List(
                    entries
                        .into_iter()
                        .map(|entry| {
                            OutcomeValue::Record(vec![
                                ("id".to_string(), OutcomeValue::Text(entry.id.to_string())),
                                ("title".to_string(), OutcomeValue::Text(entry.title)),
                            ])
                        })
                        .collect(),
                )),
                Err(e) => Outcome::Unavailable {
                    reason: e.to_string(),
                },
            }
        }),
    );
}

fn register_forget(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("memory.forget");
    let invocable = Invocable {
        id: id.clone(),
        name: "Forget",
        summary: "Delete a memory entry by id.",
        group: "memory",
        locus: Locus::Semantic,
        binders: vec![Binder {
            name: "id",
            kind: BinderKind::Text,
            optional: false,
        }],
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry
        .register(&Registrant::core(), invocable)
        .expect("core:memory.forget registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(|args| {
            let entry_id = text_arg(args, "id");
            let store = match open_memory_store() {
                Ok(store) => store,
                Err(reason) => return Outcome::Unavailable { reason },
            };
            match store.delete(entry_id) {
                Ok(true) => Outcome::Value(OutcomeValue::Record(vec![(
                    "id".to_string(),
                    OutcomeValue::Text(entry_id.to_string()),
                )])),
                // "Not found" has no dedicated Outcome variant today — the
                // closest honest fit is Unavailable (the resource this
                // operation depends on, a matching entry, could not be
                // reached), recorded here rather than silently assumed
                // correct forever; a future spec amendment may want a
                // dedicated "not found" shape.
                Ok(false) => Outcome::Unavailable {
                    reason: format!("entry '{entry_id}' not found"),
                },
                Err(e) => Outcome::Unavailable {
                    reason: e.to_string(),
                },
            }
        }),
    );
}
