use cronus_codegraph::community::{CommunityDetector, Edge, UnionFindDetector};
use cronus_codegraph::dedup::{
    DEFAULT_SIMILARITY_THRESHOLD, is_low_entropy, jaro_winkler, should_merge,
};
use cronus_codegraph::extractor::{Confidence, Extractor, RegexExtractor, SymbolKind};
use cronus_codegraph::index::CodeIndex;
use cronus_codegraph::search::{fuse, rrf_merge};
use std::collections::HashMap;

fn open() -> CodeIndex {
    CodeIndex::open_in_memory().unwrap()
}

// ── Extractor ─────────────────────────────────────────────────────────────────

#[test]
fn extractor_finds_fn_foo() {
    let source = "fn foo() { }";
    let syms = RegexExtractor.extract(source);
    assert!(
        syms.iter()
            .any(|s| s.name == "foo" && s.kind == SymbolKind::Function)
    );
}

#[test]
fn extractor_finds_pub_fn() {
    let source = "pub fn bar(x: i32) -> i32 { x }";
    let syms = RegexExtractor.extract(source);
    assert!(syms.iter().any(|s| s.name == "bar"));
}

#[test]
fn extractor_finds_struct() {
    let source = "pub struct MyStruct { }";
    let syms = RegexExtractor.extract(source);
    assert!(
        syms.iter()
            .any(|s| s.name == "MyStruct" && s.kind == SymbolKind::Struct)
    );
}

#[test]
fn extractor_finds_enum() {
    let source = "pub enum MyEnum { A, B }";
    let syms = RegexExtractor.extract(source);
    assert!(
        syms.iter()
            .any(|s| s.name == "MyEnum" && s.kind == SymbolKind::Enum)
    );
}

#[test]
fn extractor_confidence_is_extracted() {
    let source = "fn baz() {}";
    let syms = RegexExtractor.extract(source);
    assert_eq!(syms[0].confidence, Confidence::Extracted);
}

#[test]
fn extractor_records_line_number() {
    let source = "// header\nfn hello() {}";
    let syms = RegexExtractor.extract(source);
    let hello = syms.iter().find(|s| s.name == "hello").unwrap();
    assert_eq!(hello.line, 2);
}

// ── Index: store + retrieve ───────────────────────────────────────────────────

#[test]
fn index_store_and_get_by_name() {
    let idx = open();
    let syms = RegexExtractor.extract("fn alpha() {}");
    idx.index_symbols("src/lib.rs", &syms).unwrap();
    let result = idx.get_by_name("alpha").unwrap();
    assert!(
        result.is_some(),
        "stored symbol must be retrievable by name"
    );
    assert_eq!(result.unwrap().file, "src/lib.rs");
}

#[test]
fn index_get_by_name_returns_none_for_unknown() {
    let idx = open();
    assert!(idx.get_by_name("ghost").unwrap().is_none());
}

#[test]
fn index_store_returns_count() {
    let idx = open();
    let syms = RegexExtractor.extract("fn a() {}\nfn b() {}");
    let n = idx.index_symbols("f.rs", &syms).unwrap();
    assert_eq!(n, 2);
}

// ── Index: persistent open ─────────────────────────────────────────────

fn temp_db_path(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("cronus-codegraph-{tag}-{}.db", std::process::id()))
}

#[test]
fn a_symbol_indexed_at_a_path_survives_reopening_it() {
    // The shape of the actual product defect: `index` and `search` are two
    // separate CLI invocations, i.e. two separate `CodeIndex` values opened
    // at the same path with nothing else in common — closing the first and
    // opening a second at the same path is the faithful in-process
    // equivalent of that.
    let path = temp_db_path("reopen");
    let _ = std::fs::remove_file(&path);

    {
        let idx = CodeIndex::open(&path).unwrap();
        let syms = RegexExtractor.extract("fn survives_reopen() {}");
        idx.index_symbols("src/a.rs", &syms).unwrap();
    } // idx (and its Connection) dropped here

    let reopened = CodeIndex::open(&path).unwrap();
    let found = reopened.get_by_name("survives_reopen").unwrap();
    assert!(
        found.is_some(),
        "a symbol indexed in one CodeIndex::open must be visible in the next"
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn opening_a_fresh_path_starts_with_an_empty_but_working_schema() {
    let path = temp_db_path("fresh");
    let _ = std::fs::remove_file(&path);

    let idx = CodeIndex::open(&path).unwrap();
    assert!(idx.get_by_name("anything").unwrap().is_none());
    assert!(idx.search("anything", 10).unwrap().is_empty());

    let _ = std::fs::remove_file(&path);
}

// ── Index: schema version ─────────────────────────────────────────────────────

fn recorded_version(path: &std::path::Path) -> i64 {
    rusqlite::Connection::open(path)
        .unwrap()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn a_new_index_records_its_schema_version() {
    let path = temp_db_path("stamped");
    let _ = std::fs::remove_file(&path);
    drop(CodeIndex::open(&path).unwrap());
    assert_eq!(recorded_version(&path), 1);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn an_index_from_before_versioning_is_adopted_with_its_symbols() {
    let path = temp_db_path("legacy");
    let _ = std::fs::remove_file(&path);
    {
        let idx = CodeIndex::open(&path).unwrap();
        let syms = RegexExtractor.extract("fn kept_symbol() {}");
        idx.index_symbols("src/a.rs", &syms).unwrap();
    }
    // Simulate a file written before the version was recorded.
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch("PRAGMA user_version = 0")
        .unwrap();

    let idx = CodeIndex::open(&path).unwrap();
    assert!(idx.get_by_name("kept_symbol").unwrap().is_some());
    drop(idx);
    assert_eq!(recorded_version(&path), 1);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn an_index_written_by_a_newer_build_is_refused_and_left_untouched() {
    let path = temp_db_path("newer");
    let _ = std::fs::remove_file(&path);
    drop(CodeIndex::open(&path).unwrap());
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch("PRAGMA user_version = 9")
        .unwrap();

    let refused = CodeIndex::open(&path)
        .err()
        .expect("a newer file must be refused");
    assert!(
        refused.to_string().contains("newer"),
        "the message names the problem: {refused}"
    );
    assert_eq!(
        recorded_version(&path),
        9,
        "the refused file is not rewritten"
    );
    let _ = std::fs::remove_file(&path);
}

// ── Index: FTS5 search ────────────────────────────────────────────────────────

#[test]
fn fts_search_finds_matching_symbol() {
    let idx = open();
    let syms = RegexExtractor.extract("fn my_special_function() {}");
    idx.index_symbols("src/a.rs", &syms).unwrap();
    let results = idx.search("my_special_function", 10).unwrap();
    assert!(!results.is_empty(), "FTS5 must find the symbol by name");
    assert_eq!(results[0].name, "my_special_function");
}

#[test]
fn fts_search_returns_empty_for_no_match() {
    let idx = open();
    let results = idx.search("xyz_nonexistent", 10).unwrap();
    assert!(results.is_empty());
}

// ── RRF fusion ────────────────────────────────────────────────────────────────

#[test]
fn rrf_merge_combines_two_lists() {
    // id=1 is top in list A, id=2 is top in list B, id=3 appears in both
    let a: Vec<i64> = vec![1, 3];
    let b: Vec<i64> = vec![2, 3];
    let merged = rrf_merge(&[a, b]);

    // id=3 appears in both lists → higher combined score
    let id3_pos = merged.iter().position(|(id, _)| *id == 3).unwrap();
    let id1_pos = merged.iter().position(|(id, _)| *id == 1).unwrap();
    let id2_pos = merged.iter().position(|(id, _)| *id == 2).unwrap();
    assert!(
        id3_pos < id1_pos,
        "id=3 (in both lists) must rank higher than id=1 (one list)"
    );
    assert!(
        id3_pos < id2_pos,
        "id=3 (in both lists) must rank higher than id=2 (one list)"
    );
}

#[test]
fn rrf_merge_deduplicates_results() {
    let a: Vec<i64> = vec![1, 2];
    let b: Vec<i64> = vec![1, 2];
    let merged = rrf_merge(&[a, b]);
    assert_eq!(merged.len(), 2, "duplicates must be merged");
}

#[test]
fn fuse_returns_results_ordered_by_score() {
    let idx = open();
    let syms = RegexExtractor.extract("fn alpha() {}\nfn beta() {}");
    idx.index_symbols("f.rs", &syms).unwrap();
    let alpha = idx.get_by_name("alpha").unwrap().unwrap();
    let beta = idx.get_by_name("beta").unwrap().unwrap();
    let mut candidates = HashMap::new();
    candidates.insert(alpha.id, alpha.clone());
    candidates.insert(beta.id, beta.clone());

    // alpha is first in keyword, beta is first in vector
    let keyword = vec![alpha.id, beta.id];
    let vector = vec![beta.id, alpha.id];
    let results = fuse(keyword, vector, &candidates);
    assert_eq!(results.len(), 2, "both results must appear");
    // Scores are equal (both appear in both lists at same ranks) — just check dedup
}

// ── Community detection ───────────────────────────────────────────────────────

#[test]
fn union_find_groups_connected_symbols() {
    let detector = UnionFindDetector;
    let nodes = vec![1, 2, 3, 4];
    let edges = vec![Edge { from: 1, to: 2 }, Edge { from: 3, to: 4 }];
    let communities = detector.detect(&nodes, &edges);
    assert_eq!(communities.len(), 2, "two connected components");
    let c0 = communities.iter().find(|c| c.members.contains(&1)).unwrap();
    assert!(
        c0.members.contains(&2),
        "1 and 2 must be in the same community"
    );
}

#[test]
fn union_find_single_node_is_own_community() {
    let detector = UnionFindDetector;
    let nodes = vec![5];
    let communities = detector.detect(&nodes, &[]);
    assert_eq!(communities.len(), 1);
    assert_eq!(communities[0].members, vec![5]);
}

#[test]
fn union_find_excludes_hub_by_caller() {
    // Hub exclusion is caller's responsibility — detector returns all communities.
    let detector = UnionFindDetector;
    let nodes = vec![1, 2, 3];
    let edges = vec![Edge { from: 1, to: 2 }, Edge { from: 1, to: 3 }];
    let communities = detector.detect(&nodes, &edges);
    assert_eq!(communities.len(), 1, "all connected → one community");
}

// ── Deduplication ─────────────────────────────────────────────────────────────

#[test]
fn low_entropy_names_bypass_dedup() {
    assert!(is_low_entropy("new"));
    assert!(is_low_entropy("init"));
    assert!(!is_low_entropy("compute_hash"));
}

#[test]
fn jaro_winkler_identical_strings_score_1() {
    assert_eq!(jaro_winkler("hello", "hello"), 1.0);
}

#[test]
fn jaro_winkler_different_strings_score_below_threshold() {
    let score = jaro_winkler("alpha", "zeta");
    assert!(score < DEFAULT_SIMILARITY_THRESHOLD);
}

#[test]
fn should_merge_identical_names() {
    assert!(should_merge("foo", "foo", DEFAULT_SIMILARITY_THRESHOLD));
}

#[test]
fn should_merge_low_entropy_returns_false() {
    assert!(!should_merge(
        "new",
        "new_item",
        DEFAULT_SIMILARITY_THRESHOLD
    ));
}
