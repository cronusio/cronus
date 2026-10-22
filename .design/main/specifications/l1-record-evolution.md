# Record Evolution

**Version:** 1.1.0
**Status:** Stable
**Layer:** concept

## Overview

The model of **what a stored or transmitted record means to a reader that did not write it, and how the record's shape may change without destroying that meaning**. A record here is any durable structure that outlives the code that produced it: a persisted document, a synced replica, a published transcript, an exported bundle, a snapshot, a memory entry.

The organising claim is that **the rules a record must obey are set by its reader population, not by the record's contents**. A record whose only reader is the process that wrote it can change shape almost freely. A record read by code that cannot be redeployed alongside the writer must survive a reader that has never heard of half its fields. And a record read by something that then *writes it back* — a replica, a clone, an importer, a re-publisher — imposes the strongest obligation of the three: whatever that reader drops on the way through is not ignored, it is **destroyed**.

From that, four mechanisms follow. A record is partitioned into an **interpreted core** and an **opaque private region** so most evolution costs nothing. Vocabulary a reader lacks is carried through by **variant passthrough** rather than rejected. Fields a reader has never modelled are carried through by **residual capture**, which makes losslessness mechanical rather than a discipline someone has to remember. And **acceptance and authorship use different schemas**, so the tolerant path actually fires in the field instead of being unreachable behind a strict decode.

## Related Specifications

- [l1-peer-compatibility.md](l1-peer-compatibility.md) — The live twin. Same problem across a connection instead of across time; PCO-1 keeps the two version systems independent, and PCO-6's direction rule has a direct analogue here in REC-10.
- [l1-storage-model.md](l1-storage-model.md) — Where records live; this spec governs how their shapes change, not where they are kept.
- [l1-multi-device-sync.md](l1-multi-device-sync.md) — SY-3/SY-4: every replica is a reader that is also a writer, which is exactly REC-2's strongest case. A replica running an older build must not launder a newer record into a lossy one.
- [l1-data-lineage.md](l1-data-lineage.md) — Where a re-publish that silently drops content breaks the chain; REC-2 is the rule that keeps lineage claims true through an intermediary.
- [l1-context-provenance.md](l1-context-provenance.md) — Preserved-but-uninterpreted content is still provenanced content; the opaque region does not become unattributed.
- [l1-crash-recovery.md](l1-crash-recovery.md) — CR-2/CR-9: a record that a recovering reader cannot parse is a different failure from one it cannot *interpret*; REC-6 separates them.
- [l1-change-merge.md](l1-change-merge.md) — CM's three-way merge operates on records whose shape this spec governs; unmodeled regions must survive a merge the same way they survive a read.
- [l1-invariant-tripwires.md](l1-invariant-tripwires.md) — REC-12's frozen-surface guard is a tripwire in TW-1's sense, with a deliberately unusual failure policy.
- [l1-semantic-addressing.md](l1-semantic-addressing.md) — SA: where a record is addressed by the hash of its own bytes, a lossy pass-through does not merely lose data, it mints a new address (§4.2).
- [l1-work-import.md](l1-work-import.md) — Import is a read-then-write across a version boundary, and is bound by REC-2.

## 1. Motivation

**A record outlives every assumption its author held.** The writer is one build; the readers are every build that ever opened the file, plus the replica on the laptop that has not been updated since spring, plus the export someone archived. Schema changes that are trivially safe within one process are, at that scale, changes to a contract with participants who cannot be consulted.

**The costly failure is not rejection — it is silent loss.** A reader that refuses a record it does not understand produces an error someone can act on. A reader that opens it, drops the fields it does not model, and writes it back produces a record that is *valid, smaller, and wrong*, with no error anywhere. Where records are content-addressed, this is worse still: the lossy copy is not an edit of the original but a new identity for a cohort that never changed, propagating as a distinct object.

**Every field added to the interpreted surface is a permanent commitment.** Once shipped readers depend on a field's meaning, removing it, renaming it, or changing what it implies is breaking, forever, regardless of where the bytes end up. Fields therefore accumulate obligations simply by being *visible*, and a design that makes visibility the default spends its compatibility budget on data no reader ever needed.

**Tolerance implemented at the wrong layer does nothing.** An outer envelope that gracefully carries an unknown variant is defeated by one strict enumeration on a leaf inside it: the envelope recognises the shape, hands it to the known schema, and that schema fails on the leaf — taking the whole record with it. Tolerance is only as good as its strictest reachable component.

**Tolerance implemented behind a strict gate is unreachable.** If the acceptance check rejects any version other than the exact one the code was compiled against, the tolerant decode below it never runs in the field. The record fails at the gate in precisely the case the tolerance was written to survive, and the mechanism looks correct in tests and is dead in production.

**Preservation cannot rest on future authors remembering it.** "Copy the fields you do not understand" is a rule every reader must follow forever, on every path, including ones written years later by someone who never read this document. Stated as a rule it fails; built into the decode so that unmodelled data is captured and re-emitted automatically, it holds by construction.

## 2. Constraints & Assumptions

- Records may be read by builds older than the writer, newer than the writer, and by builds that will never be updated again.
- Some readers are also writers. The set of such readers is open — replicas, clones, importers, migration tools, export/re-import round trips.
- The record's schema is machine-readable and its wire form can be projected and frozen (REC-12); a record whose shape exists only in hand-written parsing code is outside this spec's protection.
- Content addressing may or may not be in use. Where it is, REC-2's cost rises sharply (§4.2), but the invariant does not change.
- This spec governs *shape and meaning*, not encryption, retention, or placement — those are the storage and confidentiality models' concerns and compose orthogonally.

## 3. Core Invariants (Layer 1 only)

- **REC-1 (The reader population sets the regime, and it is declared):** every record kind declares which population reads it — *same-build* (the writer's own process or a co-deployed component), *independently-shipped* (readers on their own release cadence), or *re-publishing* (readers that write the record back out). The regime, not the record's contents, determines which of the following invariants bind it. A record kind whose regime is undeclared is treated as re-publishing, the strictest, because an unexamined reader population only ever grows.

- **REC-2 (A re-publishing reader must preserve, not merely tolerate):** where a reader may write the record back — a replica, a clone, an importer, a migration, an export round trip — anything it fails to carry through is **destroyed**, not ignored. Such readers MUST round-trip every part of the record they do not model, and this obligation is discharged by construction (REC-5), never by asking future authors to remember it.

- **REC-3 (Interpreted core and opaque region, with one-way promotion):** a record is partitioned into a **core**, which readers interpret and which is therefore bound by the full compatibility rules, and an **opaque region**, which every reader preserves verbatim and none interprets. The writer evolves the opaque region freely — no version bump, no reader argument, no frozen-surface change. A field may be promoted from opaque to core with a compatible version bump; it may **never** move back, because once shipped readers interpret it, removing it from the core is breaking wherever the bytes end up. New state defaults to the opaque region and is promoted only when a reader actually needs it.

- **REC-4 (Unknown vocabulary is carried, not rejected — where declared):** at slots explicitly designated as extensible, a reader meeting a variant it does not know surfaces it as *unrecognised* while retaining the complete subtree unaltered, and the encoder re-emits that subtree unchanged. This — and only this — reclassifies *adding* a variant at such a slot from breaking to compatible. Removing or renaming a variant remains breaking. Slots not so designated, including unions nested inside a designated one, follow the ordinary rules; a slot's tolerance is never inherited by its children.

- **REC-5 (Unmodelled fields survive by construction, not by discipline):** every modelled object in a record captures the keys it does not model into a single reserved region and merges them back on encode, so a reader of an older shape re-emitting a newer record is **mechanically lossless**. The capture region's name is reserved at every level where it applies and may never become a modelled field. The set of captured levels is enumerated in a manifest with stable identifiers; an identifier is never reused with changed meaning, because consumers key behaviour off it.

- **REC-6 (Tolerance is for absent vocabulary, never for damage):** carrying an unknown variant or an unmodelled field is not a licence to accept anything. A *known* variant that fails to parse, a violated required shape, or a failed integrity check still rejects the record. Silent acceptance of malformed data under the banner of tolerance converts a loud, fixable fault into a corrupt store.

- **REC-7 (Acceptance and authorship use different schemas):** the schema a writer stamps a record with pins the exact contract version it was written against; the schema a reader validates against **widens acceptance** to the whole compatible line. Reading with the authoring schema makes the tolerant path unreachable in the field — a record one version ahead clears the version gate, is fetched, and is then rejected at parse, which is exactly the case the tolerance existed for. The two schemas are distinct artifacts derived from one shape.

- **REC-8 (Refuse at the cheapest already-held signal, before doing any work):** the compatibility decision is taken on the smallest piece of the record a reader already holds — its header, pointer, or descriptor — *before* fetching, decrypting, or assembling any body. This makes "do nothing further on a refused record" structural rather than a discipline: the work that must not happen is downstream of the decision and there is no call to remember to skip.

- **REC-9 (A minimum reader is about interpretation, never preservation):** a record may name a minimum reader version, and that mechanism exists **only** for changes an older reader cannot safely *understand* — a redefined meaning for an existing field, or new state that invalidates one the old reader does interpret. It is never set because a reader would fail to carry something through; REC-4 and REC-5 make carrying lossless without it. A rendering gap in an older reader is not grounds. A declared minimum is checked for coherence against the record's own version and is a deliberate, justified act.

- **REC-10 (Openness is decided by reader population, and a strict leaf defeats a tolerant envelope):** the same field may be correctly closed in a record read only by its own writer and incorrectly closed in a record read by independently-shipped readers. Where a value's set grows on the writer's own release cadence — identifiers of providers, harnesses, tools, extensions — the reader-facing form is open, and a reader that meets an unrecognised value renders it generically rather than treating it as a failure. This is checked structurally: a tolerant outer layer is worthless if any reachable leaf inside it rejects, because the outer layer will have already committed the record to the schema that fails.

- **REC-11 (An address proves the bytes; a cross-check proves the meaning):** where a record is referenced by a digest of its own content, verifying the digest establishes only that the bytes are the ones the reference named. Whether they *mean* what the referrer assumed — same owner, same section, same contract line — is a separate check against fields the referrer already knew, performed after integrity verification and before the content is used.

- **REC-12 (The wire form is frozen, and the guard fails on all drift):** each record kind's externally visible shape is captured as a generated, committed artifact, and any change to it — including one that is compatible — fails the guard. The failure is the point: it forces the diff and its compatibility reasoning into review rather than letting an additive-looking change pass unexamined. The artifact is regenerated by tool, never hand-edited, and its diff travels with the change that caused it. Where the modelled schema cannot describe its own wire form, the wire form is projected explicitly and the projection is itself asserted against real and truncated records — a frozen surface can be frozen and wrong.

- **REC-13 (Record version, contract version, and distribution version are independent):** the number describing what a stored record means is its own; it is not the peer-negotiation version and not the build's version. It advances on its own rules and its own cadence.
- **REC-14 (A reader never manufactures a missing fact to make an old record current):** [ADDED v1.1.0] where a record predates a field that current consumers rely on — an identity, an accounting of what was omitted, a dimension, a provenance mark — a reader does **not** fill it from a default, from a neighbouring record, or from the present environment so the record passes as current. It keeps the record **unchanged and readable** under its original meaning, **refuses it for uses that require the missing fact**, and names the remedy (regenerate the observation, re-import from the source). The same holds on the writing side: a producer whose output lacks a field the contract requires reports it explicitly as null or unknown, rather than relying on a reader to repair it with a default. A default supplied after the fact is indistinguishable from an observation by the time anyone reads it, and it is wrong in exactly the cases where the old producer behaved differently.

> L2 specs cannot reach RFC status until all invariants here are addressed in their "Invariant Compliance" section.

## 4. Detailed Design

### 4.1 The three regimes

```text
[REFERENCE]
same-build         : writer and reader ship together
                     -> closed enums fine, shape changes cheap, no passthrough needed
                     -> the ONLY regime where a closed vocabulary leaf is safe (REC-10)

independently-shipped : reader on its own cadence, read-only
                     -> core/opaque split, variant passthrough, widened acceptance
                     -> failure mode: refuses a record, or renders it incompletely

re-publishing      : reader writes the record back out
                     -> everything above PLUS residual capture (REC-5)
                     -> failure mode: silently produces a smaller, valid, wrong record
```

The regimes are not a maturity ladder — a product legitimately contains all three at once, and the same *field* may appear in two records under two different regimes with two different correct treatments. Declaring the regime (REC-1) is what stops a rule proven in the cheap case from being copied into the expensive one.

### 4.2 Why re-publishing is the sharp case

A read-only reader that ignores an unmodelled field costs one incomplete render. A re-publishing reader that ignores it costs the field, permanently, for everyone downstream — and the loss is invisible, because the output is structurally valid.

Where records are content-addressed, a second cost appears: the lossy copy hashes differently, so it is not an update of the original but a **new object**. A reader that drops one key while re-publishing an unchanged cohort mints a fresh address for it and re-uploads it, and the store now holds two objects that were meant to be one, differing by a field neither side can see. This is why REC-5 makes preservation automatic rather than requested: the failure is not caused by carelessness, it is caused by a reader doing exactly what its schema told it to do.

### 4.3 The partition, and why the default lane is the free one

```text
[REFERENCE]
record := {
    core        : interpreted by readers   — full compatibility rules, version-bumped
    opaque      : { revision, validated-but-uninterpreted payload }
                  every reader preserves verbatim; no reader inspects
    body/parts  : the bulk content, addressed and assembled separately
}

promote(field): opaque -> core     requires a compatible version bump
                core   -> opaque   FORBIDDEN — one-way door
```

The design pressure REC-3 creates is deliberate. Adding a field to the core is a commitment made once and honoured forever; adding it to the opaque region is free and reversible. Making the opaque region the default therefore keeps the interpreted surface small — which is the actual scarce resource, since every field in it constrains every future release.

The opaque region is *not* an escape from validation. It is a structured, validated payload whose *meaning* is private to the writer; it is not an unchecked blob, and it is not a place to hide state that readers actually need to act on. A field that readers must react to belongs in the core, with the cost that implies.

### 4.4 Two kinds of tolerance, and their exact scopes

Passthrough (REC-4) and residual capture (REC-5) are often conflated; they cover different things and neither substitutes for the other.

| | Passthrough (REC-4) | Residual capture (REC-5) |
| --- | --- | --- |
| Covers | unknown **variants** at a designated slot | unknown **fields** on any captured object |
| Reader sees | "unrecognised", plus the intact subtree | nothing — the keys are simply carried |
| Effect on versioning | adding a variant at that slot becomes compatible | adding a field never loses data on re-publish |
| Scope | exactly the designated slot, never its children | exactly the levels named in the manifest |
| Not covered | nested unions, closed structural enums | a level absent from the manifest |

Two scope rules do the real work. **Tolerance does not inherit**: a union nested inside a designated slot is closed unless it is itself designated, so "we have passthrough" is never an answer to "is this addition safe?" without naming the slot. And **capture levels are enumerated**, with a guard that fails when the code and the manifest disagree — so adding a level is a deliberate registration, and a level's identifier, which consumers branch on (an importer blanking the opaque region while carrying everything else), is never reused for restructured content.

### 4.5 Reading versus writing: the two-schema rule

REC-7 is the invariant most likely to be "simplified away" by someone consolidating duplicate-looking schemas.

```text
[REFERENCE]
authoring schema : version == exactly THIS contract version
                   used by writers, and to prove a record is exactly this line

acceptance schema: version ∈ the same compatible line, any minor
                   used by every reader that materializes a record

gate (REC-8)     : on the header alone
                     major mismatch          -> refuse, fetch nothing
                     minimum-reader exceeded -> refuse, fetch nothing
                     otherwise               -> proceed to fetch and decode
```

If readers use the authoring schema, the newer-minor record passes the gate, is fetched in full, and dies at parse. The tolerant machinery below never executes, and — worse — it never executes *in tests either*, because tests are written against the current version, where acceptance and authorship agree. The mechanism appears correct until the day it is needed.

Note the ordering: the gate runs on the header, which the reader already holds after locating the record; the widened decode runs after. This is REC-8's structural refusal — the fetch is a call the assembler makes *after* the decision, so a refused record cannot leak work through a forgotten early return.

### 4.6 Where openness is required, and the leaf that ruins it

REC-10 names an asymmetry that is easy to get backwards. Consider an identifier whose value set grows every release — a model provider, an agent harness, an extension id.

- In a record whose reader **is** its writer, the identifier is correctly a **closed** enumeration: exhaustiveness is checked at compile time, and an unknown value genuinely indicates corruption.
- In a record read by independently-shipped readers, the same closed enumeration is a defect. "This record came from a provider you have not heard of" becomes a hard rejection of the whole record — and, critically, the outer passthrough does **not** save it: the record's outer shape is recognised, so it is handed to the known schema, which then fails on the leaf and takes everything with it.

Hence the structural check: sweep the reader-facing surfaces and fail if any closed vocabulary leaf remains reachable. Not every union should be reopened — where a variant's *shape* differs per value, reopening it would fork the structure and is correctly refused; such state belongs in the opaque region instead, where it is carried without ever meeting a schema that could reject it.

### 4.7 The frozen surface, and why it fails on safe changes too

REC-12's guard deliberately fails on additive changes that everyone agrees are compatible. That looks like noise and is not: the guard's purpose is not to detect breakage, it is to make **every** shape change visible as a reviewable diff with a recorded classification. A guard that self-approves additive changes cannot distinguish an addition that is safe from one that is safe-looking (PCO-7's class), because the distinction is not structural.

Two supporting rules keep it honest. The frozen artifact is **generated**, so hand-editing it to pass is both possible and obviously wrong in review. And where the modelled schema cannot describe its own wire form — a capture wrapper, for instance, reports the post-capture shape, which requires a key no writer emits and marks captured children optional — the wire form is **projected explicitly** and the projection is asserted against a real record, a truncated one, and the absence of the reserved capture key. A frozen surface that was never checked against reality freezes a fiction.

### 4.8 Demarcation

| Neighbour | Its question | Why it is not this |
| --- | --- | --- |
| [l1-peer-compatibility.md](l1-peer-compatibility.md) | Can two live peers talk? | Connection-time, negotiated, and recoverable by upgrading one side. A record has no negotiation partner: it was written once and must stand on its own. |
| [l1-change-merge.md](l1-change-merge.md) | How are two concurrent edits reconciled? | Merge operates *within* a shape; this governs how the shape may change. Unmodelled regions must survive a merge exactly as they survive a read. |
| [l1-storage-model.md](l1-storage-model.md) | Where does state live and under what durability? | Placement and durability, not meaning. A record can be perfectly durable and still be misread. |
| [l1-crash-recovery.md](l1-crash-recovery.md) | What survives an unclean stop? | Recovery deals with records that are *damaged*; this deals with records that are *unfamiliar*. REC-6 is the line between them, and conflating the two is how a tolerant reader starts accepting corruption. |

### 4.9 Nodus relevance

| Element | nodus seam | Note |
| --- | --- | --- |
| Regime declaration (REC-1) | bundle vs run-state vs trace | A run trace is same-build; an exported bundle is re-publishing and must round-trip host-specific annotations it does not model. |
| Core / opaque split (REC-3) | workflow bundle metadata | Host-specific annotations ride an opaque region rather than growing the language surface, so a host may annotate freely without a language-level bump. |
| Variant passthrough (REC-4) | step kinds, effect classes | A runtime meeting an unknown effect class carries the step intact rather than refusing the bundle — subject to LP-8 pre-run capability checks, which is where the *refusal* belongs. |
| Residual capture (REC-5) | bundle import / export | An older toolchain re-exporting a newer bundle must not silently strip its unmodelled fields. |
| Open vocabulary leaves (REC-10) | host role and capability names | These grow on the host's cadence; a closed enumeration in the bundle format would reject a valid bundle for naming a role the reader has not heard of. |

## 5. Drawbacks & Alternatives

- **Two schemas per record kind looks like duplication.** It is, and it is load-bearing (REC-7). Both are derived from one shape, so the duplication is generated rather than maintained; consolidating them is the specific mistake that renders every tolerance mechanism unreachable in production.

- **Residual capture carries data nobody can use.** A reader accumulates bytes it will never interpret, and a long-lived record can carry residue from versions no longer supported. Accepted: the alternative is destroying data on a path where nothing reports the loss. Residue is pruned only at a deliberate major transition, where the loss is explicit.

- **The opaque region can become a dumping ground.** State that readers genuinely need can hide there and never get promoted, producing readers that render less than they could. Mitigated by REC-3's direction of travel — promotion is available and cheap, demotion is not — and by the rule that the region holds writer-private state, not reader-relevant state deferred.

- **A guard that fails on safe changes will be resented.** It costs a regeneration step on every shape change. Held anyway (REC-12): the cost is bounded and mechanical, while the class of change it catches is exactly the one that looks safe and is not.

- **Alternative — version every record strictly and migrate on read.** Rejected as the sole mechanism: eager migration requires the reader to understand the newer shape well enough to translate it, which is precisely what an older reader cannot do. Migration is the right tool at a *major* boundary and the wrong one within a line.

- **Alternative — make readers lenient everywhere, no partition, no manifest.** Rejected: blanket leniency erases REC-6's boundary between unfamiliar and damaged, and produces a store that quietly accumulates malformed records nothing will ever flag.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[PEERS]` | `.design/main/specifications/l1-peer-compatibility.md` | PCO-1/PCO-6/PCO-7 — the live-connection twin; REC-13 keeps the version systems separate. |
| `[STORAGE]` | `.design/main/specifications/l1-storage-model.md` | Where records live; orthogonal to what they mean. |
| `[SYNC]` | `.design/main/specifications/l1-multi-device-sync.md` | SY-3/SY-4 — replicas as re-publishing readers, REC-2's strongest case. |
| `[LINEAGE]` | `.design/main/specifications/l1-data-lineage.md` | What a lossy re-publish breaks. |
| `[MERGE]` | `.design/main/specifications/l1-change-merge.md` | Reconciliation within a shape; unmodelled regions must survive it. |
| `[RECOVERY]` | `.design/main/specifications/l1-crash-recovery.md` | CR-2/CR-9 — damaged versus unfamiliar, the line REC-6 draws. |
| `[TRIPWIRES]` | `.design/main/specifications/l1-invariant-tripwires.md` | TW-1/TW-7 — REC-12's guard as a tripwire with an unusual failure policy. |
| `[ADDRESSING]` | `.design/main/specifications/l1-semantic-addressing.md` | Why a lossy pass-through mints a new identity rather than editing one. |

## Document History

| Version | Date | Author | Change |
| --- | --- | --- | --- |
| 1.1.0 | 2026-10-22 | Core Team | Added REC-14 — a reader never manufactures a missing fact (from a default, a neighbour, or the present) to make an old record current: it preserves the record under its original meaning, refuses it for uses requiring the fact, and names the remedy; producers report a missing required field as explicit null/unknown rather than leaving it to a reader's default. From a reference-mining pass over an external reverse-engineering toolkit whose breaking-change history repeatedly removes default-repair of old records in favour of preserve-and-recapture. |
| 1.0.0 | 2026-08-20 | Core Team | Initial spec — record evolution across reader populations: the regime is declared and set by who reads, defaulting to the strictest when unexamined (REC-1); a re-publishing reader must preserve rather than tolerate, since what it drops is destroyed and — under content addressing — mints a new identity for an unchanged cohort (REC-2); interpreted core versus opaque writer-private region with strictly one-way promotion, making the free lane the default and keeping the committed surface small (REC-3); designated-slot variant passthrough that retains the intact subtree and reclassifies variant *addition* to compatible, with tolerance explicitly not inherited by nested slots (REC-4); residual capture of unmodelled fields so losslessness is mechanical rather than a discipline future authors must remember, with enumerated capture levels under a manifest guard and identifiers never reused (REC-5); tolerance bounded to absent vocabulary and never to damage, so a known-but-malformed variant still rejects (REC-6); separate authoring and acceptance schemas, because reading with the authoring schema makes the tolerant path unreachable in exactly the case it was written for — and unreachable in tests too (REC-7); refusal taken on the cheapest already-held signal before any fetch, making "do no further work" structural instead of a remembered early return (REC-8); a minimum-reader declaration reserved for changes an older reader cannot safely *interpret*, never for preservation (REC-9); openness decided by reader population, with the observation that one strict leaf defeats a tolerant envelope and rejects the whole record (REC-10); content address proves the bytes while a separate cross-check proves they mean what the referrer assumed (REC-11); a frozen wire-form guard that fails on all drift including compatible additions, generated never hand-edited, with the wire form projected explicitly and the projection itself asserted, since a frozen surface can be frozen and wrong (REC-12); and record/contract/distribution versions kept independent (REC-13). Demarcated from peer compatibility, change merge, storage model and crash recovery in §4.8; nodus mapping for bundle import/export. Concept-only. Distilled from an adoption pass over an external multi-provider agent-orchestration desktop client whose published transcripts are read and re-published by independently released components. |
