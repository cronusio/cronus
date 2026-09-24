//! Facade wiring for the developer office: the real git/filesystem read the
//! pure `dev_office` domain gate depends on but never performs itself
//! (repository-authenticity), the admission read over
//! `cronus-auth-local`, and the trigger-loaded module wiring
//! — the tier model has no edge from `domain` to either adapter, so all
//! three live here. Local-only — this module never opens a socket.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use cronus_auth_local::DeveloperAdmissionStore;
use cronus_domain::dev_office::{AdmissionReader, AdmissionTier, RepoAuthenticity};
use cronus_domain::extensions::{
    ExtensionKind, ExtensionManifest, ExtensionPermissions, ExtensionRegistry, ExtensionSource,
    ExtensionState,
};

/// The upstream identity a checkout must be bound to for the developer
/// office to consider it genuine. Compiled into the build — not user
/// config — so a user cannot retarget the office by editing a file; a fork
/// that wants its own dev office rebuilds with its own identity here.
const CANONICAL_UPSTREAM: &str = "https://github.com/cronusio/cronus";

/// Resolve whether `cwd` sits inside a genuine checkout of the canonical
/// upstream. Walks upward from `cwd` for the nearest worktree marker, reads
/// its bound remote from the local git config, and compares it against
/// [`CANONICAL_UPSTREAM`]. Conservative on ambiguity: a multi-remote config
/// with no single unambiguous candidate resolves `NotCanonical`, matching
/// fail-closed handling elsewhere in this gate.
pub fn repo_authenticity(cwd: &Path) -> RepoAuthenticity {
    let Some(git_dir) = find_worktree_marker(cwd) else {
        return RepoAuthenticity::NotARepo;
    };
    match read_bound_upstream(&git_dir) {
        Some(u) if upstream_matches_canonical(&u, CANONICAL_UPSTREAM) => {
            RepoAuthenticity::Genuine { upstream: u }
        }
        _ => RepoAuthenticity::NotCanonical,
    }
}

/// Walk upward from `start` for the nearest `.git` worktree marker,
/// resolving a linked-worktree/submodule `.git` file (`gitdir: <path>`) to
/// its real git directory. Returns `None` if no marker is found before the
/// filesystem root.
fn find_worktree_marker(start: &Path) -> Option<PathBuf> {
    let mut dir = start;
    loop {
        if let Some(git_dir) = resolve_git_dir(&dir.join(".git")) {
            return Some(git_dir);
        }
        dir = dir.parent()?;
    }
}

fn resolve_git_dir(marker: &Path) -> Option<PathBuf> {
    if marker.is_dir() {
        return Some(marker.to_path_buf());
    }
    if marker.is_file() {
        let contents = fs::read_to_string(marker).ok()?;
        let target = contents.trim().strip_prefix("gitdir:")?.trim();
        let target_path = Path::new(target);
        let resolved = if target_path.is_absolute() {
            target_path.to_path_buf()
        } else {
            marker.parent()?.join(target_path)
        };
        if resolved.is_dir() {
            return Some(resolved);
        }
    }
    None
}

/// The single "bound" remote URL, read from `<git_dir>/config`. A remote
/// named `origin` wins even alongside other remotes (the git convention for
/// the primary remote); with no `origin` present, exactly one remote is
/// still unambiguous; two or more non-`origin` remotes have no unambiguous
/// candidate and this returns `None`.
fn read_bound_upstream(git_dir: &Path) -> Option<String> {
    let text = fs::read_to_string(git_dir.join("config")).ok()?;
    let mut by_name: BTreeMap<String, String> = BTreeMap::new();
    for (name, url) in parse_remote_urls(&text) {
        by_name.insert(name, url);
    }
    match by_name.len() {
        0 => None,
        1 => by_name.into_values().next(),
        _ => by_name.remove("origin"),
    }
}

/// Extract `(remote_name, url)` pairs from git-config text by scanning
/// `[remote "name"]` section headers and the `url = ...` line within each.
fn parse_remote_urls(config_text: &str) -> Vec<(String, String)> {
    let mut remotes = Vec::new();
    let mut current: Option<String> = None;
    for raw_line in config_text.lines() {
        let line = raw_line.trim();
        if let Some(inner) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            current = parse_remote_section(inner);
            continue;
        }
        if let Some(name) = &current
            && let Some((key, value)) = line.split_once('=')
            && key.trim() == "url"
        {
            remotes.push((name.clone(), value.trim().to_string()));
        }
    }
    remotes
}

/// Parse a `remote "name"` section header body (the text between `[` and
/// `]`) into the remote's name, or `None` for any other section.
fn parse_remote_section(inner: &str) -> Option<String> {
    let (section, rest) = inner.split_once(char::is_whitespace)?;
    if section != "remote" {
        return None;
    }
    Some(rest.trim().trim_matches('"').to_string())
}

/// Compare a candidate remote URL against the canonical identity after
/// normalizing scheme, SSH shorthand (`git@host:owner/repo`), a trailing
/// `.git`, a trailing slash, and case — so `git@github.com:cronusio/cronus.git`
/// and `https://github.com/cronusio/cronus` are recognized as the same
/// identity rather than requiring one exact clone-URL spelling.
fn upstream_matches_canonical(candidate: &str, canonical: &str) -> bool {
    normalize_git_url(candidate) == normalize_git_url(canonical)
}

fn normalize_git_url(url: &str) -> String {
    let mut s = url.trim();
    for scheme in ["https://", "http://", "ssh://", "git://"] {
        if let Some(rest) = s.strip_prefix(scheme) {
            s = rest;
            break;
        }
    }
    let mut owned = match s.split_once(':') {
        // `git@host:owner/repo` shorthand: the part before `:` has no `/`.
        Some((host_part, path)) if !host_part.contains('/') => {
            let host = host_part.rsplit('@').next().unwrap_or(host_part);
            format!("{host}/{path}")
        }
        _ => s.to_string(),
    };
    if let Some(stripped) = owned.strip_suffix('/') {
        owned = stripped.to_string();
    }
    if let Some(stripped) = owned.strip_suffix(".git") {
        owned = stripped.to_string();
    }
    owned.to_lowercase()
}

/// Wraps the `cronus-auth-local` admission store as the domain gate's
/// read-only [`AdmissionReader`] port — the facade-side half of the
/// "admission read over auth-local" split. Exposes only the read method;
/// minting/revoking stays on [`DeveloperAdmissionStore`] itself, reachable
/// only via a `HumanPrincipal` a human-operated entry point constructs.
pub struct AuthLocalAdmissionReader {
    store: DeveloperAdmissionStore,
}

impl AuthLocalAdmissionReader {
    /// Open a reader at an explicit path. This facade function holds no
    /// path-resolution logic of its own — the caller (the CLI entry point)
    /// resolves the real state-tier location, the `knowledge_bootstrap`
    /// `open_default(path)` precedent.
    pub fn open(path: impl Into<PathBuf>) -> Self {
        AuthLocalAdmissionReader {
            store: DeveloperAdmissionStore::open(path),
        }
    }
}

impl AdmissionReader for AuthLocalAdmissionReader {
    fn is_admitted(&self) -> bool {
        self.store.is_admitted()
    }
}

const DEV_OFFICE_MODULE_ID: &str = "dev-office";

/// The dev office's trigger-loaded module. Composes the shared
/// existing extensions loader instead of inventing a parallel one — the module
/// is a single `ExtensionRegistry` entry toggled Active/Inactive by
/// [`DevOfficeModule::sync`].
pub struct DevOfficeModule {
    registry: ExtensionRegistry,
}

impl DevOfficeModule {
    /// Register the dev-office entry (`Discovered` → `Permitted`, the
    /// loader's mandatory first edge) but do not load it — `sync` performs
    /// the first real load once the trigger is actually observed `Elevated`.
    pub fn new() -> Self {
        let mut registry = ExtensionRegistry::new();
        registry
            .register(ExtensionManifest {
                id: DEV_OFFICE_MODULE_ID.to_string(),
                kind: ExtensionKind::Plugin,
                name: "Developer Office".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                source: ExtensionSource::Preset,
                capabilities: Vec::new(),
                permissions: ExtensionPermissions::default(),
            })
            .expect("the static dev-office manifest is always valid");
        registry
            .transition(DEV_OFFICE_MODULE_ID, ExtensionState::Permitted)
            .expect("Discovered -> Permitted is always a valid first edge");
        DevOfficeModule { registry }
    }

    /// Whether the module is currently loaded — the elevated floor is
    /// present only while this is `true`.
    pub fn is_loaded(&self) -> bool {
        self.registry.state(DEV_OFFICE_MODULE_ID) == Some(ExtensionState::Active)
    }

    /// Re-evaluate the trigger and load/unload to match it. **Never caches**
    /// a remembered "was elevated" value (observe, don't remember,
    /// mirroring background-activation's rule) — the caller re-derives
    /// `tier` fresh from [`cronus_domain::dev_office::DevOfficeGate::resolve`]
    /// on every input-changing event (app start, connection trigger,
    /// admission mint/revoke, workspace change) and passes it in here; this
    /// method holds no state of its own beyond the loader's current
    /// Active/Inactive fact. A same-tier resync is a no-op, never an error.
    pub fn sync(&mut self, tier: AdmissionTier) {
        let should_load = tier == AdmissionTier::Elevated;
        match (should_load, self.is_loaded()) {
            (true, false) => self
                .registry
                .transition(DEV_OFFICE_MODULE_ID, ExtensionState::Active)
                .expect("Permitted/Inactive -> Active is always a valid edge here"),
            (false, true) => self
                .registry
                .transition(DEV_OFFICE_MODULE_ID, ExtensionState::Inactive)
                .expect("Active -> Inactive is always a valid edge"),
            _ => {}
        }
    }
}

impl Default for DevOfficeModule {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_repo(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("cronus-dev-office-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".git")).unwrap();
        dir
    }

    fn write_config(repo: &Path, body: &str) {
        fs::write(repo.join(".git").join("config"), body).unwrap();
    }

    #[test]
    fn canonical_origin_resolves_genuine() {
        let repo = temp_repo("genuine");
        write_config(
            &repo,
            "[core]\n\trepositoryformatversion = 0\n[remote \"origin\"]\n\turl = https://github.com/cronusio/cronus.git\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n",
        );
        assert_eq!(
            repo_authenticity(&repo),
            RepoAuthenticity::Genuine {
                upstream: "https://github.com/cronusio/cronus.git".to_string()
            }
        );
    }

    #[test]
    fn ssh_form_of_canonical_origin_still_resolves_genuine() {
        let repo = temp_repo("ssh-form");
        write_config(
            &repo,
            "[remote \"origin\"]\n\turl = git@github.com:cronusio/cronus.git\n",
        );
        assert!(matches!(
            repo_authenticity(&repo),
            RepoAuthenticity::Genuine { .. }
        ));
    }

    #[test]
    fn non_canonical_origin_resolves_not_canonical() {
        let repo = temp_repo("non-canonical");
        write_config(
            &repo,
            "[remote \"origin\"]\n\turl = https://github.com/someone-else/fork.git\n",
        );
        assert_eq!(repo_authenticity(&repo), RepoAuthenticity::NotCanonical);
    }

    #[test]
    fn no_git_marker_resolves_not_a_repo() {
        let repo =
            std::env::temp_dir().join(format!("cronus-dev-office-no-git-{}", std::process::id()));
        let _ = fs::remove_dir_all(&repo);
        fs::create_dir_all(&repo).unwrap();
        assert_eq!(repo_authenticity(&repo), RepoAuthenticity::NotARepo);
    }

    #[test]
    fn ambiguous_multi_remote_with_no_origin_resolves_not_canonical() {
        let repo = temp_repo("ambiguous");
        write_config(
            &repo,
            "[remote \"a\"]\n\turl = https://github.com/cronusio/cronus.git\n[remote \"b\"]\n\turl = https://github.com/someone-else/fork.git\n",
        );
        // Two remotes, neither named `origin`: no unambiguous bound upstream,
        // fail-closed even though one of them happens to match canonical.
        assert_eq!(repo_authenticity(&repo), RepoAuthenticity::NotCanonical);
    }

    #[test]
    fn multi_remote_with_origin_present_still_resolves_via_origin() {
        let repo = temp_repo("multi-with-origin");
        write_config(
            &repo,
            "[remote \"origin\"]\n\turl = https://github.com/cronusio/cronus.git\n[remote \"fork\"]\n\turl = https://github.com/someone-else/fork.git\n",
        );
        assert!(matches!(
            repo_authenticity(&repo),
            RepoAuthenticity::Genuine { .. }
        ));
    }

    #[test]
    fn empty_config_resolves_not_canonical() {
        let repo = temp_repo("empty-config");
        write_config(&repo, "[core]\n\trepositoryformatversion = 0\n");
        assert_eq!(repo_authenticity(&repo), RepoAuthenticity::NotCanonical);
    }

    #[test]
    fn subdirectory_of_a_genuine_repo_still_resolves_genuine() {
        let repo = temp_repo("subdir");
        write_config(
            &repo,
            "[remote \"origin\"]\n\turl = https://github.com/cronusio/cronus\n",
        );
        let nested = repo.join("crates").join("domain").join("src");
        fs::create_dir_all(&nested).unwrap();
        assert!(matches!(
            repo_authenticity(&nested),
            RepoAuthenticity::Genuine { .. }
        ));
    }

    fn temp_admission_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "cronus-dev-office-admission-{tag}-{}.txt",
            std::process::id()
        ))
    }

    #[test]
    fn reader_reflects_not_admitted_before_any_mint() {
        let reader = AuthLocalAdmissionReader::open(temp_admission_path("fresh"));
        assert!(!reader.is_admitted());
    }

    #[test]
    fn reader_reflects_mint_then_revoke_through_the_read_only_port() {
        // Exercises the read side only through the `AdmissionReader` trait
        // object — proving the facade wrapper is a genuine port impl, not
        // just a struct with a same-named method.
        let path = temp_admission_path("mint-revoke");
        let store = DeveloperAdmissionStore::open(&path);
        let human = cronus_auth_local::HumanPrincipal::assert_human_operated();

        let reader: &dyn AdmissionReader = &AuthLocalAdmissionReader::open(&path);
        assert!(!reader.is_admitted());

        store.mint(&human).unwrap();
        let reader: &dyn AdmissionReader = &AuthLocalAdmissionReader::open(&path);
        assert!(reader.is_admitted());

        store.revoke(&human).unwrap();
        let reader: &dyn AdmissionReader = &AuthLocalAdmissionReader::open(&path);
        assert!(!reader.is_admitted());
    }

    #[test]
    fn a_fresh_module_is_registered_but_not_loaded() {
        let module = DevOfficeModule::new();
        assert!(!module.is_loaded());
    }

    #[test]
    fn syncing_elevated_loads_the_module() {
        let mut module = DevOfficeModule::new();
        module.sync(AdmissionTier::Elevated);
        assert!(module.is_loaded());
    }

    #[test]
    fn syncing_away_from_elevated_unloads_cleanly() {
        let mut module = DevOfficeModule::new();
        module.sync(AdmissionTier::Elevated);
        assert!(module.is_loaded());

        // Simulates admission revoked or the cwd leaving the canonical repo:
        // either way the gate no longer resolves Elevated.
        module.sync(AdmissionTier::Absent);
        assert!(
            !module.is_loaded(),
            "no stale elevated surface after unload"
        );
    }

    #[test]
    fn feedback_tier_never_counts_as_loaded() {
        let mut module = DevOfficeModule::new();
        module.sync(AdmissionTier::Elevated);
        module.sync(AdmissionTier::Feedback);
        assert!(
            !module.is_loaded(),
            "Feedback is not Elevated — the module must unload, not stay loaded at a lesser tier"
        );
    }

    #[test]
    fn repeated_resync_at_the_same_tier_is_idempotent() {
        // Guards the (Active, Active) / (Permitted, Active)-already-handled
        // no-op path: a same-tier resync must never hit an invalid-transition
        // panic in `ExtensionRegistry::transition`.
        let mut module = DevOfficeModule::new();
        module.sync(AdmissionTier::Elevated);
        module.sync(AdmissionTier::Elevated);
        module.sync(AdmissionTier::Elevated);
        assert!(module.is_loaded());

        module.sync(AdmissionTier::Absent);
        module.sync(AdmissionTier::Absent);
        assert!(!module.is_loaded());
    }

    #[test]
    fn the_gate_result_is_never_cached_across_alternating_syncs() {
        // Observe, don't remember: the module must track live re-evaluated
        // input, not a stored "was elevated" flag — proven by toggling
        // several times and checking `is_loaded()` reflects each toggle.
        let mut module = DevOfficeModule::new();
        let sequence = [
            AdmissionTier::Elevated,
            AdmissionTier::Absent,
            AdmissionTier::Feedback,
            AdmissionTier::Elevated,
            AdmissionTier::Absent,
        ];
        for tier in sequence {
            module.sync(tier);
            assert_eq!(module.is_loaded(), tier == AdmissionTier::Elevated);
        }
    }
}
