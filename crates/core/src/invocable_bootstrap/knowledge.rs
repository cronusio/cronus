use std::sync::Arc;

use cronus_contract::{Binder, BinderKind, Invocable, Locus, Outcome, OutcomeValue, Stability};
use cronus_domain::invocable::{Dispatcher, InvocableRegistry, Registrant};
use cronus_domain::knowledge_access::KnowledgePrincipal;
use cronus_domain::resource_sharing::GrantStore;

use crate::knowledge_bootstrap::{Collection, Document, KnowledgeService, RetrievalRequest};

use super::{core_id, list_arg, opt_text_arg, text_arg};

/// The single local CLI operator's identity — see `KnowledgeService::query`'s
/// own doc comment for why this facade treats the invoking user as the
/// collection owner rather than wiring a not-yet-existing persisted
/// multi-user grant store.
const LOCAL_USER: &str = "local";

/// A knowledge collection ingests documents for a specific project — resolves
/// against the current workspace, same as every other project-scoped
/// semantic verb.
fn db_path() -> std::path::PathBuf {
    cronus_domain::paths::resolve_workspace_root()
        .join("knowledge")
        .join("knowledge.db")
}

fn open_service() -> Result<KnowledgeService, String> {
    if let Some(parent) = db_path().parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    KnowledgeService::open_default(db_path()).map_err(|e| e.to_string())
}

pub(super) fn register(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    register_collection_create(registry, dispatcher);
    register_add(registry, dispatcher);
    register_add_url(registry, dispatcher);
    register_query(registry, dispatcher);
}

fn register_collection_create(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("knowledge.collection-create");
    let invocable = Invocable {
        id: id.clone(),
        name: "Collection Create",
        summary: "Create a named, access-controlled document collection.",
        group: "knowledge",
        locus: Locus::Semantic,
        binders: vec![
            Binder {
                name: "id",
                kind: BinderKind::Text,
                optional: false,
            },
            Binder {
                name: "name",
                kind: BinderKind::Text,
                optional: false,
            },
        ],
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry.register(&Registrant::core(), invocable).expect(
        "core:knowledge.collection-create registers cleanly at bootstrap — a duplicate id here is a bug",
    );
    dispatcher.attach(
        id,
        Arc::new(|args| {
            let coll_id = text_arg(args, "id");
            let name = text_arg(args, "name");
            let svc = match open_service() {
                Ok(s) => s,
                Err(e) => return Outcome::Unavailable { reason: e },
            };
            let collection = Collection::new(coll_id, LOCAL_USER, name);
            match svc.create_collection(&collection) {
                Ok(()) => Outcome::Value(OutcomeValue::Record(vec![(
                    "id".to_string(),
                    OutcomeValue::Text(coll_id.to_string()),
                )])),
                Err(e) => Outcome::Unavailable {
                    reason: e.to_string(),
                },
            }
        }),
    );
}

fn register_add(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("knowledge.add");
    let invocable = Invocable {
        id: id.clone(),
        name: "Add",
        summary: "Ingest a plain-text/JSON record into a collection.",
        group: "knowledge",
        locus: Locus::Semantic,
        binders: vec![
            Binder {
                name: "collection",
                kind: BinderKind::NamedText,
                optional: false,
            },
            Binder {
                name: "id",
                kind: BinderKind::NamedText,
                optional: false,
            },
            Binder {
                name: "name",
                kind: BinderKind::NamedText,
                optional: false,
            },
            Binder {
                name: "text",
                kind: BinderKind::Text,
                optional: false,
            },
        ],
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry
        .register(&Registrant::core(), invocable)
        .expect("core:knowledge.add registers cleanly at bootstrap — a duplicate id here is a bug");
    dispatcher.attach(
        id,
        Arc::new(|args| {
            let collection = text_arg(args, "collection");
            let doc_id = text_arg(args, "id");
            let name = text_arg(args, "name");
            let text = text_arg(args, "text");
            let svc = match open_service() {
                Ok(s) => s,
                Err(e) => return Outcome::Unavailable { reason: e },
            };
            let document = Document::new_agent(doc_id, collection, name);
            match svc.ingest_record(document, text) {
                Ok(doc) => Outcome::Value(OutcomeValue::Record(vec![
                    ("id".to_string(), OutcomeValue::Text(doc.id)),
                    (
                        "doc_status".to_string(),
                        OutcomeValue::Text(doc.status.as_str().to_string()),
                    ),
                ])),
                Err(e) => Outcome::Unavailable {
                    reason: e.to_string(),
                },
            }
        }),
    );
}

fn register_add_url(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("knowledge.add-url");
    let invocable = Invocable {
        id: id.clone(),
        name: "Add Url",
        summary: "Ingest a web page by URL. http:// only.",
        group: "knowledge",
        locus: Locus::Semantic,
        binders: vec![
            Binder {
                name: "collection",
                kind: BinderKind::NamedText,
                optional: false,
            },
            Binder {
                name: "id",
                kind: BinderKind::NamedText,
                optional: false,
            },
            Binder {
                name: "name",
                kind: BinderKind::NamedText,
                optional: false,
            },
            Binder {
                name: "url",
                kind: BinderKind::Text,
                optional: false,
            },
        ],
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry.register(&Registrant::core(), invocable).expect(
        "core:knowledge.add-url registers cleanly at bootstrap — a duplicate id here is a bug",
    );
    dispatcher.attach(
        id,
        Arc::new(|args| {
            let collection = text_arg(args, "collection");
            let doc_id = text_arg(args, "id");
            let name = text_arg(args, "name");
            let url = text_arg(args, "url");
            let svc = match open_service() {
                Ok(s) => s,
                Err(e) => return Outcome::Unavailable { reason: e },
            };
            let document = Document::new_agent(doc_id, collection, name);
            match svc.ingest_url(document, url) {
                Ok(doc) => Outcome::Value(OutcomeValue::Record(vec![
                    ("id".to_string(), OutcomeValue::Text(doc.id)),
                    (
                        "doc_status".to_string(),
                        OutcomeValue::Text(doc.status.as_str().to_string()),
                    ),
                ])),
                Err(e) => Outcome::Unavailable {
                    reason: e.to_string(),
                },
            }
        }),
    );
}

fn register_query(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("knowledge.query");
    let invocable = Invocable {
        id: id.clone(),
        name: "Query",
        summary: "Hybrid semantic+keyword retrieval over one or more collections.",
        group: "knowledge",
        locus: Locus::Semantic,
        binders: vec![
            Binder {
                name: "collection",
                kind: BinderKind::RepeatableNamedText,
                optional: false,
            },
            Binder {
                name: "top-k",
                kind: BinderKind::NamedText,
                optional: true,
            },
            Binder {
                name: "text",
                kind: BinderKind::Text,
                optional: false,
            },
        ],
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry.register(&Registrant::core(), invocable).expect(
        "core:knowledge.query registers cleanly at bootstrap — a duplicate id here is a bug",
    );
    dispatcher.attach(
        id,
        Arc::new(|args| {
            let collections = list_arg(args, "collection");
            let text = text_arg(args, "text");
            // `--top-k` was a clap-level `value_parser!(usize)` with
            // `default_value_t = 5` before this migration. As a
            // `NamedText`, an unparseable value is now an
            // application-level refusal (exit 1) rather than a
            // clap-level usage failure (exit 2) — the same disclosed
            // shift `loop run --max-iter` already carries.
            let top_k = match opt_text_arg(args, "top-k") {
                None => 5usize,
                Some(s) => match s.parse::<usize>() {
                    Ok(n) => n,
                    Err(_) => {
                        return Outcome::Unavailable {
                            reason: format!("--top-k must be a non-negative integer, got {s:?}"),
                        };
                    }
                },
            };
            let svc = match open_service() {
                Ok(s) => s,
                Err(e) => return Outcome::Unavailable { reason: e },
            };
            let mut request = RetrievalRequest::new(text, collections);
            request.top_k = top_k;
            let grants = GrantStore::new();
            match svc.query(&grants, KnowledgePrincipal::owner(LOCAL_USER), &request) {
                Ok(results) => Outcome::Value(OutcomeValue::List(
                    results
                        .into_iter()
                        .map(|c| {
                            OutcomeValue::Record(vec![
                                ("chunk_id".to_string(), OutcomeValue::Text(c.chunk_id)),
                                ("document_id".to_string(), OutcomeValue::Text(c.document_id)),
                                // No `OutcomeValue::Float` exists (`score`
                                // is `f32`) — formatted as `Text`, the same
                                // established workaround `learn`/`budget`
                                // already use.
                                (
                                    "score".to_string(),
                                    OutcomeValue::Text(format!("{:.4}", c.score)),
                                ),
                                ("text".to_string(), OutcomeValue::Text(c.text)),
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
