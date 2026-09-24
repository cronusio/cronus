//! OS-native path resolution for the two tiers plus cache and logs.
//!
//! The immutable program tier is install-located; the mutable state tier and the
//! cache/logs roots resolve to per-OS user directories. A portable mode groups
//! every root under a single base directory.

use std::path::PathBuf;

/// The resolvable root locations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Root {
    /// Immutable program tier (compiled binaries, templates, role catalog).
    Program,
    /// Mutable state tier (config, memory, offices, hired employees).
    State,
    /// Regenerable cache.
    Cache,
    /// Runtime logs.
    Logs,
}

/// Resolves Cronus roots to OS-native paths, or under a portable base when set.
#[derive(Debug, Clone)]
pub struct Paths {
    portable_base: Option<PathBuf>,
}

/// Overrides OS-native resolution with portable mode grouped under one
/// directory — the isolation seam a test run, a CI job, or a QA pass needs
/// so the product never touches the real per-OS user directories. Every
/// caller resolves through [`Paths::os_native`], so setting this reaches
/// all of them with no call-site changes; an explicit [`Paths::portable`]
/// base still wins when a caller chooses one directly.
pub const PORTABLE_DIR_ENV: &str = "CRONUS_PORTABLE_DIR";

impl Paths {
    /// OS-native resolution (the default deployment mode) — unless
    /// [`PORTABLE_DIR_ENV`] is set to a non-empty value, in which case every
    /// root groups under that one directory instead ([`Self::portable`]).
    pub fn os_native() -> Self {
        match std::env::var(PORTABLE_DIR_ENV) {
            Ok(dir) if !dir.is_empty() => Self::portable(dir),
            _ => Self {
                portable_base: None,
            },
        }
    }

    /// Portable mode: all roots live under `base`. Bypasses
    /// [`PORTABLE_DIR_ENV`] — an explicit base always wins over the
    /// environment.
    pub fn portable(base: impl Into<PathBuf>) -> Self {
        Self {
            portable_base: Some(base.into()),
        }
    }

    /// Resolve a root to its directory path.
    pub fn resolve(&self, root: Root) -> PathBuf {
        if let Some(base) = &self.portable_base {
            let sub = match root {
                Root::Program => "program",
                Root::State => "state",
                Root::Cache => "cache",
                Root::Logs => "logs",
            };
            return base.join(sub);
        }
        self.os_native_path(root)
    }

    #[cfg(target_os = "windows")]
    fn os_native_path(&self, root: Root) -> PathBuf {
        let env = |key: &str, fallback: &str| {
            std::env::var(key)
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from(fallback))
        };
        match root {
            Root::Program => env("ProgramFiles", "C:/Program Files").join("Cronus"),
            Root::State => env("APPDATA", ".").join("Cronus"),
            Root::Cache => env("LOCALAPPDATA", ".").join("Cronus").join("Cache"),
            Root::Logs => env("LOCALAPPDATA", ".").join("Cronus").join("Logs"),
        }
    }

    #[cfg(target_os = "macos")]
    fn os_native_path(&self, root: Root) -> PathBuf {
        let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_default();
        match root {
            Root::Program => PathBuf::from("/Applications/Cronus.app/Contents/Resources"),
            Root::State => home.join("Library/Application Support/Cronus"),
            Root::Cache => home.join("Library/Caches/Cronus"),
            Root::Logs => home.join("Library/Logs/Cronus"),
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    fn os_native_path(&self, root: Root) -> PathBuf {
        let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_default();
        let xdg =
            |key: &str, default: PathBuf| std::env::var(key).map(PathBuf::from).unwrap_or(default);
        match root {
            Root::Program => PathBuf::from("/opt/cronus"),
            Root::State => xdg("XDG_DATA_HOME", home.join(".local/share")).join("cronus"),
            Root::Cache => xdg("XDG_CACHE_HOME", home.join(".cache")).join("cronus"),
            Root::Logs => xdg("XDG_STATE_HOME", home.join(".local/state")).join("cronus"),
        }
    }
}

/// The name `cronus init` seeds under a workspace root, and the marker
/// [`resolve_workspace_root_from`] searches for — the same shape
/// `crate::state::bootstrap_at` writes.
const WORKSPACE_MARKER: &str = "app.json";

/// The workspace root every semantic verb's state resolves against: the
/// nearest ancestor of the current directory that `cronus init` scaffolded
/// (marked by [`WORKSPACE_MARKER`]), falling back to the OS state tier
/// ([`Root::State`], honoring [`PORTABLE_DIR_ENV`]) for a directory tree
/// with no initialized workspace anywhere above it.
///
/// Every semantic invocable (`board`, `memory`, `role`, `schedule`,
/// `registry`, `knowledge`, `codegraph`, …) resolves its own state through
/// this — the project a user is actually standing in, rather than one
/// machine-global tier `cronus init` never wrote to. A workspace
/// that was never initialized anywhere still resolves to the same global
/// fallback every verb already used before this existed, so existing
/// machine-global data stays exactly where it was and keeps working
/// unchanged for anyone who never adopted a per-project `.cronus/`.
pub fn resolve_workspace_root() -> PathBuf {
    let cwd = std::env::current_dir().ok();
    let fallback = Paths::os_native().resolve(Root::State);
    resolve_workspace_root_from(cwd.as_deref(), fallback)
}

/// Pure resolver: the first ancestor of `start` whose `.cronus/` holds
/// [`WORKSPACE_MARKER`], else the first ancestor that holds a bare marker
/// (the OS state tier's own shape), else `fallback`. Split out so it is
/// testable without mutating the process working directory.
pub fn resolve_workspace_root_from(start: Option<&std::path::Path>, fallback: PathBuf) -> PathBuf {
    if let Some(start) = start {
        for ancestor in start.ancestors() {
            let dot = ancestor.join(".cronus");
            if dot.join(WORKSPACE_MARKER).is_file() {
                return dot;
            }
            if ancestor.join(WORKSPACE_MARKER).is_file() {
                return ancestor.to_path_buf();
            }
        }
    }
    fallback
}

/// Render a path for human output, dropping the Windows `\\?\` (and `\\?\UNC\`)
/// verbatim/extended-length prefix that `Path::canonicalize` adds. `cronus init`
/// and `workflow scaffold` reported `\\?\C:\Users\...`, which is technically the
/// same path but reads as noise and does not match what a user would type back.
///
/// On Windows, also normalizes every `/` to `\`: Windows accepts
/// both as a separator, so a path built by joining a component that
/// happened to contain a forward slash (an env-var override, a value a
/// caller supplied) displays with a visibly inconsistent mix — `backup
/// list` showed exactly this (`C:/Users/…/iso\Cronus\backups\backup-…`).
/// Display-only: this never touches the `Path`/`PathBuf` a caller goes on
/// to use for real I/O, only the string shown to a person.
pub fn display_clean(path: &std::path::Path) -> String {
    let s = path.display().to_string();
    let s = if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        s
    };
    #[cfg(target_os = "windows")]
    let s = s.replace('/', r"\");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_groups_all_roots_under_base() {
        let paths = Paths::portable("/tmp/cronus-portable");
        for root in [Root::Program, Root::State, Root::Cache, Root::Logs] {
            assert!(paths.resolve(root).starts_with("/tmp/cronus-portable"));
        }
    }

    #[test]
    fn display_clean_strips_the_windows_verbatim_prefix() {
        use std::path::Path;
        assert_eq!(display_clean(Path::new(r"\\?\C:\Users\a")), r"C:\Users\a");
        assert_eq!(
            display_clean(Path::new(r"\\?\UNC\server\share")),
            r"\\server\share"
        );
    }

    /// A path containing a `/` (a env-var override, a caller-supplied
    /// value) must display with the platform's own separator throughout,
    /// never a mix — `backup list` showed exactly the mixed form this
    /// guards against (`C:/Users/…/iso\Cronus\backups\backup-…`). Windows
    /// accepts `/` as an alternate separator with no normalization of its
    /// own; elsewhere `/` is the only separator, so there is nothing to
    /// normalize.
    #[test]
    #[cfg(target_os = "windows")]
    fn display_clean_normalizes_forward_slashes_on_windows() {
        use std::path::Path;
        assert_eq!(
            display_clean(Path::new("C:/Users/a/b/c")),
            r"C:\Users\a\b\c"
        );
        assert_eq!(
            display_clean(Path::new(r"C:/Users/a\b/c")),
            r"C:\Users\a\b\c",
            "a mix of both separators must normalize to one"
        );
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn display_clean_leaves_plain_paths_unchanged_off_windows() {
        use std::path::Path;
        assert_eq!(
            display_clean(Path::new("/plain/unix/path")),
            "/plain/unix/path"
        );
    }

    #[test]
    fn os_native_roots_are_nonempty_and_distinct() {
        let paths = Paths::os_native();
        let state = paths.resolve(Root::State);
        let cache = paths.resolve(Root::Cache);
        assert!(!state.as_os_str().is_empty());
        assert!(!cache.as_os_str().is_empty());
        assert_ne!(state, cache);
    }

    fn unique_temp_dir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "cronus-workspace-root-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ))
    }

    #[test]
    fn resolves_the_nearest_dot_cronus_ancestor_over_the_fallback() {
        let root = unique_temp_dir("ancestor");
        let nested = root.join("a").join("b");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(root.join(".cronus")).unwrap();
        std::fs::write(root.join(".cronus").join("app.json"), "{}\n").unwrap();
        let fallback = root.join("nonexistent-fallback");

        let resolved = resolve_workspace_root_from(Some(&nested), fallback.clone());
        assert_eq!(
            resolved,
            root.join(".cronus"),
            "must resolve the nearest ancestor's .cronus/, not the fallback"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn falls_back_when_no_ancestor_has_a_workspace_marker() {
        let bare = unique_temp_dir("bare");
        std::fs::create_dir_all(&bare).unwrap();
        let fallback = bare.join("nonexistent-fallback");

        assert_eq!(
            resolve_workspace_root_from(Some(&bare), fallback.clone()),
            fallback,
            "with no app.json in any ancestor, resolve to the fallback"
        );

        let _ = std::fs::remove_dir_all(&bare);
    }

    #[test]
    fn a_bare_app_json_without_dot_cronus_is_also_recognized() {
        // The OS state tier's own shape: app.json directly at the root, not
        // nested under `.cronus/` — the fallback path itself must resolve
        // this way when it is the ancestor being searched.
        let root = unique_temp_dir("bare-marker");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("app.json"), "{}\n").unwrap();
        let fallback = root.join("nonexistent-fallback");

        assert_eq!(resolve_workspace_root_from(Some(&root), fallback), root);

        let _ = std::fs::remove_dir_all(&root);
    }
}
