# Prompt Templates

**Version:** 1.0.1
**Status:** Stable
**Layer:** concept

## Overview

A prompt template is a **named, reusable instruction** — "go through the whole document, build a coverage map item by item, miss nothing" — with declared parameters, written once and invoked by name. The person invokes one as a command; the orchestrator puts one into a worker's brief; a workflow references one in a step. All three receive the same text, rendered by the core from one definition.

Templates carry instructions only. They grant no tool, raise no autonomy level and pass no gate; what an actor may do still comes from its role, its skills and policy. They differ from skills, which bundle capability loaded when a task matches, and from workflows, which are executable step graphs: a template is the instruction an actor chooses to give.

## Related Specifications

- [l1-artifact-reach.md](l1-artifact-reach.md) - RCH-10 three origins — built-in, the user's, the project's — and their resolution order, which templates follow (PTM-1).
- [l1-context-provenance.md](l1-context-provenance.md) - Argument values are untrusted content composed into a model-facing prompt; they never become instructions (PTM-3).
- [l1-cache-stable-context.md](l1-cache-stable-context.md) - Stable prefix first, volatile values after (PTM-4).
- [l1-progressive-disclosure.md](l1-progressive-disclosure.md) - PD-1: only a template's name and when-to-use line are resident until it is invoked (PTM-6).
- [l1-pattern-codification.md](l1-pattern-codification.md) - A repeated instruction becomes a candidate, adopted only on the person's ratification (PTM-7).
- [l1-instruction-economy.md](l1-instruction-economy.md) - An instruction that changes nothing is a candidate for removal; the same holds for a template (PTM-8).
- [l1-capability-reachability.md](l1-capability-reachability.md) - Composition by invocation through the registry, not by path (PTM-9).
- [l1-workflow-language.md](l1-workflow-language.md) - The nodus projection (§4.3).
- [l2-extension-registry.md](l2-extension-registry.md) - §4.11 command definition format: the existing realization of a template invoked as a command.

## 1. Motivation

The person who works with agents long enough accumulates a handful of instructions they type again and again: build a coverage map and miss nothing; argue against your own plan before acting; check every claim against the source; finish the whole list, not the first half. Each retyping varies a little, the strongest wording is lost, and none of it reaches the office's own agents, who brief each other in whatever words the moment produces.

The project already has half the mechanism — a command definition is a markdown instruction a person invokes with arguments — but it is framed as a slash command for people only, with untyped arguments, and nothing lets an orchestrator or a workflow use the same instruction by name.

## 2. Constraints & Assumptions

- **Confirmed (2026-10-22, the person's decision):** templates serve both the person and the office's agents; one definition, three callers. Not taken: person-invoked only.
- The existing command definition format is the template format; nothing here introduces a second one.
- A small curated set ships with the program; most templates come from the person and from projects.

## 3. Core Invariants

Rules every Layer 2 implementation MUST NOT violate:

- **PTM-1 (A named, versioned definition with an origin):** a template has a stable name, a one-line statement of when to use it, its instruction text, its declared parameters, a version, and one origin — built-in, the user's, or the project's. Where names collide the project's wins over the user's and the user's over the built-in, and the origin of the resolved template is always reported.
- **PTM-2 (One definition, three callers, one rendering):** a person invokes a template as a command, an orchestrator or agent includes it in a brief by name, and a workflow references it in a step. All three receive the text rendered by the core from the same definition; no caller keeps a copy. A template may declare itself person-only or agent-only.
- **PTM-3 (Typed parameters; arguments are content, never instructions):** parameters are declared with a type and whether they are required. A missing required argument is a rejection naming it, never a guess. Argument values are inserted as untrusted content with their provenance and cannot rewrite, extend or escape the template's instructions.
- **PTM-4 (Deterministic, cache-stable rendering):** the same template version and the same arguments render the same text. The template's own text forms the stable leading part of what it contributes; argument values follow.
- **PTM-5 (Instructions only, no authority):** a template cannot grant a tool, raise an autonomy level, change a gate, or select a model with more capability than the caller's policy allows. A requested tool or model in a definition is a request reviewed like any other, never a grant.
- **PTM-6 (Discovered by descriptor):** only names and when-to-use lines are resident for discovery; a template's body enters context when it is invoked.
- **PTM-7 (Proposed from repetition, adopted by ratification):** when the person repeatedly writes substantially the same instruction, the office may propose it as a template, showing the instances. It is created only on the person's acceptance; the office never creates one silently. Built-in templates are few and curated.
- **PTM-8 (Use and effect are measured locally):** each invocation and the outcome of the work it shaped are recorded on the device. A template whose use shows no effect on outcomes is a candidate for revision or retirement, decided by the person or the curated set's owner.
- **PTM-9 (Composition by name, bounded):** a template may include another by name through the same resolution; includes are bounded in depth and a cycle is a rejection at definition time.

> L2 specs cannot reach RFC status until all invariants here are addressed in their "Invariant Compliance" section.

## 4. Detailed Design

### 4.1 Where templates live

| Origin | Where | Reach |
| --- | --- | --- |
| Built-in | the program tier's command store | part of the release |
| The user's | the user's state, alongside their skills | personal |
| The project's | the project folder's `commands/` | project-shared (after the trust decision) |

### 4.2 Example built-in set

| Name | When to use | Parameters |
| --- | --- | --- |
| `coverage-map` | before claiming a document or list is fully handled | `source` (required): the document or list |
| `counter-plan` | before executing a plan with irreversible steps | `plan` (required) |
| `claim-check` | before reporting facts drawn from sources | `sources` (optional) |
| `finish-the-list` | when a multi-item request risks partial completion | `items` (required) |

### 4.3 Nodus projection

A workflow reaches a template by name through the host's registry; its rendered text enters a model-calling step's input as a reusable segment, so it takes the stable-prefix position of the language's cache-stable composition rule, and arguments interpolated into it carry provenance under the language's provenance-safe interpolation rule. Nothing in the language changes for this. <!-- TBD: whether nodus gains a first-class declaration for a parameterized prompt fragment (beside its step macros), so a workflow file can define and test a template it ships — recorded as a nodus feedback candidate -->

## 5. Drawbacks & Alternatives

- **Templates can harden into ritual.** Mitigated by PTM-8: a template that does not change outcomes is visible as such.
- **Alternative — a separate prompt library beside commands.** Rejected (§2): two formats for one idea would drift.
- **Alternative — let templates carry tool grants for convenience.** Rejected (PTM-5): an instruction file becomes a privilege escalation path.
- **Alternative — learn templates automatically.** Rejected (PTM-7): a silently created instruction is a rule nobody ratified.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[COMMANDS]` | `.design/main/specifications/l2-extension-registry.md` | §4.11 command definition format — the template format |
| `[REACH]` | `.design/main/specifications/l1-artifact-reach.md` | Origins and resolution order |
| `[PROVENANCE]` | `.design/main/specifications/l1-context-provenance.md` | Arguments as untrusted content |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.0 | 2026-10-22 | Core Team | Initial concept — PTM-1…PTM-9, from the user's backlog (templates for frequently used agent prompts, "probably a nodus idea"): a named, versioned definition with an origin and resolution order; one definition rendered by the core for the person, the office's agents and workflows; typed parameters whose arguments stay untrusted content; deterministic cache-stable rendering; instructions without authority; discovery by descriptor; proposed from repetition and adopted only on ratification; use and effect measured locally; bounded composition by name. Realized by the existing command definition format (`l2-extension-registry` §4.11, amended); the nodus side needs no language change, a first-class fragment declaration is a recorded candidate. |
| 1.0.1 | 2026-10-22 | Core Team | The forecast assumption is confirmed by the person (design-layer audit survey, 2026-10-22) and recorded as a decision; the runner-up is now the alternative not taken and the override hint is dropped. No requirement changed. |
