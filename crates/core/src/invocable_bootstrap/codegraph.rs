use std::sync::Arc;

use cronus_codegraph::extractor::{Extractor, RegexExtractor};
use cronus_codegraph::index::CodeIndex;
use cronus_contract::{Binder, BinderKind, Invocable, Locus, Outcome, OutcomeValue, Stability};
use cronus_domain::invocable::{Dispatcher, InvocableRegistry, Registrant};

use super::{core_id, text_arg};

/// On-disk location of the codegraph index. An index is symbols from a
/// specific project's own source tree — resolves against the current
/// workspace, the same resolution `board.rs`/`memory.rs` use.
fn index_db_path() -> std::path::PathBuf {
    cronus_domain::paths::resolve_workspace_root()
        .join("codegraph")
        .join("index.db")
}

/// Open the **persistent** codegraph index. `index` used to open a fresh
/// `open_in_memory()` database that was dropped when the handler returned,
/// so a later `search` in a separate invocation never saw what an earlier
/// `index` stored (the same class of defect `memory.rs`'s own
/// `open_memory_store` already fixed — see its doc comment). One file,
/// opened by both verbs, is what makes the round-trip work.
fn open_index() -> Result<CodeIndex, String> {
    let path = index_db_path();
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return Err(format!("cannot create codegraph directory: {e}"));
    }
    CodeIndex::open(&path).map_err(|e| e.to_string())
}

pub(super) fn register(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    register_index(registry, dispatcher);
    register_search(registry, dispatcher);
}

fn register_index(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("codegraph.index");
    let invocable = Invocable {
        id: id.clone(),
        name: "Index",
        summary: "Index a directory or file into the code graph.",
        group: "codegraph",
        locus: Locus::Semantic,
        binders: vec![Binder {
            name: "path",
            kind: BinderKind::Text,
            optional: false,
        }],
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry.register(&Registrant::core(), invocable).expect(
        "core:codegraph.index registers cleanly at bootstrap — a duplicate id here is a bug",
    );
    dispatcher.attach(
        id,
        Arc::new(|args| {
            let path = std::path::PathBuf::from(text_arg(args, "path"));
            let index = match open_index() {
                Ok(index) => index,
                Err(reason) => {
                    return Outcome::Unavailable { reason };
                }
            };
            let extractor = RegexExtractor;
            let mut total = 0usize;
            if path.is_file() {
                total += index_file(&index, &path, &extractor);
            } else if path.is_dir()
                && let Ok(entries) = std::fs::read_dir(&path)
            {
                for entry in entries.filter_map(|e| e.ok()) {
                    let p = entry.path();
                    if p.extension().and_then(|e| e.to_str()) == Some("rs") {
                        total += index_file(&index, &p, &extractor);
                    }
                }
            }
            Outcome::Value(OutcomeValue::Record(vec![
                (
                    "path".to_string(),
                    OutcomeValue::Text(path.display().to_string()),
                ),
                ("symbols".to_string(), OutcomeValue::Integer(total as i64)),
            ]))
        }),
    );
}

fn index_file(index: &CodeIndex, path: &std::path::Path, extractor: &RegexExtractor) -> usize {
    let source = std::fs::read_to_string(path).unwrap_or_default();
    let syms = extractor.extract(&source);
    let n = syms.len();
    let _ = index.index_symbols(&path.display().to_string(), &syms);
    n
}

fn register_search(registry: &mut InvocableRegistry, dispatcher: &mut Dispatcher) {
    let id = core_id("codegraph.search");
    let invocable = Invocable {
        id: id.clone(),
        name: "Search",
        summary: "Search the code graph by keyword.",
        group: "codegraph",
        locus: Locus::Semantic,
        binders: vec![Binder {
            name: "query",
            kind: BinderKind::Text,
            optional: false,
        }],
        stability: Stability::Shipped,
        journal_raw_input: true,
    };
    registry.register(&Registrant::core(), invocable).expect(
        "core:codegraph.search registers cleanly at bootstrap — a duplicate id here is a bug",
    );
    dispatcher.attach(
        id,
        Arc::new(|args| {
            let query = text_arg(args, "query");
            let index = match open_index() {
                Ok(index) => index,
                Err(reason) => {
                    return Outcome::Unavailable { reason };
                }
            };
            match index.search(query, 10) {
                Ok(results) => Outcome::Value(OutcomeValue::List(
                    results
                        .into_iter()
                        .map(|s| {
                            OutcomeValue::Record(vec![
                                ("name".to_string(), OutcomeValue::Text(s.name)),
                                ("file".to_string(), OutcomeValue::Text(s.file)),
                                ("line".to_string(), OutcomeValue::Integer(s.line as i64)),
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
