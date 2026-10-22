# Parity Obligations

**Version:** 1.0.0
**Status:** Stable
**Layer:** concept

## Overview

Some work is judged against something that already exists rather than against criteria written for it. A port of a module to another language, a rewrite that must not change behaviour, a migration that must read every record the old version wrote, a feature rebuilt after studying how another program does it — each is finished only when the new thing does what the original did, in the cases that matter. The corpus already says how a completion claim is backed (fresh evidence from the check that actually proves it) and how criteria are written so they can fail. It does not say what the **claim set** of such work is, where it comes from, or when one of its claims may be called closed.

This specification owns that object: the **parity obligation ledger**. Obligations are derived conservatively from observations of the original; each names the cases it must cover, the strength of evidence that observed the original behaviour, and the strength of evidence a verifier must have to close it. An obligation closes only when one owner in the new work is bound to it, the required case kinds are covered on both sides, and a verifier of comparable strength names the obligation and passed. Everything else — blocked, out of scope, contradicted, missing a dependency — stays visibly open.

The unifying property is that **parity is a claim per behaviour, not a feeling about the whole**, and that a check weaker than the observation it answers cannot close it, however green it is.

## Related Specifications

- [l1-completion-verification.md](l1-completion-verification.md) — CMP-4 says a check must prove the claim and not a cheaper proxy. POB-2 is CMP-4 made into an ordered, declared scale for one class of claim: the strength of the original observation sets the minimum strength of the check that closes it. CMP judges any completion claim; this owns the claim set of parity work.
- [l1-acceptance-oracle.md](l1-acceptance-oracle.md) — AO governs criteria written **before** the work by someone who knows the intent. Parity obligations are **derived from observing an original** whose intent may be unknown. AO-5 (an absence claim needs a positive control) and AO-10 (an empty set is refused, never satisfied) apply to obligations unchanged; POB-1 is AO-10 for a ledger built from no evidence.
- [l1-observation-comparison.md](l1-observation-comparison.md) — the verdict a verifier returns when it compares the original's observations with the new work's (changed / unchanged / unknown per dimension). POB-8 takes its comparison modes from there.
- [l1-evidence-currency.md](l1-evidence-currency.md) — EVC-1 binds evidence to exact bytes; POB-4's owner binding is EVC-1 applied to the implementing unit, and EVC-5 (a failure is never a skip) governs the verifier's status.
- [l1-epistemic-status.md](l1-epistemic-status.md) — the standing of each fact the ledger consumes (observed / computed / inferred). A static reading of the original yields candidates, never observations of behaviour (POB-1).
- [l1-inverse-derivation.md](l1-inverse-derivation.md) — IVD reconstructs **how** exemplary work was produced, to learn method. This specification is about reproducing **what** an original does, to preserve behaviour. The two meet only when an episode is itself a port; neither depends on the other.
- [l1-capability-leveling.md](l1-capability-leveling.md) — LVL-1 rejects "reproduce the exemplar" as an objective for model leveling. That rejection is about learning method; here reproduction of observed behaviour **is** the objective, bounded to the declared obligations.

## 1. Motivation

Five ways parity work reports done while it is not:

- **The green proxy.** The original was observed as a shipped program reading its configuration file and rejecting a bad option with a specific message. The rewrite has a unit test of the option parser that passes. The installed program prints something else. A test of a part was used to close a claim about the whole program as shipped.
- **The happy path only.** The original was observed accepting good input. Nobody observed how it rejects malformed input, how it behaves when cancelled, or what it leaves behind at shutdown. The rewrite matches on the observed case and the ledger, having no other case, reads complete.
- **The unenumerated suite.** A large suite passes. No test in it exercises the behaviour in question, but "the suite passed" is recorded against every obligation.
- **The quiet disposition.** An obligation that turned out to be hard is marked "out of scope". The roll-up counts it with the closed ones, because it is no longer open.
- **The total order on a concurrent behaviour.** The original emits two responses in either order depending on scheduling. The verifier compares sequences and either fails at random or, worse, is tuned until it passes against one ordering and is called verified.

## 2. Constraints & Assumptions

- **The original is observed, not read.** Its source may be unavailable, partial, or not the thing that shipped. What the ledger knows about it comes from observations of a stated strength.
- **Static facts suggest; they do not establish behaviour.** Reading a structure, a call graph or a bundle tells you where behaviour might be; it does not tell you what happens when it runs.
- **Some behaviour is concurrent.** Where the original's events are not totally ordered, a comparison that assumes they are produces noise or false confidence.
- **Not every obligation can be closed here.** The strength needed to close it may be unavailable on this machine (no packaged build, no device, no network peer). That is a state to record, not a reason to lower the bar.

## 3. Core Invariants

Rules every Layer 2 implementation MUST NOT violate. They are technology-neutral.

- **POB-1 (Obligations derive conservatively from evidence; no evidence yields an unknown ledger, never a closed one):** a structural or static fact about the original creates a **candidate** obligation and nothing more; it never proves behaviour and never closes anything. A ledger built from an empty evidence set is **unknown with zero obligations**, not complete. Reviewed obligations may be added by a person or an agent for behaviour the automatic sources missed, each citing the evidence it rests on, and each carrying a stable identity and a version so that a change to an obligation is a reviewable event rather than a silent edit.

- **POB-2 (Evidence strength is a declared, ordered scale, and the closing check must reach the original's strength):** every obligation declares three strengths from one ordered scale — the strength at which the **original** behaviour was observed, the strength the new work's **fixtures** must have, and the strength the **verifier** must have. The scale runs, at minimum, from a reading of the code without running it, through an isolated unit of the new work, through components assembled together, through the running program, to the program as packaged and installed. A check below the declared strength **cannot close** the obligation: a passing unit test does not close an obligation observed on the installed program, and no reading of code closes an obligation about running behaviour. Where the required strength is not available, the obligation records that unavailability and stays open.

- **POB-3 (Each obligation names the case kinds it requires, and both sides cover every one):** an obligation declares the **case kinds** it is about — at minimum from: the accepted case, the refused case, malformed input, cancellation part-way, and shutdown or teardown. Closure requires an observation of the **original** for every declared kind and a fixture of the **new work** for every declared kind. An obligation observed only on its accepted case declares only that kind, and the ledger reports the other kinds as not required **by declaration**, never by silence.

- **POB-4 (One owner, bound by content identity):** an obligation closes only when exactly **one** unit of the new work is bound to it as its owner, identified by location and a content identity of that unit (EVC-1). Two candidate owners, or two definitions of the same obligation, keep it open until one is chosen. Where an obligation concerns the parsing or validation of an input, the binding also names the parser and the type it produces, so that "where is this enforced" has one answer.

- **POB-5 (The verifier names the obligation; a passing suite that does not name it does not close it):** a verifier closes an obligation only if it **enumerates that obligation's identity** among those it checks, and its result is present in the evidence the ledger was built from. A green run that does not name the obligation is evidence about something else.

- **POB-6 (A disposition is recorded, never counted as verified):** an obligation may be marked **blocked** (its required strength or dependency is unavailable) or **out of scope** (deliberately excluded, with the reason and who decided). Both are recorded with their reason and both appear in every summary under their own names; neither is added to the closed count, and a summary that reports a percentage reports it over the obligations that were **not** set aside, with the set-aside count beside it.

- **POB-7 (Contradiction, residual unknowns and missing dependencies keep closure open):** an obligation whose evidence contradicts itself, which carries an open unknown, or which depends on an obligation that is not closed, **cannot close**. A dependency is declared by obligation identity; the ledger closes in dependency order, and a cycle is a defect in the ledger, reported, not resolved by picking an order.

- **POB-8 (The comparison mode is declared, and concurrent behaviour is never compared as a total order):** the verifier states how it compares the original with the new work — exact, as a partial order with stated constraints, or as a finite set of accepted traces (OBC-7). A verifier that compares a behaviour the original exhibits concurrently as one fixed sequence is **refused** as a verifier for that obligation, whatever its result.

- **POB-9 (The ledger has an identity, and so does each closure):** the ledger's result — every obligation, its status, owner, dependencies, evidence references and limitations — is identified by a digest over the complete result, so two reports of "the same ledger" can be compared byte for byte, and a closure is attributable to exactly the evidence it rested on.

- **POB-10 (A closed ledger claims exactly the declared obligations, never equivalence):** a ledger with every required obligation closed states that **those** behaviours were shown to match at **those** strengths. It is never reported as "behaviourally equivalent", "a faithful port", or any phrase that extends the claim beyond the declared set. An obligation nobody declared was not checked.

> An L2 implementation cannot reach RFC until every invariant above is addressed in its Invariant Compliance section.

## 4. Detailed Design

### 4.1 The strength scale (POB-2)

| Strength | What observed it | Can close obligations observed at |
| --- | --- | --- |
| `static` | reading code, structure or a bundle without running it | `static` only |
| `unit` | an isolated unit of the new work, its collaborators replaced | `static`, `unit` |
| `integration` | real components assembled, no packaged entry point | up to `integration` |
| `runtime` | the program running through its real entry point | up to `runtime` |
| `packaged` | the program as built, packaged and installed | any |

Specialized strengths (a protocol peer, a native interface harness, a device) sit at the level of the entry point they exercise. An implementation may refine the scale; it may not collapse two levels into one, because the collapsed pair is exactly where the green proxy lives.

### 4.2 The closure rule

```
closed(o) ⇔ declared(o)
          ∧ owner(o) is unique and identity-bound           (POB-4)
          ∧ ∀ kind ∈ o.case_kinds:
                original observation exists for kind
                fixture of new work exists for kind           (POB-3)
          ∧ ∃ verifier v: v.enumerates(o.id)
                ∧ v.result ∈ evidence ∧ v.status = passed     (POB-5)
                ∧ strength(v) ≥ o.required_verifier_strength  (POB-2)
                ∧ v.mode admissible for o                     (POB-8)
          ∧ no contradiction, no open unknown on o            (POB-7)
          ∧ ∀ d ∈ o.depends_on: closed(d)                     (POB-7)
```

A ledger is **ready** only when it contains at least one required obligation and every required obligation is closed. An empty ledger is unknown, never ready (POB-1).

### 4.3 Failure modes named

| Mode | Why it passes | Closed by |
| --- | --- | --- |
| Green proxy | the unit test really passed | POB-2 |
| Happy path only | the only declared case matched | POB-3 |
| Unenumerated suite | the suite really passed | POB-5 |
| Quiet disposition | set-aside items leave the open count | POB-6 |
| Total order on concurrency | a lucky interleaving matched | POB-8 |
| Two owners | each owner's tests pass | POB-4 |
| Empty ledger | nothing failed | POB-1 |

## 5. Implementation Notes

- The automatic sources (structure of the original, recorded process or interface behaviour) produce candidates; a reviewed obligation is the usual way a high-consequence behaviour enters the ledger with the right strength.
- The ledger is cheap to recompute and should be: it is a function of the evidence, the reviewed obligations and the owner bindings. Storing a ledger's closure status separately from the evidence it was computed from invites the two to drift.
- Where a project's parity work is small, the ledger may be a handful of entries in the work's handoff record. The invariants do not require tooling; they require that the claim set is explicit.

## 6. Drawbacks & Alternatives

- **The strength scale makes closure expensive.** Accepted: the cost lands exactly where a cheap check would have lied.
- **Declaring case kinds per obligation is tedious.** Held: the undeclared refused case is where rewrites diverge most, and silence about it reads as coverage.
- **Alternative — one end-to-end comparison of the whole program:** rejected as the closing mechanism. It is a useful smoke check, but a whole-program pass names no obligation (POB-5), and a whole-program difference localizes nothing.
- **Alternative — treat the original's source, where available, as the specification:** rejected as sufficient. Source is a static reading; it creates candidates (POB-1) and says nothing about what the shipped artifact did.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[VERIFY]` | `.design/main/specifications/l1-completion-verification.md` | CMP-4 — the check proves the claim, the principle POB-2 orders |
| `[ORACLE]` | `.design/main/specifications/l1-acceptance-oracle.md` | AO-5, AO-10 — positive controls and refusal of an empty set |
| `[COMPARE]` | `.design/main/specifications/l1-observation-comparison.md` | Comparison modes and per-dimension verdicts a verifier returns |
| `[CURRENCY]` | `.design/main/specifications/l1-evidence-currency.md` | EVC-1, EVC-5 — identity binding and honest statuses |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.0 | 2026-10-22 | Core Team | Initial concept — the **parity obligation ledger**: the claim set of work judged against an existing original (a port, a behaviour-preserving rewrite, a migration, a feature rebuilt from observation). Mined from an external reverse-engineering toolkit whose reconstruction workflow closes each claim only with an owner, case fixtures and a verifier of comparable authority that names the claim. Ten invariants: conservative derivation, an empty ledger being unknown (POB-1); an ordered evidence-strength scale with the closing check at or above the original's strength (POB-2); declared case kinds covered on both sides (POB-3); one identity-bound owner (POB-4); a verifier that names the obligation (POB-5); dispositions recorded and never counted as verified (POB-6); contradiction, open unknowns and open dependencies keeping closure open (POB-7); a declared comparison mode, total order refused for concurrent behaviour (POB-8); ledger identity by digest (POB-9); a closed ledger claiming the declared obligations only, never equivalence (POB-10). Concept-only; no L2 yet. |
