//! Schema versioning for the SQLite-backed stores.
//!
//! Every database file records the schema it holds in SQLite's own
//! `user_version` header field, so a build never has to guess what shape of
//! file it was handed. Opening a file does exactly one of four things:
//!
//! - **fresh file** (version 0, nothing in it): the baseline schema is created
//!   and the current version is stamped;
//! - **file from before versioning** (version 0, tables present): adopted as
//!   the current version — the baseline is idempotent, so nothing is lost and
//!   the versioning epoch simply starts there;
//! - **older versioned file**: the ordered migration steps run, each in its
//!   own transaction together with its version stamp, after a consistent copy
//!   of the file is taken (a migration is one-way, so the copy is the only way
//!   back);
//! - **newer versioned file**: refused. A build that does not know a shape
//!   must not read it optimistically or write into it — an older build
//!   quietly "repairing" a newer file would corrupt it for the build that
//!   understands it.
//!
//! The migration path is validated before anything is touched, so a broken
//! step list can never leave a file half-upgraded.

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

/// One forward step of a store's migration path.
pub struct Migration {
    /// The version the database holds once this step has been applied.
    pub to: i64,
    /// Short description, carried in the error when the step fails.
    pub name: &'static str,
    /// The change itself. Runs inside a transaction that also stamps `to`, so
    /// a failing step leaves the file exactly as it found it.
    pub apply: fn(&Connection) -> rusqlite::Result<()>,
}

/// Why a database file could not be brought to the schema this build reads.
#[derive(Debug)]
pub enum SchemaError {
    /// The file was written by a newer build than this one.
    NewerThanSupported {
        store: &'static str,
        found: i64,
        supported: i64,
    },
    /// The store's step list does not cover every version between the file's
    /// and the current one — a defect in the store, found before any change.
    IncompletePath {
        store: &'static str,
        from: i64,
        to: i64,
        missing: i64,
    },
    /// The safety copy taken before a migration could not be written, so the
    /// migration did not run.
    Backup {
        store: &'static str,
        target: PathBuf,
        cause: rusqlite::Error,
    },
    /// A migration step failed; its transaction was rolled back.
    Step {
        store: &'static str,
        step: &'static str,
        to: i64,
        cause: rusqlite::Error,
    },
    Database(rusqlite::Error),
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SchemaError::NewerThanSupported {
                store,
                found,
                supported,
            } => write!(
                f,
                "the {store} database has schema version {found}, newer than the {supported} \
                 this build understands — it was written by a newer version; update Cronus \
                 rather than opening it with this one"
            ),
            SchemaError::IncompletePath {
                store,
                from,
                to,
                missing,
            } => write!(
                f,
                "the {store} database cannot be migrated from version {from} to {to}: \
                 no migration step reaches version {missing}"
            ),
            SchemaError::Backup {
                store,
                target,
                cause,
            } => write!(
                f,
                "the {store} database was not migrated: the safety copy at {} could not be \
                 written ({cause})",
                target.display()
            ),
            SchemaError::Step {
                store,
                step,
                to,
                cause,
            } => write!(
                f,
                "migrating the {store} database to version {to} failed at \"{step}\" and was \
                 rolled back ({cause})"
            ),
            SchemaError::Database(e) => write!(f, "schema version check failed: {e}"),
        }
    }
}

impl std::error::Error for SchemaError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SchemaError::Backup { cause, .. } | SchemaError::Step { cause, .. } => Some(cause),
            SchemaError::Database(e) => Some(e),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for SchemaError {
    fn from(e: rusqlite::Error) -> Self {
        SchemaError::Database(e)
    }
}

/// The schema version recorded in `conn`'s header (0 when never stamped).
pub fn schema_version(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("PRAGMA user_version", [], |row| row.get(0))
}

fn stamp(conn: &Connection, version: i64) -> rusqlite::Result<()> {
    // `PRAGMA` takes no bound parameters; `version` is an integer, so the
    // formatted statement cannot carry anything but a number.
    conn.execute_batch(&format!("PRAGMA user_version = {version}"))
}

/// Bring `conn` to schema `current`, creating it with `baseline` when the
/// file is new (or predates versioning) and running `steps` when it is older.
///
/// `baseline` must be idempotent (`CREATE ... IF NOT EXISTS`): it also runs on
/// files already at `current`, where it is a cheap repair of any object a
/// crashed earlier open never got to create.
pub fn open_schema<E>(
    conn: &Connection,
    store: &'static str,
    current: i64,
    baseline: impl FnOnce(&Connection) -> Result<(), E>,
    steps: &[Migration],
) -> Result<(), E>
where
    E: From<SchemaError> + From<rusqlite::Error>,
{
    let found = schema_version(conn)?;
    if found > current {
        return Err(SchemaError::NewerThanSupported {
            store,
            found,
            supported: current,
        }
        .into());
    }
    if found > 0 && found < current {
        migrate_forward(conn, store, found, current, steps)?;
    }
    baseline(conn)?;
    if found != current {
        stamp(conn, current)?;
    }
    Ok(())
}

fn migrate_forward(
    conn: &Connection,
    store: &'static str,
    found: i64,
    current: i64,
    steps: &[Migration],
) -> Result<(), SchemaError> {
    // Validate the whole path first: every version in (found, current] needs
    // exactly one step, or nothing is touched.
    let mut path = Vec::new();
    for version in (found + 1)..=current {
        let step = steps
            .iter()
            .find(|s| s.to == version)
            .ok_or(SchemaError::IncompletePath {
                store,
                from: found,
                to: current,
                missing: version,
            })?;
        path.push(step);
    }

    backup_before_migration(conn, store, found)?;

    for step in path {
        let tx = conn
            .unchecked_transaction()
            .map_err(|cause| SchemaError::Step {
                store,
                step: step.name,
                to: step.to,
                cause,
            })?;
        (step.apply)(&tx)
            .and_then(|()| stamp(&tx, step.to))
            .and_then(|()| tx.commit())
            .map_err(|cause| SchemaError::Step {
                store,
                step: step.name,
                to: step.to,
                cause,
            })?;
    }
    Ok(())
}

/// Take a consistent copy of a file-backed database next to it, named after
/// the version it holds. An in-memory database has nothing to lose to a
/// failed migration, so it is skipped.
fn backup_before_migration(
    conn: &Connection,
    store: &'static str,
    found: i64,
) -> Result<Option<PathBuf>, SchemaError> {
    let Some(path) = conn.path().filter(|p| !p.is_empty()).map(Path::new) else {
        return Ok(None);
    };
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(format!(".pre-v{found}-{nanos}.bak"));
    let target = path.with_file_name(name);
    // `VACUUM INTO` writes a transactionally consistent copy even while the
    // file is in WAL mode, which a plain file copy would not guarantee.
    conn.execute("VACUUM INTO ?1", [target.to_string_lossy().as_ref()])
        .map_err(|cause| SchemaError::Backup {
            store,
            target: target.clone(),
            cause,
        })?;
    Ok(Some(target))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    /// Minimal error type standing in for a store's own.
    #[derive(Debug)]
    enum TestError {
        Schema(SchemaError),
        Db(rusqlite::Error),
    }

    impl From<SchemaError> for TestError {
        fn from(e: SchemaError) -> Self {
            TestError::Schema(e)
        }
    }

    impl From<rusqlite::Error> for TestError {
        fn from(e: rusqlite::Error) -> Self {
            TestError::Db(e)
        }
    }

    impl TestError {
        /// The schema error a test expects; a plain database error fails the
        /// test with its cause instead.
        fn schema(self) -> SchemaError {
            match self {
                TestError::Schema(e) => e,
                TestError::Db(e) => panic!("expected a schema error, got a database error: {e}"),
            }
        }
    }

    fn baseline(conn: &Connection) -> Result<(), TestError> {
        conn.execute_batch("CREATE TABLE IF NOT EXISTS items (id INTEGER PRIMARY KEY, name TEXT)")?;
        Ok(())
    }

    /// Baseline of a later schema: the column a migration adds is part of it.
    fn baseline_v3(conn: &Connection) -> Result<(), TestError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS items (id INTEGER PRIMARY KEY, name TEXT, tag TEXT DEFAULT 'none', rank INTEGER DEFAULT 0)",
        )?;
        Ok(())
    }

    fn add_tag(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch("ALTER TABLE items ADD COLUMN tag TEXT DEFAULT 'none'")
    }

    fn add_rank(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch(
            "ALTER TABLE items ADD COLUMN rank INTEGER DEFAULT 0; UPDATE items SET rank = id",
        )
    }

    fn broken_step(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch("ALTER TABLE items ADD COLUMN half_done TEXT")?;
        conn.execute_batch("THIS IS NOT SQL")
    }

    const STEPS: &[Migration] = &[
        Migration {
            to: 2,
            name: "add tag",
            apply: add_tag,
        },
        Migration {
            to: 3,
            name: "add rank",
            apply: add_rank,
        },
    ];

    fn temp_db(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cronus-versioning-{tag}-{}-{}",
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

    fn columns(conn: &Connection) -> Vec<String> {
        let mut stmt = conn
            .prepare("PRAGMA table_info(items)")
            .expect("table_info");
        stmt.query_map([], |r| r.get::<_, String>(1))
            .expect("rows")
            .map(|r| r.expect("column name"))
            .collect()
    }

    fn backups_of(db: &Path) -> Vec<PathBuf> {
        let dir = db.parent().expect("parent");
        std::fs::read_dir(dir)
            .expect("read dir")
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.to_string_lossy().contains(".pre-v"))
            .collect()
    }

    #[test]
    fn a_fresh_database_gets_the_baseline_and_the_current_version() {
        let conn = Connection::open_in_memory().unwrap();
        assert_eq!(schema_version(&conn).unwrap(), 0);
        open_schema(&conn, "test", 1, baseline, &[]).unwrap();
        assert_eq!(schema_version(&conn).unwrap(), 1);
        assert_eq!(columns(&conn), ["id", "name"]);
    }

    #[test]
    fn a_database_from_before_versioning_is_adopted_without_losing_rows() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT); INSERT INTO items VALUES (1, 'kept')",
        )
        .unwrap();
        assert_eq!(schema_version(&conn).unwrap(), 0);
        open_schema(&conn, "test", 1, baseline, &[]).unwrap();
        assert_eq!(schema_version(&conn).unwrap(), 1);
        let name: String = conn
            .query_row("SELECT name FROM items WHERE id = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(name, "kept");
    }

    #[test]
    fn reopening_at_the_current_version_changes_nothing() {
        let conn = Connection::open_in_memory().unwrap();
        open_schema(&conn, "test", 1, baseline, &[]).unwrap();
        conn.execute_batch("INSERT INTO items VALUES (1, 'a')")
            .unwrap();
        open_schema(&conn, "test", 1, baseline, &[]).unwrap();
        assert_eq!(schema_version(&conn).unwrap(), 1);
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn a_newer_database_is_refused_and_left_untouched() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA user_version = 7").unwrap();
        let ran = Cell::new(false);
        let err = open_schema(
            &conn,
            "test",
            1,
            |c| {
                ran.set(true);
                baseline(c)
            },
            &[],
        )
        .unwrap_err();
        assert!(!ran.get(), "the baseline must not run against a newer file");
        assert_eq!(schema_version(&conn).unwrap(), 7);
        match err.schema() {
            SchemaError::NewerThanSupported {
                found, supported, ..
            } => {
                assert_eq!((found, supported), (7, 1));
            }
            other => panic!("expected NewerThanSupported, got {other:?}"),
        }
        let shown = SchemaError::NewerThanSupported {
            store: "test",
            found: 7,
            supported: 1,
        }
        .to_string();
        assert!(
            shown.contains("newer"),
            "the message names the problem: {shown}"
        );
    }

    #[test]
    fn an_older_file_is_migrated_forward_in_order_after_a_safety_copy() {
        let db = temp_db("forward");
        {
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch(
                "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT);
                 INSERT INTO items VALUES (1, 'one'), (2, 'two');
                 PRAGMA user_version = 1",
            )
            .unwrap();
        }
        let conn = Connection::open(&db).unwrap();
        open_schema(&conn, "test", 3, baseline_v3, STEPS).unwrap();

        assert_eq!(schema_version(&conn).unwrap(), 3);
        assert_eq!(columns(&conn), ["id", "name", "tag", "rank"]);
        let (tag, rank): (String, i64) = conn
            .query_row("SELECT tag, rank FROM items WHERE id = 2", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!((tag.as_str(), rank), ("none", 2));

        let backups = backups_of(&db);
        assert_eq!(
            backups.len(),
            1,
            "one safety copy before migrating: {backups:?}"
        );
        let copy = Connection::open(&backups[0]).unwrap();
        assert_eq!(
            schema_version(&copy).unwrap(),
            1,
            "the copy holds the old shape"
        );
        assert_eq!(columns(&copy), ["id", "name"]);
        let n: i64 = copy
            .query_row("SELECT COUNT(*) FROM items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 2, "and the old rows");
        drop((conn, copy));
        cleanup(&db);
    }

    #[test]
    fn a_failing_step_rolls_back_and_keeps_the_previous_version() {
        let db = temp_db("failing");
        {
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch(
                "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT);
                 PRAGMA user_version = 1",
            )
            .unwrap();
        }
        let conn = Connection::open(&db).unwrap();
        let steps = [Migration {
            to: 2,
            name: "break halfway",
            apply: broken_step,
        }];
        let err = open_schema(&conn, "test", 2, baseline, &steps)
            .unwrap_err()
            .schema();
        assert!(matches!(err, SchemaError::Step { to: 2, .. }), "{err:?}");
        assert_eq!(
            schema_version(&conn).unwrap(),
            1,
            "the stamp did not advance"
        );
        assert_eq!(
            columns(&conn),
            ["id", "name"],
            "the partial ALTER was rolled back"
        );
        drop(conn);
        cleanup(&db);
    }

    #[test]
    fn a_gap_in_the_step_list_is_found_before_anything_changes() {
        let db = temp_db("gap");
        {
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch(
                "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT);
                 PRAGMA user_version = 1",
            )
            .unwrap();
        }
        let conn = Connection::open(&db).unwrap();
        // Current is 3 but only the step to 3 exists: version 2 is unreachable.
        let steps = [Migration {
            to: 3,
            name: "add rank",
            apply: add_rank,
        }];
        let err = open_schema(&conn, "test", 3, baseline_v3, &steps)
            .unwrap_err()
            .schema();
        assert!(
            matches!(err, SchemaError::IncompletePath { missing: 2, .. }),
            "{err:?}"
        );
        assert_eq!(schema_version(&conn).unwrap(), 1);
        assert!(
            backups_of(&db).is_empty(),
            "no copy for a migration that never started"
        );
        assert_eq!(columns(&conn), ["id", "name"]);
        drop(conn);
        cleanup(&db);
    }
}
