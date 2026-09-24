# Mission Mode

**Version:** 1.0.6
**Status:** Stable
**Layer:** implementation
**Implements:** l1-orchestration.md

## Overview

Mission Mode is a focused, two-phase autonomous execution unit: the agent first explores the scope and writes a structured plan (PRD), then iterates on implementation until every acceptance criterion is satisfied or a safety limit is reached. Unlike the ongoing office work loop, a mission has a clear start, a user-controlled checkpoint between phases, and a deterministic termination condition.

## Related Specifications

- [l1-orchestration.md](l1-orchestration.md) - Autonomous execution and budget circuit-breaker.
- [l2-orchestration.md](l2-orchestration.md) - Goal/judge/budget loop; missions are the execution engine for a single delegated goal.
- [l2-kanban-board.md](l2-kanban-board.md) - A mission may reference a board card as its task source.
- [l2-filesystem-layout.md](l2-filesystem-layout.md) - Workspace path where missions are stored.
- [l2-security.md](l2-security.md) - Sandbox constrains what the executing agent can do.
- [l2-tool-security.md](l2-tool-security.md) - Tool guard is active throughout mission execution.

## 1. Motivation

Long-running autonomous tasks need a checkpoint: the agent should not blindly execute an unreviewed plan. Mission Mode inserts exactly one human-controlled checkpoint — after planning and before execution — without requiring continuous supervision. The two-phase design also makes the acceptance criteria explicit (prd.json) so termination is decided by declared checks and an independent judge, never by the executor's own assessment.

## 2. Constraints & Assumptions

- A mission runs within a single workspace; it operates on the workspace's filesystem context.
- Phase 1 (planning) has full read and exploration access, and writes only inside the mission's own directory — a plan the user has not confirmed must not already be executing; Phase 2 (execution) has scaffolding tools deactivated so the agent codes and reasons but does not directly invoke build/package-manager commands.
- The acceptance criteria (user stories with `passes` flags) are the sole termination signal; the agent does not self-terminate based on its own judgment. The executing agent therefore never writes `passes`: it records a claim, and the flag is set by verification it does not control (§4.3).
- A `max_iterations` guard prevents runaway loops when stories never pass.
- Mission state files are written atomically; a crash during a mission leaves the state readable for resume.

## 3. Invariant Compliance (Layer 2 only)

| L1 Invariant | Implementation |
| --- | --- |
| ORC-6 Judged autonomous termination | Termination is determined by `prd.json.userStories[*].passes` — all true → done. `passes` is set only by verification the executor does not control: the story's declared host-run check, or the independent judge of `l2-orchestration` §4.3 for a judged story (§4.3). The executor's own assessment is recorded as a claim and never terminates the loop. |
| ORC-7 Budget circuit-breaker | `max_iterations` is the mission-level circuit-breaker; it stops the loop with a partial-completion report. Each iteration also runs under the per-turn budget (`l2-agent-session` §4.2) and the budget engine's hard stop. |
| ORC-9 Approval gate for high-impact work | User must confirm PRD before Phase 2 starts — this is the mission's mandatory approval checkpoint. The confirmation binds the confirmed story set: a later change to it needs a new confirmation (§4.4). |
| ORC-10 Resumable | `prd.json` and `loop_config.json` persist state; `mission resume` re-enters Phase 2 from the last verified story states (resumption is explicit, never automatic — §4.6). |

## 4. Detailed Design

### 4.1 Mission phases

```mermaid
graph TD
    START[user: /mission start "task text"] --> P1[Phase 1: Exploration + PRD]
    P1 --> PRESENT[agent presents prd.json to user]
    PRESENT --> CONFIRM{user confirms?}
    CONFIRM -->|edits + confirms| P2[Phase 2: Execution loop]
    CONFIRM -->|rejects| REVISE[agent revises prd.json → loop back to PRESENT]
    P2 --> CHECK{all stories pass?}
    CHECK -->|yes| DONE[mission complete]
    CHECK -->|no, within budget| ITER[inject continuation → next iteration]
    ITER --> P2
    CHECK -->|max_iterations reached| PARTIAL[partial completion report]
```

### 4.2 Phase 1 — Exploration and PRD generation

The agent receives the task description and explores the workspace context (codebase, existing board cards, memory). It then writes `prd.json` with the structured plan.

Full read and exploration access is available in Phase 1 — the agent may read files, query memory, and run searches — and it writes only the mission's own artifacts in `<ws>/missions/<mission-id>/`. Changes to the workspace itself wait for the confirmation that ends Phase 1 (ORC-9).

Phase 1 ends when the agent finishes its exploration turn and a valid `prd.json` exists. The agent presents the PRD to the user and pauses. Control returns to the user for review, optional edits to `prd.json`, and explicit confirmation before Phase 2 starts.

### 4.3 PRD format

```text
[REFERENCE]
prd.json {
  project: String,          // short project/task name
  description: String,      // one-paragraph task summary
  userStories: [
    {
      id: String,           // e.g. "US-001"
      title: String,        // one-line story title
      story: String,        // "As a … I want … so that …" or acceptance text
      check: {              // how the story is verified — declared, never silently either
        kind: "runnable" | "judged",
        run?: String,       // runnable: the host-run command or observable condition
      },
      claimed: bool,        // executor: "I believe this is satisfied" — never terminates
      passes: bool          // false initially; set only by the verifier
    }
  ]
}
```

User stories are the unit of work and the unit of verification. The executing agent sets `claimed: true` when it believes a story is satisfied; that triggers verification, never completion. For a **runnable** story the host runs the declared check outside the agent's tool surface — which is how a build or test result reaches a mission whose executor cannot run the toolchain (§4.4) — and for a **judged** story the independent judge evaluates the claim against the story text. Only the verifier writes `passes`; a failed verification clears `claimed` and records why, and a story with `passes: false` triggers another iteration. A check that cannot fail is not a check: the criteria are reviewed for falsifiability at confirmation (AO-3, AO-7), and an empty or malformed story set is refused, never satisfied (AO-10).

### 4.4 Phase 2 — Execution loop

After user confirmation, the loop runs:

```
while not all_stories_pass(prd) and iteration < max_iterations:
    agent.run_turn(context)
    prd = read_prd(loop_dir)
    iteration += 1

if all_stories_pass(prd):
    emit mission_complete(passed=total, total=total)
else:
    emit mission_max_iterations(passed=count_passed, total=total, max=max_iterations)
```

Each iteration the agent receives a continuation message summarizing remaining stories and may use any tools except the deactivated scaffolding set (package-manager CLIs, build runners). This constraint is intentional: the agent writes correct code; builds and tests are verified by the host-run story checks (§4.3), external CI, or the user — not by the agent invoking them directly within the loop. The deactivation is a workflow boundary, not a security one (SEC-12): containment is the sandbox's job.

The confirmed story set is bound to the confirmation (`l1-consent-binding` CB-3). In Phase 2 the executor may update `claimed` and append to `progress.txt`; it cannot add, remove, or reword a story or its check — otherwise it could reach "all stories pass" by editing the stories rather than satisfying them. A change it needs is proposed to the user, and the mission continues only after the changed set is confirmed again.

### 4.5 State files

Every mission lives in an isolated directory:

```plaintext
<ws>/missions/<mission-id>/
├── loop_config.json   # environment metadata (git, session, paths)
├── prd.json           # task list — worker records `claimed`, verifier sets `passes`
├── progress.txt       # append-only iteration log
└── task.md            # original task description (read-only)
```

`<mission-id>` is generated at creation time from a timestamp: `mission-YYYYMMDD-HHMMSS`, with a counter suffix (`-2`, `-3`, …) when a mission with that id already exists, so two missions started within one second never share a directory.

#### loop_config.json

```text
[REFERENCE]
loop_config.json {
  session_id: String,
  branch_name: String,
  git_installed: bool,
  is_git_repo: bool,
  default_branch: String,   // "main" | "master" | <current>
  current_branch: String,
  repo_root: String
}
```

Git context is detected asynchronously at mission creation. `is_git_repo: false` is a valid state (no-VCS workspace). The `session_id` links the loop directory back to its originating session, enabling `get_active_loop_dir` to find the correct mission when multiple missions exist.

#### progress.txt

Append-only log, seeded with a `## Codebase Patterns` section that the agent populates with reusable insights discovered during the run. New entries are appended after each iteration summary. This file is never truncated during a mission.

### 4.6 Resuming a mission

On restart, the user can call `mission resume` to re-enter Phase 2 from the persisted state:

1. Locate the named mission, or else the most recent unfinished mission in the workspace. A restart always runs in a new session, so the recorded `session_id` identifies where the mission came from; it is not required to match.
2. Read `prd.json` — stories with `passes: true` are already done; only false stories remain. A `claimed` story whose verification had not finished is verified again before it counts.
3. Re-enter the execution loop from the current story count.

Missions are not automatically resumed on process restart; the user issues `mission resume` explicitly.

### 4.7 Command surface

| Action | CLI | TUI | Library (no code) |
| --- | --- | --- | --- |
| start mission | `cronus mission start "<task>"` | `/mission start …` | `mission.start(task) -> Mission` |
| confirm PRD (enter Phase 2) | `cronus mission confirm` | `/mission confirm` | `mission.confirm() -> void` |
| show status | `cronus mission status` | `/mission status` | `mission.status() -> MissionStatus` |
| list missions | `cronus mission list` | `/mission list` | `mission.list() -> Mission[]` |
| resume | `cronus mission resume [<id>]` | `/mission resume` | `mission.resume(id?) -> void` |
| abort | `cronus mission abort [<id>]` | `/mission abort` | `mission.abort(id?) -> void` |

### 4.8 Discuss-phase decision capture

Before a mission is planned, a structured discussion phase extracts implementation decisions. The discussion agent is a thinking partner, not an interviewer: it identifies ambiguous implementation choices ("gray areas"), presents them to the user, and captures concrete decisions for downstream planners and researchers to act on.

#### Decision document format (CONTEXT.md)

```text
[REFERENCE]
# Phase [N]: [Name] — Context

**Gathered:** [date]
**Status:** Ready for planning

<domain>
## Phase Boundary
[Clear scope statement from the roadmap — fixed; discussion clarifies HOW to implement, never WHETHER to add new capabilities]
</domain>

<decisions>
## Implementation Decisions

### [Topic area]
- **D-01:** [Specific decision — concrete enough for planner to act without asking again]
- **D-02:** [Another decision if applicable]

### Agent's Discretion
[Areas where user explicitly deferred to the agent — agent has flexibility here]
</decisions>

<deferred>
## Deferred Ideas
[Ideas that surfaced but belong in other phases. Captured so they're not lost, but explicitly out of scope for this phase.]
</deferred>

<canonical_refs>
## Canonical References
[Specs, ADRs, or design docs that agents must read before planning or implementing — full relative paths]
</canonical_refs>
```

#### Decision ID traceability (D-NN)

Each decision is assigned a stable ID (D-01, D-02, …). Planner agents must:

- Reference the decision ID in task actions ("per D-03")
- Include at least one task implementing every locked decision
- Never include tasks implementing a deferred idea

Decisions are binding; the planner honors them even when research suggests a different approach. Any conflict between a locked decision and research findings is documented with an explanation ("using X per D-02; research suggested Y").

#### Scope guardrail

Discussion scope is fixed by the roadmap boundary. New capabilities cannot be added during discussion:

- **Allowed:** clarifying HOW to implement what is already in scope (layout, behavior, edge cases)
- **Not allowed:** adding new capabilities ("what about also adding X?")

When the user suggests out-of-scope work, the agent captures it under "Deferred Ideas" — not lost, not acted on.

#### Gray area identification

Gray areas are implementation choices the user cares about that could go multiple ways and would change the delivered result. The agent identifies them by reading the phase goal and considering what would be visible/observable to the user:

- Things users SEE, CALL, RUN, or READ
- Choices between multiple valid implementations where the user has a preference
- Behaviors on edge cases or empty states that are product decisions, not engineering decisions

### 4.9 Phase lifecycle state machine

Mission phases follow a deterministic status progression. Each phase status is machine-readable and gates whether planning, execution, or verification may run:

```text
[REFERENCE]
Phase status values:
  Pending      — Phase exists in roadmap; no discussion or planning has started
  Planned      — CONTEXT.md or PLAN.md files exist; planning complete, not yet executing
  In Progress  — Execution is running; some plans have summaries, some do not
  Executed     — All plans have SUMMARY.md files; awaiting verification
  Needs Review — Execution complete but verification has FAIL items without overrides
  Complete     — All SUMMARY.md files exist AND VERIFICATION.md shows status: passed

Status transition guards:
  Planned → In Progress  : at least one executor has committed work
  Executed → Complete    : VERIFICATION.md with status: passed added
  Complete → replanned   : HARD BLOCK unless --force flag; prevents overwriting shipped evidence
```

The `Complete` status is a hard gate: replanning a closed phase silently rewrites plan documents that no longer match the shipped code. An explicit override flag is required, and a warning banner must be emitted if that flag is used.

Prior-phase completeness scan runs before any phase advance:

- Plans without matching summaries (execution started, not completed) → warning with deferred-backlog option
- Prior phases with unresolved VERIFICATION.md FAIL items → hard stop

### 4.10 SUMMARY.md rich frontmatter

Each plan produces a SUMMARY.md after completion. The frontmatter carries a structured dependency graph and tech-tracking metadata that downstream agents and future planning phases use to understand what each plan built and how it relates to adjacent work:

```text
[REFERENCE]
SUMMARY.md frontmatter:

---
phase: XX-name
plan: NN
subsystem: [primary category: auth, payments, ui, api, database, infra, testing, etc.]
tags: [searchable tech terms: jwt, stripe, react, postgres, prisma]

# Dependency graph
requires:
  - phase: [prior phase or plan this depends on]
    provides: [what that phase built that this plan uses]
provides:
  - [bullet list of what this plan built/delivered]
affects: [list of phase names or keywords that will need this context]

# Tech tracking
tech-stack:
  added: [libraries/tools added in this plan]
  patterns: [architectural/code patterns established]

key-files:
  created: [important files created]
  modified: [important files modified]

key-decisions:
  - "Decision 1"
  - "Decision 2"

patterns-established:
  - "Pattern 1: description"

requirements-completed: []  # REQUIRED — all requirement IDs from this plan's `requirements` field

# Metrics
duration: Xmin
completed: YYYY-MM-DD
---
```

The `requires/provides/affects` graph enables future planner agents to detect what prior plans built and whether they need to load earlier summaries for context. The `requirements-completed` field is mandatory: it must copy every requirement ID from the plan's own `requirements` frontmatter field so that coverage can be verified across the entire phase.

### 4.11 Operation mode ladder

Missions can run at multiple intensity levels, controlling how aggressively the agent applies simplification, verification depth, and context loading. The active mode persists for the duration of the session and is resolved from three sources in priority order.

#### Intensity levels

| Mode | Behavior |
| --- | --- |
| `lite` | Minimal overhead — plan quickly, execute with reduced verification depth. Use for low-stakes or well-understood tasks. |
| `full` | Default mode — full discuss-phase, complete planning, all quality gates. Balanced cost and thoroughness. |
| `ultra` | Maximum rigor — extended adversarial review, goal-backward verification with multiple passes, exhaustive must_haves coverage. Use for high-stakes or novel work. |
| `off` | Disable mission mode — the agent runs as a standard session without structured planning or execution phases. |

#### Resolution hierarchy

The active mode is resolved at session start using a three-source priority chain:

```text
[REFERENCE]
Mode resolution order (first source that provides a value wins):

  1. Environment variable: CRONUS_MISSION_MODE=lite|full|ultra|off
     Set by CI/CD pipelines, launch scripts, or per-session shell exports.
     Highest priority — overrides all other sources.

  2. Workspace config file: <ws>/config.json → { "missionMode": "full" }
     Set by the user once per workspace. Survives session restarts.
     Mid-priority — overrides the compiled default but not the env var.

  3. Compiled default: full
     Always available. No setup required — missions work out-of-box.
```

The agent reads the resolved mode at session start and applies it to all phases (discuss/plan/execute/verify) for the current session. The mode does not auto-advance or escalate mid-mission unless explicitly changed. The mode in force is recorded in the mission's `loop_config.json` when the mission starts. Because the mode sets verification rigor, changing it is a user act (`/mission mode`): the executing agent cannot change it, and a lower mode written into a file the agent can edit (the workspace config) never applies to a mission in progress — a resumed mission whose recorded mode differs from the currently resolved one asks the user which applies.

#### Flag file state tracking

The active mission mode is written to a flag file so that status line renderers and hooks can read it without running the full resolution chain:

```text
[REFERENCE]
Flag file: <ws>/.mission-mode
Content: plain text, one of: lite | full | ultra | off | (empty = no mission active)

Write events:
  - Written when a mission starts (contains the resolved mode for this run)
  - Updated when the user changes mode mid-session (/mission mode ultra)
  - Cleared (or written "off") when a mission completes or is aborted

Read by:
  - Status line hook: renders [MISSION], [MISSION:FULL], [MISSION:ULTRA] etc.
  - Pre-phase hooks: may skip expensive steps when mode is lite
  - Resume logic: verifies the persisted mode matches the current config before re-entering;
    on a mismatch it asks the user, never silently downgrading
```

The flag file is the cross-session state signal — it lets peripheral tooling observe mission state without importing core logic.

### 4.12 Proposal artifact

Before discussion and planning begin, a structured proposal captures the **why and what** of a change — its intent, scope, expected capabilities, and impact on existing subsystems. The proposal is the root node of the artifact dependency graph (§4.15 in l2-orchestration.md): all downstream artifacts (CONTEXT.md, PLAN.md, DECISION-*.md) draw from it.

#### Proposal format

```text
[REFERENCE]
proposal.md (stored at <ws>/planning/changes/<change-id>/proposal.md):

---
change-id: <slug>
created: YYYY-MM-DD
status: draft | ready | accepted | rejected
mode: lite | full | ultra   # active operation mode when proposal was written
---

## Intent
Why are we making this change? What problem does it solve?
One paragraph. No jargon. Written for someone unfamiliar with the task.

## Scope

### In scope
- What this change will deliver

### Out of scope
- What this change explicitly will NOT address (at least one entry required)

## Approach
Technical direction — the high-level "how". Not a full design, but enough to
make the scope concrete. Reference existing architecture decisions (AD-n, D-NN) where relevant.

## Capabilities

### New capabilities
- `<capability-id>`: One-line description of what this capability enables.

### Modified capabilities (if any)
- `<capability-id>`: What changes and why.

## Impact
Which Cronus subsystems are affected? List by spec name:
- orchestration: ...
- quality-pipeline: ...
- agent-constitution: ...

## Rollback plan
(Required when mode = full or ultra. Optional for lite.)
How do we revert if this change turns out to be wrong?
What artifacts, files, or database entries would need to be undone?
```

#### Proposal lifecycle

```text
[REFERENCE]
Proposal status transitions:

  draft     — being written; not yet reviewed by user
  ready     — agent has completed the proposal; awaiting user review
  accepted  — user explicitly confirms; downstream work may begin
  rejected  — user rejects; proposal archived with reason; no downstream artifacts created

Transition rules:
  - draft → ready: triggered when agent signals completion of the proposal turn
  - ready → accepted: triggered by user confirmation (/mission confirm or an explicit
    "accepted" from the user), recorded by the host — a `status: accepted` the agent
    writes into the file is not an acceptance
  - ready → rejected: triggered by user rejection with reason
  - accepted → (no further transitions): the proposal is immutable once accepted

Proposal immutability:
  Once accepted, proposal.md becomes read-only. If scope changes after acceptance,
  a new change-id is created with a fresh proposal that references the prior one as context.
  The prior proposal is NOT edited — changes are always additive, never retroactive.
```

#### Mode-specific proposal behavior

| Mode | Scope section | Rollback required | Out-of-scope items |
| --- | --- | --- | --- |
| `lite` | Bullet list only | No | Skipped if none surfaced |
| `full` | Bullets + brief explanation | Yes | At least 1 entry required |
| `ultra` | Full paragraph + alternatives considered | Yes + tested | At least 3 entries, with rationale |

### 4.13 Spec persistence model

Teams that update the PRD or encounter mid-mission discoveries face a choice: propagate changes forward into `plan.md` and `tasks.md`, absorb them back into `prd.json`, or keep all files as live sources. Each strategy has different drift risk and audit-trail properties.

#### Three models

| Model | Definition | Strengths | Risk |
| --- | --- | --- | --- |
| **Flow-forward** | New requirements add new task phases; previous phases are immutable | Full audit trail; old decisions preserved | Growing task file; some duplication |
| **Flow-back** | Any artifact may be edited; changes reconciled manually | Fast iteration in early exploration | Drift between plan, PRD, and tasks |
| **Living spec** | `prd.json` is the single source; `plan.md` and `tasks.md` regenerated from it | Consistency guaranteed | Implementation rationale not captured in PRD |

#### Model selection

```json
{
  "mission": {
    "spec_persistence": "flow-forward"
  }
}
```

Default: `flow-forward`. Switching models mid-mission requires a manual migration step.

#### Enforcement per model

**Flow-forward**: the orchestrator refuses edits to task phases older than the current wave. New discoveries append a new phase at the end of `tasks.md`.

**Flow-back**: agents warn when `prd.json` and `tasks.md` diverge by more than 20% of task count. A reconciliation command (`cronus mission reconcile`) surfaces the diff.

**Living spec**: agents regenerate `plan.md` and `tasks.md` from `prd.json` on every mission start. Manual edits to those files are overwritten — the prior content is archived before regeneration, and the user is told which non-PRD content was replaced and where it went.

The default `flow-forward` matches Cronus's append-only archive principle: decisions are never rewritten, only superseded.

### 4.14 Structured clarification protocol

The discuss-phase (§4.8) captures decisions in CONTEXT.md via free-form agent reasoning. That works for architectural choices but leaves scope ambiguities unresolved until plan generation, where rework is costly. A structured clarification step runs before `plan.md` is written to surface questions the agent cannot answer alone.

#### When it runs

Clarification runs automatically at the end of the discuss phase, before plan generation. Triggered on demand with `cronus mission clarify`.

#### Protocol

The agent generates up to 7 clarifying questions from the PRD and CONTEXT.md content. Questions target:

- Scope boundaries ("does this include mobile?")
- Acceptance criteria ambiguity ("what counts as 'fast enough'?")
- Dependency ownership ("who provides the auth token?")
- Non-functional requirements (uptime, data retention)
- Out-of-scope confirmation ("is X intentionally excluded?")

Questions are written to `.planning/clarifications.md`:

```markdown
# Clarifications

Status: pending | complete

## Q1: [Question text]

**Answer**: [User fills this in, or agent fills from PRD evidence]
**Source**: user | prd | inferred
**Locked**: true | false
```

The agent fills questions it can answer from the PRD (marking `source: prd` and citing the passage). User-facing questions remain `source: user` until answered, and only the user answers or skips them: the agent never supplies the user's side (XPL-4). An `inferred` answer is the agent's assumption, recorded and shown as one (GRD-8), never presented as the user's decision. Plan generation does not begin until all non-locked questions either have an answer or are marked `skipped: true` with a rationale.

#### Iteration

Running `cronus mission clarify` after PRD edits appends new questions; answered questions are not re-asked. The planner treats `clarifications.md` as a locked input and MUST NOT contradict any `locked: true` answer.

### 4.15 Session intent classification

The structured clarification protocol (§4.14) collects questions, but the system does not yet know what *kind of work* the agent is doing. Intent classification auto-assigns a work type and phase intent to each execution session, enabling phase-specific routing and retrospective segmentation.

#### Work types

```text
feature     — new capability (add, build, create, implement)
bugfix      — defect correction (fix, bug, error, broken, patch)
refactor    — restructuring without behavior change (refactor, rewrite, clean, optimize)
review      — inspection or explanation (review, audit, inspect, explain)
docs        — documentation update (doc, readme, comment, docstring)
test        — test authoring or correction (test, spec, unit, integration, e2e)
config      — environment or tooling (config, env, setup, docker, deploy)
```

Classification is derived from the first-prompt text, existing artifact types, and the active operation mode.

#### Phase intent

Orthogonal to work type, phase intent maps execution sessions to lifecycle stages:

| Intent | Signals | Lifecycle stage |
| --- | --- | --- |
| `Planning` | plan mode, `plan/architect/design/scope/RFC` keywords | Discuss / CONTEXT-building |
| `Implementation` | code artifacts present, `build/create/implement` keywords | Wave execution |
| `Debugging` | `fix/error/exception/crash/trace` keywords | Rework / error correction |
| `Review` | `review/explain/audit/walk-me-through` keywords | Decision review / safety gate |
| `Verification` | `test/validate/check/assert/confirm` keywords | VERIFIED phase |
| `Exploration` | `how-to/what-is/example/tutorial` keywords | Spike / learning |

#### Recording

Both are written to the phase's SUMMARY.md:

```markdown
---
work_type: feature
phase_intent: Implementation
intent_confidence: 82
---
```

#### Routing rules

| work_type | Routing effect |
| --- | --- |
| `bugfix` | Rework gate: verify a D-NN decision exists before phase starts |
| `refactor` | Architecture gate: require AD reference before wave execution |
| `test` | Verification gate: emit VERIFIED finding on completion |
| `review` | Safety gate: adversarial review (3-lane, §4.11 of `l2-quality-pipeline.md`) |
| `feature` | Default story flow (PLAN.md → wave → SUMMARY.md) |
| `docs` | Lightweight flow: CONTEXT.md + SUMMARY.md only; no PLAN.md required |
| `config` | Infrastructure gate: require env-safety checklist (no secrets, .gitignore present) |

The classification is a heuristic read from prompt wording (SEC-12), so it may **add** gates but never remove them: the gates that apply are those of every work type the session's actual changes fall under. A session classified `docs` whose changes touch code gets the gates for code; the lightweight flow applies only when the change set is documentation-only.

## 5. Drawbacks & Alternatives

- **Deactivated scaffolding tools:** the agent cannot run `cargo build` or `npm test` directly. Acceptance criteria that require a passing build are written as runnable story checks, which the host runs as the verifier (§4.3) — the executor claims, the check decides.
- **PRD as sole termination signal:** if the agent writes a story with poorly-chosen acceptance criteria, the mission may loop indefinitely until `max_iterations`. Mitigation: the user reviews and can edit `prd.json` before Phase 2; `max_iterations` is the backstop.
- **No parallel story execution:** stories execute sequentially within one agent session. A future enhancement could run independent stories in parallel agent forks (requires story-level dependency declarations in prd.json).
- **Alternative — continuous autonomous loop without checkpoint:** rejected. The Phase 1 → user-confirm → Phase 2 pattern is the safety contract; removing the checkpoint eliminates the user's ability to catch a badly-scoped plan before execution.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[ORC]` | `.design/main/specifications/l1-orchestration.md` | Autonomy invariants |
| `[L2ORC]` | `.design/main/specifications/l2-orchestration.md` | Goal/judge/budget loop |
| `[LAYOUT]` | `.design/main/specifications/l2-filesystem-layout.md` | Workspace missions directory |
| `[CLI]` | `.design/main/specifications/l2-cli.md` | Command grammar standard |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.6 | 2026-09-23 | Core Team | Consistency pass (2026-09-23): ORC-6: the executing agent set the `passes` flags that terminate the loop — self-assessment the spec claimed to exclude. The executor now records `claimed`; `passes` is set only by the story's host-run check or the independent judge, with falsifiable criteria (AO-3/AO-7/AO-10). ORC-9: Phase 1 had full tool access before the plan was confirmed, and in Phase 2 the executor could rewrite the confirmed stories — Phase 1 writes only planning artifacts, and the confirmed story set is bound to the confirmation (CB-3). Resume matched the recorded session, which never matches after a restart. Mission ids could collide within a second. The verification-rigor mode could be lowered through an agent-writable file and a mode mismatch on resume was unspecified. A `status: accepted` written by the agent counted as proposal acceptance. The agent could answer or skip the user's clarification questions (XPL-4). Keyword work-type classification could remove gates — it may only add them. A vendor-specific heading in the decision template renamed; the adversarial-review reference pointed at the wrong quality-pipeline section (§4.8 → §4.11); living-spec regeneration archives what it overwrites. |
| 1.0.5 | — | Core Team | Last version before this section was added; earlier revisions are recorded in version control. |
