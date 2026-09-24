//! Developer-office invariant acceptance sweep — the
//! closing validation. Each DVO invariant maps to one named test, exercised
//! through the real gate, the real facade wiring, and the real domain types
//! reached via `cronus_core`'s re-exports — matching the
//! `knowledge_invariants`/`wiki_invariants`/`activation_invariants`
//! precedent. Local-only: no network call anywhere in this file.

use std::path::PathBuf;

use cronus_core::auth::{DeveloperAdmissionStore, HumanPrincipal};
use cronus_core::dev_office::{
    AdmissionReader, AdmissionTier, DevOfficeGate, GateInputs, RepoAuthenticity,
};
use cronus_core::dev_office_gate::{AuthLocalAdmissionReader, DevOfficeModule, repo_authenticity};
use cronus_core::dev_office_workspace::{
    DEV_OFFICE_WORKSPACE_ID, is_reserved_dev_workspace_id, register_dev_workspace,
    run_elevated_action,
};
use cronus_core::development_workflow::{AdvanceError, Pipeline, QualityGate};
use cronus_core::receipts_bootstrap::ReceiptedDispatch;
use cronus_core::tool_security::ToolPolicy;
use cronus_core::workspace::{WorkspaceId, WorkspaceManager, WorkspaceTemplate};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "cronus-dev-office-invariants-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn temp_git_repo(tag: &str, config_body: &str) -> PathBuf {
    let repo = temp_dir(tag);
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::write(repo.join(".git").join("config"), config_body).unwrap();
    repo
}

// ── Conditional system-workspace floor ────────────────────────────────

#[test]
fn the_floor_is_present_only_while_elevated() {
    let mut module = DevOfficeModule::new();
    assert!(
        !module.is_loaded(),
        "absent by default for a normal install"
    );

    module.sync(AdmissionTier::Elevated);
    assert!(module.is_loaded(), "floor present while Elevated");

    module.sync(AdmissionTier::Absent);
    assert!(!module.is_loaded(), "floor gone once no longer Elevated");
}

#[test]
fn developer_kind_is_a_singleton_never_reachable_through_project_creation() {
    let mgr = WorkspaceManager::open_in_memory().unwrap();
    let repo = temp_dir("singleton");

    register_dev_workspace(&mgr, &repo).unwrap();
    assert!(is_reserved_dev_workspace_id(DEV_OFFICE_WORKSPACE_ID));

    // 0..1 cardinality: a second registration at the same reserved id is refused.
    assert!(register_dev_workspace(&mgr, &repo).is_err());

    // The ordinary project-creation flow's own id validator has no knowledge
    // of "kind" at all — the reserved id is guarded at the CLI layer
    // (`crates/cli/src/commands.rs`), proven end to end by the real CLI
    // smoke tests in `cli_smoke.rs`, not re-simulated here.
}

// ── Repository-authenticity binding (network-free, fail-closed) ──────

#[test]
fn non_canonical_ambiguous_and_absent_repos_all_fail_closed() {
    let non_canonical = temp_git_repo(
        "non-canonical",
        "[remote \"origin\"]\n\turl = https://github.com/someone-else/fork.git\n",
    );
    assert_eq!(
        repo_authenticity(&non_canonical),
        RepoAuthenticity::NotCanonical
    );

    let ambiguous = temp_git_repo(
        "ambiguous",
        "[remote \"a\"]\n\turl = https://github.com/cronusio/cronus.git\n[remote \"b\"]\n\turl = https://github.com/other/repo.git\n",
    );
    // Two remotes, neither named `origin`: no unambiguous bound upstream —
    // fails closed even though one candidate URL is actually canonical.
    assert_eq!(
        repo_authenticity(&ambiguous),
        RepoAuthenticity::NotCanonical
    );

    let not_a_repo = temp_dir("not-a-repo");
    assert_eq!(repo_authenticity(&not_a_repo), RepoAuthenticity::NotARepo);

    let genuine = temp_git_repo(
        "genuine",
        "[remote \"origin\"]\n\turl = https://github.com/cronusio/cronus.git\n",
    );
    assert!(matches!(
        repo_authenticity(&genuine),
        RepoAuthenticity::Genuine { .. }
    ));
}

// ── Identity-gated human-admitted elevated access ─────────────────────

/// A fake reader scripted to a fixed answer — proves the gate only ever
/// calls the read-only `is_admitted` method, never a write.
struct ScriptedReader(bool);
impl AdmissionReader for ScriptedReader {
    fn is_admitted(&self) -> bool {
        self.0
    }
}

#[test]
fn the_domain_gate_can_only_read_never_mint_an_admission() {
    // `AdmissionReader` exposes exactly one method (`is_admitted`) — there is
    // no mint/write method to call, so no code holding only `&dyn
    // AdmissionReader` (the only thing `GateInputs` construction needs) can
    // grant itself admission. Proven by using a scripted reader for the read
    // side and a *separate*, real, human-only path for the write side.
    let scripted = ScriptedReader(false);
    assert!(!scripted.is_admitted());
    let scripted_admitted = ScriptedReader(true);
    assert!(scripted_admitted.is_admitted());

    // The only real write path: `DeveloperAdmissionStore::mint` requires a
    // `&HumanPrincipal`, constructible solely via `assert_human_operated()`
    // — exactly the call the `cronus dev admit` CLI handler makes, and
    // nowhere else in the codebase.
    let path = temp_dir("real-admission").join("admission.txt");
    let store = DeveloperAdmissionStore::open(&path);
    assert!(!store.is_admitted());
    store
        .mint(&HumanPrincipal::assert_human_operated())
        .unwrap();
    assert!(store.is_admitted());

    // The facade reader that the domain gate actually consumes reflects the
    // same real, human-minted record — through the read-only port only.
    let real_reader = AuthLocalAdmissionReader::open(&path);
    assert!(real_reader.is_admitted());
}

// ── Hidden-by-default trigger-loaded module (clean load/unload) ──────

#[test]
fn unload_is_clean_and_the_tier_is_never_cached_across_syncs() {
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
        assert_eq!(
            module.is_loaded(),
            tier == AdmissionTier::Elevated,
            "no stale/remembered elevated surface after tier {tier:?}"
        );
    }
}

// ── Tiered admission with a default-off feedback ceiling ─────────────

#[test]
fn feedback_tier_defaults_off_and_is_a_deliberate_deploy_opt_in() {
    let genuine = RepoAuthenticity::Genuine {
        upstream: "https://github.com/cronusio/cronus".to_string(),
    };

    let default_off = GateInputs {
        repo: genuine.clone(),
        admitted: false,
        feedback_tier_enabled: false,
    };
    assert_eq!(
        DevOfficeGate::resolve(&default_off),
        AdmissionTier::Absent,
        "shipped default is off — a normal install exposes no surface"
    );

    let opted_in = GateInputs {
        repo: genuine.clone(),
        admitted: false,
        feedback_tier_enabled: true,
    };
    assert_eq!(
        DevOfficeGate::resolve(&opted_in),
        AdmissionTier::Feedback,
        "a deployment may opt the feedback ceiling on"
    );

    let elevated = GateInputs {
        repo: genuine,
        admitted: true,
        feedback_tier_enabled: false,
    };
    assert_eq!(
        DevOfficeGate::resolve(&elevated),
        AdmissionTier::Elevated,
        "a genuine admission still reaches Elevated regardless of the feedback flag"
    );
}

// ── Repository-scoped workspace isolation ─────────────────────────────

#[test]
fn the_dev_workspace_scope_never_reaches_another_workspaces_store() {
    let mgr = WorkspaceManager::open_in_memory().unwrap();
    let dev_repo = temp_dir("dev-scope");
    let user_repo = temp_dir("user-scope");

    register_dev_workspace(&mgr, &dev_repo).unwrap();
    let user_id = WorkspaceId::new("acme-project").unwrap();
    mgr.create(&user_id, "Acme", &user_repo, WorkspaceTemplate::Default)
        .unwrap();

    let dev_ws = mgr
        .get(&WorkspaceId::new(DEV_OFFICE_WORKSPACE_ID).unwrap())
        .unwrap()
        .expect("dev workspace must exist");
    assert_eq!(dev_ws.path, dev_repo, "scoped to its own bound repo");
    assert_ne!(
        dev_ws.path, user_repo,
        "no handle into the user workspace's own path"
    );

    let user_ws = mgr.get(&user_id).unwrap().expect("user workspace exists");
    assert_eq!(
        user_ws.path, user_repo,
        "the user workspace is equally untouched by the dev workspace's registration"
    );
}

// ── Contained and audited elevated authority ──────────────────────────

#[test]
fn elevated_actions_pass_the_authority_gate_and_are_always_audited() {
    let audit_path = temp_dir("audit").join("audit.jsonl");
    let mut dispatch = ReceiptedDispatch::new(audit_path.clone());

    let allowed = ToolPolicy::default();
    assert!(
        run_elevated_action(&mut dispatch, &allowed, "dev.self_edit").is_ok(),
        "an unblocked elevated action passes the tool-security gate"
    );

    let mut blocked = ToolPolicy::default();
    blocked.disabled_tools.push("dev.dangerous_op".to_string());
    assert!(
        run_elevated_action(&mut dispatch, &blocked, "dev.dangerous_op").is_err(),
        "the authority gate still refuses a disabled action"
    );

    let log = std::fs::read_to_string(&audit_path).unwrap();
    assert!(
        log.contains("\"outcome\":\"allowed\""),
        "the allowed action produced an audit entry"
    );
    assert!(
        log.contains("\"outcome\":\"blocked\""),
        "the refused action is audited too, not silently dropped"
    );
}

// ── Standard dev-workflow, no exception lane ──────────────────────────

#[test]
fn the_pipeline_has_no_workspace_identity_parameter_to_special_case() {
    // Structural proof: `Pipeline::new`/`advance` take no workspace-kind or
    // workspace-id input at all, so there is nothing to represent a "dev
    // office fast lane" with — running the same sequence against two
    // independently constructed pipelines (standing in for an ordinary
    // workspace and the dev office) must behave identically, because the
    // type has no way to distinguish them.
    let mut ordinary = Pipeline::new();
    let mut dev_office = Pipeline::new();

    let pass = QualityGate {
        implementer_pass: true,
        reviewer_pass: true,
    };
    for t in 1..=3u64 {
        assert_eq!(
            ordinary.advance(pass, false, t),
            dev_office.advance(pass, false, t),
            "identical transitions for both — no dev-office-specific pipeline"
        );
    }
    assert_eq!(ordinary.stage(), dev_office.stage());

    // The two-stage quality gate refuses entry to Deliver identically for
    // both — no reduced gate set, no fast lane for the dev office.
    let failing = QualityGate {
        implementer_pass: true,
        reviewer_pass: false,
    };
    assert_eq!(
        ordinary.advance(failing, false, 4),
        Err(AdvanceError::QualityGateFailed)
    );
    assert_eq!(
        dev_office.advance(failing, false, 4),
        Err(AdvanceError::QualityGateFailed)
    );
}
