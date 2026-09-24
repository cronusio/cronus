# Agent Framework Skeleton

**Version:** 1.0.1
**Status:** Stable
**Layer:** concept

## Overview

A paradigm-neutral reference skeleton for agent systems. It names the small set of
primitives, coordination patterns, and cross-cutting concerns that every agent
framework is assembled from — regardless of whether the framework presents itself
as a linear role-crew, a graph state-machine, a conversational group, a handoff
swarm, an autonomous self-evolving loop, or a catalog of specialist roles. These
surface forms differ; the underlying skeleton is nearly identical.

This spec is a **meta-spec**: it does not introduce a new subsystem. It generalizes
the project's existing concept specs into one coherent template and a reusable
checklist, and it flags the few cross-paradigm ideas that are not yet captured
elsewhere (conditional handoff ordering, the anti-collapse rule for self-improvement).
Concrete subsystems remain authoritative for their own design; this document is the
map that shows how they compose.

## Related Specifications

- [l1-orchestration.md](l1-orchestration.md) - Adaptive topology, delegation, error containment (ORC-11); the runtime realization of §5.2/§5.4.
- [l1-execution-graph.md](l1-execution-graph.md) - Typed state channels, supersteps, interrupt/resume, checkpoint durability; the graph engine of §5.3–§5.5.
- [l1-office-model.md](l1-office-model.md) - Orchestrator + roles + client interaction; the primitive triad of §5.1 in product form.
- [l1-roles.md](l1-roles.md) - Roles as specialties; the role-definition unit of §5.8.
- [l1-deliberation.md](l1-deliberation.md) - Parallel independent reasoning then synthesis; the fan-out/in and mesh patterns of §5.2.
- [l1-output-contracts.md](l1-output-contracts.md) - Inline output validation with retry; the evaluator-optimizer loop of §5.2 and AFS-11.
- [l1-tool-composition.md](l1-tool-composition.md) - Toolkits, dependency DAG, single authorization surface; AFS-12.
- [l1-inner-monologue.md](l1-inner-monologue.md) - Non-interrupting background reflection; part of the self-improvement loop of §5.7.
- [l1-harness-engineering.md](l1-harness-engineering.md) - Evidence-backed iterative improvement; the disciplined counterpart to §5.7.
- [l1-quality-standards.md](l1-quality-standards.md) - Tiered quality gates; the deployment gate of AFS-11.
- [l1-telemetry.md](l1-telemetry.md) - Privacy-first observation; the data-safety boundary of AFS-9.
- [l1-security.md](l1-security.md) - Secret isolation, sandboxing, no exfiltration; AFS-12.
- [l1-host-native-rendering.md](l1-host-native-rendering.md) - [ADDED v1.0.1] The full contract AFS-14's "canonical-source → per-host adapter distribution with drift detection" hands off to: render determinism (HNR-2), declared fidelity classes per-unit/collapsed/opaque with honest loss (HNR-3/HNR-4), section roles as the split contract (HNR-5), stable slug identity (HNR-6), declared destinations (HNR-7), and previewable idempotent materialization (HNR-8). AFS-14 states that a canonical catalog projects outward; that spec states what a projection must guarantee.
- [l1-corpus-originality.md](l1-corpus-originality.md) - [ADDED v1.0.1] The admission gate keeping the AFS-14 role catalog free of re-skins; demarcated from AFS-13, whose novelty rule guards a self-improvement **lineage** across generations rather than a **catalog** at rest.

## 1. Motivation

Agent frameworks look diverse on the surface but converge on a shared anatomy. A
linear role-crew, a directed graph of nodes, a turn-taking conversation, a handoff
swarm, and an always-on autonomous agent all reduce to: *some actors, each with an
identity and tools, exchanging typed work along an explicit topology, under a
coordinator that decides who acts next and when to stop.* Once that is seen, three
benefits follow:

1. **No reinvention.** New subsystems plug into a known skeleton instead of
   rediscovering it. The primitive set and the pattern catalog are a starting
   template, not a blank page.
2. **A completeness checklist.** The concerns that are easy to skip in a demo and
   fatal in production — human gates, traceability, failure containment, evaluation,
   least privilege — become an explicit, auditable list rather than tribal knowledge.
3. **Surfaced blind spots.** Cross-paradigm study reveals patterns no single paradigm
   makes obvious: cheap-before-expensive handoff condition ordering, and the
   observation that a self-referential improvement loop with no external novelty does
   not stabilize — it degenerates into self-repetition.

The cost of *not* having this map is silent divergence: each subsystem re-deriving
topology, state-merge, and failure semantics slightly differently, with no shared
vocabulary to catch the contradictions.

## 2. Constraints & Assumptions

- **Technology-agnostic.** This is a Layer 1 concept. It names no language, library,
  transport, or storage engine. Concrete bindings live in Layer 2 specs.
- **Paradigm-neutral.** The skeleton MUST accommodate at least: linear pipelines,
  graph state-machines, turn-taking conversations, handoff swarms, and autonomous
  long-running loops. A rule that holds for only one surface form does not belong here.
- **On-device-first.** User data and intermediate artifacts stay local unless the user
  authorizes egress; the skeleton assumes a privacy-preserving substrate (inherited
  from the security and telemetry concepts).
- **Generalization, not replacement.** Where a concrete spec already governs a concern,
  this document defers to it and links it. This spec wins only on the cross-paradigm
  vocabulary and the two gap-filling invariants (AFS-7 ordering, AFS-13 anti-collapse).
- **Composition over inheritance.** Patterns are composed (a hierarchical orchestrator
  whose subagents each run an evaluator-optimizer loop), not arranged in a type
  hierarchy.

## 3. Core Invariants

Layer 2 realizations and concrete subsystems MUST NOT violate these.

- **AFS-1 Primitive triad.** Every agent system reduces to three primitives — an
  **Agent** (identity + tools + an act loop), a **Work-Unit** (a typed task, message,
  or goal with a contract), and a **Coordinator** (the policy that selects the next
  actor and the termination condition). Any design must be expressible in these terms.
- **AFS-2 Explicit contracts.** Each agent declares what it *receives*, what it
  *produces*, what it is *not responsible for*, and its *success criteria*. Prose
  responsibilities without an input/output contract are not a design.
- **AFS-3 Topology is explicit and justified.** The coordination shape is documented
  with its data flow, not left implicit. Peer/mesh topology — the most expensive to
  debug — requires a moderator and a termination condition and an explicit
  justification; the default is hierarchical.
- **AFS-4 Typed shared state with deterministic merge.** State exchanged between agents
  is typed, and concurrent writes are merged by a declared reducer (last-write,
  accumulate, topic-append, barrier). Each agent reads and writes only its scoped
  fields. Required fields are never silently truncated to fit a budget — on overflow
  the system compresses, and if compression still drops a required field it halts and
  escalates.
- **AFS-5 Bounded coordination.** Every coordination loop has a termination condition
  expressed as a hard ceiling (max rounds, max iterations, token/cost budget, or an
  explicit done-signal). No loop relies solely on the actors deciding to stop.
- **AFS-6 Durable boundaries.** State is checkpointed at coordination boundaries
  (each superstep) and before every irreversible side effect, so a run can be
  interrupted, inspected, and resumed from the last boundary without re-doing
  committed work.
- **AFS-7 Ordered conditional handoff.** When control transfers between agents by
  condition, conditions are evaluated cheapest-first: deterministic context
  conditions before model-evaluated conditions before a default fallback transition.
  An unconditional default (where control goes when nothing else fires) is always
  defined.
- **AFS-8 Human gates on consequence.** Irreversible, high-blast-radius, or
  low-confidence actions pass through a human-in-the-loop gate. Gate strength is
  calibrated to avoid both over-escalation (rubber-stamping) and under-escalation
  (false confidence); every blocking gate defines its timeout behavior.
- **AFS-9 Traceable by construction.** A run carries one shared trace identifier;
  every agent action emits a structured span attributing latency, token, and cost,
  with a status (success/failure/partial/escalated). A wrong answer that cannot be
  traced to the action that produced it means the system is not observable. Traces
  carry no raw secret or private user text.
- **AFS-10 Contained failure.** Each coordination boundary classifies failures by a
  taxonomy (hard / silent / partial / contradiction / cascade / loop / context) and
  contains them before propagation: circuit-break repeated failures, fall back through
  a degraded chain, and make retryable actions idempotent or paired with a
  compensation. The system always produces *something* — a structured degraded result
  beats a silent failure.
- **AFS-11 Evaluation-gated change.** A new or modified agent ships only with an eval
  suite, a recorded baseline, a meets-or-exceeds score, and a full-pipeline regression
  check. "It worked when I ran it" is not a deployment gate.
- **AFS-12 Least privilege and hostile input.** Each agent receives only the tools and
  data its role requires; capability grants are not passed between agents. Any content
  from outside the trust boundary is treated as data, never as instructions, and
  outputs are schema-validated.
- **AFS-13 Novelty-bound self-improvement.** A loop that improves an agent from its own
  outputs MUST draw on a source of external novelty (new tasks, fresh inputs, outside
  text, or human feedback). A purely self-referential improvement loop does not
  stabilize — it narrows into self-repetition. Self-improvement is additionally
  bounded (token budget per reflection cycle) and disciplined (log the intended action
  before acting, and never mutate shared stores except through the standard
  interfaces).
- **AFS-14 Canonical role catalog with projected distribution.** Reusable agent roles
  are preset definitions in one canonical source, organized by a declared division
  set that is the single source of truth. Each host environment receives the same
  roles via thin generated adapters, never hand-maintained divergent copies; drift
  between the source of truth and its projections is detectable.

## 5. Detailed Design

### 5.1 The primitive triad

Every system in scope is built from three primitives. The fields below are the
union observed across paradigms; a given system populates the subset it needs.

**Agent** — an actor that can take a turn.

| Facet | Purpose |
| --- | --- |
| Identity | A role/name plus an intent statement (a goal and a grounding "backstory"/persona/vibe). Anchors behavior and makes the agent selectable by a coordinator from its description. |
| Model binding | The reasoning engine, with per-agent overrides (reasoning depth, system prompt template). |
| Tools | The scoped capability set (see AFS-12). |
| Memory | Working memory plus optional long-term/entity recall (see §5.6). |
| Act loop | The internal reason→act→observe cycle, bounded by a max-iteration ceiling. |
| Autonomy knobs | Human-input mode (always / never / on-terminate), delegation permission, max consecutive auto-replies, termination predicate. |

**Work-Unit** — the typed thing that flows.

| Facet | Purpose |
| --- | --- |
| Description / payload | What is to be done, or the message content. |
| Assignee | The agent (or selection policy) responsible. |
| Input contract | The fields this unit carries in, with types. |
| Expected output | The shape of the result, optionally a schema. |
| Success criteria | Machine- or model-checkable conditions for "done" (see AFS-2, AFS-11). |
| Context | Upstream results this unit may read, scoped (see AFS-4). |

**Coordinator** — the policy that runs the system.

| Facet | Purpose |
| --- | --- |
| Topology | The pattern from §5.2 governing data flow. |
| Selection policy | How the next actor is chosen (static order / manager decision / model-picked speaker / condition-triggered handoff / round-robin / random). |
| Termination | The hard ceiling and done-signal (AFS-5). |
| State ownership | Which actor may write which field (AFS-4). |

### 5.2 Coordination pattern catalog

Five canonical patterns recur across all paradigms. They compose. For each: when to
use, the dominant failure mode, and the design rules that contain it.

| Pattern | Shape | Use when | Dominant failure mode | Containment rules |
| --- | --- | --- | --- | --- |
| **Sequential chain** | A→B→C | Each step depends on the prior; linear progression; debuggability over latency. | One failure halts all; context loss compounds across hops. | Pass structured outputs, not prose; each step appends a short context summary; cap chain length (~5); state each step's not-responsible-for. |
| **Parallel fan-out / fan-in** | router→{A,B,C}→synthesizer | Subtasks independent; latency matters; multiple perspectives on one input. | Partial results; race on shared mutable state. | Fan-out agents must be truly independent (no shared mutable state); synthesizer handles all-present / partial / zero; merge strategy chosen up front (vote/weight/concat/defer); cap width (~7). |
| **Hierarchical (orchestrator–subagent)** | orchestrator→{subagents}, feedback up | Complex, dynamically decomposed tasks; subtask set unknown up front; needs a judging layer. | Orchestrator becomes bottleneck; prompt grows unbounded; subagents locally succeed but mutually contradict. | Orchestrator decomposes/delegates/synthesizes — does not execute; keeps a task ledger; subagents return structured result + confidence; orchestrator detects and resolves contradiction; subagent outputs summarized, not appended whole. |
| **Evaluator–optimizer loop** | generator→evaluator→(pass\|fail+feedback) | Output quality is scorable; first pass expected imperfect; refinement worth the cost. | Infinite loop on impossible criteria; plateau; evaluator shares the generator's blind spots. | Hard max iterations (~3); evaluator framed differently from generator (ideally different model/prompt); structured score + actionable reasons; exit and escalate on a 2-round plateau. |
| **Mesh / peer** | A⟷B⟷C⟷D | Agents must negotiate or reach consensus; no single agent has full context. | Highest complexity; circular dependency; consensus deadlock; exponential context growth. | Rarely correct for production — justify over hierarchical; require a moderator and termination (max rounds / consensus threshold); scope peer read access (summary vs full); define the consensus rule; circuit-break to a human after N rounds. |

Three concrete **engines** realize these patterns; a system usually picks one as its
substrate and composes patterns inside it:

- **Graph / superstep engine** — patterns expressed as a directed graph of nodes over
  typed state channels, advanced in deterministic supersteps (active nodes run, their
  writes merge via reducers, the next active set is computed). Native fit for
  sequential, fan-out/in, and conditional routing; durable by checkpointing each
  superstep. (Governed by the execution-graph concept.)
- **Conversation / speaker-selection engine** — patterns expressed as a shared message
  log with a manager that selects the next speaker (model-picked, manual, round-robin,
  or random) until a termination message. Native fit for hierarchical and mesh; human
  participation is just an agent whose input mode is "always".
- **Handoff / swarm engine** — a flat set of peers where control transfers by
  declarative condition (AFS-7). Native fit for routing and dynamic hierarchies
  without a central orchestrator holding all context.

### 5.3 Shared state and handoff

State is a typed object with per-field **reducers** that make concurrent writes
deterministic:

- *last-write* — the field holds the most recent write (default scalar).
- *accumulate* — writes combine via a declared associative operation (sum, list-append).
- *topic-append* — every write is retained in order (an event stream).
- *barrier* — the field is ready only once all expected writers have written.

Scoping rules: each agent's contract names the fields it reads and the fields it
writes; an agent never receives another agent's full instructions; sensitive fields
are excluded from inter-agent state (AFS-12). Context pressure is managed by
summarization (full output + a short summary; downstream reads the summary, with
named fields preserved verbatim), a structured state object (each agent reads only
its fields), external memory (large artifacts written to a store, retrieved on
demand), or checkpoint compression (milestone summaries replace prior detail).

**Conditional handoff** evaluates transitions in the AFS-7 order: deterministic
context conditions first (cheap, no model call), then model-evaluated conditions,
then a default "after-work" target. The default is mandatory — control always has
somewhere to go.

### 5.4 Execution engines and termination

Whatever the substrate, the act loop is a bounded reason→act→observe cycle. Three
termination layers apply together: the per-agent iteration ceiling (AFS-1 act loop),
the per-coordination round/budget ceiling (AFS-5), and an explicit done-signal
(a terminal message, a passing evaluation, or an emitted stop). Loop pathologies —
text repetition, goal re-entry without progress — are detected by counters and
recovered (mild nudge → strong reset → escalate).

### 5.5 Durability, interrupt, and resume

Checkpointing at coordination boundaries (AFS-6) yields three capabilities at once:
**resume** after a crash or pause from the last boundary; **interrupt** for a human
gate (AFS-8) — pause, surface a structured decision, resume on reply; and
**inspection / time-travel** — replay or branch from a past checkpoint. Side-effecting
actions checkpoint *before* the effect and are idempotent or compensable (AFS-10) so a
resume never double-commits.

### 5.6 Cross-cutting concerns

These apply to every pattern and are the production-readiness checklist.

- **Human-in-the-loop gates (AFS-8).** Place a gate on irreversibility, high blast
  radius, or low confidence. Gate types: *blocking approval* (pipeline pauses, human
  approves/rejects/modifies, timeout behavior defined), *advisory flag* (continues,
  flags for async review within a rollback window), *sampling* (review X% at a rate
  that rises with error rate). Every review surface shows the decision, the reasoning
  trace, the alternatives, the consequence of approve vs reject, and the agent's
  confidence — with one-click actions.
- **Observability (AFS-9).** Per action: shared trace id, span id, role id, step,
  timing, token in/out, cost, input hash, output, confidence, tools called, status.
  Per run: total latency/cost/tokens, which agents ran/were skipped/failed, gates
  triggered and human decisions, final status. Root-cause is then a backward trace from
  the wrong output to the producing action, classifying the cause (prompt ambiguity /
  context overload / model limitation / schema mismatch / missing information).
- **Memory and knowledge.** Working memory within a run; optional long-term and
  entity memory across runs; knowledge as access-controlled retrieval (hybrid
  semantic + keyword), recalled as non-authoritative context. In long-lived autonomous
  systems, memory may be the *only* inheritance channel across restarts (§5.7).
- **Failure engineering (AFS-10).** The taxonomy plus circuit breaker
  (closed→open→half-open on a rolling failure rate), the fallback chain
  (full → narrowed → degraded/rule-based → human), and rollback/compensation at every
  irreversible action.
- **Evaluation (AFS-11).** Agent-level evals (functional, instruction adherence,
  schema compliance, confidence calibration, edge cases) and pipeline-level evals
  (end-to-end accuracy, failure recovery, cost compliance, latency SLA, escalation
  rate, regression). Eval-driven development is the deployment gate.
- **Cost and latency governance.** A hard cost ceiling per run with a circuit breaker;
  per-agent cost as a share of total; latency reduced by parallelizing independent
  agents, using lighter models for low-stakes steps, caching subtask outputs, or
  trimming context — each with its named trade-off.

### 5.7 The autonomous self-improvement loop

A distinct mode: a single agent (or lineage) running continuously toward a standing
goal rather than a one-shot task. Its anatomy:

- **Heartbeat.** A thin outer loop re-invokes the agent against a persistent goal and
  a behavior contract, run after run.
- **Voluntary stop and lineage.** The agent may choose to end its own run (a
  stop-signal). A meta-loop then begins a fresh **generation** seeded from canonical
  templates, with only text artifacts carried across — there is no shared live memory
  between generations, only what one generation wrote for the next to read (journal,
  inbox, reflections).
- **Inner monologue.** A non-interrupting background reflection cycle that logs its
  intended action before acting and dispatches changes only through standard
  interfaces (never raw store mutation), bounded by a per-cycle token budget.
- **Self-modification.** Within limits, the agent may rewrite its own goal, contract,
  or tools when that demonstrably serves the standing goal.
- **Anti-collapse (AFS-13).** The load-bearing lesson. Without an external source of
  novelty, such a loop does not converge on stability — it collapses into
  self-repetition: outputs stay locally fluent while the distribution of behaviors
  narrows, the agent increasingly imitating its own past successes. The mitigation is
  structural, not cosmetic: every cycle must ingest something from outside its own
  corpus — new tasks, fresh external inputs, or human feedback — and improvement claims
  must be validated against held-out work, not the agent's own prior output. The
  disciplined, evidence-backed variant of this loop (frozen evaluation, single-variable
  change, transfer validation) is the harness-engineering concept; AFS-13 is the
  minimum guard that keeps any such loop from degenerating.

### 5.8 Role catalog and multi-host distribution

Reusable roles are not ad-hoc prompts; they are structured definitions in a canonical
catalog (AFS-14).

- **Division organization.** Roles are grouped into divisions (a named, colored,
  iconed set). The division registry is the single source of truth; tooling fails
  closed when the registry disagrees with what exists on disk.
- **Role-definition template.** Each role declares: metadata (name, short
  description, visual identity), an identity-and-memory section (what the role is and
  what it tracks across a session), a communication style, critical domain rules,
  core competencies, domain-specific deliverable templates, a workflow, and success
  metrics. This is the unit the office/role concepts instantiate.
- **Canonical-source projection.** One canonical definition per role is projected into
  each host environment by generated thin adapters (the adapter format differs per
  host; the role content does not). Hand-maintained per-host copies are prohibited;
  alignment between source and projection is verifiable (AFS-14).

### 5.9 Ideas-to-adopt mapping

What each studied paradigm contributes to this project, and where it lands. Paradigms
are named by their structural form, not by product.

| Source paradigm | Idea worth adopting | Where it lands |
| --- | --- | --- |
| Graph / state-machine runtimes | Typed state channels with reducers; deterministic supersteps; checkpoint-based interrupt/resume and time-travel. | Already in the execution-graph concept; this spec generalizes it as the substrate of §5.3–§5.5. |
| Role-crew frameworks | The identity triad (role/goal/backstory); the task contract (description + expected output + schema + guardrail); the sequential↔hierarchical process switch with a manager. | Office and orchestration concepts; formalized as the primitive triad §5.1 and patterns §5.2. |
| Conversational group systems | Speaker-selection as a routing policy; explicit termination predicates; human participation modeled as an agent with "always" input mode. | Orchestration and deliberation concepts; captured in §5.2 conversation engine and §5.4 termination. |
| Handoff / swarm systems | Declarative transfer by condition, evaluated cheapest-first with a mandatory default. | **New** — captured as AFS-7 and §5.3; not previously a dedicated invariant. |
| Autonomous self-evolving agents | Heartbeat with voluntary stop; generational lineage with text-only inheritance; and the anti-collapse rule. | Inner-monologue, learning, and harness concepts; the anti-collapse rule is **new** as AFS-13 / §5.7. |
| Role-catalog agencies | Division-organized preset roles; a single role-definition template; canonical-source → per-host adapter distribution with drift detection. | Role and constitution concepts; consolidated as AFS-14 / §5.8. |

## 7. Drawbacks & Alternatives

- **Over-generalization risk.** A meta-spec can drift into vacuous abstraction. Mitigation:
  it earns its place only through the two gap-filling invariants (AFS-7, AFS-13) and the
  shared vocabulary; it defers to concrete specs everywhere else and links them explicitly.
- **Maintenance coupling.** If a concrete spec changes a concern this document
  summarizes, the summary can go stale. Mitigation: this spec states *principles and
  invariants*, not parameters — concrete thresholds and schemas live in the linked
  specs, so it changes far less often than they do.
- **Alternative — no meta-spec.** Let each subsystem stand alone. Rejected: it is how
  topology, state-merge, and failure semantics silently diverge across subsystems, with
  no shared vocabulary to catch the contradictions, and how cross-paradigm lessons
  (AFS-7, AFS-13) stay tribal instead of enforced.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[GRAPH]` | `.design/main/specifications/l1-execution-graph.md` | Authoritative model for typed channels, supersteps, and interrupt/resume referenced by §5.3–§5.5. |
| `[ORCH]` | `.design/main/specifications/l1-orchestration.md` | Authoritative runtime topology and error containment (ORC-11) referenced by §5.2/§5.4 and AFS-10. |
| `[OUTPUT]` | `.design/main/specifications/l1-output-contracts.md` | Authoritative output-validation/retry contract referenced by the evaluator-optimizer pattern and AFS-11. |
| `[HARNESS]` | `.design/main/specifications/l1-harness-engineering.md` | Authoritative disciplined self-improvement loop referenced by §5.7 and AFS-13. |

## Document History

| Version | Date | Change |
| --- | --- | --- |
| 1.0.0 | 2026-06-24 | Initial specification: the paradigm-neutral reference skeleton for agent systems — the primitive triad, coordination patterns, and cross-cutting concerns every agent framework is assembled from. |
| 1.0.1 | 2026-08-06 | Patch — Related Specifications gains links to the host-native rendering contract (the hand-off from the role-catalog distribution invariant) and to the corpus-originality admission gate; no requirement changed. |
