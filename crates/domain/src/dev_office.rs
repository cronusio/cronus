//! Developer-office admission gate — the pure decision core.
//!
//! Resolves what capability tier, if any, the developer office exposes, from
//! two already-computed inputs: whether the working tree is a genuine
//! checkout of the canonical repository, and whether a human has admitted
//! the operator on the write-only admission plane. Pure and I/O-free — no
//! git read, no filesystem probe, no auth-plane query happens here — so the
//! resolution table is testable against fabricated inputs with no real repo
//! present. The facade (`crates/core`) performs the real reads and calls
//! [`DevOfficeGate::resolve`] with the results.

/// The outcome of the repository-authenticity check: whether the
/// current working tree is a verified checkout of the canonical upstream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoAuthenticity {
    /// A local worktree marker resolves to an unambiguous canonical-upstream match.
    Genuine { upstream: String },
    /// A worktree marker exists but the bound upstream is not the canonical
    /// one — including an ambiguous multi-remote config with no unambiguous
    /// match, which fails closed to this variant rather than `Genuine`.
    NotCanonical,
    /// No local worktree marker at all.
    NotARepo,
}

/// The capability ceiling the developer office resolves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionTier {
    /// No surface renders — the default for a normal install.
    Absent,
    /// Report/improve surface only; opt-in via `feedback_tier_enabled`, default off.
    Feedback,
    /// The full self-maintenance developer office.
    Elevated,
}

/// Inputs to [`DevOfficeGate::resolve`], already computed by the facade so
/// the gate itself stays I/O-free.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateInputs {
    pub repo: RepoAuthenticity,
    /// Read from the human-write-only admission plane via [`AdmissionReader`].
    pub admitted: bool,
    /// Deploy/build flag; default `false`.
    pub feedback_tier_enabled: bool,
}

/// Read-only view onto the human-write-only admission plane.
///
/// No mint/write method exists on this trait. The domain gate depends only
/// on this port, so no path reachable through domain logic can mint or
/// escalate an admission — the capability is unrepresentable at the type
/// level here, not merely guarded at runtime. The write side (mint/revoke)
/// is exposed only on the human-principal auth surface, a plane this crate
/// has no dependency edge to.
pub trait AdmissionReader {
    fn is_admitted(&self) -> bool;
}

/// The developer-office admission gate.
pub struct DevOfficeGate;

impl DevOfficeGate {
    /// Resolve the admission tier from already-computed inputs.
    ///
    /// `Elevated` is reachable only through the `(Genuine, admitted)` arm —
    /// the repository check is a hard precondition, not a bonus signal, so an
    /// admission credential carried into a non-canonical or absent tree
    /// resolves to `Absent` (never `Feedback`, never `Elevated`) rather than
    /// granting anything.
    pub fn resolve(inp: &GateInputs) -> AdmissionTier {
        match (&inp.repo, inp.admitted) {
            (RepoAuthenticity::Genuine { .. }, true) => AdmissionTier::Elevated,
            // Not admitted (or not in the canonical repo): the office is
            // absent unless a deployment opted the feedback ceiling on.
            _ if inp.feedback_tier_enabled && !inp.admitted => AdmissionTier::Feedback,
            _ => AdmissionTier::Absent,
        }
    }
}

/// The kind of workspace a floor entry represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceKind {
    /// The ordinary, user-created kind reachable through the workspace-creation flow.
    Project,
    /// The sole system-owned kind: materializes only while the gate
    /// resolves to `Elevated`, never through user action.
    Developer,
}

impl WorkspaceKind {
    /// Whether the engine owns and maintains a workspace of this kind
    /// itself, rather than a user.
    pub fn is_system_owned(self) -> bool {
        matches!(self, WorkspaceKind::Developer)
    }

    /// The maximum number of workspaces of this kind that may exist at
    /// once. `None` means unbounded.
    pub fn max_instances(self) -> Option<u8> {
        match self {
            WorkspaceKind::Developer => Some(1),
            WorkspaceKind::Project => None,
        }
    }

    /// Whether a workspace of this kind can be deleted through the ordinary
    /// workspace-management surface.
    pub fn is_deletable(self) -> bool {
        matches!(self, WorkspaceKind::Project)
    }

    /// Whether this kind is reachable through the project-creation flow.
    /// `Developer` is never user-instantiable — its floor materializes only
    /// via [`DevOfficeGate::resolve`] resolving to `Elevated`.
    pub fn is_project_creatable(self) -> bool {
        matches!(self, WorkspaceKind::Project)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(repo: RepoAuthenticity, admitted: bool, feedback_tier_enabled: bool) -> GateInputs {
        GateInputs {
            repo,
            admitted,
            feedback_tier_enabled,
        }
    }

    #[test]
    fn genuine_and_admitted_resolves_elevated() {
        let inp = inputs(
            RepoAuthenticity::Genuine {
                upstream: "canonical".to_string(),
            },
            true,
            false,
        );
        assert_eq!(DevOfficeGate::resolve(&inp), AdmissionTier::Elevated);
    }

    #[test]
    fn genuine_and_admitted_resolves_elevated_even_with_feedback_tier_on() {
        // Elevated is strictly stronger than Feedback; the deploy flag never
        // downgrades an already-admitted developer in the canonical repo.
        let inp = inputs(
            RepoAuthenticity::Genuine {
                upstream: "canonical".to_string(),
            },
            true,
            true,
        );
        assert_eq!(DevOfficeGate::resolve(&inp), AdmissionTier::Elevated);
    }

    #[test]
    fn admitted_credential_in_non_canonical_tree_resolves_absent() {
        let inp = inputs(RepoAuthenticity::NotCanonical, true, false);
        assert_eq!(DevOfficeGate::resolve(&inp), AdmissionTier::Absent);
    }

    #[test]
    fn admitted_credential_outside_any_repo_resolves_absent() {
        let inp = inputs(RepoAuthenticity::NotARepo, true, false);
        assert_eq!(DevOfficeGate::resolve(&inp), AdmissionTier::Absent);
    }

    #[test]
    fn admitted_credential_in_wrong_tree_never_downgrades_to_feedback() {
        // A credential does not buy the feedback ceiling either — the
        // resolve arms require `!admitted` for that path, so a misplaced
        // credential grants nothing at all rather than a lesser tier.
        let inp = inputs(RepoAuthenticity::NotCanonical, true, true);
        assert_eq!(DevOfficeGate::resolve(&inp), AdmissionTier::Absent);
    }

    #[test]
    fn feedback_tier_off_by_default_not_admitted_resolves_absent() {
        let inp = inputs(RepoAuthenticity::NotARepo, false, false);
        assert_eq!(DevOfficeGate::resolve(&inp), AdmissionTier::Absent);
    }

    #[test]
    fn feedback_tier_enabled_not_admitted_resolves_feedback() {
        let inp = inputs(RepoAuthenticity::NotARepo, false, true);
        assert_eq!(DevOfficeGate::resolve(&inp), AdmissionTier::Feedback);
    }

    #[test]
    fn feedback_tier_enabled_not_admitted_in_genuine_repo_still_resolves_feedback() {
        // The feedback ceiling does not require the canonical repo — it is
        // meant for ordinary installs too.
        let inp = inputs(
            RepoAuthenticity::Genuine {
                upstream: "canonical".to_string(),
            },
            false,
            true,
        );
        assert_eq!(DevOfficeGate::resolve(&inp), AdmissionTier::Feedback);
    }

    #[test]
    fn developer_workspace_kind_is_system_owned_capped_and_non_deletable() {
        assert!(WorkspaceKind::Developer.is_system_owned());
        assert_eq!(WorkspaceKind::Developer.max_instances(), Some(1));
        assert!(!WorkspaceKind::Developer.is_deletable());
        assert!(!WorkspaceKind::Developer.is_project_creatable());
    }

    #[test]
    fn project_workspace_kind_is_user_owned_unbounded_and_creatable() {
        assert!(!WorkspaceKind::Project.is_system_owned());
        assert_eq!(WorkspaceKind::Project.max_instances(), None);
        assert!(WorkspaceKind::Project.is_deletable());
        assert!(WorkspaceKind::Project.is_project_creatable());
    }

    /// A fake reader that scripts a fixed answer — proves the gate only ever
    /// calls the read-only `is_admitted` method, with no way to mint through it.
    struct FakeReader(bool);

    impl AdmissionReader for FakeReader {
        fn is_admitted(&self) -> bool {
            self.0
        }
    }

    #[test]
    fn admission_reader_is_a_pure_read_port() {
        let admitted = FakeReader(true);
        let not_admitted = FakeReader(false);
        assert!(admitted.is_admitted());
        assert!(!not_admitted.is_admitted());
    }
}
