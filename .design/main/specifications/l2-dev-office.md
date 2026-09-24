# Developer (Self-Hosting) Office (Implementation)

**Version:** 1.0.1
**Status:** Stable
**Layer:** implementation
**Implements:** l1-dev-office.md

## Overview

Concrete realization of the developer office (DVO-1…DVO-8): a conditional
**system** workspace kind that materializes only in a genuine checkout of the
canonical Cronus repository, only for a human-admitted developer identity, and
is otherwise entirely absent. This spec is an **orchestration layer** — it
introduces almost no new domain logic, instead composing already-shipped
subsystems behind one gate: the workspace registry (a new `WorkspaceKind`),
the extension/module loader (trigger-loaded on demand), the human-write-only
authority plane (admission), the sandbox/tool-security confinement, the
tool-receipt audit path, and the standard development-workflow engine. The one
policy decision the L1 left open (DVO-5, feedback-tier default) is resolved to
**off by default**, so the L2 binds the feedback tier as an opt-in that ships
disabled.

## Related Specifications

- [l1-dev-office.md](l1-dev-office.md) — the concept this spec implements (DVO-1…DVO-8).
- [l2-workspace-management.md](l2-workspace-management.md) — the workspace registry the new `Developer` kind extends (DVO-1/DVO-6).
- [l2-navigation.md](l2-navigation.md) — the conditional-floor rendering surface the dev office appears on (DVO-1).
- [l2-development-workflow.md](l2-development-workflow.md) — the Design→Plan→Execute→Review→Deliver engine the dev office reuses unchanged (DVO-8).
- [l2-tool-security.md](l2-tool-security.md) — the confinement + authority gate the elevated tier runs behind (DVO-3/DVO-7).
- [l2-sandbox-policy.md](l2-sandbox-policy.md) — the process-boundary isolation between dev authority and user offices (DVO-7).
- [l1-attestation.md](l1-attestation.md) — the optional stronger witness for repository authenticity + admitted membership (DVO-2/DVO-3).
- [l1-issue-reporting.md](l1-issue-reporting.md) — the shipped consent-gated pipeline the feedback ceiling routes to (DVO-5).
- [l1-tool-receipts.md](l1-tool-receipts.md) — the durable per-action audit path every elevated action is recorded on (DVO-7).
- [l2-crate-topology.md](l2-crate-topology.md) — the tier model this spec places its pieces into.

## 1. Motivation

The concept spec makes self-maintenance a first-class, gated capability. The
implementation must realize that gate **without weakening any existing security
boundary and without inventing a parallel one**. The design principle here is
therefore *composition over construction*: every hard property the dev office
needs — human-only authority admission, sandboxed process boundaries, audited
actions, the rigorous dev pipeline — already exists as a shipped subsystem. The
L2's job is to (1) add exactly one conditional workspace kind, (2) wire a
network-free repository-authenticity check, (3) resolve an admission tier from
those two gates, and (4) load/unload the office as a module accordingly. New
code is confined to the gate and the tier resolution; the elevated capabilities
themselves are the existing subsystems, reached only once the gate opens.

## 2. Constraints & Assumptions

- No new authority mechanism: admission rides the existing human-write-only
  authority plane (`l1-security` SEC-10 / `crates/auth-local`); the agent has no
  write path to it, exactly as with background-activation and other SEC-10 grants.
- The repository-authenticity check is **local and network-free**: it inspects a
  version-control marker plus the bound upstream identity, never contacting a
  remote to activate.
- The canonical repository identity is a compiled-in constant of the product
  build (the upstream URL/identity the genuine repo carries), not user
  configuration — a user cannot change *which* upstream counts by editing Cronus
  configuration. A tree can still *claim* that upstream in its own `.git/config`,
  so the check is a heuristic that scopes where the office runs (SEC-12), not a
  proof of authenticity: the boundary is admission (DVO-3) plus containment
  (DVO-7), and the optional attestation witness (§4.3) is what binds content.
- The dev office's working scope is the bound repository directory only; it holds
  no handle to any other workspace's data (DVO-6), enforced by the same
  workspace-isolation the project/home kinds already use.
- The feedback tier is **off by default** (DVO-5, resolved); enabling it is a
  deployment-build/opt-in choice, never an agent- or runtime-flippable default.

## 3. Invariant Compliance (Layer 2)

| L1 Invariant | Implementation |
| --- | --- |
| DVO-1 Distinct, conditional system floor | A new `WorkspaceKind::Developer` in the workspace registry (`crates/store-local` + `crates/domain`), cardinality `0..1`, `system`-owned: not creatable through the project-creation flow, not renameable/deletable like a project. It is surfaced to `l2-navigation` as a conditional system floor **only** while `DevOfficeGate::resolve(...)` yields `Elevated`; otherwise the registry reports it absent and nothing renders. |
| DVO-2 Repository-authenticity binding | `repo_authenticity(cwd)` (facade tier, `crates/core`) — reads the local version-control marker (a `.git` working tree) and its bound upstream/remote identity from `.git/config`, compares the upstream against the compiled-in `CANONICAL_UPSTREAM` constant, and returns `Genuine`/`NotCanonical`/`NotARepo`. Pure local filesystem read, **no network call**. A `NotCanonical`/`NotARepo` result makes the gate return `Absent` unconditionally, independent of any credential. |
| DVO-3 Identity-gated, human-admitted elevated access | Admission is a `DeveloperAdmission` record on the human-write-only authority plane (`crates/auth-local`, the SEC-10 plane) — minted only by a human principal presenting a verified developer identity (credential/key, external-account verification, or an `l1-attestation` attested membership). The agent-facing API is read-only over this plane: `is_admitted()` may be *read* by domain logic, but no agent-reachable call can *write* an admission. A prompt-injected or ordinary-office agent has no code path to mint one. |
| DVO-4 Hidden by default, trigger-loaded module | The dev office is registered with the extension/module loader (`l1-extensions`) as a trigger-loaded module whose trigger is `DevOfficeGate::resolve(...) == Elevated`. A default install (no admission) never fires the trigger, so the module never loads and nothing renders. Revoking admission (or leaving the canonical repo) re-resolves the gate to `Absent` and unloads the module cleanly, leaving no elevated surface. |
| DVO-5 Tiered admission — feedback ceiling (default off) | `DevOfficeGate::resolve` returns one of `Absent` / `Feedback` / `Elevated`. `Elevated` requires `Genuine` repo **and** an admission. `Feedback` is returned **only** when the `feedback_tier_enabled` build/deploy flag is set (default `false`) **and** no admission is present; with the flag off (the default) a non-admitted user resolves to `Absent`. The feedback surface, when enabled, delegates to the shipped `l1-issue-reporting` consent-gated pipeline and exposes nothing else. No transition promotes `Feedback → Elevated` without a fresh DVO-3 admission (the tiers are distinct enum states; there is no upgrade method). |
| DVO-6 Purpose confinement — self-maintenance only | The `Developer` workspace's working directory is the bound repository path from DVO-2; the workspace-isolation model gives it no handle to any other workspace's store/data (identical to how a project workspace cannot read another's). Its capability set is the dev-workflow over that one tree; there is no API to target a user project or a second repository. |
| DVO-7 Contained & audited elevated authority | Elevated actions run behind `l2-tool-security`'s authority gate and inside `l2-sandbox-policy`'s process boundary (INV-8 sanctioned boundaries), so a compromised user office cannot reach dev authority. Every effectful elevated action produces a durable per-action receipt on the `l1-tool-receipts` path; irreversible/externally-visible actions require the `l1-development-workflow` DW-8 human checkpoint. Removing admission (DVO-3/DVO-4) revokes the reach. |
| DVO-8 Standard dev workflow, no exception lane | Work inside the dev office invokes the same `l2-development-workflow` engine (Design→Plan→Execute→Review→Deliver + the two-stage quality gate) as any other software work — the dev office passes its bound repo as the workspace and adds no bypass. It changes *who is admitted* and *where the work happens*, realized as the gate + the workspace kind; it changes nothing about *how* the pipeline runs. |

## 4. Detailed Design

### 4.1 Tier split (crate placement)

Following the crate topology, new code is split by I/O:

```plaintext
[REFERENCE]
crates/domain/src/dev_office.rs   // I/O-free: AdmissionTier, DevOfficeGate::resolve,
                                  // WorkspaceKind::Developer classification, tier→capability map
crates/core/src/dev_office_gate.rs// facade: repo_authenticity() (git/fs read),
                                  // admission read over auth-local, module load/unload wiring
```

The domain module holds the pure decision logic (gate resolution, tier
semantics) so it is testable against fabricated inputs with no repo and no
filesystem; the facade module performs the real git/filesystem read and wires
the module loader and the auth-plane read.

### 4.2 The activation gate

The gate is a pure function of two already-computed inputs plus the deploy flag:

```rust
[REFERENCE]
pub enum RepoAuthenticity {
    Genuine { upstream: String },
    NotCanonical,
    NotARepo,
}

pub enum AdmissionTier {
    Absent,    // nothing renders — the default for a normal install
    Feedback,  // report/improve surface only (opt-in; default off)
    Elevated,  // full self-maintenance dev office
}

pub struct GateInputs {
    pub repo: RepoAuthenticity,
    pub admitted: bool,           // read from the human-write-only plane
    pub feedback_tier_enabled: bool, // deploy/build flag, default false
}

impl DevOfficeGate {
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
```

`Elevated` is reachable only through the `(Genuine, admitted)` arm — the
repository gate is a hard precondition even for an admitted developer, so an
admission credential carried into a non-canonical tree resolves to `Absent`
(with the feedback tier off) rather than granting anything.

### 4.3 Repository-authenticity check (DVO-2)

```rust
[REFERENCE]
// Compiled into the product build — NOT user config, so a user cannot retarget
// the dev office by editing a file.
const CANONICAL_UPSTREAM: &str = "…canonical Cronus upstream identity…";

pub fn repo_authenticity(cwd: &Path) -> RepoAuthenticity {
    let Some(git_dir) = find_worktree_marker(cwd) else {
        return RepoAuthenticity::NotARepo;
    };
    match read_bound_upstream(&git_dir) {   // parse .git/config remotes, local read
        Some(u) if upstream_matches_canonical(&u, CANONICAL_UPSTREAM) => {
            RepoAuthenticity::Genuine { upstream: u }
        }
        _ => RepoAuthenticity::NotCanonical,
    }
}
```

Local and network-free — and, because a tree's own `.git/config` can name any remote, a heuristic rather than a verification (SEC-12, §2). An optional stronger binding composes `l1-attestation`:
a signed witness over the checkout (AT-2 content-set binding) can be verified
offline to raise confidence, but the base gate needs only the local marker +
upstream identity, so the office works in a fresh clone before any attestation
exists.

### 4.4 Admission on the human-write-only plane (DVO-3)

`DeveloperAdmission` lives with the other SEC-10 authority records in
`crates/auth-local`. The write path is human-principal-only (the same plane
background-activation consent and other grants use); the domain gate reads it
through a read-only port:

```rust
[REFERENCE]
pub trait AdmissionReader {            // domain depends only on this (read-only)
    fn is_admitted(&self) -> bool;
}
// The WRITE side (mint/revoke) is exposed only on the human-principal auth
// surface, never on any agent-reachable API — structurally, not by convention.
```

Because the domain gate holds only an `&dyn AdmissionReader`, no agent path
routed through domain logic can mint an admission — the capability to escalate
is *unrepresentable* on the agent-facing surface, the same structural-enforcement
pattern used for background-activation (BA-4) and the office-archetype no-authority
schema (OA-4).

### 4.5 Module load / unload (DVO-4)

The dev office registers as a trigger-loaded module with the extension loader.
The trigger predicate is `DevOfficeGate::resolve(...) == Elevated`. On app
start / connection, the facade evaluates the gate; a transition into `Elevated`
loads the module (and surfaces the floor), and any transition out of it (leaving
the canonical repo, admission revoked) unloads it and drops the floor. Unload is
clean: no elevated capability, registration, or rendered surface persists past
the trigger going false.

### 4.6 Tier resolution & the feedback ceiling (DVO-5, default off)

The feedback tier is a build/deploy opt-in (`feedback_tier_enabled`, default
`false`). With it off — the shipped default — a non-admitted user resolves to
`Absent`: the dev office ships no surface at all. Ordinary-user feedback is not
lost; it flows through the already-shipped `l1-issue-reporting` consent-gated
pipeline, which is independent of the dev office. When a deployment does opt the
feedback tier on, the surface it exposes is *only* a thin front-end over that
same reporting pipeline — no source read, no engine access, no visibility into
dev-office work — and it can never become `Elevated` without a fresh DVO-3
admission (the tiers are distinct enum variants with no upgrade path).

### 4.7 Confinement & audit (DVO-6 / DVO-7)

The elevated tier is not a new capability set — it is the existing tool surface
run behind the existing guards, but scoped to the bound repository:

- **Isolation:** the `Developer` workspace's process runs inside the
  `l2-sandbox-policy` boundary; INV-8 sanctioned process boundaries keep a
  fault/compromise in a user office from reaching dev authority (transitive guard
  enforcement).
- **Authority:** every effectful action passes the `l2-tool-security` gate.
- **Audit:** every elevated action produces a durable `l1-tool-receipts` receipt;
  irreversible or externally-visible actions additionally require the DW-8 human
  checkpoint from the development workflow.
- **Scope:** the workspace-isolation model gives the dev office a handle to its
  bound repository directory only — no read path into another workspace's store.

### 4.8 The dev workflow, unchanged (DVO-8)

Work inside the dev office calls the same `l2-development-workflow` engine as any
project, passing the bound repository as the active workspace. There is no
`dev_office`-specific pipeline, no reduced gate set, no fast lane — the dogfooding
bar (QLY-6) is the standard bar. The only difference is the workspace the pipeline
runs over and the admission required to reach it.

## 5. Implementation Notes

1. `CANONICAL_UPSTREAM` is a build constant; a fork that wants its own dev office
   rebuilds with its own canonical identity rather than editing runtime config —
   this keeps the canonical identity un-retargetable through Cronus configuration
   (a checkout's own remote can still claim it, §2).
2. `repo_authenticity` should treat an ambiguous/multi-remote `.git/config`
   conservatively: `Genuine` only when the bound upstream unambiguously matches the
   canonical identity; anything else is `NotCanonical` (fail-closed, matching AT-6).
3. The gate is re-evaluated on the events that can change its inputs (app start,
   connection trigger, admission mint/revoke, workspace change) — never cached as a
   remembered "was elevated" value, so a revoked admission cannot leave a stale
   elevated surface (mirrors the background-activation observe-not-remember rule).
4. The feedback-tier front-end, when enabled, must not introduce a second
   reporting path — it binds the existing `l1-issue-reporting` pipeline, so there
   is exactly one consent-gated egress for user feedback.

## 6. Drawbacks & Alternatives

- **Build-constant canonical identity vs. configurable.** A build constant means a
  fork must rebuild to get its own dev office. Accepted: runtime-configurable
  canonical identity would reintroduce exactly the retargeting risk DVO-2 exists to
  remove.
- **Feedback tier off by default loses a per-install report affordance.** Accepted
  and non-lossy: the shipped `l1-issue-reporting` pipeline already gives every
  install a consent-gated feedback path independent of the dev office, so the
  default-off dev-office feedback surface removes redundant surface without removing
  the capability.
- **Composition over a dedicated dev-authority subsystem.** Building a bespoke
  elevated-authority stack for the dev office was rejected: it would duplicate (and
  risk diverging from) the shipped sandbox/tool-security/receipt guards. Reusing them
  keeps one audited authority path.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[L1]` | `.design/main/specifications/l1-dev-office.md` | Invariants DVO-1…DVO-8 realized here. |
| `[WORKSPACE]` | `.design/main/specifications/l2-workspace-management.md` | The registry the `Developer` kind extends. |
| `[DEV-WORKFLOW]` | `.design/main/specifications/l2-development-workflow.md` | The pipeline the dev office reuses unchanged (DVO-8). |
| `[TOOL-SEC]` | `.design/main/specifications/l2-tool-security.md` | The authority gate + confinement the elevated tier runs behind (DVO-7). |
| `[ISSUE-REPORTING]` | `.design/main/specifications/l1-issue-reporting.md` | The consent-gated pipeline the feedback ceiling routes to (DVO-5). |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.1 | 2026-09-23 | Core Team | Consistency pass (2026-09-23): The repository-authenticity check (remote identity in `.git/config`) was presented as un-retargetable verification, but any tree can declare the canonical remote — labeled a scoping heuristic (SEC-12), with admission (DVO-3) and containment (DVO-7) as the boundary and attestation as what binds content. Section numbering corrected (5 → 6). |
| 1.0.0 | 2026-07-19 | Core Team | Initial Stable — the concrete developer-office realization implementing DVO-1…DVO-8 as an orchestration layer over shipped subsystems: a conditional `WorkspaceKind::Developer` (DVO-1), a local network-free `repo_authenticity` check against a compiled-in `CANONICAL_UPSTREAM` (DVO-2), admission on the human-write-only auth plane read through a read-only `AdmissionReader` port so escalation is unrepresentable on the agent surface (DVO-3, the BA-4/OA-4 structural-enforcement pattern), trigger-loaded module gated on `DevOfficeGate::resolve == Elevated` with clean unload (DVO-4), a three-state `AdmissionTier` (Absent/Feedback/Elevated) with the feedback tier a default-off deploy opt-in routing to the shipped `l1-issue-reporting` pipeline (DVO-5, resolved), repository-scoped workspace isolation (DVO-6), confinement + audit composing `l2-sandbox-policy`/`l2-tool-security`/`l1-tool-receipts`/DW-8 (DVO-7), and the unchanged `l2-development-workflow` engine (DVO-8). Domain/facade tier split: pure gate + tier logic in `crates/domain`, the git/fs authenticity read + module wiring in the `crates/core` facade. No new authority mechanism, no new domain logic beyond the gate — the frontend surfacing is a thin conditional-floor binding over the already-built `l2-navigation`. |
