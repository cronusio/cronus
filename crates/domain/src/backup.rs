//! Backup & restore: copy the mutable state tier — minus
//! secrets and regenerable cache — to a self-contained destination, and drop
//! it back to resume from later. "Nothing extra": the program tier is never
//! backed up (it is reinstallable), and secrets never leave the device
//! through this path.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Top-level state-tier entries excluded from every backup by default:
/// the secret file, the regenerable cache, and the backups
/// directory itself — a backup must never contain other backups nested
/// inside it, which is also what the default `backups_dir` sitting under
/// `state_root` would otherwise produce (see the destination guard in
/// [`copy_tree_excluding`] for the general case, where an explicit `--to`
/// resolves inside `state_root` under a different name). `logs` is excluded
/// by default too but is the one entry a caller may opt back in (
/// "optional").
const ALWAYS_EXCLUDED: &[&str] = &[".env", "cache", "backups"];
const LOGS_ENTRY: &str = "logs";

/// What to leave out of a backup, beyond the always-excluded secret file
/// and cache directory.
#[derive(Debug, Clone, Copy, Default)]
pub struct BackupOptions {
    /// `logs` is excluded by default; set true to include it anyway.
    pub include_logs: bool,
}

/// A reference to one backup: its id (also its directory name) and location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupRef {
    pub id: String,
    pub path: PathBuf,
    pub created_at_unix: u64,
}

fn excluded_top_level_names(options: BackupOptions) -> Vec<&'static str> {
    let mut names: Vec<&'static str> = ALWAYS_EXCLUDED.to_vec();
    if !options.include_logs {
        names.push(LOGS_ENTRY);
    }
    names
}

/// Recursively copy `from` into `to`, skipping any entry whose *top-level*
/// name (relative to the original `from` root) is in `excluded`, and — at
/// any depth — an entry whose canonical path equals `dest_guard`.
///
/// `dest_guard` is what actually prevents a backup from copying itself into
/// itself: `ALWAYS_EXCLUDED`'s literal `"backups"` only catches the default
/// wiring (`backups_dir` named `backups` directly under `state_root`); an
/// explicit `--to` can resolve anywhere, including some other path inside
/// `state_root` under a different name. Without this guard that copies the
/// destination into itself, recursing until the process runs out of stack.
/// Creates `to` and any needed parent directories.
fn copy_tree_excluding(
    from: &Path,
    to: &Path,
    excluded: &[&str],
    dest_guard: Option<&Path>,
    is_top_level: bool,
) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if is_top_level && excluded.contains(&name_str.as_ref()) {
            continue;
        }
        let src = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir()
            && let Some(guard) = dest_guard
            && fs::canonicalize(&src).is_ok_and(|c| c == guard)
        {
            continue;
        }
        let dst = to.join(&name);
        if file_type.is_dir() {
            copy_tree_excluding(&src, &dst, excluded, dest_guard, false)?;
        } else if file_type.is_file() {
            fs::copy(&src, &dst)?;
        }
        // Symlinks are neither: skipped rather than followed, so a backup
        // never silently escapes the state tier through a link target.
    }
    Ok(())
}

/// A sibling path `list()` never recognizes as a backup (it only matches a
/// top-level `backup-<unix-seconds>` name): the same directory name with a
/// leading dot and a `.partial` suffix. Always a sibling of `target`, so a
/// rename from here into `target` stays on the same filesystem even when
/// `target` itself (an explicit `--to`) is on a different volume than the
/// default `backups_dir`.
fn staging_path_for(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "backup".to_string());
    match target.parent() {
        Some(parent) => parent.join(format!(".{name}.partial")),
        None => PathBuf::from(format!(".{name}.partial")),
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Back up `state_root` into a fresh timestamped directory under
/// `backups_dir` (or `dest` if given explicitly, `--to <path>`), excluding
/// secrets/cache/logs per `options`. Self-contained: nothing outside the
/// returned directory is needed to restore it.
///
/// Written under a staging name first and renamed into place only once the
/// copy fully succeeds: `list()` only ever recognizes the final
/// `backup-<unix-seconds>` name, so a crash or I/O error mid-copy leaves no
/// half-written directory for a later `list`/`restore` to pick up.
pub fn create(
    state_root: &Path,
    backups_dir: &Path,
    dest: Option<&Path>,
    options: BackupOptions,
) -> io::Result<BackupRef> {
    let created_at_unix = unix_now();
    let id = format!("backup-{created_at_unix}");
    let target = dest
        .map(Path::to_path_buf)
        .unwrap_or_else(|| backups_dir.join(&id));

    let staging = staging_path_for(&target);
    let _ = fs::remove_dir_all(&staging); // a leftover from a prior crash, if any
    fs::create_dir_all(&staging)?;
    let dest_guard = fs::canonicalize(&staging).ok();

    let excluded = excluded_top_level_names(options);
    if let Err(err) =
        copy_tree_excluding(state_root, &staging, &excluded, dest_guard.as_deref(), true)
    {
        let _ = fs::remove_dir_all(&staging);
        return Err(err);
    }

    // `fs::rename` fails on Windows when `target` already exists (an
    // explicit `--to` may name a directory the caller already created) —
    // remove it first and retry once the replacement is fully written, so
    // the old target is never destroyed unless a complete new one is ready
    // to take its place.
    if let Err(err) = fs::rename(&staging, &target) {
        if target.exists() {
            fs::remove_dir_all(&target)?;
            fs::rename(&staging, &target)?;
        } else {
            return Err(err);
        }
    }

    Ok(BackupRef {
        id,
        path: target,
        created_at_unix,
    })
}

/// List backups found directly under `backups_dir` (each is one directory,
/// named `backup-<unix-seconds>`), most recent first.
pub fn list(backups_dir: &Path) -> io::Result<Vec<BackupRef>> {
    if !backups_dir.exists() {
        return Ok(Vec::new());
    }
    let mut refs = Vec::new();
    for entry in fs::read_dir(backups_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(created_at_unix) = name.strip_prefix("backup-").and_then(|ts| ts.parse().ok())
        else {
            continue;
        };
        refs.push(BackupRef {
            id: name,
            path: entry.path(),
            created_at_unix,
        });
    }
    refs.sort_by_key(|backup_ref| std::cmp::Reverse(backup_ref.created_at_unix));
    Ok(refs)
}

/// Restore-by-copy: drop a backup's contents into `dest`, ready for
/// the runtime to resume from. `dest` is created if missing; existing files
/// at colliding paths are overwritten (a fresh restore target is expected).
pub fn restore(backup: &BackupRef, dest: &Path) -> io::Result<()> {
    fs::create_dir_all(dest)?;
    let dest_guard = fs::canonicalize(dest).ok();
    copy_tree_excluding(&backup.path, dest, &[], dest_guard.as_deref(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cronus-backup-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    /// A representative state tier: included content plus everything the design
    /// says must be excluded.
    fn seed_state_tier(root: &Path) {
        write(&root.join("config.json"), "{\"theme\":\"dark\"}");
        write(&root.join("AGENTS.md"), "# agents");
        write(&root.join("memory/notes.db"), "memory-bytes");
        write(&root.join("workspaces/acme/board.json"), "[]");
        write(&root.join(".env"), "API_KEY=super-secret-value");
        write(&root.join("cache/embeddings.bin"), "regenerable-bytes");
        write(&root.join("logs/daemon.log"), "log-line");
    }

    #[test]
    fn backup_includes_state_content_and_excludes_secrets_and_cache() {
        let state_root = temp_dir("state");
        let backups_dir = temp_dir("backups");
        seed_state_tier(&state_root);

        let backup_ref = create(&state_root, &backups_dir, None, BackupOptions::default()).unwrap();

        assert!(backup_ref.path.join("config.json").exists());
        assert!(backup_ref.path.join("AGENTS.md").exists());
        assert!(backup_ref.path.join("memory/notes.db").exists());
        assert!(backup_ref.path.join("workspaces/acme/board.json").exists());

        assert!(
            !backup_ref.path.join(".env").exists(),
            "secrets must never be backed up"
        );
        assert!(
            !backup_ref.path.join("cache").exists(),
            "regenerable cache is excluded"
        );
        assert!(
            !backup_ref.path.join("logs").exists(),
            "logs excluded by default"
        );

        let secret_leaked = fs::read_to_string(backup_ref.path.join("config.json"))
            .unwrap_or_default()
            .contains("super-secret-value");
        assert!(!secret_leaked);

        let _ = fs::remove_dir_all(&state_root);
        let _ = fs::remove_dir_all(&backups_dir);
    }

    #[test]
    fn logs_can_be_opted_back_in() {
        let state_root = temp_dir("state-logs");
        let backups_dir = temp_dir("backups-logs");
        seed_state_tier(&state_root);

        let backup_ref = create(
            &state_root,
            &backups_dir,
            None,
            BackupOptions { include_logs: true },
        )
        .unwrap();

        assert!(backup_ref.path.join("logs/daemon.log").exists());
        assert!(
            !backup_ref.path.join(".env").exists(),
            "secrets are never opted back in"
        );

        let _ = fs::remove_dir_all(&state_root);
        let _ = fs::remove_dir_all(&backups_dir);
    }

    #[test]
    fn explicit_destination_overrides_the_default_backups_directory() {
        let state_root = temp_dir("state-dest");
        let backups_dir = temp_dir("backups-dest");
        let chosen = temp_dir("chosen-dest");
        seed_state_tier(&state_root);

        let backup_ref = create(
            &state_root,
            &backups_dir,
            Some(&chosen),
            BackupOptions::default(),
        )
        .unwrap();
        assert_eq!(backup_ref.path, chosen);
        assert!(backup_ref.path.join("config.json").exists());

        let _ = fs::remove_dir_all(&state_root);
        let _ = fs::remove_dir_all(&backups_dir);
        let _ = fs::remove_dir_all(&chosen);
    }

    #[test]
    fn list_returns_backups_most_recent_first() {
        let state_root = temp_dir("state-list");
        let backups_dir = temp_dir("backups-list");
        seed_state_tier(&state_root);

        let older = BackupRef {
            id: "backup-1000".into(),
            path: backups_dir.join("backup-1000"),
            created_at_unix: 1000,
        };
        let newer = BackupRef {
            id: "backup-2000".into(),
            path: backups_dir.join("backup-2000"),
            created_at_unix: 2000,
        };
        fs::create_dir_all(&older.path).unwrap();
        fs::create_dir_all(&newer.path).unwrap();
        // A non-backup directory must be ignored, not misparsed.
        fs::create_dir_all(backups_dir.join("not-a-backup")).unwrap();

        let listed = list(&backups_dir).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, "backup-2000", "most recent first");
        assert_eq!(listed[1].id, "backup-1000");

        let _ = fs::remove_dir_all(&state_root);
        let _ = fs::remove_dir_all(&backups_dir);
    }

    #[test]
    fn backup_does_not_recurse_into_itself_under_the_production_layout() {
        // The real wiring (`crates/cli/src/commands.rs`) resolves
        // `backups_dir` as `state_root.join("backups")` — the destination
        // sits *inside* the tree being backed up. Before the fix this
        // recursed until the stack overflowed; this reproduces that exact
        // layout and asserts it completes and produces no nested `backups`.
        let state_root = temp_dir("state-selfref");
        let backups_dir = state_root.join("backups");
        seed_state_tier(&state_root);

        let backup_ref = create(&state_root, &backups_dir, None, BackupOptions::default())
            .expect("must not recurse into its own destination");

        assert!(backup_ref.path.join("config.json").exists());
        assert!(
            !backup_ref.path.join("backups").exists(),
            "a backup must never contain other backups nested inside it"
        );

        let _ = fs::remove_dir_all(&state_root);
    }

    #[test]
    fn an_explicit_destination_inside_state_root_is_also_guarded() {
        // A `--to` the caller chose to point somewhere else *inside*
        // `state_root`, under a name other than "backups" — the literal
        // `ALWAYS_EXCLUDED` entry can't catch this; the canonical-path
        // destination guard in `copy_tree_excluding` is what must.
        let state_root = temp_dir("state-selfref-to");
        let backups_dir = temp_dir("backups-selfref-to");
        seed_state_tier(&state_root);
        let chosen = state_root.join("my-backup");

        let backup_ref = create(
            &state_root,
            &backups_dir,
            Some(&chosen),
            BackupOptions::default(),
        )
        .expect("must not recurse when --to resolves inside state_root");

        assert_eq!(backup_ref.path, chosen);
        assert!(backup_ref.path.join("config.json").exists());
        assert!(!backup_ref.path.join("my-backup").exists());

        let _ = fs::remove_dir_all(&state_root);
        let _ = fs::remove_dir_all(&backups_dir);
    }

    #[test]
    fn a_completed_backup_replaces_an_existing_destination() {
        // `explicit_destination_overrides_the_default_backups_directory`
        // already covers a fresh `--to`; this covers `--to` naming a
        // directory the caller already created (the staging+rename fallback
        // path — `fs::rename` fails on Windows when the target exists).
        let state_root = temp_dir("state-reuse-dest");
        let backups_dir = temp_dir("backups-reuse-dest");
        let chosen = temp_dir("chosen-reuse-dest"); // pre-created and non-empty
        write(&chosen.join("stale.txt"), "from a previous run");
        seed_state_tier(&state_root);

        let backup_ref = create(
            &state_root,
            &backups_dir,
            Some(&chosen),
            BackupOptions::default(),
        )
        .unwrap();

        assert!(backup_ref.path.join("config.json").exists());
        assert!(
            !backup_ref.path.join("stale.txt").exists(),
            "a completed backup replaces whatever was at an existing --to, not merges with it"
        );

        let _ = fs::remove_dir_all(&state_root);
        let _ = fs::remove_dir_all(&backups_dir);
        let _ = fs::remove_dir_all(&chosen);
    }

    #[test]
    fn a_leftover_staging_directory_is_never_listed_or_restorable() {
        let backups_dir = temp_dir("backups-partial");
        // Simulate what a crash mid-copy leaves behind: the staging name,
        // never renamed into the final `backup-<unix-seconds>` form.
        let leftover = staging_path_for(&backups_dir.join("backup-1000"));
        fs::create_dir_all(&leftover).unwrap();

        let listed = list(&backups_dir).unwrap();
        assert!(
            listed.is_empty(),
            "a staging directory must not be reported as a usable backup"
        );

        let _ = fs::remove_dir_all(&backups_dir);
    }

    #[test]
    fn a_retry_at_the_same_destination_cleans_up_a_leftover_staging_directory_first() {
        // If a prior run crashed leaving a staging directory in place, the
        // next `create()` for the same destination must not treat leftover
        // bytes inside it as already-correct content. `dest` pins the
        // target path exactly (bypassing the timestamp-derived default id),
        // so the collision is deterministic rather than timing-dependent.
        let state_root = temp_dir("state-retry");
        let backups_dir = temp_dir("backups-retry");
        seed_state_tier(&state_root);

        let target = temp_dir("chosen-retry");
        let leftover = staging_path_for(&target);
        write(&leftover.join("stale-from-a-crash.txt"), "poison");

        let backup_ref = create(
            &state_root,
            &backups_dir,
            Some(&target),
            BackupOptions::default(),
        )
        .unwrap();
        assert!(backup_ref.path.join("config.json").exists());
        assert!(
            !backup_ref.path.join("stale-from-a-crash.txt").exists(),
            "a leftover staging directory from a prior crash must not survive into the new backup"
        );

        let _ = fs::remove_dir_all(&state_root);
        let _ = fs::remove_dir_all(&backups_dir);
        let _ = fs::remove_dir_all(&target);
        let _ = fs::remove_dir_all(&leftover);
    }

    #[test]
    fn listing_a_missing_backups_directory_returns_empty_not_an_error() {
        let missing =
            std::env::temp_dir().join(format!("cronus-backup-missing-{}", std::process::id()));
        let _ = fs::remove_dir_all(&missing);
        assert_eq!(list(&missing).unwrap(), Vec::new());
    }

    #[test]
    fn restore_round_trips_a_backup_into_a_resumable_state_tier() {
        let state_root = temp_dir("state-rt");
        let backups_dir = temp_dir("backups-rt");
        let restore_target = temp_dir("restored-rt");
        seed_state_tier(&state_root);

        let backup_ref = create(&state_root, &backups_dir, None, BackupOptions::default()).unwrap();
        restore(&backup_ref, &restore_target).unwrap();

        assert_eq!(
            fs::read_to_string(restore_target.join("config.json")).unwrap(),
            fs::read_to_string(state_root.join("config.json")).unwrap()
        );
        assert!(restore_target.join("memory/notes.db").exists());
        assert!(restore_target.join("workspaces/acme/board.json").exists());
        assert!(
            !restore_target.join(".env").exists(),
            "a restored tier still carries no secrets"
        );

        let _ = fs::remove_dir_all(&state_root);
        let _ = fs::remove_dir_all(&backups_dir);
        let _ = fs::remove_dir_all(&restore_target);
    }

    #[test]
    fn a_backup_is_self_contained_and_restorable_after_the_source_is_gone() {
        let state_root = temp_dir("state-sc");
        let backups_dir = temp_dir("backups-sc");
        let restore_target = temp_dir("restored-sc");
        seed_state_tier(&state_root);

        let backup_ref = create(&state_root, &backups_dir, None, BackupOptions::default()).unwrap();
        fs::remove_dir_all(&state_root).unwrap(); // the original state tier is gone

        restore(&backup_ref, &restore_target).unwrap();
        assert!(
            restore_target.join("config.json").exists(),
            "the backup needed nothing outside itself"
        );

        let _ = fs::remove_dir_all(&backups_dir);
        let _ = fs::remove_dir_all(&restore_target);
    }
}
