# Office Deliberation

**Version:** 1.1.1
**Status:** Stable
**Layer:** concept

## Overview

Office deliberation is a structured protocol for multi-worker debate. When the orchestrator faces a decision that benefits from independent perspectives — architectural choices, ambiguous requirements, risk evaluation — it assembles a deliberation round: selected workers reason independently from their specialties, and the orchestrator reads all arguments before making the final decision.

The deliberation log is also the primary mechanism for inter-role communication visibility. Users who want to understand HOW the office reached a decision — not just WHAT was decided — read the deliberation log in the Channels sidebar tab.

## Related Specifications

- [l1-office-model.md](l1-office-model.md) — orchestrator authority and role workers that participate
- [l1-orchestration.md](l1-orchestration.md) — delegation and budget protocol used to run deliberation rounds
- [l1-navigation-model.md](l1-navigation-model.md) — Channels tab where the deliberation log surfaces
- [l2-inbox.md](l2-inbox.md) — storage backing for deliberation log entries
- [l1-competitive-execution.md](l1-competitive-execution.md) — [ADDED v1.1.0] the sibling that **selects** one of N whole attempts (CE-4); deliberation **synthesizes** independent arguments and, since v1.1.0, runs a blind cross-critique between them (DL-6).
- [l1-agent-coevaluation.md](l1-agent-coevaluation.md) — [ADDED v1.1.0] shares the anonymized-evaluation discipline; DL-7 anonymizes and position-randomizes arguments in the critique round to defeat authority/positional bias, exactly as blind evaluation does.
- [l1-parallel-staffing.md](l1-parallel-staffing.md) — [ADDED v1.1.0] the throughput fan-out; the critique round (DL-6) is a second parallel fan-out over the anonymized argument set, reusing the same wave model.

## 1. Motivation

Two problems in multi-agent offices motivate deliberation:

1. **Convergence without diversity**: if one agent proposes a solution and others simply affirm it, the multi-agent benefit is illusory. Deliberation forces each participant to reason independently before synthesis.

2. **Invisible decision-making**: autonomous offices make hundreds of decisions per session. Without a visible reasoning trail, users cannot understand, audit, or trust the office's judgment. The deliberation log makes reasoning legible.

## 2. Constraints & Assumptions

- Deliberation is initiated by the orchestrator; workers cannot initiate a deliberation round themselves.
- Participation is bounded (default: 3 participants); larger panels increase cost with diminishing returns.
- The orchestrator always makes the final decision. Deliberation is advisory; it cannot override orchestrator authority.
- Each round operates within a token budget declared before the round opens; if budget is exhausted, the orchestrator decides on available evidence (no round is abandoned without a decision).

## 3. Core Invariants

- **DL-1 Independent reasoning**: each participant generates its argument before seeing any other participant's output. Arguments are gathered in parallel; no participant is influenced by another during argument generation.
- **DL-2 Specialty diversity**: participants are selected to maximise coverage of distinct specialties relevant to the question. If no diverse-enough set of active workers exists, the orchestrator may instantiate temporary specialist workers for the round.
- **DL-3 Orchestrator finality**: the orchestrator issues the final decision after reading all arguments. No voting rule or majority mechanism determines the outcome — the orchestrator synthesises and decides.
- **DL-4 Append-only log**: every deliberation round is written to the deliberation log immediately upon close. The log is immutable; past decisions cannot be retroactively edited. The log is the office's audit trail.
- **DL-5 Budget-bounded execution**: each round declares a token budget upfront. Arguments that exceed the budget are truncated with a marker; they are not silently discarded. Truncation is visible in the log.
- **DL-6 Blind cross-critique round** [ADDED v1.1.0]: between independent argument generation (DL-1) and synthesis (DL-3), a round MAY run in which **each participant critiques the full set of the others' arguments** — naming which argument is strongest and why, which carries the biggest blind spot, and what **all** of them missed. This is the round that makes deliberation more than "ask N times": independent arguments cluster around what each participant individually saw, and the highest-value insight — the flaw in one argument only another notices, the gap the whole panel shares — emerges **only** from cross-examination. The critique round is itself independent (each critic writes before seeing another critic's review, reusing DL-1's parallelism); it is a **single** blind pass, never iterative cross-reading during argument generation, so it does not reintroduce the premature convergence DL-1 forbids. A deliberation that skips straight from N arguments to synthesis leaves every argument's blind spots uncaught. The critique round is **within the round's declared budget** (DL-5), not additive to it: because it roughly doubles the calls, the orchestrator sizes the budget for both rounds up front, and if the budget cannot cover a critique round it is skipped (the decision still proceeds on the arguments alone) rather than the critique being truncated into uselessness — an optional round that would blow the budget is not silently run.
- **DL-7 Anonymized, position-randomized critique — merit, not authority** [ADDED v1.1.0]: in the critique round (DL-6) arguments are presented to each critic **anonymized** (participant identity stripped) and in a **randomized order** (position shuffled per critic), so a critic evaluates the *argument*, not *who produced it* or *where it appeared*. This defeats two specific, well-known biases: **deference** (over-weighting a high-status or high-capability participant — evaluating the model rather than the reasoning) and **positional/primacy bias** (over-weighting whichever argument was seen first or last). A critique round that reveals identities or uses a fixed order reintroduces exactly the biases it exists to remove. De-anonymization happens **only** at synthesis, where the orchestrator (DL-3) sees who said what.
- **DL-8 Full-stance arguments — independence is leaning in, not hedging** [ADDED v1.1.0]: each participant argues its assigned angle **fully**, without softening toward a balanced middle — because a panel of pre-balanced arguments collapses to one averaged view and forfeits the diversity DL-2 selected for. Diversity may be by **specialty** (DL-2) or by deliberately-opposed **stance** engineered to create tension (a downside-seeker against an upside-seeker; a first-principles rethinker against a pragmatic executor; a fresh-eyes outsider against the domain expert). The tension **is** the instrument; a participant that hedges toward consensus has broken it. Balancing the tensions into a decision is the orchestrator's job at synthesis (DL-3), never the participant's during argument.
- **DL-9 Synthesis surfaces disagreement and may side with a minority on merit** [ADDED v1.1.0]: the orchestrator's synthesis (DL-3) MUST **name where participants converged** (independent convergence is a high-confidence signal), **name where they genuinely clashed without smoothing it over**, and **surface the blind spots the critique round caught** (DL-6). The final decision MAY side with a **minority** argument — or against every participant — when that reasoning is strongest. Averaging clashes into a mushy "it depends", hiding a real disagreement, and defaulting to the majority are each forbidden: deliberation exists to produce a **decisive** judgment *informed by* the disagreement, not a consensus that erases it. (Sharpens DL-3 from "not a vote" to "disagreement is preserved and the minority can win.")

## 4. Detailed Design

### 4.1 Deliberation Round Lifecycle

```text
[REFERENCE]
1. OPEN    — orchestrator defines: question, participants (N workers), token budget
2. ARGUE   — N workers generate arguments in parallel (DL-1); no cross-reading; full-stance (DL-8)
3. CRITIQUE — [optional, DL-6] anonymize + position-randomize the N arguments (DL-7);
             each participant critiques the set in parallel (strongest / blind spot / what all missed)
4. SYNTHESISE — orchestrator reads all arguments (de-anonymized) + all critiques
5. DECIDE  — orchestrator emits: decision + reasoning; names convergence, clashes,
             critique-caught blind spots; MAY side with a minority (DL-9)
6. LOG     — full round entry written to deliberation log (DL-4)
```

Total wall-clock time ≈ max(argument times) + [max(critique times)] + orchestrator synthesis time, since both the argument and critique rounds run in parallel. The critique round is optional (DL-6): the orchestrator may skip it for low-stakes decisions, exactly as it may skip the devil's-advocate designation (§4.2).

### 4.4a Blind Critique Round [ADDED v1.1.0]

```text
[REFERENCE]
anonymize(arguments):
    strip participant identity from each argument      # DL-7: merit, not authority
    for each critic:
        present the set in a freshly randomized order   # DL-7: defeat positional bias
critique(critic, anonymized_set):
    answers, independently (DL-1):
      - strongest argument, and why
      - the biggest blind spot in the set
      - what ALL arguments missed
at synthesis: re-attach identities                      # orchestrator alone sees who said what
```

The three fixed questions are chosen so the round produces what independent arguments cannot: a *comparative* judgement (strongest), a *cross-argument* flaw (a blind spot one argument has that another catches), and a *panel-wide* gap (what every argument missed at once). The last is the most valuable and the least available from any single argument, because a shared blind spot is invisible from inside it — it takes the panel examining itself to see what the whole panel assumed.

### 4.2 Participant Selection

Priority order:

1. Active workers in the office whose declared specialties overlap with the question domain
2. Preset-catalog roles with relevant specialties, hired on demand for the round and released afterward
3. Workers on different model tiers — mixing a high-capability reasoning model with a cost-efficient model broadens perspective diversity

Additionally, the orchestrator MAY designate one participant as a **devil's advocate** — tasked with arguing against the most plausible answer. This is applied via a system-prompt constraint on that participant's session, not a separate agent type.

### 4.3 Deliberation Log Entry Format

Each log entry stored in the Channels tab:

| Field | Content |
| --- | --- |
| `round_id` | Unique identifier |
| `opened_at` | ISO-8601 timestamp |
| `question` | The question posed |
| `participants` | List of `{role_name, model_used}` |
| `arguments` | Per-participant: `{role, position_summary, key_points[], confidence}` |
| `decision` | Orchestrator's final decision |
| `reasoning` | Which arguments influenced the decision and why |
| `token_budget` | Declared / actual used |
| `truncated` | `true` if any argument was cut short |

### 4.4 Communication Visibility

The deliberation log is the structured form of "inter-worker chat" visible to the user. Unlike unstructured chat, it provides:

- Clear attribution per argument (which worker said what)
- Explicit decision and reasoning (what the orchestrator concluded and why)
- Searchable, filterable history of all deliberation rounds in the office session

This is the office's public reasoning surface — not private, not ephemeral.

## 5. Implementation Notes

1. Parallel argument generation uses the wave-based parallel execution model from `l2-orchestration.md`.
2. Log entries share the SQLite database with the inbox (`l2-inbox.md`) but live in an append-only table of their own — inbox message rows are deleted on delivery and pruned by age, which an immutable audit trail (DL-4) cannot tolerate.
3. The devil's advocate designation is a per-round setting; the orchestrator may skip it for low-stakes decisions.

## 6. Drawbacks & Alternatives

**Alternative: open group discussion** — participants read each other's prior arguments and iterate freely. Violates DL-1; cross-contamination causes premature convergence. Note the blind critique round (DL-6) is **not** this: it is a *single* pass over *anonymized* arguments *after* independent generation is closed, not iterative cross-reading *during* it — it adds cross-examination without reopening the argument round to mutual influence.

**Alternative: majority vote** — the option with the most votes wins. Rejected: majority vote discards argument quality. Minority insights that the orchestrator judges significant would be lost — DL-9 makes preserving them mandatory and lets a minority argument win outright on merit.

**Alternative: skip the critique round (arguments → synthesis directly).** The pre-v1.1.0 behavior, and still valid for low-stakes rounds (DL-6 is optional). Rejected as the default for consequential decisions: it leaves every argument's blind spots uncaught, which is precisely the value a panel is supposed to add over a single answer.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[ORCHESTRATION]` | `.design/main/specifications/l1-orchestration.md` | Delegation and budget used to run rounds |
| `[INBOX]` | `.design/main/specifications/l2-inbox.md` | Backing store for log entries |
| `[OFFICE-MODEL]` | `.design/main/specifications/l1-office-model.md` | Orchestrator authority (DL-3) and role hiring (DL-2) |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.1.1 | 2026-09-23 | Core Team | Consistency pass (2026-09-23): Implementation note placed the immutable deliberation log in inbox message rows, which the inbox deletes on delivery and prunes by age — it lives in the same database in an append-only table of its own (DL-4). |
| 1.1.0 | 2026-07-26 | Core Team | Added DL-6…DL-9 + §4.4a — the **blind cross-critique round** between independent argument and synthesis, the step that makes deliberation more than "ask N times": each participant critiques the full set (strongest / biggest blind spot / what all missed), surfacing cross-argument flaws and panel-wide gaps no single independent argument contains (DL-6); the critique is on **anonymized, position-randomized** material so it evaluates the argument not who produced it or where it appeared, defeating deference and positional/primacy bias, with de-anonymization only at synthesis (DL-7); **full-stance arguments** — independence means leaning fully into the assigned specialty or deliberately-opposed stance, not hedging toward a balanced middle that collapses the diversity DL-2 selected for; the tension is the instrument and balancing is the orchestrator's job at synthesis not the participant's during argument (DL-8); synthesis MUST name convergence, name clashes without smoothing them, surface the critique-caught blind spots, and MAY side with a minority argument on merit — averaging into "it depends", hiding disagreement, and defaulting to the majority all forbidden (DL-9, sharpening DL-3 from "not a vote" to "disagreement preserved, minority can win"). §4.1 lifecycle gains an optional CRITIQUE step; §6 distinguishes the blind round from open group discussion (single anonymized pass after argument closes, not iterative cross-reading during it) and notes skipping it is valid only for low-stakes rounds. Related Specifications extended with l1-competitive-execution / l1-agent-coevaluation / l1-parallel-staffing. |
| 1.0.0 | 2026-06-24 | Core Team | Initial spec — DL-1…DL-5, round lifecycle, participant selection, log format, communication visibility |
