//! Disposable worlds: the place a simulated run of the real product happens.
//!
//! Real surfaces, real effects, inside
//! a world built for one run and destroyed after it. Containment here is
//! **environmental**: a world never lands inside this repository's working
//! tree, and its child processes never inherit the ambient `CRONUS_*`
//! environment a developer's own shell happens to carry.
//!
//! A world is one directory tree with two subdirectories:
//!
//! - `cwd/` — the working directory every spawned process runs in, so a
//!   relative `.cronus/` workspace state directory (see
//!   `crates/cli/src/commands.rs::init`) lands there and nowhere else.
//! - `global/` — the target of `CRONUS_PORTABLE_DIR` (`crates/domain/src/paths.rs`),
//!   so the product's OS-native program/state/cache/logs roots (`%APPDATA%`,
//!   `$HOME/.local/share`, …) are never touched by a run — this is the real,
//!   already-shipped isolation seam the product built for exactly this case,
//!   and using it is strictly more portable than trying to override
//!   `HOME`/`APPDATA` ourselves (Windows never reads `HOME`, and a scenario
//!   run on any of the four supported OS families should not need four
//!   different override recipes).

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// A world build or teardown failure.
#[derive(Debug)]
pub enum WorldError {
    /// The resolved root would land inside a repository's working tree.
    /// This is a hard boundary: a world that cannot be shown to be outside
    /// the repository is refused, never built and hoped about.
    Containment {
        attempted: PathBuf,
        repo_root: PathBuf,
    },
    /// A filesystem operation failed while building or tearing down a world.
    Io(io::Error),
}

impl std::fmt::Display for WorldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorldError::Containment {
                attempted,
                repo_root,
            } => write!(
                f,
                "refusing to build a world at {} — it lies inside the repository rooted at {}",
                attempted.display(),
                repo_root.display()
            ),
            WorldError::Io(err) => write!(f, "world I/O error: {err}"),
        }
    }
}

impl std::error::Error for WorldError {}

impl From<io::Error> for WorldError {
    fn from(err: io::Error) -> Self {
        WorldError::Io(err)
    }
}

/// A disposable place for one simulated run.
#[derive(Debug)]
pub struct World {
    root: PathBuf,
    id: String,
    /// The running crate's own version — inherited from `[workspace.package]`,
    /// so it is identical to `cronus-cli`'s shipped version without shelling
    /// out to the product just to ask it.
    pub product_version: &'static str,
    /// A digest of the repository's own tracked-file dirtiness at the moment
    /// this world was built (`None` when no repository ancestor is found —
    /// contamination cannot be checked, and callers must not treat that
    /// absence as "clean"). Compared against a later digest to detect a run
    /// that wrote into tracked files instead of only into its world.
    pub repo_dirty_digest: Option<String>,
}

impl World {
    /// Build a world under the OS temp root. Refuses if the temp root itself
    /// resolves inside this checkout's repository.
    pub fn build(scenario_id: &str) -> Result<Self, WorldError> {
        Self::build_with_base(&std::env::temp_dir(), scenario_id)
    }

    /// Build a world under an explicit base directory. Exposed so the
    /// containment refusal is testable without mutating process-global temp
    /// directory configuration (`TMPDIR`/`TEMP` are process environment, and
    /// mutating them races every other test running in the same process).
    pub fn build_with_base(base: &Path, scenario_id: &str) -> Result<Self, WorldError> {
        let cwd = std::env::current_dir()?;
        let repo_root = find_repo_root(&cwd);

        if let Some(repo_root) = &repo_root {
            let repo_root = canonical_or_self(repo_root);
            let base_canon = canonical_or_self(base);
            if base_canon.starts_with(&repo_root) {
                return Err(WorldError::Containment {
                    attempted: base.to_path_buf(),
                    repo_root,
                });
            }
        }

        let id = format!("{}-{}", sanitize(scenario_id), stamp());
        let root = base.join(world_dir_name(&id));
        std::fs::create_dir_all(root.join("cwd"))?;
        std::fs::create_dir_all(root.join("global"))?;

        let repo_dirty_digest = repo_root.as_deref().and_then(repo_dirty_digest);

        Ok(World {
            root,
            id,
            product_version: env!("CARGO_PKG_VERSION"),
            repo_dirty_digest,
        })
    }

    /// Stable identity for this world (scenario id + build stamp).
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The root a world with this id has (or would have) under the OS temp
    /// root — the cross-process handle. A world's id alone is enough for a
    /// later, separate `cronus-sim` invocation to find it again; nothing
    /// beyond string formatting is required, and nothing is looked up.
    pub fn root_for_id(id: &str) -> PathBuf {
        std::env::temp_dir().join(world_dir_name(id))
    }

    /// Reattach to an already-built world by its root directory. Does not
    /// create or validate anything — `root` must already exist. Used by a
    /// later CLI invocation that has only a world id (via
    /// [`Self::root_for_id`]) and needs [`Self::cwd`]/[`Self::global_dir`]/
    /// [`Self::child_env`]/[`Self::state_digest`], none of which read
    /// anything but `root`.
    ///
    /// `repo_dirty_digest` on the returned value is always `None` — the
    /// meaningful one is the value captured at [`Self::build`] time and
    /// persisted by the caller; re-deriving it here would silently replace
    /// that baseline with a mid-run value. Use
    /// [`Self::current_repo_dirty_digest`] for a fresh comparison value.
    pub fn attach(root: PathBuf) -> Self {
        let id = root
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_prefix("cronus-sim-"))
            .unwrap_or_default()
            .to_string();
        World {
            root,
            id,
            product_version: env!("CARGO_PKG_VERSION"),
            repo_dirty_digest: None,
        }
    }

    /// Recompute the repository's tracked-file dirtiness right now, using
    /// this *process's* current directory to find the repository root (not
    /// the world's — the world was deliberately built outside any
    /// repository). Compared against [`Self::repo_dirty_digest`] to detect
    /// a run that wrote into tracked files instead of only into its world.
    pub fn current_repo_dirty_digest(&self) -> Option<String> {
        let cwd = std::env::current_dir().ok()?;
        let repo_root = find_repo_root(&cwd)?;
        repo_dirty_digest(&repo_root)
    }

    /// The root directory. Not itself a valid working directory — spawn
    /// children in [`Self::cwd`], never here.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The working directory every spawned process runs in.
    pub fn cwd(&self) -> PathBuf {
        self.root.join("cwd")
    }

    /// The directory `CRONUS_PORTABLE_DIR` points children at.
    pub fn global_dir(&self) -> PathBuf {
        self.root.join("global")
    }

    /// The environment a child process must run with: `parent_env` with
    /// every `CRONUS_*` entry removed, then `CRONUS_PORTABLE_DIR` forced to
    /// this world's `global_dir()` — so the real OS-native isolation seam
    /// the product already ships (`crates/domain/src/paths.rs`) does the
    /// work, on every supported OS, with one override instead of four.
    ///
    /// Takes the parent environment explicitly rather than reading
    /// `std::env::vars()` itself, so a caller can assert this filters an
    /// arbitrary ambient variable without mutating real process environment
    /// (env mutation is process-global and races other tests).
    pub fn child_env<I>(&self, parent_env: I) -> Vec<(String, String)>
    where
        I: IntoIterator<Item = (String, String)>,
    {
        let mut env: Vec<(String, String)> = parent_env
            .into_iter()
            .filter(|(k, _)| !k.starts_with("CRONUS_"))
            .collect();
        env.push((
            "CRONUS_PORTABLE_DIR".to_string(),
            self.global_dir().display().to_string(),
        ));
        env
    }

    /// Remove the world's directory tree. Bounded retries: a just-exited
    /// child can hold a Windows file handle open for a moment after its
    /// process has ended, and a single failed `remove_dir_all` there is an
    /// environment hiccup, not a world that must be leaked.
    pub fn teardown(self) -> Result<(), WorldError> {
        remove_dir_all_retrying(&self.root, 5)
    }

    /// A snapshot digest of everything currently inside this world (`cwd/`
    /// and `global/`): every relative path present, each file's length and
    /// modification time, folded into one value. Two calls around a spawned
    /// invocation that differ mean that invocation wrote something into the
    /// world; two calls that agree mean it did not. Not a security digest —
    /// only a "did anything change here" signal, and only ever compared
    /// against another digest from the same world in the same process.
    pub fn state_digest(&self) -> io::Result<u64> {
        use std::hash::{Hash, Hasher};
        let mut entries = Vec::new();
        collect_entries(&self.root, &self.root, &mut entries)?;
        entries.sort();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        entries.hash(&mut hasher);
        Ok(hasher.finish())
    }
}

/// Recursively collect `(relative_path, len, mtime_nanos)` for every file
/// under `dir`. Missing entries (a file removed between `read_dir` and
/// `metadata` — real under concurrent activity) are skipped rather than
/// failing the whole snapshot, since a vanished file's absence is exactly
/// what the next digest should reflect.
fn collect_entries(root: &Path, dir: &Path, out: &mut Vec<(String, u64, u128)>) -> io::Result<()> {
    let read = match std::fs::read_dir(dir) {
        Ok(read) => read,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    for entry in read {
        let entry = entry?;
        let path = entry.path();
        let meta = match entry.metadata() {
            Ok(meta) => meta,
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err),
        };
        if meta.is_dir() {
            collect_entries(root, &path, out)?;
        } else {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let mtime_nanos = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            out.push((rel, meta.len(), mtime_nanos));
        }
    }
    Ok(())
}

fn remove_dir_all_retrying(path: &Path, attempts: u32) -> Result<(), WorldError> {
    if !path.exists() {
        return Ok(());
    }
    let mut last_err = None;
    for attempt in 0..attempts {
        match std::fs::remove_dir_all(path) {
            Ok(()) => return Ok(()),
            Err(err) => {
                last_err = Some(err);
                if attempt + 1 < attempts {
                    std::thread::sleep(std::time::Duration::from_millis(50 * (attempt as u64 + 1)));
                }
            }
        }
    }
    Err(WorldError::Io(last_err.expect("attempts > 0")))
}

/// Walk up from `start` looking for a `.git` entry. `None` means no
/// repository ancestor was found — callers must not treat that as "safe",
/// only as "containment cannot be checked against a repository here".
fn find_repo_root(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        if dir.join(".git").exists() {
            return Some(dir);
        }
        dir = dir.parent()?.to_path_buf();
    }
}

fn canonical_or_self(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// A digest of the repository's tracked-file dirtiness, so a later call can
/// detect whether a run left tracked files changed (contamination
/// check). Uses `git status --porcelain` output verbatim as the digest
/// rather than hashing it — small, human-diffable when a contamination
/// finding is reported, and avoids pulling in a hashing crate for a value
/// nothing here needs to be fixed-width.
///
/// Excludes `crates/simulation/tests/replays/` by pathspec: `cronus-sim
/// pin` writes a new file there *by design*, and it must run before
/// `finish` (which deletes the world `pin` reads from) — so a legitimate
/// pin always lands between the digest this function computes at world
/// creation and the one it recomputes at finish. Without the exclusion,
/// every pinned discovery would report its own run `contaminated`, the
/// exact false positive this check exists to avoid; the exclusion is
/// narrow enough that no other write, product or otherwise, is hidden by
/// it.
fn repo_dirty_digest(repo_root: &Path) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("status")
        .arg("--porcelain")
        .arg("--")
        .arg(".")
        .arg(":(exclude)crates/simulation/tests/replays")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn world_dir_name(id: &str) -> String {
    format!("cronus-sim-{id}")
}

fn sanitize(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn stamp() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos:x}-{}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_refuses_a_base_inside_the_repository() {
        let cwd = std::env::current_dir().expect("cwd");
        let repo_root = find_repo_root(&cwd).expect("this crate is checked out inside a git repo");

        let err = World::build_with_base(&repo_root, "containment-probe")
            .expect_err("a base inside the repository must be refused");
        match err {
            WorldError::Containment { repo_root: got, .. } => {
                assert_eq!(canonical_or_self(&got), canonical_or_self(&repo_root));
            }
            other => panic!("expected Containment, got {other}"),
        }

        // No directory may have been created under the refused base.
        let leaked = std::fs::read_dir(&repo_root)
            .expect("repo root readable")
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().starts_with("cronus-sim-"));
        assert!(!leaked, "containment refusal must not create a directory");
    }

    #[test]
    fn build_outside_the_repository_has_no_repo_ancestor_and_filters_cronus_vars() {
        let base = std::env::temp_dir();
        let world = World::build_with_base(&base, "clean-world").expect("build outside repo");

        let repo_root = find_repo_root(&std::env::current_dir().expect("cwd"))
            .expect("this crate is checked out inside a git repo");
        let cwd = world.cwd();
        assert!(
            !cwd.starts_with(&repo_root),
            "world cwd {cwd:?} must not sit under the repository root {repo_root:?}"
        );

        let parent_env = vec![
            ("CRONUS_MISSION_MODE".to_string(), "1".to_string()),
            ("UNRELATED_VAR".to_string(), "kept".to_string()),
        ];
        let child_env = world.child_env(parent_env);
        assert!(
            !child_env.iter().any(|(k, _)| k == "CRONUS_MISSION_MODE"),
            "CRONUS_MISSION_MODE must be absent from the recorded child environment"
        );
        assert!(
            child_env
                .iter()
                .any(|(k, v)| k == "UNRELATED_VAR" && v == "kept"),
            "a non-CRONUS_ variable must pass through unchanged"
        );
        assert!(
            child_env.iter().any(|(k, v)| k == "CRONUS_PORTABLE_DIR"
                && v == &world.global_dir().display().to_string()),
            "CRONUS_PORTABLE_DIR must be forced to this world's global_dir()"
        );

        world.teardown().expect("teardown");
    }

    #[test]
    fn teardown_removes_the_tree_and_is_idempotent() {
        let base = std::env::temp_dir();
        let world = World::build_with_base(&base, "teardown-probe").expect("build");
        let root = world.root().to_path_buf();
        assert!(root.exists());

        world.teardown().expect("first teardown");
        assert!(!root.exists());

        // A second teardown on the same (now-nonexistent) path is a no-op,
        // not an error — `remove_dir_all_retrying` short-circuits when the
        // path is already gone.
        remove_dir_all_retrying(&root, 1).expect("second teardown is a no-op");
    }
}
