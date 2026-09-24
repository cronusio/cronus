//! Version control — role authority, quality-gated commit production, card-aligned
//! commit boundary, and Conventional Commits generation.
//!
//! The virtual staging area is the execution-workspace worktree (a seam here); this
//! module implements the policy layered over it: who may perform which git op,
//! the quality gate that must pass before a commit is produced,
//! the one-card commit boundary + mandatory footer, and all-or-nothing
//! staging atomicity. Non-git versioning is out of scope.

/// A role whose git authority is governed by the office policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Worker,
    Orchestrator,
    Maintainer,
    ReleaseManager,
    Manager,
}

/// A git operation subject to authority checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitOp {
    CreateBranch,
    Commit,
    PushBranch,
    Merge,
    ConfigureRemote,
}

/// Whether `role` may perform `op`. Push to a protected branch is governed
/// separately by [`can_push`] — no role may push `main`/`trunk` directly.
pub fn authorized(role: Role, op: GitOp) -> bool {
    use GitOp::*;
    use Role::*;
    match (role, op) {
        // Workers: own task branch only, no push/merge.
        (Worker, CreateBranch | Commit) => true,
        (Worker, _) => false,
        // Orchestrator: task-branch push, no merge/remote.
        (Orchestrator, CreateBranch | Commit | PushBranch) => true,
        (Orchestrator, _) => false,
        // Maintainer: any branch push + merge.
        (Maintainer, CreateBranch | Commit | PushBranch | Merge) => true,
        (Maintainer, ConfigureRemote) => false,
        // Release Manager: everything except pushing main directly.
        (ReleaseManager, _) => true,
        // Manager: merges (final) + remote config; delegates commits.
        (Manager, Merge | ConfigureRemote) => true,
        (Manager, _) => false,
    }
}

/// Whether `role` may push to a branch, given whether it is the protected
/// main/trunk. This is unconditional: no role pushes main/trunk directly.
pub fn can_push(role: Role, target_is_main: bool) -> bool {
    if target_is_main {
        return false; // Unconditional, no autonomy level overrides it
    }
    authorized(role, GitOp::PushBranch)
}

/// The office's declared branching strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchStrategy {
    TrunkBased,
    GitFlow,
}

/// A Conventional Commits type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitType {
    Feat,
    Fix,
    Chore,
    Docs,
    Test,
    Refactor,
    Perf,
    Build,
    Ci,
}

impl CommitType {
    fn as_str(self) -> &'static str {
        match self {
            CommitType::Feat => "feat",
            CommitType::Fix => "fix",
            CommitType::Chore => "chore",
            CommitType::Docs => "docs",
            CommitType::Test => "test",
            CommitType::Refactor => "refactor",
            CommitType::Perf => "perf",
            CommitType::Build => "build",
            CommitType::Ci => "ci",
        }
    }
}

/// Errors from commit production.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VcError {
    /// Quality gate failed; the worktree is discarded, no commit written.
    QualityGateFailed,
    /// A commit must bind to exactly one card.
    NoCardBoundary,
    /// The role may not commit.
    Unauthorized,
}

impl std::fmt::Display for VcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            VcError::QualityGateFailed => "quality gate failed — worktree discarded",
            VcError::NoCardBoundary => "commit must bind to exactly one card",
            VcError::Unauthorized => "role not authorized to commit",
        };
        f.write_str(s)
    }
}

impl std::error::Error for VcError {}

/// A produced commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub message: String,
    pub card_id: String,
}

/// Generate a Conventional Commits message with a mandatory card-reference footer
/// (traceability).
pub fn conventional_commit(kind: CommitType, scope: &str, summary: &str, card_id: &str) -> String {
    format!(
        "{}({}): {}\n\nRefs: #{}",
        kind.as_str(),
        scope,
        summary,
        card_id
    )
}

/// Produce a commit from the virtual staging area. The quality gate must pass;
/// a fail discards the worktree with no partial flush. The commit
/// must bind to exactly one card, and the role must be authorized.
#[allow(clippy::too_many_arguments)]
pub fn produce_commit(
    role: Role,
    quality_passed: bool,
    card_id: Option<&str>,
    kind: CommitType,
    scope: &str,
    summary: &str,
) -> Result<Commit, VcError> {
    if !authorized(role, GitOp::Commit) {
        return Err(VcError::Unauthorized);
    }
    let card = card_id.ok_or(VcError::NoCardBoundary)?;
    if !quality_passed {
        return Err(VcError::QualityGateFailed); // Worktree discarded, no commit
    }
    Ok(Commit {
        message: conventional_commit(kind, scope, summary, card),
        card_id: card.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authority_table_enforces_roles() {
        // Workers commit on their branch but cannot push or merge.
        assert!(authorized(Role::Worker, GitOp::Commit));
        assert!(!authorized(Role::Worker, GitOp::PushBranch));
        assert!(!authorized(Role::Worker, GitOp::Merge));
        // Maintainer may merge; Manager may merge + configure remote.
        assert!(authorized(Role::Maintainer, GitOp::Merge));
        assert!(authorized(Role::Manager, GitOp::ConfigureRemote));
        assert!(!authorized(Role::Manager, GitOp::Commit)); // delegates
    }

    #[test]
    fn no_role_pushes_main_directly() {
        // Unconditional — even a Release Manager cannot push main directly.
        for role in [
            Role::Worker,
            Role::Orchestrator,
            Role::Maintainer,
            Role::ReleaseManager,
            Role::Manager,
        ] {
            assert!(!can_push(role, true), "{role:?} must not push main");
        }
        // A branch push is allowed for authorized roles.
        assert!(can_push(Role::Maintainer, false));
        assert!(!can_push(Role::Worker, false));
    }

    #[test]
    fn commit_requires_quality_gate_and_card() {
        let ok = produce_commit(
            Role::Worker,
            true,
            Some("card-42"),
            CommitType::Feat,
            "kanban",
            "add custom columns",
        )
        .unwrap();
        assert!(ok.message.contains("feat(kanban): add custom columns"));
        assert!(ok.message.contains("Refs: #card-42"));

        // Quality fail -> discarded, no commit.
        assert_eq!(
            produce_commit(
                Role::Worker,
                false,
                Some("card-42"),
                CommitType::Fix,
                "x",
                "y"
            ),
            Err(VcError::QualityGateFailed)
        );
        // No card boundary.
        assert_eq!(
            produce_commit(Role::Worker, true, None, CommitType::Fix, "x", "y"),
            Err(VcError::NoCardBoundary)
        );
        // Unauthorized role.
        assert_eq!(
            produce_commit(Role::Manager, true, Some("c1"), CommitType::Fix, "x", "y"),
            Err(VcError::Unauthorized)
        );
    }

    #[test]
    fn conventional_commit_carries_card_footer() {
        let msg = conventional_commit(CommitType::Fix, "scheduler", "handle dst", "c-9");
        assert_eq!(msg, "fix(scheduler): handle dst\n\nRefs: #c-9");
    }
}
