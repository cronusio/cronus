# Archetype Catalog

**Version:** 1.0.2
**Status:** Stable
**Layer:** implementation
**Implements:** l1-office-archetype.md

## Overview

The concrete realization of office archetypes: where a definition lives, what it contains, which archetypes ship, how one is selected and applied, how deviations are recorded, and the command surface.

It also records a finding that governs what can ship. The preset role catalog is engineering-heavy — eighteen technical roles against five business roles — and of the three domains this specification set out to cover, **only software engineering is expressible today**. An advertising agency and a finance department name specialties the catalog does not contain, and OA-10 forbids an archetype from defining them inline. Those two archetypes are therefore specified here as *declared and blocked*, with the exact roles each requires, rather than shipped against invented specialties.

## Related Specifications

- [l1-office-archetype.md](l1-office-archetype.md) - The concept this realizes; OA-1…OA-11.
- [l2-role-catalog.md](l2-role-catalog.md) - The preset roles an archetype references; §4.4 records the roles it must gain before the blocked archetypes can ship.
- [l1-roles.md](l1-roles.md) - ROL-9, the gate each missing role must clear on its own merits before entering the catalog.
- [l2-filesystem-layout.md](l2-filesystem-layout.md) - The program/state tiers an archetype's catalog-vs-instance split maps onto (STO-3).
- [l1-office-model.md](l1-office-model.md) - OFF-4 adaptive staffing; OFF-6 governs when selection may ask the client.
- [l1-workspace-lifecycle.md](l1-workspace-lifecycle.md) - WSL-4 blueprint instantiation and WSL-5 manager bootstrap, the steps §4.5 slots archetype selection between.
- [l1-orchestration.md](l1-orchestration.md) - The manager that consumes the prior and grows the shape.
- [l2-cli.md](l2-cli.md) - The verb-first grammar the command surface conforms to.
- [l1-component-scanning.md](l1-component-scanning.md) - Admission vetting applied to imported archetype prose (§4.7).
- [l1-extension-marketplace.md](l1-extension-marketplace.md) - XM-7 bundle distribution; an archetype is a content bundle, not a new channel.
- [l1-pattern-codification.md](l1-pattern-codification.md) - Consumes the deviation signals of §4.6.
- [l2-technology-stack.md](l2-technology-stack.md) - §4.8 configuration format policy, which the definition split follows.

## 1. Motivation

The concept fixes what an archetype *is* and what it may never do. What remains is concrete: a place on disk, a definition shape that separates references from prose, a shipped set honest about its own coverage, and a way to observe whether each archetype's prediction held.

## 2. Constraints & Assumptions

- An archetype references roles by catalog identity (OA-10). A domain specialty absent from the role catalog blocks its archetype; it does not license an inline definition.
- Structured references and human-authored prose are separate artifacts, because they have different trust properties: references are checked mechanically, prose is agent-instruction and must be vetted (§4.7).
- The definition format follows the stack's policy (`l2-technology-stack` §4.8): JSON for structured, machine-read data; Markdown for prose that a human hand-edits and an agent reads.
- Selection runs at office instantiation and on demand thereafter (OA-8). It is never a blocking prompt on the ordinary path.
- The catalog ships a small curated set. Breadth is a maintenance liability (`l1-roles` §5) and, for an unfalsifiable archetype, an active harm (OA-9).

## 3. Invariant Compliance (Layer 2 only)

| L1 Invariant | Implementation |
| --- | --- |
| OA-1 Prior, not roster | The definition (§4.2) carries exactly four content fields — `pool`, `shape`, `seed`, `norms` — beside its identity (`id`, `domain`), and no field expresses "hire these". Application (§4.5) hires only the `seed`; every later hire enters through the manager's ordinary path, whose gate does not read the archetype (§4.5 flow). There is no code path from a `pool` entry to a hire. |
| OA-2 Bounded, justified seed | `seed` is capped at **two** roles beyond the WSL-5 manager, enforced at catalog validation, and each entry carries a mandatory `justification` string naming the first-contact work it performs. A definition exceeding the cap, or omitting a justification, fails validation and is not admitted to the catalog. The shipped `software-engineering` archetype seeds **zero** roles (§4.3). |
| OA-3 Pool is not a cage | The hire path consults `pool` **after** the hire has been decided — including, for a new custom role, the ROL-9 gate — and only to classify the outcome (§4.5). No branch refuses a hire, and none exists to add: `pool` is read by the deviation recorder, never by the gate. A hire outside the pool is recorded as `hired_outside_pool` and proceeds. |
| OA-4 No authority | The definition schema admits no permission, autonomy, budget, egress, trust, or channel field. Validation rejects a definition carrying any key outside the four content fields and the identity pair (§4.2), so an archetype cannot smuggle authority under an unrecognized name. A domain's capability needs are expressed as prose in `NORMS.md`, which the human resolves through the ordinary approval path — data the agent may read, never a grant it receives. |
| OA-5 Untrusted until vetted | First-party archetypes ship in the immutable program tier. Any other source — marketplace, sideload, custom import — is admitted only after content vetting of its prose artifacts (§4.7), and its text carries untrusted provenance into every context that reads it. `archetype.json` is a reference list and is validated structurally; `NORMS.md` and any persona overlay are the vetted surface. |
| OA-6 Preset + custom | Program-tier definitions are read-only. `archetype create <name> --from <preset>` copies into the state tier and records `derived_from`, never mutating the source (§4.8), mirroring the role catalog's preset/custom split exactly (STO-3, ROL-7). |
| OA-7 Inferred selection | At office instantiation the orchestrator infers the archetype from captured intent and applies it without asking, narrating the choice. It asks the client only when two or more archetypes score within an ambiguity band **and** the choice changes the pool materially; the question names kinds of work ("building software" / "running campaigns"), never roles or structure (OFF-5/OFF-6). No archetype is a prerequisite for proceeding (OA-11). |
| OA-8 One active, revisable | The office records exactly one `active_archetype` identity. `archetype set <id>` re-scopes future decisions and touches no staff: it does not release a role, discard memory, or invalidate work. Switching away from an archetype leaves the roles it seeded in place, because they were hired by the manager and are the manager's to release (ROL-4/ROL-5). |
| OA-9 Falsifiable | Three deviation counters are recorded per office and attributed to the archetype identity (§4.6): `hired_outside_pool`, `seeded_never_worked`, `shape_never_grown`. They are readable through the command surface and are the input `l1-pattern-codification` consumes. An archetype with no recorded outcomes is reported as *unvalidated*, not as *correct*. |
| OA-10 Composes, never forks | `pool` and `seed` hold catalog role identifiers only; the schema has no field in which a role could be defined. Catalog validation resolves every identifier against the role catalog and **rejects the archetype** if any is unknown — which is precisely why the advertising and finance archetypes are blocked rather than shipped (§4.4). |
| OA-11 Archetype-free is complete | The absence of an `active_archetype` is a valid, fully-functional office state and the fallback when inference is inconclusive. Staffing then draws on the whole catalog, unchanged. `archetype set --clear` returns an office to this state at any time. No lifecycle stage, capability, or command requires an archetype to be set. |

## 4. Detailed Design

### 4.1 Catalog and instances

Mirroring the role catalog's split (`l2-role-catalog` §4.1) and the storage model's STO-3:

```plaintext
<program>/archetypes/            # read-only, ships with the program
├── CATALOG.md                   # index: identity, domain, status, one-line intent
└── <id>/
    ├── archetype.json           # pool, shape, seed — structured references only
    └── NORMS.md                 # prose: vocabulary, conventions, quality bar

<state>/archetypes/<id>/         # custom archetypes (OA-6), same shape
<state>/workspaces/<ws>/         # office records `active_archetype` + deviation counters
```

The split of `archetype.json` from `NORMS.md` is not cosmetic. The JSON contains identifiers the system resolves and validates mechanically; the Markdown contains language a model will read as instruction. They have different trust properties and therefore different admission paths (§4.7), and keeping them in one file would force the vetting gate to parse structure it should not need to trust.

### 4.2 The definition

```json
// [REFERENCE] <program>/archetypes/software-engineering/archetype.json
{
  "id": "software-engineering",
  "domain": "Building and maintaining software",
  "pool": ["architect", "backend-engineer", "frontend-engineer", "api-designer",
           "sql-expert", "code-reviewer", "test-writer", "debugger", "refactorer",
           "performance-optimizer", "security-auditor", "accessibility-auditor",
           "devops-engineer", "incident-responder", "doc-writer", "data-analyst",
           "prompt-engineer", "archivist"],
  "shape": {
    "departments": ["engineering", "quality", "operations"],
    "grow_when": "sustained parallel width in a department exceeds the manager's span"
  },
  "seed": [],
  "norms": "NORMS.md"
}
```

Four content fields beside the identity pair (`id`, `domain`), and validation rejects any other key. This is how OA-4 is enforced rather than merely asserted: there is no key named `permissions`, no key named `budget`, and an unrecognized key is a validation failure, not an ignored extra. An archetype cannot express authority because the schema gives it no vocabulary for authority.

`seed` entries, when present, take the form `{ "role": "<id>", "justification": "<first-contact work this role performs>" }`, and the array is capped at two (OA-2).

`shape.grow_when` is a condition, not a structure to build. Departments are named so that the manager knows what to *call* a layer when it introduces one; nothing instantiates them (OA-1).

### 4.3 Shipped archetypes

| Identity | Domain | Status | Seed | Pool size |
| --- | --- | --- | --- | --- |
| *(none)* | any | **default** (OA-11) | — | whole catalog |
| `software-engineering` | building and maintaining software | **ships** | 0 roles | 18 |
| `advertising-agency` | campaigns, creative, media | **blocked** — §4.4 | 0 roles | 1 of 5 |
| `finance-department` | accounting, controlling, analysis | **blocked** — §4.4 | 0 roles | 2 of 5 |

Every shipped archetype seeds **zero** roles. This is a finding, not an oversight: applying the OA-2 test — *what does this office do before it knows anything about the project?* — returns "intake, plan, and occasionally clarify" in all three domains, and WSL-5's manager already performs exactly that. No domain examined here has a first act specialized enough to justify seating a specialist before the first sentence is spoken.

The `seed` mechanism is retained because OA-2 defines it and a future domain may need it. That it is currently unused in every shipped archetype is the strongest available evidence that the "office with staff already at their desks" intuition (`l1-office-archetype` §5) was describing an aesthetic rather than a need.

### 4.4 Role coverage: why two archetypes are blocked

OA-10 forbids an archetype from defining a specialty inline; every `pool` and `seed` identifier must resolve against the role catalog. Measured against `l2-role-catalog` §4.2, the catalog holds eighteen technical roles and five business roles, and the shortfall falls entirely on the two non-technical domains.

| Archetype | Resolves today | Missing from the role catalog |
| --- | --- | --- |
| `software-engineering` | all 18 | — |
| `advertising-agency` | `marketing` | `account-manager`, `strategist`, `copywriter`, `art-director`, `media-planner` |
| `finance-department` | `finance`, `data-analyst` | `accountant`, `controller`, `financial-analyst`, `tax-specialist` |

Each missing role must enter the role catalog on its own merits, clearing the ROL-9 anti-sprawl gate — two independent axes of justification — before any archetype may reference it. `copywriter` versus the existing `marketing` role, for instance, clears on distinct expertise and context isolation; whether `strategist` clears against `marketing` at all is a genuine question for that amendment, not for this spec. That `l2-role-catalog` §4.2 amendment — the nine missing roles, each subject to ROL-9 on its own merits — is a prerequisite for unblocking the two archetypes and is deliberately out of scope here.

Shipping the two archetypes against invented, undefined specialties would violate OA-10 and would forge the catalog's quality gate — presenting nine unreviewed roles as though they had passed ROL-9 because an archetype named them. Blocking them is the honest state, and it names precisely what would unblock them.

### 4.5 Selection and application

```text
[REFERENCE] office instantiation
  1. materialize workspace from blueprint      // WSL-4, unchanged
  2. bootstrap manager                          // WSL-5, unchanged
  3. infer archetype from captured intent       // OA-7
       inconclusive        -> leave unset (OA-11), proceed
       ambiguous between N -> ask the client, in kinds-of-work terms (OFF-6)
       confident           -> set active_archetype, narrate
  4. hire seed, if any                          // OA-2; currently always empty
```

The hire path afterward is the office's ordinary one, and the archetype does not appear in its decision:

```text
[REFERENCE] manager needs a specialty
  1. pick the catalog role that covers it; if none does, a new custom role
     must first clear the ROL-9 justification gate   -> refuse, or allow
  2. if a role is available:
       hire the role
       if role NOT in active_archetype.pool:
           record deviation `hired_outside_pool`   // OA-3, OA-9 — classify, never refuse
```

Step 2's pool check is a **write** to the deviation log, never a **read** by the gate. Inverting that order — consulting the pool before the gate — would reconstruct the fixed org chart the concept spec rejects, and it is the single most likely implementation error. The pool identifier is not passed to the gate; it is not in scope there.

### 4.6 Deviation recording

Three counters per office, attributed to the archetype identity so signals aggregate across every office that adopted it (OA-9):

| Signal | Recorded when | What it means |
| --- | --- | --- |
| `hired_outside_pool` | a hire proceeds and its role is absent from `pool` | the domain draws on a specialty the archetype did not anticipate |
| `seeded_never_worked` | a seeded role is released, or the office closes, having received no delegated task | the seed seated someone first-contact work did not need |
| `shape_never_grown` | the office closes with no department layer introduced | the shape planned a structure the work never required |

An archetype whose counters have never been read is reported as **unvalidated**. The distinction between "no deviations recorded" and "no offices observed" is exactly the distinction between a validated prior and a guess, and collapsing them would let an unexamined default accumulate authority it never earned.

### 4.7 Imported archetypes

An archetype from any source outside the program tier is a **content bundle** and rides the existing distribution machinery (`l1-extension-marketplace` XM-7) rather than a parallel channel: addressable identity, pinned version, declared trust tier, publishing gate.

Admission is two-staged, and the stages examine different artifacts:

1. **`archetype.json` — structural validation.** Schema conformance, no unrecognized keys (OA-4), every `pool`/`seed` identifier resolves against the role catalog (OA-10), `seed` within its cap (OA-2). Mechanical; no model reads it.
2. **`NORMS.md` and any persona overlay — content vetting.** This prose becomes agent instruction, so it passes the admission-vetting gate (`l1-component-scanning`) before any of it can reach a context, and carries untrusted provenance thereafter (OA-5).

The asymmetry is the point. Stage 1 cannot be subverted by persuasive language, because it reads no language. Stage 2 cannot be subverted by a malformed reference, because structure was already settled. A single-file definition would collapse both into one artifact and force the vetting gate to trust structure it parsed from text it does not trust.

### 4.8 Command surface

Conforming to the verb-first grammar (`l2-cli` §4), with the TUI mirroring each and the app binding the same core calls (INV-3 parity):

| Action | CLI | Library |
| --- | --- | --- |
| list | `cronus archetype list [--catalog\|--active]` | `archetypes.list({scope?})` |
| show | `cronus archetype info <id> [--deviations]` | `archetypes.get(id)` |
| apply / re-scope | `cronus archetype set <id>` | `archetypes.apply(id)` |
| return to default | `cronus archetype set --clear` | `archetypes.clear()` |
| create custom | `cronus archetype create <name> [--from <preset>]` | `archetypes.create(name, opts)` |

There is deliberately **no** `archetype hire` verb. Hiring belongs to `role` (`l2-role-catalog` §4.5) and to the manager (ROL-5); an archetype command that hired anyone would place the prior on the wrong side of §4.5's decision boundary.

`archetype set` is non-destructive by construction (OA-8) and needs no confirmation: it changes expectations, not staff.

## 5. Implementation Notes

1. **Schema and validation first.** The closed four-key schema (§4.2) is what enforces OA-4, and the identifier-resolution check is what enforces OA-10. Both must exist before any definition is loaded, or the invariants are documentation rather than mechanism.
2. **Ship `software-engineering` alone.** Its pool resolves fully today. It exercises inference, application, and deviation recording end to end without waiting on the role catalog.
3. **Deviation recording before a second archetype.** OA-9 is what keeps the catalog honest as it grows; adding archetypes before the signal exists means adding unfalsifiable defaults.
4. **The pool check lives in the deviation recorder, not the hire gate.** Enforce this with a test that a hire outside the pool succeeds. Written as an assertion, it is the executable form of OA-3.
5. **The role-catalog amendment is a separate change.** Nine roles, each through ROL-9 on its own merits (§4.4). Only then do the two blocked archetypes become writable, and each still needs its pool and shape authored against the roles as they were actually admitted — not as this spec anticipated them.
6. **Import path last.** Structural validation (stage 1) is cheap and can land with the schema; content vetting (stage 2) depends on the admission-vetting gate and must not be stubbed, because a stubbed content gate is worse than an absent import feature.

## 6. Drawbacks & Alternatives

**Drawback — one shipped archetype is a thin catalog.** It is also an honest one. The alternative was three archetypes, two of which name specialties that do not exist, which would have made the catalog look complete while making the role catalog's quality gate a formality.

**Drawback — the two-artifact definition is more files.** Justified by §4.7: the trust boundary runs between references and prose, and a file boundary is the cheapest place to put it.

**Alternative — define missing specialties inline in the archetype.** Rejected by OA-10, and independently by ROL-9. It would let an archetype mint nine roles that no gate reviewed, and would fork any specialty later added to the catalog under the same name.

**Alternative — ship the blocked archetypes with reduced pools (`advertising-agency` with only `marketing`).** Rejected. A one-role pool is not a prior about a trade; it is a label. It would record `hired_outside_pool` on nearly every hire (§4.6), poisoning the signal OA-9 depends on, while giving the manager no usable expectation.

**Alternative — let the archetype's `shape` instantiate departments at application time.** Rejected by OA-1. A department with no one in it is an org chart, and the concept spec's §5 rejects exactly that. `grow_when` states the condition under which the manager introduces a layer; the manager introduces it.

**Alternative — allow a `capabilities` field so a domain "works out of the box".** Rejected by OA-4/SEC-10, and the closed schema (§4.2) makes it unrepresentable rather than merely discouraged. Domain capability needs are prose the human resolves.

**Risk — inference (OA-7) misfires and silently applies a wrong archetype.** Bounded by the fact that a wrong archetype costs a poorer prior and some deviation records, never a wrong hire (§4.5) and never — since every seed is empty (§4.3) — a wasted specialist. The fallback on inconclusive inference is the archetype-free office, which is a complete product (OA-11).

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[CONCEPT]` | `.design/main/specifications/l1-office-archetype.md` | OA-1…OA-11, the invariants this realizes |
| `[ROLE-CATALOG]` | `.design/main/specifications/l2-role-catalog.md` | The preset roles archetypes reference; §4.2 is the list §4.4 measures against |
| `[ROLES]` | `.design/main/specifications/l1-roles.md` | ROL-9, the gate each of the nine missing roles must clear |
| `[LAYOUT]` | `.design/main/specifications/l2-filesystem-layout.md` | Program/state tier placement of catalog and custom definitions |
| `[SCANNING]` | `.design/main/specifications/l1-component-scanning.md` | The content-vetting gate stage 2 of §4.7 invokes |
| `[CLI]` | `.design/main/specifications/l2-cli.md` | The verb-first grammar `cronus archetype …` conforms to |

## Document History

| Version | Date | Notes |
| --- | --- | --- |
| 1.0.2 | 2026-09-23 | Consistency pass (2026-09-23): The definition was said to carry exactly four fields and validation to reject a fifth, while the shipped example carries six keys — restated as four content fields beside the `id`/`domain` identity pair, any other key rejected. The hire flow put every hire through the ROL-9 gate, which governs minting a custom role, not hiring a catalog one — the flow picks a catalog role first and applies ROL-9 only to a new custom role (OA-3 row and `hired_outside_pool` trigger aligned). "Applies it silently, narrating" reworded. |
| 1.0.1 | 2026-07-17 | Promoted RFC→Stable via `/magic.spec` adversarial review (spec-critic + prompt-engineer PASS). No requirement change: OA-1…OA-11 were reviewed as-authored and found substantively complete, coherent, and honest about their own coverage (the ship-one-of-three finding, the pool-read-by-recorder-not-gate discipline, the closed-schema enforcement of OA-4). The only edit was cosmetic — the §4.4 deferred-scope `<!-- TBD -->` marker was converted to visible prose, since a Stable spec should carry no dangling TBD and its content (the ROL-9-gated `l2-role-catalog` §4.2 amendment that would unblock the two blocked archetypes) was already fully stated in the surrounding text. Now buildable at its shipped scope: `software-engineering` archetype + the declared-blocked treatment of `advertising-agency`/`finance-department`. |
| 1.0.0 | 2026-07-10 | Initial spec. Realizes OA-1…OA-11: two-artifact definition (`archetype.json` references + `NORMS.md` prose) split along the trust boundary, closed four-key schema that makes OA-4 unrepresentable rather than merely forbidden, pool consulted by the deviation recorder and never by the hire gate (OA-3), three deviation counters with an explicit *unvalidated* state (OA-9). Ships `software-engineering` (18-role pool, empty seed); declares `advertising-agency` and `finance-department` **blocked**, naming the nine roles each requires and deferring them to a ROL-9-gated amendment of `l2-role-catalog` §4.2. Records the finding that every examined domain seeds zero roles, since WSL-5's manager already performs all first-contact work. |
