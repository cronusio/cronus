# Version Control

**Version:** 1.0.1
**Status:** Stable
**Layer:** implementation
**Implements:** l1-version-control.md

## Overview

The concrete version-control layer in `crates/core`: the virtual staging area built on the existing execution-workspace worktrees, role-based git authority enforced in the orchestrator's delegation guard, quality-gated commit production, Conventional Commits generation with a card-reference footer, and platform-neutral remote integration. This spec layers policy over the existing execution-workspace worktree infrastructure — it adds no new worktree mechanism.

## Related Specifications

- [l1-version-control.md](l1-version-control.md) — the model this implements (VC-1…VC-6).
- [l2-execution-workspace.md](l2-execution-workspace.md) — git worktrees backing the virtual staging area (VC-4).
- [l2-quality-pipeline.md](l2-quality-pipeline.md) — the gate that must pass before a commit is produced (VC-2).
- [l2-kanban-board.md](l2-kanban-board.md) — card boundary defining commit scope (VC-3).
- [l2-orchestration.md](l2-orchestration.md) — the delegation guard that enforces git authority (VC-1, VC-5).

## 1. Motivation

The model requires a clean commit log, a pre-commit review buffer, and enforced role authority. Reusing the execution-workspace worktree as the staging area gives isolation for free; enforcing authority in the orchestration guard (not at commit time) blocks unauthorized pushes before they dispatch.

## 2. Constraints & Assumptions

- This layer applies to office workspaces whose project is git-backed. A workspace without git has no version-control layer, and nothing here assumes one exists; versioning of anything other than git-backed source is out of scope (VC-6).
- The delegation guard is a boundary only because it is backed by confinement: a worker with a shell could otherwise run `git push` itself. Worker sandboxes therefore carry no remote credentials and no network route to the remote (`l2-sandbox-policy`), so a push or merge to a shared branch can happen only through the guarded path (SEC-12).
- The VSA is per-task and in-process; its only durable output is a real git commit.
- Branching strategy (trunk-based | Git Flow) is a Local Settings value; trunk-based is the default.
- `main`/`trunk` is always protected; all work lands via a branch + PR/MR regardless of strategy (VC-5).

## 3. Invariant Compliance (Layer 2)

| L1 Invariant | Implementation |
| --- | --- |
| VC-1 Role-based commit authority | The orchestration delegation guard checks the office's `VersionControlPolicy` authority table before dispatching any `git commit`/`push`/`merge`; an unauthorized request halts + escalates. No worker self-promotes. |
| VC-2 Quality gate before commit | `produce_commit()` runs the quality-pipeline gate against the worktree content; a fail aborts before any commit object is written. |
| VC-3 Card-aligned commit boundary | A commit binds to exactly one card id (or an explicit sub-task within it); the card ref is a mandatory footer. Multi-card or card-less commits are rejected. |
| VC-4 Virtual staging atomicity | The VSA accumulates mutations in an isolated worktree; a failed/cancelled task discards the worktree with no partial flush (all-or-nothing). A failed quality gate is not a failed task: the worktree stays for the fix loop, and no commit is written until the gate passes. |
| VC-5 Remote isolation by branch | No `git push` targets `main`/`trunk`; the guard forces a branch + PR/MR path unconditionally — no autonomy level overrides it. |
| VC-6 Non-git versioning out of scope | Spec/document/binary versioning is untouched here; this layer governs only git-backed source. |

## 4. Detailed Design

### 4.1 Virtual staging area

```text
[REFERENCE]
produce_commit(card, worktree):
  1. quality_pipeline.run(worktree)          -> pass | fail          (VC-2)
  2. fail  -> no commit; findings go back to the task's fix loop, the worktree is kept
              (it is discarded only when the task itself fails or is cancelled, VC-4)
  3. pass  -> collect all mutations into one commit
  4. msg   := conventional_commit(card.title, diff_summary, token_excerpt)
  5. commit on the task branch; finalize worktree                    (VC-3)
```

Workers never write the main working tree; all writes go through the worktree, giving a diff review surface before the commit is sealed. The VSA reuses the execution-workspace worktree lifecycle (slug naming, boot, isPristine, reset+prune) — no new infrastructure.

### 4.2 Branching strategies

| Strategy | Branch lifetime | Merger |
| --- | --- | --- |
| Trunk-based (default) | ≤1 day, small frequent PRs | Orchestrator / Maintainer |
| Git Flow | days–weeks, milestone-gated | Release Manager |

Hotfix branches follow trunk-based semantics even under Git Flow. `main`/`trunk` protected in both (VC-5).

### 4.3 Role authority table

The `VersionControlPolicy` encodes the per-role matrix (Worker/Orchestrator/Maintainer/Release Manager/Manager) over create-branch / commit / push / merge / push-main / configure-remote. The orchestration guard evaluates it at git-op dispatch time; only Manager/Maintainer/Release Manager may merge, none may push `main` directly. The matrix decides which role may *request* a merge or push; where the office runs the development workflow, a merge into the base branch or a push is also externally visible work that waits for the Deliver-stage human authorization (`l1-development-workflow` DW-8) — role authority does not replace it. `configure-remote` decides where the office's code may be sent — an egress destination (SEC-3) — so it is the user's to change, never a role's.

### 4.4 Commit message

```text
[REFERENCE]
<type>(<scope>): <summary>

[body]
Refs: #<card-id>          -- mandatory footer (VC-3 traceability)
```

Types: feat/fix/chore/docs/test/refactor/perf/build/ci. Generation is a harness step reading the VSA diff.

### 4.5 Remote platform integration

Platform-neutral at the git-wire + PR/MR-API level; platform-specific features (PR templates, CI hooks, issue linkage) install as office-scoped extensions in Local Settings → Integrations. The core layer depends on no specific platform.

## 5. Implementation Notes

1. The VSA is the execution-workspace worktree with a commit-production policy on top — no new worktree code.
2. VC-1 authority is a guard in the orchestration delegation layer, called before any `git push`/`merge` dispatch.
3. Commit-message generation is a nodus `@steps` block reading the VSA diff into a Conventional Commits string.

## 6. Drawbacks & Alternatives

**Alternative — direct commits to main by any worker**: violates VC-5; one bad autonomous commit breaks shared state with no buffer. Rejected.

**Alternative — one universal branching strategy**: release-managing offices differ materially from personal-project offices. Rejected — strategy is per-office.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[MODEL]` | `.design/main/specifications/l1-version-control.md` | Invariants VC-1…VC-6 |
| `[EXEC-WS]` | `.design/main/specifications/l2-execution-workspace.md` | Worktree backing the VSA (VC-4) |
| `[QUALITY]` | `.design/main/specifications/l2-quality-pipeline.md` | Pre-commit gate (VC-2) |
| `[KANBAN]` | `.design/main/specifications/l2-kanban-board.md` | Card boundary (VC-3) |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.1 | 2026-09-23 | Core Team | Consistency pass (2026-09-23): A failed quality gate discarded the task's whole worktree, although VC-4 discards only on a failed or cancelled task — a gate failure now keeps the worktree for the fix loop. "Every office workspace is git-backed" contradicted no-VCS workspaces elsewhere — the layer applies where the project is git-backed. The delegation guard is backed by confinement (no remote credentials or route in worker sandboxes), since a shell could otherwise push directly (SEC-12). Role authority now names who may request a merge or push; the development workflow's Deliver-stage human authorization (DW-8) still applies, and configuring a remote is the user's egress decision (SEC-3). A build-plan phase reference removed. |
| 1.0.0 | 2026-07-03 | Core Team | Initial implementation spec — VSA over execution-workspace worktrees, orchestrator-enforced role authority, quality-gated commit production, Conventional Commits + card footer, platform-neutral remotes; maps VC-1…VC-6. |
