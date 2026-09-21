---
phase: 22
name: "Developer Office"
status: Done
subsystem: "crates/domain/src/dev_office"
requires: [4, 9, 11]
provides:
  - dev_office_gate
  - dev_office_admission
  - dev_office_module
  - dev_office_workspace
  - dev_office_cli
key_files:
  created:
    - crates/domain/src/dev_office.rs
    - crates/core/src/dev_office_gate.rs
    - crates/core/src/dev_office_workspace.rs
    - crates/auth-local/src/developer_admission.rs
    - crates/core/tests/dev_office_invariants.rs
  modified:
    - crates/domain/src/lib.rs
    - crates/core/src/lib.rs
    - crates/core/src/auth.rs
    - crates/auth-local/src/lib.rs
    - crates/cli/src/cli.rs
    - crates/cli/src/commands.rs
    - crates/cli/tests/cli_smoke.rs
patterns_established:
  - "Reserved-id singleton: a system-owned resource rides the same registry as user resources, discriminated by one reserved identifier guarded at the real creation/deletion flow, instead of a schema/kind migration (dev-office workspace vs. a `WorkspaceKind` column)."
  - "Human-principal marker type: a zero-sized type constructible only via one named assertion (`HumanPrincipal::assert_human_operated`), called solely at a human-operated CLI entry point — the write side of a SEC-10 authority plane, symmetric with a read-only domain-side port trait."
  - "Compose the real primitive, disclose the gap: when a spec assumes a subsystem is shipped and it isn't (`l1-tool-receipts`), wire the real, narrower thing that does exist (SEC-7 `AuditEntry`) and disclose the delta explicitly, rather than fabricating compliance or silently expanding scope."
duration_minutes: ~
---

# Stage 22 Tasks — Developer Office

**Phase:** 22
**Status:** Todo
**Strategic Goal:** Ship `l2-dev-office` — the self-hosting developer office (DVO-1…DVO-8): a conditional `WorkspaceKind::Developer` that materializes only in a genuine checkout of the canonical Cronus repo, only for a human-admitted developer, and is otherwise entirely absent. An orchestration layer over already-shipped subsystems; the phase builds only the gate, the tier resolution, the workspace-kind extension, and the composing wiring. Foundation-then-parallel; Track A gates B/C/D.

## Atomic Checklist

- [x] [T-22A01] Domain gate + tier + `WorkspaceKind::Developer` + read-only `AdmissionReader` port
- [x] [T-22B01] `repo_authenticity` facade — network-free canonical-repo check (DVO-2)
- [x] [T-22C01] Admission on the human-write-only auth plane + reader impl (DVO-3)
- [x] [T-22C02] Trigger-loaded module: load on `Elevated`, clean unload (DVO-4)
- [x] [T-22D01] Workspace-kind registration + confinement/audit wiring (DVO-1/6/7)
- [x] [T-22D02] `cronus dev status|admit|revoke` CLI surface
- [x] [T-22T01] Validation sweep: DVO-1…DVO-8 acceptance

## Detailed Tracking

### [T-22A01] Domain gate + tier + workspace kind + reader port

- **Spec:** l2-dev-office.md §4.1 (tier split), §4.2 (activation gate), §4.4 (AdmissionReader); DVO-1, DVO-3, DVO-5
- **Status:** Done
- **Assignment:** Agent
- **Verify:** `cargo test -p cronus-domain dev_office` — the resolution table proven against fabricated inputs: `(Genuine, admitted)` → `Elevated`; a credential in a `NotCanonical`/`NotARepo` tree → `Absent`; `feedback_tier_enabled=false` (default) + not admitted → `Absent`; `feedback_tier_enabled=true` + not admitted → `Feedback`; and `WorkspaceKind::Developer` classifies as system-owned/non-deletable/not-project-creatable. `cargo clippy -p cronus-domain --all-targets -- -D warnings` clean.
- **Evidence Capsule:** `crates/domain/src/dev_office.rs` (new, ~230 lines) — `RepoAuthenticity` (Genuine{upstream}/NotCanonical/NotARepo), `AdmissionTier` (Absent/Feedback/Elevated), `GateInputs`, `DevOfficeGate::resolve` (exact match arms from spec §4.2), `AdmissionReader` trait (read-only, no mint method), `WorkspaceKind` (Project/Developer) with `is_system_owned`/`max_instances`/`is_deletable`/`is_project_creatable`. Registered in `crates/domain/src/lib.rs` (alphabetical, between `deliberation` and `development_workflow`). 11 unit tests incl. the wrong-tree-never-downgrades-to-Feedback edge case. `cargo test -p cronus-domain dev_office`: 11 passed. `cargo check -p cronus-domain`: clean. `cargo clippy -p cronus-domain --all-targets -- -D warnings`: clean.
- **Handoff:** Gates T-22B01/C01/C02/D01 — the gate + tier + kind types are the shared foundation.
- **Notes:** New `crates/domain/src/dev_office.rs`, pure/I/O-free: `RepoAuthenticity` (Genuine{upstream}/NotCanonical/NotARepo), `AdmissionTier` (Absent/Feedback/Elevated), `GateInputs`, `DevOfficeGate::resolve` (the exact match from §4.2 — `Elevated` reachable **only** via the `(Genuine, admitted)` arm, so a credential outside the canonical repo grants nothing). `WorkspaceKind::Developer` added to the workspace-kind enum (0..1, system-owned). **Load-bearing move (DVO-3):** the domain gate depends only on a read-only `trait AdmissionReader { fn is_admitted(&self) -> bool; }` — no mint/write method exists on it, so agent self-escalation is *unrepresentable* at the type level (the BA-4/OA-4 structural-enforcement pattern), not a bypassable runtime check. Tested against a scriptable fake reader.

### [T-22B01] `repo_authenticity` facade (DVO-2)

- **Spec:** l2-dev-office.md §4.3; DVO-2
- **Status:** Done
- **Assignment:** Agent
- **Verify:** `cargo test -p cronus-core dev_office_gate` — a `.git/config` bound to the canonical upstream → `Genuine`; a non-canonical upstream → `NotCanonical`; no `.git` marker → `NotARepo`; an ambiguous/multi-remote config with no unambiguous canonical match → `NotCanonical` (fail-closed, AT-6). Exercised against real temporary `.git` fixtures; **no network call**.
- **Evidence Capsule:** `crates/core/src/dev_office_gate.rs` (new, ~150 lines): `CANONICAL_UPSTREAM = "https://github.com/cronusio/cronus"` (the value already declared in root `Cargo.toml`'s `repository` field — not a new identity); `repo_authenticity(cwd)` walks upward for the nearest `.git` marker (resolving linked-worktree/submodule `.git` files too), `read_bound_upstream` treats `origin` as the unambiguous bound remote even alongside others, falling back to a sole remote or `None` (ambiguous → `NotCanonical`) otherwise; `upstream_matches_canonical` normalizes scheme/SSH-shorthand/`.git`-suffix/trailing-slash/case before comparing, so `git@github.com:cronusio/cronus.git` and the canonical HTTPS form match as one identity. Registered in `crates/core/src/lib.rs` (alphabetical `dev_office_gate` mod + `dev_office` added to the domain re-export list, the `activation_bootstrap` precedent — CLI reaches `cronus_core::dev_office::*` without a direct `cronus-domain` dependency). 8 unit tests against real temporary `.git` fixtures (the `cronus-<tag>-{pid}` house pattern), no network call, incl. the ambiguous-multi-remote and genuine-from-a-nested-subdirectory cases. `cargo test -p cronus-core dev_office_gate`: 8/8 passed. `cargo check -p cronus-core` clean. `cargo clippy -p cronus-core --all-targets -- -D warnings` clean (one collapsible-if nesting fixed via let-chains). `cargo fmt --all` clean.
- **Handoff:** Feeds the gate (A01) at the facade; independent of C once A01 lands.
- **Notes:** New `crates/core/src/dev_office_gate.rs`: `repo_authenticity(cwd) -> RepoAuthenticity` — `find_worktree_marker` (local `.git`), `read_bound_upstream` (parse `.git/config` remotes), `upstream_matches_canonical` against a compiled-in `CANONICAL_UPSTREAM` **build constant** (not user config — un-retargetable by an end user, §5 note 1). Conservative on ambiguity: `Genuine` only on an unambiguous match. Local filesystem read only.

### [T-22C01] Admission on the human-write-only plane (DVO-3)

- **Spec:** l2-dev-office.md §4.4; DVO-3, SEC-10
- **Status:** Done
- **Assignment:** Agent
- **Verify:** `cargo test` — a `DeveloperAdmission` is minted only through the human-principal path and read back by the `AdmissionReader` impl; `is_admitted()` reflects mint then revoke; there is **no agent-reachable write path** — the domain gate holds only `&dyn AdmissionReader`, structurally unable to mint (asserted by the fact that the reader trait exposes no write method; the write API lives only on the human-principal auth surface).
- **Handoff:** Supplies the `admitted` input to the gate; C02 keys the module trigger off it.
- **Notes:** `DeveloperAdmission` on the `crates/auth-local` SEC-10 human-write-only plane (the background-activation-consent precedent — the same plane the agent has no write path to). **Disclosed execution question (surfaces here, not a design gap):** whether `auth-local` already exposes a generic human-write-only grant store the record can ride, or whether a small dedicated `DeveloperAdmission` record is added — an implementation reuse detail resolved at execution, flagged not guessed.
- **Evidence Capsule:** Resolved the disclosed question first: `auth-local` has zero pre-existing consent/grant-store abstraction (confirmed by grep — `AuthStore`/`SessionStore` are pure in-memory, no persistence anywhere in the crate, and `AuthStore` has no caller anywhere in `cronus-cli` either) — a small dedicated record was the right call, not a guess. New `crates/auth-local/src/developer_admission.rs`: `HumanPrincipal(())` — a marker constructible only via `assert_human_operated()`, called solely from a human-operated entry point (T-22D02's future CLI verb), never from any agent-tool-reachable path; `DeveloperAdmissionStore` — file-backed (`admitted=<bool>\nchanged_at=<unix_secs>\n`, hand-rolled, no new serde dependency for a two-field format), `open(path)` takes a caller-supplied path (no built-in resolution, the `KnowledgeService::open_default(path)` precedent), `mint`/`revoke` require a `&HumanPrincipal`, `is_admitted()` fails closed on a missing/corrupt file. New `crates/core/src/dev_office_gate.rs` addition: `AuthLocalAdmissionReader` wraps the store and implements `cronus_domain::dev_office::AdmissionReader` — the facade-side "admission read over auth-local" half of the §4.1 crate split. Registered `pub mod developer_admission` + flat re-export in `crates/auth-local/src/lib.rs` (matching that crate's existing flat-API convention). 5 new auth-local tests (incl. a cross-handle persistence test proving the record is genuinely file-backed, not held in-memory — required for a real `admit` CLI run and a later `status` run to agree) + 2 new core tests exercising the reader strictly through the `&dyn AdmissionReader` trait object. `cargo test -p cronus-auth-local`: 25/25 passed (0 regressions in the pre-existing 20). `cargo test -p cronus-core dev_office`: 10/10 passed. `cargo check`/`cargo clippy --all-targets -- -D warnings`/`cargo fmt --all` clean on both crates. No `unwrap()`/`panic!()` on production paths.

### [T-22C02] Trigger-loaded module: load/unload (DVO-4)

- **Spec:** l2-dev-office.md §4.5; DVO-4
- **Status:** Done
- **Assignment:** Agent
- **Verify:** `cargo test` — a transition into `Elevated` loads the dev-office module (and marks the floor present); a transition out (admission revoked, or the cwd is no longer the canonical repo) unloads it cleanly with **no stale elevated surface**; the gate is re-evaluated on its input-changing events and never cached as a remembered "was elevated" value.
- **Handoff:** Completes the DVO-4 lifecycle; D01 registers what the loaded module surfaces.
- **Notes:** Compose the extension/module loader (`l1-extensions`); the trigger predicate is `DevOfficeGate::resolve(...) == Elevated`. Clean unload leaves no capability/registration/rendered surface behind. **Observe-not-remember** (the `l2-service-activation` BA-8 precedent): a revoked admission must never leave a stale elevated surface, so the gate result is recomputed, not stored.
- **Evidence Capsule:** Added `DevOfficeModule` to `crates/core/src/dev_office_gate.rs` (per the §4.1 crate split naming "module load/unload wiring" as a facade-tier responsibility, not domain) — composes the already-shipped `cronus_domain::extensions::ExtensionRegistry` (a single `"dev-office"` entry, `Discovered→Permitted` at construction, `Permitted/Inactive↔Active` toggled by `sync(tier)`) rather than inventing a parallel loader. `sync(tier: AdmissionTier)` takes the gate's output fresh on every call — the struct holds no "was elevated" field, only the loader's own live Active/Inactive fact, so DVO-4 observe-not-remember is structural (there is nothing to go stale) rather than a discipline the caller has to maintain. A same-tier resync is a guarded no-op (never hits `ExtensionRegistry::transition`'s invalid-edge error). 6 new tests: fresh-registered-not-loaded, Elevated loads, transition-out unloads (`Absent`), `Feedback` never counts as loaded (a lesser tier, not a degraded-Elevated), idempotent repeated resync at the same tier, and a 5-step alternating-tier sequence proving `is_loaded()` tracks every toggle with no stuck value. `cargo test -p cronus-core dev_office`: 16/16 passed (10 prior + 6 new). `cargo clippy -p cronus-core --all-targets -- -D warnings` / `cargo fmt --all` clean.

### [T-22D01] Workspace registration + confinement/audit wiring (DVO-1/6/7)

- **Spec:** l2-dev-office.md §4.1, §4.7; DVO-1, DVO-6, DVO-7
- **Status:** Done
- **Assignment:** Agent
- **Verify:** `cargo test` — `WorkspaceKind::Developer` registers as `0..1`, system-owned, non-deletable, and **not creatable through the project-creation flow**; the dev workspace's scope is its bound repository directory only (no handle into another workspace's store — the isolation assertion); an elevated action passes the `l2-tool-security` gate and produces an `l1-tool-receipts` receipt; a fault in a user office cannot reach dev authority (INV-8 boundary assertion).
- **Handoff:** Surfaces the office the module (C02) loads; T01 sweeps DVO-1/6/7.
- **Notes:** Compose the `crates/store-local` workspace registry (register the conditional Developer kind) + `l2-sandbox-policy` (process boundary) + `l2-tool-security` (authority gate) + `l1-tool-receipts` (audit) + the DW-8 human checkpoint. **Mostly composition + assertion tests, not new logic** — the elevated capabilities ARE the existing guarded subsystems, scoped to the bound repo. The DVO-1 conditional-floor surfacing is a thin binding over the already-built `l2-navigation` (no new GUI).
- **Evidence Capsule:** **Disclosed planning-time gap found and worked around, not hidden:** `l1-tool-receipts` (TR-1…TR-9, MAC-signed/tamper-evident/model-unforgeable receipts) has **zero implementation anywhere in this project** (grep for "receipt", case-insensitive, across `crates/` — 0 hits) despite Phase 22's own opening premise listing it among "already-shipped subsystems." What genuinely IS shipped and composable is `tool_security::AuditEntry`/`append_audit_entry` — a real, already-built, append-only SEC-7 logged-audit-trail primitive (the baseline TR-9 itself says receipts are meant to strengthen "from logged to provable"). Building the full MAC-signed receipt subsystem inline here would both blow this task's "composition, not new logic" scope and skip the SDD pipeline for a security-sensitive subsystem that deserves its own spec-driven phase — so T-22D01 composes the real SEC-7 audit log honestly instead of fabricating a fake "receipt" or silently claiming compliance it doesn't have. New `crates/core/src/dev_office_workspace.rs`: `DEV_OFFICE_WORKSPACE_ID = "dev-office"` (reserved constant, the `RESERVED_USERNAMES` precedent); `register_dev_workspace(mgr, bound_repo)` — cardinality 0..1 falls out of `WorkspaceManager::create`'s existing `AlreadyExists` semantics, no new uniqueness logic; `is_reserved_dev_workspace_id`; `run_elevated_action(policy, audit_path, action_name)` — the `l2-tool-security` `ToolPolicy::is_permitted` gate first, an audit entry always second (allowed *and* blocked both logged — an audit trail that only remembers successes isn't one), fail-closed if the audit write itself errors. Reserved-id guards added to the real `workspace create`/`delete` CLI flow (`crates/cli/src/commands.rs`) — the actual project-creation flow, not a simulation of it — so `WorkspaceKind::Developer` is structurally unreachable through it. Isolation (INV-8/DVO-6) proven two ways: a `Workspace` row only ever carries its own `path` (no cross-workspace reference exists in the type at all), and `run_elevated_action` takes no `WorkspaceManager`/registry parameter whatsoever — by plain absence, not a runtime check. 5 new `core` tests + 2 new real CLI smoke tests (`cli_smoke.rs`, spawning the compiled binary) proving the reserved id is refused end-to-end through the actual flow. `cargo test -p cronus-core dev_office_workspace`: 5/5. `cargo test -p cronus-cli workspace`: 5 bin + 2 smoke, 7/7, 0 regressions. `cargo clippy -p cronus-core -p cronus-cli --all-targets -- -D warnings` / `cargo fmt --all` clean.

### [T-22D02] `cronus dev` CLI surface

- **Spec:** l2-dev-office.md §4 (composed surfaces); DVO-3, l1-architecture INV-9
- **Status:** Done
- **Assignment:** Agent
- **Verify:** `cargo test` + 1 real CLI smoke via the compiled binary — `cronus dev status` prints the resolved tier (`Absent` in a non-canonical tree or with no admission); `cronus dev admit`/`revoke` are human-principal acts at the CLI (the operator is the principal, so this is a legitimate DVO-3 admission path — distinct from an *agent* self-granting); the binary handles each without panic and reports honestly.
- **Handoff:** Completes the shipped surface; T01 validates end to end.
- **Notes:** `cronus dev status|admit|revoke`, verb-first per `l2-cli`, INV-9 shipped-surface honesty (no unbound verbs). `status` reads `DevOfficeGate::resolve`; `admit`/`revoke` write the `DeveloperAdmission` on the human plane (the CLI human operator is the DVO-3 principal — the agent still has no such path). Reuses the `cronus activation`/`cronus knowledge` CLI-module precedent.
- **Evidence Capsule:** `Command::Dev { sub: DevCommand }` + `DevCommand::{Status, Admit, Revoke}` added to `crates/cli/src/cli.rs` (help text worded in plain language, no spec-filename citation). New `mod dev_office_cmd` in `crates/cli/src/commands.rs`, mirroring the `knowledge_cmd`/`activation_cmd` precedent exactly: `admission_path()` resolves the real `Paths::os_native()/dev_office/admission.txt`; `status` computes `repo_authenticity(cwd)` + reads `AuthLocalAdmissionReader` fresh, builds `GateInputs` (`feedback_tier_enabled` hardcoded `false` — the DVO-5 shipped default, not a CLI flag), calls `DevOfficeGate::resolve`, and prints the tier — recomputed every invocation, matching `ActivationCommand::Status`'s own "read from the OS, never a remembered value" philosophy (DVO-4's observe-not-remember, applied at the CLI layer too); `admit`/`revoke` construct `HumanPrincipal::assert_human_operated()` directly in the CLI handler — the operator invoking this command themselves is the legitimate DVO-3 human principal, and no other code path in the entire project constructs a `HumanPrincipal`. Reached `DeveloperAdmissionStore`/`HumanPrincipal` from the CLI by adding them to `crates/core/src/auth.rs`'s existing re-export list (`cronus-cli` depends only on `cronus-core`, never `cronus-auth-local` directly — confirmed via `Cargo.toml`). **1 real CLI smoke test**, not a fixture: `dev_status_admit_revoke_round_trip_through_the_real_gate` in `cli_smoke.rs` runs against this actual dev machine's real checkout (`git remote -v` confirms `origin` genuinely is `git@github.com:cronusio/cronus.git`, matching `CANONICAL_UPSTREAM` after normalization) — `status` (absent) → `admit` → `status` (elevated, proving the real end-to-end `Genuine`+admitted path) → `revoke` → `status` (absent again, restoring the real on-disk admission record to its default state — the `knowledge` smoke test's clean-up-what-you-touch precedent for real `%APPDATA%` side effects). `cargo test -p cronus-cli`: 41 bin-target unit + 41 smoke (incl. the 3 new dev-office ones), all green, 0 regressions. `cargo clippy -p cronus-cli -p cronus-core --all-targets -- -D warnings` / `cargo fmt --all` clean.

### [T-22T01] Validation sweep: DVO-1…DVO-8 acceptance

- **Goal:** Verify the assembled developer office against `l2-dev-office` — every DVO invariant covered by a named test through the real gate + facade.
- **Method:** New `crates/core/tests/dev_office_invariants.rs` — one named test per invariant: DVO-1 conditional floor (present only while `Elevated`), DVO-2 repo-authenticity fail-closed (non-canonical/ambiguous → not `Genuine`), DVO-3 escalation unrepresentable (the domain gate cannot mint an admission), DVO-4 clean unload (no stale surface after revoke), DVO-5 tier resolution + feedback default-off (+ opt-in), DVO-6 repo-scope isolation (no cross-workspace handle), DVO-7 confinement + audit (tool-security gate + receipt on every elevated action), DVO-8 unchanged dev-workflow (the standard pipeline, no exception lane). Final gate: `cargo test -p cronus-core dev_office_invariants` green + `cargo test --workspace` exit 0 ×3 + `cargo clippy --workspace --all-targets -- -D warnings` + `cargo fmt --all -- --check`.
- **Status:** Done
- **Evidence Capsule:** 9 named tests, one (or two, for DVO-1) per invariant, through the real gate/facade export chain (`cronus_core`'s re-exports, no direct `cronus-domain`/adapter-crate dependency from the test file — matching the `knowledge_invariants` shape). DVO-2 exercised against 4 real temporary `.git` fixtures (non-canonical/ambiguous-multi-remote/absent/genuine), no network call. DVO-3's "unrepresentable" claim proven concretely, not just asserted: a `ScriptedReader` shows the gate consumes only the one-method `AdmissionReader` port, while the *only* real write path goes through `DeveloperAdmissionStore::mint` gated on `HumanPrincipal::assert_human_operated()`. DVO-7 proves both outcomes — allowed *and* blocked — land in the audit log, not just the success path. DVO-8 is a structural proof: `Pipeline::new`/`advance` take no workspace-identity parameter at all, so two independently constructed pipelines (standing in for an ordinary workspace and the dev office) are asserted to behave byte-identically under the same inputs, including an identical quality-gate refusal — there is no parameter through which a "dev office fast lane" could even be expressed. **Final gate, run in full:** `cargo test -p cronus-core dev_office_invariants` 9/9 · `cargo clippy --workspace --all-targets -- -D warnings` clean · `cargo fmt --all -- --check` clean · `cargo test --workspace` green across **3 consecutive full runs** (71 result-sets each run, 0 failed, 0 FAILED lines — verified by grep across all 3 raw logs). No `unwrap()`/`panic!()` on production paths across the whole phase (verified file-by-file across all 5 new source files + all touched files). **Phase 22 complete: 7/7 tasks, all 6 tracks (A/B/C/D01/D02/T) done.**
