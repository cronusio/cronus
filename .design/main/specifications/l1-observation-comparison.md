# Observation Comparison

**Version:** 1.0.0
**Status:** Stable
**Layer:** concept

## Overview

Much of the corpus ends in the same question: are these two observations of the same thing? A before-and-after run of a fixed bug, a run of the original program against a rewrite, a driven session replayed after a change, two executions of one workflow through different paths, a page captured yesterday and today. Every one of them is answered by comparing two recorded observations, and every one of them can be answered wrongly in the same few ways — by comparing records taken under different settings, by treating a dimension that was never recorded as equal, by insisting on an order the system never promised, or by normalizing away the very difference that mattered.

This specification owns the comparison itself: what makes two observations comparable, what a comparison returns, and which outcomes it must never produce. The answer is per dimension and three-valued — **changed**, **unchanged**, **unknown** — and *unchanged* is the one that has to be earned.

The unifying property is that **a difference proves something by itself, and sameness proves something only when both sides looked**.

## Related Specifications

- [l1-parity-obligations.md](l1-parity-obligations.md) — a parity verifier returns a comparison under this contract; POB-8's admissible modes are OBC-7's.
- [l1-completion-verification.md](l1-completion-verification.md) — CMP-12 requires an uncapped report for a before/after proof. OBC-4 is the general rule behind it: a capped or truncated dimension is unknown, and unknown never supports "the symptom is gone".
- [l1-surface-driver.md](l1-surface-driver.md) — DRV-14's recorded, replayable sessions are the most common pair of observations this contract compares; DRV-6's *unavailable* frame is an OBC-4 unknown.
- [l1-evidence-currency.md](l1-evidence-currency.md) — EVC-1 binds each observation to the bytes it describes; a comparison over observations whose subjects are not what the caller thinks they are is answered correctly and is irrelevant.
- [l1-epistemic-status.md](l1-epistemic-status.md) — a comparison verdict is a computed standing over two observed ones; *unknown* here is a finding in EPS-8's sense, with its reason.
- [l1-fanout-attestation.md](l1-fanout-attestation.md) — FAN-1: concurrency is attested, never assumed. OBC-7 is the reading-side counterpart: where concurrency is attested, ordering is not compared as if it were absent.
- [../../nodus/specifications/l1-nodus-testing.md](../../nodus/specifications/l1-nodus-testing.md) — nodus NT-11 differential parity and nodus NT-13 are the workflow-language instance of this contract.

## 1. Motivation

Six ways a comparison reports *unchanged* without having earned it:

- **Different settings.** One run had a wider terminal, another working directory, a different set of watched paths. Their outputs differ, or worse, happen to agree, and the verdict is about the settings.
- **The unrecorded dimension.** One side captured the rendered page and the other only its address. The address matches; the page was never compared, and nothing in the result says so.
- **The capped report.** Both sides reported their first fifty findings. The fifty match. The fifty-first, on one side only, is the regression.
- **The imposed order.** Two responses arrive in either order. The comparison of sequences flips between *changed* and *unchanged* on reruns, and the noise teaches everyone to ignore it.
- **The normalization drift.** A rule that blanks out port numbers was broadened between the two captures and now also blanks out version numbers. The comparison is consistent and meaningless.
- **The misaligned step.** Two recorded sessions are aligned by position; one gained a step, and every later step is compared with its neighbour.

## 2. Constraints & Assumptions

- **Observations are recorded once and compared many times.** The comparison may run long after either observation, by a different actor, under different software versions.
- **Recording is bounded.** Output, files hashed, samples taken and time spent are all limited; a record can be partial in one dimension and complete in another.
- **Some content must be normalized to compare at all** — timestamps, temporary identifiers, ports — and every normalization is a decision to stop seeing something.
- **Sensitive values are redacted before anything is durable.** Comparisons run over redacted records.

## 3. Core Invariants

Rules every Layer 2 implementation MUST NOT violate. They are technology-neutral.

- **OBC-1 (Each observation commits its comparison contract, and differing contracts are refused by name):** an observation records a **commitment** to every setting that affects what it could observe — the working location, the explicitly set environment, the observed paths, geometry, scheduled inputs, timeouts, limits, and the normalization in force — as an identity over those values. Comparing two observations whose contracts differ is **refused**, and the refusal **names the differing settings**. What deliberately may differ (the program under test, its arguments) is declared outside the contract. Agreement between observations taken under different contracts is not evidence of anything.

- **OBC-2 (The verdict is per dimension and three-valued):** a comparison returns, for each dimension it covers (output, exit, files, events, rendered state, and so on), exactly one of **changed**, **unchanged**, **unknown**, each changed or unknown dimension carrying the evidence for it (the before and after identities, the location of the first difference where it can be localized, or the reason it is unknown). There is no aggregate *unchanged* unless every dimension is *unchanged*; there is an aggregate *changed* as soon as one dimension is.

- **OBC-3 (Unchanged is earned only where both sides looked completely):** a dimension is *unchanged* only when **both** observations recorded it **completely** under the same contract and the normalized contents are equal. A dimension one side did not request is *unknown* — not requested means not looked, never "nothing there". A dimension observed as empty and declared complete is comparable; a dimension with no completeness declaration is *unknown*, even if both sides are empty.

- **OBC-4 (Truncation, omission and partial coverage make that dimension unknown, and only that dimension):** where a record's bound was reached, a part was omitted, or enumeration was not exhausted, the affected dimension is *unknown* and states why; **unaffected dimensions keep their results**, so one oversized output does not erase a clean comparison of exit status and files. Absence of an item from a partial record never proves the item was absent.

- **OBC-5 (A difference proves change even when sameness cannot be proven):** an alignment failure, a contract mismatch in a declared-variable field, or a partial record **prevents an *unchanged* claim**, but an observed difference between parts that **were** comparably recorded still proves *changed*. A comparison that cannot reach a verdict of sameness is not therefore silent about difference.

- **OBC-6 (Steps align by explicit identity, never by position):** where observations are sequences of steps, steps are matched by an **explicit, unique step identity** recorded in both. A duplicate identity, a step present on one side only, or a step whose action kind changed is an **alignment failure** reported as such; no step is ever compared with its positional neighbour.

- **OBC-7 (Ordering is compared at the strength the system promises — total, partial, or a finite accepted set):** events the observed system emits concurrently are **not** compared as one sequence. A comparison declares, per event family, whether it compares a **total order**, a **partial order** (order within each independent source, no order across them, synchronization points as barriers), or membership in a **finite set of accepted traces** stated as exact events plus ordering constraints. Concurrent events are attributed to the step that caused them, not the step during which they happened to arrive. Timestamps are recorded data, never evidence of causality. A trace specification never discards an observed event it does not mention; it makes the comparison *unknown* instead.

- **OBC-8 (Normalization is committed, canonical, versioned, and applied only to redacted content):** every normalization rule applied before comparison is part of the commitment (OBC-1) with its **version**; the rule set is applied in a **canonical order**, so the same policy written in a different order yields the same result; observations normalized under different versions of a rule are **not** compared, and the refusal says to re-observe both sides under one version. Normalization runs on the already-redacted durable record and can never re-expose a redacted value. Where normalization changed a recorded chunk, the original is retained beside the normalized form.

- **OBC-9 (A historical record lacking what the current contract requires is preserved and refused, never repaired):** an observation recorded before the contract required some field — an accounting of what was truncated, the identity of an event, a dimension — is **kept unchanged** and **refused for current comparison**, with the instruction to observe again. Missing facts are never synthesized from defaults, from the other side, or from the current run to make two records comparable.

- **OBC-10 (The comparison's own result has an identity):** a comparison result records the identities of both observations, the contract, the normalization digest, and its per-dimension verdicts, and is identified by a digest over all of them, so the same comparison reproduced elsewhere is checkable byte for byte.

> An L2 implementation cannot reach RFC until every invariant above is addressed in its Invariant Compliance section.

## 4. Detailed Design

### 4.1 Verdict per dimension

| Both sides recorded completely, same contract | Normalized contents | Verdict |
| --- | --- | --- |
| yes | equal | `unchanged` |
| yes | differ | `changed` (with first difference) |
| no — truncated, omitted, not requested, undeclared coverage | differ where both recorded | `changed` (OBC-5) |
| no | no difference found where both recorded | `unknown` (with reason) |
| contract differs | — | refused (OBC-1) |

### 4.2 Ordering modes (OBC-7)

| Mode | States | Use when |
| --- | --- | --- |
| total | one sequence | the system serializes the events |
| partial | per-source sequences; barriers | independent sources run concurrently, joined at known points |
| finite-trace | exact events + ordering constraints | interleavings are bounded and enumerable, and only some are acceptable |

A network exchange with concurrent requests is the canonical partial-order case: group events by exchange, attribute each exchange to the step that started it, and compare the groups as an unordered collection.

### 4.3 Failure modes named

| Mode | Closed by |
| --- | --- |
| Different settings | OBC-1 |
| Unrecorded dimension read as equal | OBC-3 |
| Capped report | OBC-4 |
| Imposed order | OBC-7 |
| Normalization drift | OBC-8 |
| Positional alignment | OBC-6 |
| Old record repaired to fit | OBC-9 |

## 5. Implementation Notes

- The contract commitment is cheapest computed when the observation is taken, from the resolved settings, not reconstructed later from what the record happens to contain.
- Keeping separate truncation accounting per dimension (raw output, normalized output, rendered state, files before, files after, process samples) is what makes OBC-4's "only that dimension" possible; one aggregate truncated flag is useful as a summary and useless for the verdict.
- Normalization that cannot know whether a token is complete (a value split across two recorded chunks) leaves it unchanged rather than guessing.

## 6. Drawbacks & Alternatives

- **More comparisons end in *unknown*.** Accepted: each one names what to record next, which is the point.
- **Contract refusal is strict.** Two runs from different working directories cannot be compared at all. Held: the alternative compares settings and reports it as behaviour.
- **Alternative — fuzzy matching with a similarity score:** rejected as a verdict. A score without per-dimension coverage hides exactly the unrecorded dimension and the capped report.
- **Alternative — repair old records with defaults so history stays comparable:** rejected (OBC-9). A synthesized fact is indistinguishable from an observed one by the time anyone reads it.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[VERIFY]` | `.design/main/specifications/l1-completion-verification.md` | CMP-12 — the uncapped before/after report OBC-4 generalizes |
| `[DRIVER]` | `.design/main/specifications/l1-surface-driver.md` | DRV-6, DRV-14 — recorded sessions, unavailable frames |
| `[PARITY]` | `.design/main/specifications/l1-parity-obligations.md` | POB-8 — the consumer of comparison modes |
| `[FANOUT]` | `.design/main/specifications/l1-fanout-attestation.md` | FAN-1 — attested concurrency |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.0 | 2026-10-22 | Core Team | Initial concept — **comparing two recorded observations**: what makes them comparable and what the comparison may claim. Mined from an external reverse-engineering toolkit's capture comparisons (process, terminal, browser scenario, network), whose breaking-change history repeatedly tightens exactly these rules. Ten invariants: a committed comparison contract with differing contracts refused by name (OBC-1); a per-dimension, three-valued verdict (OBC-2); *unchanged* only where both sides recorded completely (OBC-3); truncation making only the affected dimension unknown (OBC-4); a difference proving change even when sameness cannot be shown (OBC-5); alignment by explicit step identity (OBC-6); ordering compared at the promised strength — total, partial, or a finite accepted set — with concurrent events attributed to their causing step (OBC-7); committed, canonical, versioned normalization over redacted content (OBC-8); historical records preserved and refused, never repaired (OBC-9); a result identity (OBC-10). Concept-only; no L2 yet. |
