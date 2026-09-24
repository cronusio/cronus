//! Every SQLite-backed store records its schema version in the database header
//! when it creates a file, adopts a file from before versioning without losing
//! anything, and refuses a file written by a newer build — without touching it.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use cronus_store_local::inbox;
use cronus_store_local::knowledge::KnowledgeDb;
use cronus_store_local::memory::MemoryStore;
use cronus_store_local::versioning::schema_version;
use cronus_store_local::wiki::WikiStore;
use cronus_store_local::workspace::WorkspaceManager;
use rusqlite::Connection;

fn temp_db(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "cronus-schema-{tag}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir.join("store.db")
}

fn cleanup(db: &Path) {
    if let Some(dir) = db.parent() {
        let _ = std::fs::remove_dir_all(dir);
    }
}

fn version_on_disk(db: &Path) -> i64 {
    schema_version(&Connection::open(db).expect("reopen")).expect("read version")
}

fn set_version_on_disk(db: &Path, version: i64) {
    Connection::open(db)
        .expect("reopen")
        .execute_batch(&format!("PRAGMA user_version = {version}"))
        .expect("set version");
}

/// A store's `open(path)` reduced to what these tests need: create, and try.
struct Store {
    name: &'static str,
    open: fn(&Path) -> Result<(), String>,
}

fn stores() -> Vec<Store> {
    vec![
        Store {
            name: "memory",
            open: |p| MemoryStore::open(p).map(drop).map_err(|e| e.to_string()),
        },
        Store {
            name: "workspace registry",
            open: |p| {
                WorkspaceManager::open(p)
                    .map(drop)
                    .map_err(|e| e.to_string())
            },
        },
        Store {
            name: "wiki",
            open: |p| WikiStore::open(p).map(drop).map_err(|e| e.to_string()),
        },
        Store {
            name: "knowledge",
            open: |p| KnowledgeDb::open(p).map(drop).map_err(|e| e.to_string()),
        },
        Store {
            name: "inbox",
            open: |p| {
                let conn = Connection::open(p).map_err(|e| e.to_string())?;
                inbox::migrate(&conn).map_err(|e| e.to_string())
            },
        },
    ]
}

#[test]
fn a_new_database_records_schema_version_one() {
    for store in stores() {
        let db = temp_db("new");
        (store.open)(&db).unwrap_or_else(|e| panic!("{}: {e}", store.name));
        assert_eq!(version_on_disk(&db), 1, "{}", store.name);
        cleanup(&db);
    }
}

#[test]
fn a_database_from_before_versioning_is_adopted() {
    for store in stores() {
        let db = temp_db("legacy");
        (store.open)(&db).unwrap_or_else(|e| panic!("{}: {e}", store.name));
        set_version_on_disk(&db, 0);
        (store.open)(&db).unwrap_or_else(|e| panic!("{} reopen: {e}", store.name));
        assert_eq!(version_on_disk(&db), 1, "{}", store.name);
        cleanup(&db);
    }
}

#[test]
fn reopening_a_current_database_is_a_no_op() {
    for store in stores() {
        let db = temp_db("again");
        (store.open)(&db).unwrap_or_else(|e| panic!("{}: {e}", store.name));
        (store.open)(&db).unwrap_or_else(|e| panic!("{} reopen: {e}", store.name));
        assert_eq!(version_on_disk(&db), 1, "{}", store.name);
        cleanup(&db);
    }
}

#[test]
fn a_database_written_by_a_newer_build_is_refused_and_left_untouched() {
    for store in stores() {
        let db = temp_db("newer");
        (store.open)(&db).unwrap_or_else(|e| panic!("{}: {e}", store.name));
        set_version_on_disk(&db, 42);

        let refused = (store.open)(&db).expect_err(store.name);
        assert!(
            refused.contains("newer"),
            "{}: the message names the problem: {refused}",
            store.name
        );
        assert_eq!(
            version_on_disk(&db),
            42,
            "{}: the refused file is not rewritten",
            store.name
        );
        cleanup(&db);
    }
}
