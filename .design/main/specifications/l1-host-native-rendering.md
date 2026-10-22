# Host-Native Rendering

**Version:** 1.2.0
**Status:** Stable
**Layer:** concept

## Overview

Host-native rendering is the **outbound** distribution contract: how a definition authored once inside the system — a role, a persona, a skill, an archetype, a workflow — is **materialized into a foreign host's own native format** so that host can use it without knowing anything about this system.

It is the mirror image of the inbound path. The extension marketplace governs *what comes in* (an addressable third-party entry, resolved → attested → vetted → activated). This spec governs *what goes out*: the same authored definition rendered into a neighbouring tool's config directory, in that tool's file layout, at that tool's expressive level — while the authored definition remains the single source of truth and the rendition remains a build output that nobody edits.

The contract's centre of gravity is **honest degradation**. Foreign hosts are not equally expressive: some accept one artifact per definition, some accept only a single combined file for everything, some accept no text artifact at all and can only be fed through their own tooling. A distribution layer that pretends these are the same silently loses per-unit identity, selective activation, and metadata, and reports success while doing it.

## Related Specifications

- [l1-agent-framework-skeleton.md](l1-agent-framework-skeleton.md) — AFS-14 names the canonical-source→per-host adapter distribution shape; this spec is the contract AFS-14 hands off to, expanding it into render determinism, fidelity classes, identity, destinations, and binding modes.
- [l1-extension-marketplace.md](l1-extension-marketplace.md) — the **inbound** twin (XM-1…XM-9). Both are distribution; they point in opposite directions and share the stable-identity rule (XM-2 ↔ HNR-6).
- [l1-derived-artifact-handoff.md](l1-derived-artifact-handoff.md) — DAH-1 derived-only-never-authority is the same discipline applied to rebuildable indexes; a rendition is a derived artifact by exactly that argument (HNR-1, HNR-12).
- [l1-declarative-configuration.md](l1-declarative-configuration.md) — DC-1 forbids a second separately-maintained copy of a declaration because it drifts invisibly; the target catalog (HNR-7) is such a declaration, and every consumer derives from it rather than hardcoding a copy.
- [l1-progressive-disclosure.md](l1-progressive-disclosure.md) — PD-1/PD-5 decouple catalog breadth from resident context cost; §4.5 applies them *on the foreign host* when that host eagerly loads its whole catalog.
- [l1-deployment-neutrality.md](l1-deployment-neutrality.md) — DN-1 local-first: rendering writes outside the workspace into a principal's own tool directories, which is a consented act (HNR-11), never a silent side effect.
- [l1-security.md](l1-security.md) — a rendition carries instruction text into another agent's context; the write is authority-neutral (it grants the foreign host nothing this principal has not already granted it).
- [l1-corpus-originality.md](l1-corpus-originality.md) — the admission gate on the source corpus that keeps the rendered set free of near-identical descriptors, whose cost is routing ambiguity on every target host.
- [l1-rejection-memory.md](l1-rejection-memory.md) — a principal's explicit opt-out of an install tier/component is a declined proposal; HNR-8 requires a self-refresh to honour it rather than silently re-introduce it.
- [l1-file-management.md](l1-file-management.md) — the write discipline the plan/apply steps (HNR-8) ride on.
- [../../nodus/specifications/l1-nodus-portability.md](../../nodus/specifications/l1-nodus-portability.md) — LP-9 extraction attestation and LP-13 stable-identity/governed-rename are the workflow-library counterparts of HNR-6 and of the integrity expectation on a shipped bundle.

## 1. Motivation

A system that authors high-quality definitions and keeps them to itself is a walled garden; one that copies them by hand into every neighbouring tool is a drift generator. The moment the same persona exists as five hand-maintained files in five tool directories, four of them are wrong and nobody knows which.

Left unspecified, an implementation improvises in five predictable ways, each of which is a defect class:

- **Editing the rendition.** A fix is applied where it was noticed — in the rendered file — and is destroyed by the next regeneration, or worse, survives and forks the definition.
- **Pretending all hosts are equal.** N definitions are concatenated into a host's single combined config file, and the operator is told "installed" while per-unit selection, per-unit metadata, and selective activation quietly no longer exist.
- **Referencing by display name.** A rename of a human-facing title breaks every reference that pointed at it, because the display string was doing an identity job it cannot do.
- **Writing wherever it seems right.** Paths are guessed from folklore about where a tool keeps its config, with no detection, no scope distinction between principal-wide and project-local, and no override.
- **Non-deterministic output.** The same source renders differently on two runs (timestamps, ordering, environment leakage), so renditions cannot be diffed, reviewed, or verified, and every regeneration looks like a change.

Naming the contract once fixes all five: **one authored source, deterministic derived renditions, declared fidelity, stable machine identity, declared destinations, and a previewable idempotent write.**

## 2. Constraints & Assumptions

- **The foreign host is not modifiable.** Its format, its directory layout, and its expressive limits are given; the contract adapts to them and never assumes the host will change.
- **Renditions are disposable.** Deleting every rendition costs a regeneration and never loses information (HNR-12); this is what makes them safe to overwrite.
- **The rendered set can be large.** A catalog of hundreds of definitions is normal, so per-run selection (HNR-10) and host-side resident cost (§4.5) are first-order concerns, not optimizations.
- **Writes land outside the workspace.** Destinations are the principal's own tool directories, which makes materialization a consented, enumerable act rather than an internal file operation.
- **A rendition is instruction text.** What is written becomes another agent's context. It is derived from first-party authored content, so it inherits that content's trust level — it does not launder untrusted content into a neighbouring tool.

## 3. Core Invariants

Rules every Layer 2 implementation MUST NOT violate. They are technology-neutral.

- **HNR-1 (One authored source, derived renditions):** each definition is authored **exactly once** in the system's own form. Every host-native rendition is a **derived artifact** produced from it. A rendition MUST NOT be edited in place, MUST NOT be a second place a property is declared, and MUST NOT be treated as an input to anything. Editing a rendition is editing a build output.

- **HNR-2 (Named render contract, deterministic output):** a rendition is produced by a **named render contract** (a format). The name is the guarantee: the same source, under the same format, at the same contract version, produces **byte-identical** output on every run and on every machine. Two targets MAY share a format name **only if** their rendered artifacts are identical; a target needing one different byte needs its own format. Non-determinism — run timestamps, unordered iteration, environment leakage into content — is a defect, because it makes renditions undiffable and every regeneration indistinguishable from a real change.

- **HNR-3 (Declared fidelity class):** every target declares, once and upstream of all consumers, **how much of the source model it can carry**:
  - **per-unit** — one artifact (file or directory) per definition; unit identity and selective activation survive;
  - **collapsed** — all selected definitions render into a **single combined artifact**; per-unit identity and selective activation do **not** survive;
  - **opaque** — the target cannot accept a rendered text artifact at all and is reachable only through its own tooling; no consumer may render it as a string.

  Consumers **branch on the declared class**; a consumer that infers a target's shape from its file extension, or assumes per-unit because that is the common case, has re-introduced the guesswork this class exists to remove.

- **HNR-4 (Honest loss, never silent absorption):** where a target's fidelity class cannot carry a source property — per-unit identity, per-unit activation, structured metadata, a section the format has no slot for — the loss is **declared to the operator at plan time** and recorded with the result. A collapsed rendition MUST NOT be reported in terms that imply per-unit behavior it cannot deliver. Rendering into a less expressive host is a legitimate, useful act; misreporting what happened is not.

- **HNR-5 (Section roles are the portability contract):** a definition's body carries **declared semantic section roles** — minimally, *who the unit is* (identity, voice, boundaries) versus *what the unit does* (mission, procedure, deliverables, measures). Renderers targeting hosts that separate these planes split on the declared roles, never on prose heuristics. A definition whose sections classify into **none** of the roles, or into only one where a target needs both, is **flagged at authoring time** — it will render wrong on every such host, and authoring time is the only moment the author is still present to fix it.

- **HNR-6 (Stable machine identity, distinct from display name):** every rendition is addressed by a **stable slug** derived from the source by a **declared policy** (which field it comes from, how it is normalized, any target-required prefix). Display names are human-facing, are expected to drift, and MUST NOT be the reference key — anything that points at a definition (a set membership, a manifest, another artifact) points at the slug. **Slug collisions are a build failure**, never a last-writer-wins overwrite. A rename of the slug is a governed migration (XM-2), never a silent re-point.

- **HNR-7 (Declared destination, detected host, never guessed):** a target declares its **detection evidence** (what proves the host is present), its **install scopes** (principal-wide, project-local, or both — a target that has no notion of one of them says so), and its **destination path templates**. Materialization MUST NOT write outside a declared destination, MUST NOT invent a path for an undetected host, and MUST honour an explicit operator override of any default path. The target catalog is a single declaration (DC-1): every consumer — renderer, installer, validator, presentation surface — derives from it, and a hardcoded second copy of the target list is a defect, because it silently drops targets the catalog gains.

- **HNR-8 (Plan before write, idempotent, attributable):** materialization is **previewable as a plan** — every artifact that would be written, to which path, under which scope, with which declared losses (HNR-4) — and the plan is producible **without writing anything**. Applying the same plan twice yields the same state as applying it once. Every written artifact is **attributable to its source unit**, so the set this system placed on the host is enumerable and removable without collateral damage to artifacts it did not write. Where a destination is a file the system **does not own** (a shared host-config file that already holds unrelated user settings), the system writes only a **marker-fenced region of its own** and leaves every other byte untouched; on re-run it replaces that region and nothing else; and a slot the format allows only one of (a single statusline / entrypoint field) that is currently occupied by a value **not** written by this system is left in place, not overwritten. Removal (the inverse of the install) deletes **exactly** the fenced regions and files the install wrote, identified by attribution, never a best-effort guess. A principal's **explicit opt-out** of a tier or component is **recorded durably** (a persisted stamp), and a later automatic self-refresh MUST honour it — a self-updating installer that silently re-introduces a component the principal declined has overridden a decision it was never granted (this is the write-time face of the declined-proposal discipline in [l1-rejection-memory.md](l1-rejection-memory.md)).

- **HNR-9 (Explicit binding mode):** a rendition is placed either as a **snapshot** (an independent copy that is frozen until the next regeneration) or as a **live binding** (a reference that tracks the source as it changes). The mode is an **explicit choice**, and the operator can always determine which mode is in effect for a given rendition. The two have different truth semantics — a snapshot goes stale silently, a live binding propagates an unreviewed edit instantly — and a layer that leaves the mode implicit will be wrong about which failure it is exposed to.

- **HNR-10 (Selection is a first-class input):** *which* definitions are rendered is a **filter over the catalog** — by grouping, by individual unit, or by an explicit externally-supplied list — composable with the target selection. All-or-nothing is a default, never a structural limit. On a **collapsed** target (HNR-3) the selection is the only control the operator has over what the host ends up carrying, which makes it load-bearing rather than convenient.

- **HNR-11 (Out-of-workspace materialization is consented and enumerable):** writing into a foreign host's configuration directory is an act **outside** this system's own storage. It requires the principal's consent, is enumerated before it happens (HNR-8), and never occurs as an invisible side effect of an unrelated operation. Rendering grants the foreign host **no authority**: it moves instruction text the principal already owns into a tool the principal already runs, and the receiving host's own permissions continue to govern what that text can cause (SEC-10 — nothing in this path mints authority). Consent is **tiered by blast radius**, and the tiers are separately declinable: a write into the workspace's own **version-controlled** files propagates to every teammate through the repository; a write into the principal's **machine-wide** tool config affects every project they open with that tool; a write into a **project-local, un-tracked** path affects only this checkout. A single "yes" to one tier is not consent to a broader one, the plan (HNR-8) groups writes by tier, and a flag that suppresses one tier suppresses it completely (a partial suppression that still performs the broadest write is the defect this tiering exists to prevent). Where there is **no channel to obtain consent** — a non-interactive invocation — the system writes nothing and emits the command the principal would run to grant it.

- **HNR-12 (The rendition set is regenerable from source alone):** no information exists **only** in a rendition. Losing every rendered artifact costs a regeneration and nothing else. This is the property that makes HNR-1's "never edit a rendition" enforceable rather than merely advisory, and it is the same argument DAH-1 makes for derived indexes: derived-only, never authority.

- **HNR-13 (A registration's state is judged from the files; a live connection is never inferred from them):** [ADDED v1.2.0] checking whether this system is registered in a foreign host answers, per host and scope, from a closed set — **aligned** (the registration matches what would be written now), **stale** (present but different: an old command path, an older version), **missing**, or **invalid** (present and unreadable) — and reports the **live connection** separately as **unknown** unless a running session supplies its own identity. A correct file proves the host *will* find this system on its next start; it does not prove the running host has. The verdicts route to different remedies and are never merged: stale, missing or invalid → a plan-first repair (HNR-8); aligned but the capability absent in the running host → restart or reconnect it, **not** another install. A repair without an explicit scope refreshes only registrations this system already owns; configuring every detected host is a separately named, consented choice (HNR-11). A registration repair never installs or upgrades software the registration does not itself consist of.

## 4. Detailed Design

### 4.1 The target declaration

One machine-readable entry per target, holding everything any consumer needs and nothing any consumer should decide for itself:

| Field group | Carries | Consumed by |
| --- | --- | --- |
| Identity | stable target key, display label | every surface |
| Detection | evidence that the host is installed; where it keeps definitions | detector, plan |
| Render | the **format** name (HNR-2), the **fidelity class** (HNR-3) | renderer, installer |
| Identity policy | which source field the slug derives from, normalization, required prefix | renderer, any referencing artifact |
| Destination | per-scope path templates, scope availability, override hooks | installer, plan |

The entry is the *only* place these facts live. A validator holds the declaration against reality — every declared destination template well-formed, every declared format actually implemented, every target in the catalog reachable by the installer — and fails the build on divergence, because the failure mode of a stale declaration is silent (a target that exists in the catalog but is skipped by an installer holding its own hardcoded list).

### 4.2 Fidelity classes and what each one costs

| Class | Artifact shape | Survives | Lost |
| --- | --- | --- | --- |
| per-unit | one file/dir per definition | unit identity, per-unit activation, per-unit metadata | — |
| collapsed | one combined artifact for the selected set | the *content* of every selected definition | per-unit identity, selective activation, per-unit metadata; ordering becomes semantically load-bearing |
| opaque | none renderable | — | the whole rendering path; only the host's own tooling can install |

The collapsed class is where honesty is hardest and most necessary. The content is all there, so the operation *looks* complete; what is gone is the ability to address, activate, or update one unit independently. HNR-4 requires that to be said out loud at plan time.

### 4.3 Why the slug, not the name

A reference to a definition — membership in a curated set, a dependency, an operator's saved selection — must survive the definition being retitled. The display name is the field most likely to be improved and least likely to be checked for referential impact. Deriving a stable slug and referencing *that* makes renames free and makes every reference mechanically checkable: a validator resolves every referencing slug against the real corpus and fails on the first one that does not exist. A reference set that is never resolved against the corpus rots into a list of names that used to mean something.

### 4.4 Plan, apply, and removal

```
detect targets ──▶ resolve selection ──▶ render (deterministic) ──▶ PLAN
                                                                     │
                                          operator reviews losses ◀──┤
                                                                     ▼
                                                                   APPLY (idempotent, attributed)
                                                                     │
                                                                   REMOVE (only what we wrote)
```

The plan is the consent surface (HNR-11) and the honesty surface (HNR-4) at once. Removal is only credible because of attribution (HNR-8): without it, cleanup is either incomplete or destroys artifacts the operator authored themselves.

### 4.5 Resident cost on the receiving host

Some hosts load their entire definition catalog into every session's context. Rendering a large catalog into such a host is not a neutral act — it spends the *host's* context budget on descriptors it will mostly not use, which is precisely the failure PD-5 exists to prevent, occurring one system away.

Where a target permits it, the correct rendition of a large catalog is therefore **not** N eagerly-advertised descriptors but a **small fixed surface plus an on-disk index**: the host sees a minimal always-resident entry point, and the full catalog is consulted and expanded only on demand (PD-1/PD-2/PD-3). This is progressive disclosure realized across a system boundary, and choosing it is a property of the target's render contract (HNR-2), not an ad-hoc optimization.

### 4.6 What this contract is not

- **Not the inbound path.** Bringing a third-party artifact *in* is the marketplace contract (resolve → attest → vet → activate). Nothing here weakens it; rendering outward never becomes a route back in.
- **Not synchronization.** Renditions are one-way derived output (HNR-12). A change made in a rendition is not a change; it is damage, and the next regeneration repairs it.
- **Not authority distribution.** A rendition carries instruction text, never permission (HNR-11).

## 5. Implementation Notes

- Determinism (HNR-2) is testable: render twice into separate trees and compare byte-for-byte, and render on two platforms and compare. Making this a standing check is cheaper than diagnosing a spurious diff later.
- The section-role classifier (HNR-5) belongs at authoring time, alongside the definition's other structural checks, and should report *which* role has no matching section and *which* targets that will break — a check that only says "malformed" gets suppressed rather than satisfied (TW-4).
- Slug collision (HNR-6) is best detected while building the catalog index, before any target is touched, so the failure names both colliding sources rather than surfacing as a mysterious missing rendition.
- Snapshot vs live binding (HNR-9) is often an operating-system-level distinction (copy versus link). Where the platform cannot express the live form, the layer says so and falls back to snapshot **explicitly**, rather than silently producing a different truth semantics than the operator asked for.

## 6. Drawbacks & Alternatives

- **Maintenance cost per target.** Each supported host is a format, a destination declaration, and a determinism test. Mitigated by format sharing (HNR-2) where output is genuinely identical, and bounded by the fact that an unsupported host costs nothing — the catalog simply does not list it.
- **Alternative — hand-maintained per-host files:** rejected. It is the drift generator this contract exists to eliminate, and it fails silently: the copies diverge long before anyone notices which one is authoritative.
- **Alternative — let each target's renderer decide its own shape:** rejected (HNR-3). Fidelity is a property of the *target*, known once, and consumers other than the renderer (planner, installer, presentation) need it too; leaving it implicit forces each of them to re-derive it, and they will not agree.
- **Alternative — reference definitions by display name:** rejected (HNR-6). It couples every reference to a field that exists to be improved.
- **Collapsed targets remain genuinely lossy.** No contract can give a single-file host per-unit selection. This spec does not fix that; it requires the loss to be stated (HNR-4) so the operator's expectations match reality.
- **Consent friction (HNR-11).** Enumerating every out-of-workspace write costs a confirmation step. That is the intended price of writing into a principal's other tools. <!-- TBD: whether a standing per-target grant is the right ergonomic relief, and what invalidates it -->

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[SKELETON]` | `.design/main/specifications/l1-agent-framework-skeleton.md` | AFS-14 canonical-source→per-host adapter distribution, the shape this contract expands |
| `[MARKET]` | `.design/main/specifications/l1-extension-marketplace.md` | The inbound twin; stable-identity and governed-rename discipline |
| `[DERIVED]` | `.design/main/specifications/l1-derived-artifact-handoff.md` | Derived-only-never-authority (DAH-1), the argument behind HNR-1/HNR-12 |
| `[DECLCONF]` | `.design/main/specifications/l1-declarative-configuration.md` | DC-1 single declaration, no drifting second copy (HNR-7) |
| `[DISCLOSURE]` | `.design/main/specifications/l1-progressive-disclosure.md` | PD-1/PD-5 resident-cost discipline applied on the receiving host (§4.5) |
| `[ORIGINALITY]` | `.design/main/specifications/l1-corpus-originality.md` | Keeps the rendered corpus free of near-identical descriptors |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.2.0 | 2026-10-22 | Core Team | Added HNR-13 — a registration verdict from a closed set (aligned / stale / missing / invalid) judged from the files, with the live connection reported separately as unknown unless the running session supplies identity; stale/missing/invalid route to a plan-first repair, aligned-but-absent to a restart, never a reinstall; unscoped repair refreshes only owned registrations; a repair installs nothing else. From a reference-mining pass over an external tool whose diagnostic command reports exactly this split and whose setup refuses to touch unrelated software. |
| 1.1.0 | 2026-09-09 | Core Team | Refined from a fourth code-graph engine's self-install / self-uninstall lifecycle. HNR-11 extended: consent is **tiered by blast radius** (version-controlled repo files ⊃ machine-wide tool config ⊃ project-local un-tracked), the tiers are separately declinable, a flag that suppresses a tier suppresses it *completely*, and with no consent channel the system writes nothing and prints the command instead. HNR-8 extended: a **marker-fenced own region** in a file the system does not own (touch only that region on re-run; never overwrite an occupied single-value slot holding a foreign value); removal deletes exactly what the install wrote; a principal's explicit opt-out is recorded durably and a later self-refresh must honour it (write-time face of `l1-rejection-memory`). |
| 1.0.0 | 2026-08-06 | Core Team | Initial concept: outbound materialization of authored definitions into foreign hosts' native formats. One authored source with derived, never-edited renditions (HNR-1) regenerable from source alone (HNR-12); named render contracts with byte-identical determinism and format sharing only on identical output (HNR-2); **declared fidelity classes** per-unit / collapsed / opaque that consumers branch on rather than infer (HNR-3), with honest declared loss at plan time and no claim of per-unit behavior a collapsed target cannot deliver (HNR-4); declared section roles as the split contract for hosts that separate identity from operation, flagged at authoring time (HNR-5); stable machine slug distinct from the drift-prone display name, collisions a build failure (HNR-6); declared detection / scopes / destination templates derived by every consumer from one catalog declaration, never a hardcoded copy (HNR-7); previewable, idempotent, attributable write with credible removal (HNR-8); explicit snapshot-vs-live binding mode (HNR-9); selection as a first-class filter, load-bearing on collapsed targets (HNR-10); consented enumerable out-of-workspace materialization that carries text and never authority (HNR-11). §4.5 applies PD-1/PD-5 across the system boundary: a large catalog renders as a small fixed surface plus an on-disk index rather than N eagerly-resident descriptors. The outbound counterpart of `l1-extension-marketplace`; expands AFS-14. |
