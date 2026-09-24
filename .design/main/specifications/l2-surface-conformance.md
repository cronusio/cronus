# Surface Conformance Corpus

**Version:** 1.0.2
**Status:** Stable
**Layer:** implementation
**Implements:** l1-surface-parity.md

## Overview

The concrete realization of SP-3 through SP-8 in this project's stack: the **finding inventory** that tracks each re-derivation as a defect with a definition of repaid, the **tombstone and debt ledgers** that move in one direction each, and the **conformance corpus** — a shared fixture library plus a harness that every surface runs inside its own test target, driving that surface's *real* projection.

Its sibling [l2-invocable-registry.md](l2-invocable-registry.md) removes the duplication by construction. This spec exists because construction is not proof: two projections of one descriptor can still disagree in behavior, and that disagreement is visible only when the same input is driven through both.

## Related Specifications

- [l1-surface-parity.md](l1-surface-parity.md) — Parent concept. SP-3…SP-8 and SP-10 are this spec's subject.
- [l2-invocable-registry.md](l2-invocable-registry.md) — Sibling. It supplies the single primitive; this spec proves the consumers agree on it and tracks the debt until they do.
- [l1-invariant-tripwires.md](l1-invariant-tripwires.md) — The enforcement model this instantiates; TW-6 separates structural checks from behavioral tests, and the corpus is the third kind neither catches alone.
- [l1-convergence-gate.md](l1-convergence-gate.md) — Where the corpus runs. Parity checks are gate content, not a separate ceremony.
- [l1-acceptance-oracle.md](l1-acceptance-oracle.md) — The failure mode this spec is built against: a check that cannot fail. The present hand-copied verb mirror is exactly that.
- [l2-cli.md](l2-cli.md) · [l2-tui.md](l2-tui.md) · [l2-application-shell.md](l2-application-shell.md) — The three registering consumers.
- [l1-architecture.md](l1-architecture.md) — INV-3 parity is what the corpus measures; INV-9 honesty is what the surface-listing fixtures assert.
- [l1-capability-reachability.md](l1-capability-reachability.md) — REA-1: agent-facing projections omit human-only invocables, which the surface-set family asserts.

## 1. Motivation

**The project already ships a parity check that cannot fail.** The terminal UI asserts its catalog against a constant that mirrors the command-line verb set — copied by hand, into the crate being tested, precisely so the test would not need a dependency on the crate it is checking. The reasoning was sound and the result is an oracle that compares a copy against itself. The command line has since grown to twenty-nine groups; the mirror still lists twenty-one; the assertion is green. This is the concrete local instance of the general rule that a duplicated oracle degrades silently, and it is the reason a corpus must drive **real projections** rather than any restatement of them.

**Removing the duplication does not retire the risk it created.** Once every surface renders from one registry, no surface can hold a verb the registry lacks. But two surfaces can still render the same descriptor into different behavior — a rejection shown by one and swallowed by another, an empty result and an unavailable store distinguished on one surface and merged on the next. That class survives the unification, and no import gate, type check, or per-surface test can see it.

**A duplication found and fixed from memory is a duplication that returns.** The three mappings collapsing into one, the re-derived on-disk location, the twice-implemented and once-omitted redaction — each is a finding with sites, a reachable divergence, and a replacing primitive. Recorded, they shrink monotonically. Discussed in review and fixed later from recollection, they oscillate, and the second discovery costs a reconciliation the first would not have.

**The migration will uncover behavior that is wrong on every surface at once.** A store failure reported as an empty result with a success code is agreed upon by exactly one surface today and will be inherited by all three the moment they share a dispatch. Correcting it inside the unification would present a regression to users as a refactor to reviewers. The residual mechanism is what lets convergence land provably behavior-preserving and the correction land disclosed and separate.

**The corpus cannot be one test.** The desktop shell is deliberately built as a detached workspace so WebView dependencies stay out of the engine build. A single workspace-level test cannot link its projection. The corpus is therefore shaped as a library every surface runs, which is also the shape SP-7 asks for: registration is something a surface does as part of becoming a surface, not something a central suite does on its behalf.

## 2. Constraints & Assumptions

- **The corpus is a library plus a harness, not a suite.** Fixtures and assertions live in a shared crate; each surface invokes them from its own test target against its own projection.
- **The desktop shell builds in a separate workspace.** Cross-workspace linkage exists only in the direction the shell already depends on the core, so the corpus crate must be reachable from that direction.
- **Fixtures are adversarial by construction.** Cases drawn from the happy path prove nothing, because the happy path is where implementations already agree.
- **A surface's *real* projection is the subject.** A corpus that exercises a shared helper both surfaces are assumed to call proves nothing about a surface that quietly does not call it.
- **The corpus runs in the ordinary quality gate.** A parity suite that runs on request runs never.
- **The inventory is seeded by one deliberate audit,** not accumulated from incoming reports. The findings that matter most have no user-visible symptom yet.

## 3. Invariant Compliance (Layer 2 only)

| L1 Invariant | Implementation |
| --- | --- |
| SP-1 One decision point per observable behavior | The corpus is how a violation becomes visible: a fixture driven through two projections that answer differently names a behavior decided in two places. The registry removes the cause; this spec detects residual instances. |
| SP-2 Derived facts are consumed, never recomputed | §4.2's seed inventory enumerates every present re-derivation. Each is a finding whose replacing primitive is the registry, and each stays open until all four repayment conditions hold. |
| SP-3 Divergence is a tracked defect class | §4.1 defines the finding record: sites with the authoritative one named, the divergence in user-visible terms, class (*observed* or *preemptive*), the replacing primitive, per-condition repayment, and any residual. §4.2 seeds it. |
| SP-4 A finding is repaid only when all four hold | Repayment requires all of: copies deleted; deletion pinned in the tombstone ledger; an adversarial fixture landed for the named divergence; every consumer registered against the corpus that runs it. Three of four is not partial credit and is recorded as *open*, not as *mostly repaid*. |
| SP-5 Deletion is pinned; ledgers move one way | Two ledgers, §4.3. The tombstone ledger is append-only and literal — a path or an owner-qualified symbol, plus its finding. The accepted-debt ledger is shrink-only. A change that reverses either direction is visible as such rather than arriving as a quiet re-addition. |
| SP-6 Parity is proven by one corpus through real projections | §4.4. One fixture set, driven through each surface's actual projection function, asserting identical semantic outcomes. Explicitly not: a shared helper both surfaces are assumed to call, and explicitly not a restated verb list. |
| SP-7 A new consumer registers before it ships | §4.5. Implementing the projection interface and running the corpus in the surface's own test target is part of becoming a surface. The registration is what converts an intention to agree into a mechanically failing condition when agreement stops. |
| SP-8 Legitimate difference is named, with its reason | §4.6 holds the do-not-unify record: what is genuinely surface-specific and why. Terminal row measurement against browser layout, and the host-owned settings facility, are its first entries. |
| SP-9 Extract before the second implementation exists | The inventory carries a *preemptive* class, and an inventory with no preemptive entries is one that has given up on SP-9. §4.2 seeds two, ahead of the surfaces that would otherwise create them. |
| SP-10 Converge first, correct second | §4.7. Four known-wrong behaviors are recorded as residuals **at the invocable that owns them**, not in a tracker, so the next reader sees the known-wrong behavior where they are already looking. Each ships as a separate disclosed change. |
| SP-11 Shared vocabulary, declared exposure | The corpus asserts the catalog property directly: for every surface, the set of exposed invocables equals the registry's shipped set minus that surface's **declared** exclusions — the loci it does not take (`l2-invocable-registry` §4.7) count as declared, and an agent-facing projection additionally omits every `HumanOnly` invocable (REA-1). An undeclared omission fails the corpus rather than passing unnoticed. |
| SP-12 Projectable and executable faces | The surface-set family builds every projected entry from descriptors alone, and a fixture round-trips the desktop's serialized catalog as data: no handler or host handle is reachable from any payload a surface receives. Pending realization. |
| SP-13 Unrecognized and unavailable are different answers | The outcome family carries an `Unknown` fixture — an invocation naming nothing registered — and asserts each surface's declared reaction (terminal UI: passes the line on as input; command line: usage error with the near-miss suggestion; desktop: catalog refresh), never an `Outcome`; a separate fixture asserts `Unavailable` for a resolved invocation whose backend cannot answer. Pending realization. |

## 4. Detailed Design

### 4.1 Anatomy of a finding

| Field | Content |
| --- | --- |
| **Sites** | Every implementation, named, with the authoritative one marked |
| **Divergence** | The observed or reachable disagreement, in user-visible terms |
| **Class** | *observed* (found in production or review) or *preemptive* (the copy does not exist yet) |
| **Primitive** | The single derivation that replaces the copies |
| **Repayment** | The four SP-4 conditions, each checked off individually |
| **Residual** | Any known-incorrect behavior the extraction preserved, and why |

### 4.2 Seed inventory

Findings from the deliberate audit that produced this spec and its sibling.

| # | Sites | Divergence | Class | Primitive |
| --- | --- | --- | --- | --- |
| F-1 | Command-line parser enum · terminal-UI slash catalog · desktop per-capability IPC list (authoritative: none — three peers) | The three describe different verb sets; the command line carries twenty-nine groups, the terminal UI twenty-one, the desktop four | observed | Invocable registry |
| F-2 | Terminal-UI hand-copied verb mirror | Parity asserted against a stale copy of the thing under test; green while the surfaces differ by eight verbs | observed | Corpus over real projections |
| F-3 | Command-line specification parity table | Documents verbs the product lacks and omits every group it has | observed | Catalog projection, not a maintained table |
| F-4 | Command-line board path derivation | A frontend computes a domain fact (where the board lives); a second surface reaching parity would necessarily re-derive it | observed | Core-owned invocable |
| F-5 | Terminal-UI redaction · desktop bridge redaction · command line (absent) | Two implementations and one omission of one security property | observed | Redaction at the dispatch boundary |
| F-6 | Per-command output rendering across the command line | Structured output ignored in nine sites; hand-built and unescaped in five; the two surfaces that render the same value do so by separate code | observed | `Outcome` plus one renderer per surface |
| F-7 | Desktop WebView client | A hand-written client mirroring the IPC command list — the fourth copy, before it grows | preemptive | Catalog-derived client |
| F-8 | Any future surface's help/completion listing | Each new surface would restate the verb set to render discovery | preemptive | Catalog projection |

### 4.3 The two ledgers

**Tombstones (append-only).** Each entry is a path or an owner-qualified symbol that must not return, plus the finding it belongs to. Kept boringly literal: cleverness here produces a check nobody trusts and everybody bypasses. First entries, on their respective migration steps — the hand-copied verb mirror, the per-capability IPC command list and its hand-written client, and the specification's maintained parity table.

**Accepted debt (shrink-only).** Findings knowingly left open, each with its reason. A change that adds an entry is a change that must say so.

The directions are the point. One ledger only grows, the other only shrinks, and a reversal of either is legible as a reversal rather than arriving as an ordinary edit.

### 4.4 The corpus

```plaintext
       fixtures (shared crate)
              │
   ┌──────────┼──────────────────┬─────────────────────┐
   ▼          ▼                  ▼                     ▼
 CLI       TUI            desktop shell          (next surface)
projection  projection      projection             projection
   └──────────┴──────────────────┴─────────────────────┘
                    identical semantic outcome asserted
```

Each surface's test target calls the harness with its own projection function. The harness drives every fixture through it and asserts the semantic outcome, not the rendering: a text renderer and a widget renderer legitimately differ in bytes and may not differ in meaning.

Three assertion families:

- **Surface set** — exposed invocables equal the registry's shipped set for the loci the surface takes, minus declared exclusions, and minus `HumanOnly` invocables on any agent-facing projection (SP-11, INV-9, REA-1).
- **Schema** — each invocable's advertised argument schema on this surface matches its declared binders (IB-1).
- **Outcome** — for each fixture invocation, the semantic outcome agrees across surfaces, including the rejection mode and location, and including the distinction between an empty result and an unavailable one.

Fixtures are written from the divergence rather than the feature: empty, single-element, zero-count, boundary-crossing, renamed, oversized, absent, unavailable, secret-bearing, and identity-colliding.

> Expect the corpus to fail immediately and unflatteringly when it first runs against the existing surfaces. That output **is** the initial inventory; it is converted into findings before anything is fixed, or the repairs are rediscovered later as duplicates.

### 4.5 Registration

A surface registers by supplying its projection to the harness from its own test target. The obligation is discharged where the surface lives, which is what makes it work across the detached desktop workspace, and it is an order of magnitude cheaper before the surface has behavior to preserve.

A surface that cannot register is not yet a surface. This is a gate on shipping, not a recommendation.

### 4.6 Do-not-unify record

| Item | Why it is legitimately per-surface |
| --- | --- |
| Text measurement and layout | Terminal cell metrics and browser layout are different problems with different correct answers |
| Input mechanics | Key handling, argument tokenization, and pointer interaction are surface property |
| Per-client view state | Focus, scroll position, and panel visibility belong to the viewer, not the model |
| Host-owned settings facility | Declared `HostOnly`; shell configuration is marshalling, not core logic |

An unstated exception is read by the next author either as an oversight to be unified or as licence for the next divergence. Naming it removes both readings.

### 4.7 Residuals

Recorded at the invocable that owns each, and corrected separately after convergence.

| Residual | Present behavior | Correction |
| --- | --- | --- |
| Unavailability reported as emptiness | A store error prints an empty listing and exits successfully | `[MODIFIED v1.0.1]` A **distinct `Outcome` variant naming why**, never a success-shaped empty `Value`. Not a rejection: a rejection is strictly binder-scoped and always carries a real location within a bound argument (IB-4), which an unreachable backend does not have |
| Output format ignored | Nine sites discard the requested format and print text | One renderer, driven by the requested format |
| Structured output hand-built | Five sites assemble it by string formatting, unescaped; an identifier containing a quote or backslash emits invalid output | Serialization of `Outcome` |
| Redaction fed nothing | Every surface's secret list is constructed empty, so masking is inert wherever it exists | Core exposes its secret store to the dispatch boundary |

The first and third are correctness defects, not stylistic ones: the first misreports failure as success to any script that consumes it, and the third can emit output a parser rejects. Both are nonetheless preserved through convergence and corrected after it, because a diff that unifies and corrects at once is one neither audience can read.

## 5. Implementation Notes

1. **Fixture crate and harness**, with the three assertion families and no consumers yet.
2. **Command line registers** — the first real projection; the corpus is expected to fail and the failures become findings.
3. **Ledgers seeded** with §4.2's inventory and the first tombstones.
4. **Terminal UI registers**; the hand-copied mirror is deleted and tombstoned in the same change.
5. **Desktop registers** from its own workspace, proving the cross-workspace shape.
6. **Corpus enters the ordinary quality gate.**
7. **Residual corrections**, one disclosed change each, after convergence is complete.

## 6. Drawbacks & Alternatives

- **The corpus is a maintenance surface of its own.** Every new semantic behavior wants a fixture and every new surface pays a registration cost. That cost scales with surfaces rather than with features, and it is the price of the guarantee.
- **Per-surface registration can be forgotten in a way a central suite could not.** The detached desktop workspace forces this shape; the mitigation is that the registry's projection interface is what a surface must implement to exist at all, so an unregistered surface is conspicuous rather than merely untested.
- **Preserving a known-wrong behavior for a release is genuinely unpleasant,** and is exactly why the residual is recorded at the primitive rather than left to memory.
- **Alternative — assert parity structurally and skip the corpus.** Rejected. Structural derivation removes the cause of set divergence and cannot see behavioral divergence between two projections of one descriptor.
- **Alternative — one workspace-level parity test.** Rejected on a build fact: the desktop shell is a detached workspace and cannot be linked from the engine workspace's tests.
- **Alternative — repair the existing mirror by regenerating it.** Rejected. A generated copy is still a copy, and the failure it produces is a stale-artifact failure rather than a disagreement between the things that actually run.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[PARITY]` | `.design/main/specifications/l1-surface-parity.md` | Parent; SP-3…SP-10 and §4.1's finding anatomy |
| `[REGISTRY]` | `.design/main/specifications/l2-invocable-registry.md` | The primitive whose consumers this corpus compares |
| `[TRIPWIRE]` | `.design/main/specifications/l1-invariant-tripwires.md` | The enforcement model; TW-6's separation of check kinds |
| `[GATE]` | `.design/main/specifications/l1-convergence-gate.md` | Where the corpus runs |
| `[ORACLE]` | `.design/main/specifications/l1-acceptance-oracle.md` | The check-that-cannot-fail failure mode this spec is built against |

## Document History

| Version | Date | Notes |
| --- | --- | --- |
| 1.0.2 | 2026-09-23 | Consistency pass (2026-09-23): SP-12 and SP-13 compliance rows added (pending realization): descriptor-only projections and an `Unknown` fixture per surface. The surface-set assertion predated locus filtering and reachability — it now counts the loci a surface does not take as declared exclusions and omits HumanOnly invocables from agent-facing projections (REA-1). |
| 1.0.1 | 2026-09-06 | Patch. The unavailability-as-emptiness residual's correction column said 'a rejection carries its mode', which named the wrong mechanism: a rejection is strictly binder-scoped and always carries a real location within a bound argument (IB-4), and an unreachable backend has none. Corrected to a **distinct `Outcome` variant naming why** — the shape the registry actually implements. Wording only; the residual, its owner, and its SP-10 staging are unchanged. |
| 1.0.0 | 2026-09-05 | Initial spec. Realizes SP-3…SP-8 and SP-10 as a **finding inventory**, two one-way **ledgers**, and a **conformance corpus** shaped as a shared fixture library plus a harness each surface runs in its own test target — the shape forced by the desktop shell's detached build workspace, and the shape SP-7 asks for independently. Seeds the inventory with eight audited findings (§4.2), including the live check-that-cannot-fail: parity asserted against a hand-copied verb list, green while the surfaces differ by eight verbs. Defines repayment as all four SP-4 conditions with no partial credit; pins deletions in an append-only tombstone ledger against a shrink-only debt ledger, so a reversal of either is legible. The corpus drives each surface's **real** projection across three assertion families — surface set (declared exclusions only, INV-9), advertised schema against declared binders (IB-1), and semantic outcome including rejection mode and the empty-versus-unavailable distinction — with fixtures written from the divergence rather than the feature. Records four residuals at their owning invocables (unavailability reported as emptiness, ignored output format, unescaped hand-built structured output, inert empty secret list), two of them correctness defects, all preserved through convergence and corrected separately per SP-10. Names the legitimate per-surface differences (§4.6) so an unstated exception cannot be read as either oversight or licence. |
