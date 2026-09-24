//! State-tier bootstrap: create the mutable state skeleton, idempotently.
//!
//! Only the state tier is written; the program tier is never touched.
//! Secrets are seeded as `.env.example` only — never `.env`.

use crate::paths::{Paths, Root};
use std::fs;
use std::io;
use std::path::Path;

/// Directories created in a fresh state tier (per the filesystem layout).
const STATE_DIRS: &[&str] = &[
    "memory/notes",
    "skills",
    "employees",
    "workspaces",
    "backups",
];

/// Files seeded if absent: (relative name, contents).
const STATE_FILES: &[(&str, &str)] = &[
    (
        "app.json",
        "{\n  \"locale\": \"en\",\n  \"theme\": \"system\"\n}\n",
    ),
    ("config.json", "{}\n"),
    (
        ".env.example",
        "# Local secrets — copy to .env (never committed).\n",
    ),
    ("AGENTS.md", "# Agents Instructions\n"),
];

/// Bootstrap the state tier at the resolved `Root::State`.
pub fn bootstrap(paths: &Paths) -> io::Result<()> {
    bootstrap_at(&paths.resolve(Root::State))
}

/// Bootstrap the state tier at an explicit root. Idempotent: existing files are kept.
pub fn bootstrap_at(root: &Path) -> io::Result<()> {
    fs::create_dir_all(root)?;
    for dir in STATE_DIRS {
        fs::create_dir_all(root.join(dir))?;
    }
    for (name, contents) in STATE_FILES {
        let path = root.join(name);
        if !path.exists() {
            fs::write(&path, contents)?;
        }
    }
    Ok(())
}

/// Name of the version-control exclusion file written into a project-local
/// state root.
pub const VCS_EXCLUSION_FILE: &str = ".gitignore";

/// The exclusion a project-local state root carries.
///
/// The root is office state — sessions, memory, boards, databases, backups —
/// not project content, so a checkout must not carry it and a `git add .` must
/// not sweep it up. The single `*` also excludes this file itself, so `init`
/// leaves no trace in `git status` (the convention tool caches use).
/// Configuration a team shares on purpose is opted in by name instead.
const PROJECT_LOCAL_EXCLUSION: &str = "\
# Cronus office state: sessions, memory, boards, databases, backups. It belongs
# to the office, not to the project, and must not travel with a checkout.
*
# Project configuration meant to be shared is opted in by name.
!settings.json
!commands/
!commands/**
";

/// Bootstrap a state root that lives *inside a project* (`cronus init`).
///
/// Everything [`bootstrap_at`] does, plus the version-control exclusion:
/// unlike the OS state tier, this root sits next to the project's own files,
/// where nothing else keeps it out of a commit. An existing exclusion file is
/// left exactly as it is — it may carry the user's own edits.
pub fn bootstrap_project_local(root: &Path) -> io::Result<()> {
    bootstrap_at(root)?;
    let exclusion = root.join(VCS_EXCLUSION_FILE);
    if !exclusion.exists() {
        fs::write(&exclusion, PROJECT_LOCAL_EXCLUSION)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("cronus-test-{tag}-{}", std::process::id()))
    }

    #[test]
    fn bootstrap_creates_tree_and_is_idempotent() {
        let root = temp_root("bootstrap");
        let _ = fs::remove_dir_all(&root);

        bootstrap_at(&root).expect("first bootstrap");
        assert!(root.join("memory/notes").is_dir());
        assert!(root.join("workspaces").is_dir());
        assert!(root.join(".env.example").is_file());
        assert!(!root.join(".env").exists(), "must never seed a real .env");

        // Mutate a file, then re-run: an idempotent run must not overwrite it.
        fs::write(root.join("config.json"), "{\"keep\":true}\n").unwrap();
        bootstrap_at(&root).expect("second bootstrap");
        let kept = fs::read_to_string(root.join("config.json")).unwrap();
        assert!(kept.contains("keep"), "existing files preserved");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_project_local_root_gets_a_vcs_exclusion_and_keeps_edits_to_it() {
        let root = temp_root("project-local");
        let _ = fs::remove_dir_all(&root);

        bootstrap_project_local(&root).expect("first bootstrap");
        let written = fs::read_to_string(root.join(VCS_EXCLUSION_FILE)).unwrap();
        assert!(
            written.lines().any(|l| l == "*"),
            "the root is excluded wholesale by default"
        );
        assert!(
            root.join("app.json").is_file(),
            "the state tree is still seeded"
        );

        // A user's own edit to the exclusion survives a second `init`.
        fs::write(root.join(VCS_EXCLUSION_FILE), "# mine\n").unwrap();
        bootstrap_project_local(&root).expect("second bootstrap");
        assert_eq!(
            fs::read_to_string(root.join(VCS_EXCLUSION_FILE)).unwrap(),
            "# mine\n"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_os_state_tier_gets_no_exclusion_file() {
        let root = temp_root("os-tier");
        let _ = fs::remove_dir_all(&root);
        bootstrap_at(&root).expect("bootstrap");
        assert!(!root.join(VCS_EXCLUSION_FILE).exists());
        let _ = fs::remove_dir_all(&root);
    }

    /// The exclusion has to work in git, not merely exist: state stays out of
    /// `git status`, and opted-in shared configuration stays visible. Skipped
    /// where git is not installed.
    #[test]
    fn git_ignores_office_state_but_not_opted_in_configuration() {
        use std::process::Command;

        let project = temp_root("git-effect");
        let _ = fs::remove_dir_all(&project);
        fs::create_dir_all(&project).unwrap();
        let git = |args: &[&str]| {
            Command::new("git")
                .arg("-C")
                .arg(&project)
                .args(args)
                .output()
        };
        match git(&["init", "-q"]) {
            Ok(out) if out.status.success() => {}
            _ => {
                let _ = fs::remove_dir_all(&project);
                return;
            }
        }

        let root = project.join(".cronus");
        bootstrap_project_local(&root).expect("bootstrap");
        fs::write(root.join("memory").join("memory.db"), "state").unwrap();
        fs::write(root.join("settings.json"), "{}\n").unwrap();
        fs::create_dir_all(root.join("commands")).unwrap();
        fs::write(root.join("commands").join("deploy.md"), "shared\n").unwrap();

        let out = git(&["status", "--porcelain", "--untracked-files=all"]).expect("git status");
        let status = String::from_utf8_lossy(&out.stdout).replace('\\', "/");
        let listed: Vec<&str> = status.lines().map(|l| l[3..].trim()).collect();

        assert!(
            listed.contains(&".cronus/settings.json"),
            "opted-in configuration must stay visible: {listed:?}"
        );
        assert!(
            listed.contains(&".cronus/commands/deploy.md"),
            "opted-in commands must stay visible: {listed:?}"
        );
        assert_eq!(
            listed.len(),
            2,
            "nothing else of the office state may show up: {listed:?}"
        );

        let _ = fs::remove_dir_all(&project);
    }
}
