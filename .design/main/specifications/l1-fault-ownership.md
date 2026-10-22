# Fault Ownership

**Version:** 1.0.1
**Status:** Stable
**Layer:** concept

## Overview

A product that assembles other people's components inherits their failures without inheriting their defects. When something breaks inside a composed system — a model provider, an extension, a commissioned foreign agent, a package the product merely installs, a library loaded into the product's own process — a question arrives before any repair is possible: **whose defect is this?** The fault-lifecycle contract gives the failure an identity and a status; the reporting contract carries it to someone. Neither answers ownership, and in the absence of an answer an implementation improvises one, always in the same direction: *it happened in our system, so it is ours.*

That default is expensive in both directions. Absorbing a third party's defect means repairing symptoms forever in a place the cause does not live, and the repair is a permanent local divergence nobody planned. Disowning too eagerly means telling a user their problem is elsewhere when the product's own configuration of the component is what broke it. Both mistakes are made confidently, because the evidence that distinguishes them is not the evidence that identified the fault.

This concept names the discipline that produces the ownership verdict deliberately: **an enumerated sphere of control, environmental causes excluded before any component is blamed, evidence separated from inference, and a fault owned elsewhere routed rather than absorbed.**

## Related Specifications

- [l1-fault-lifecycle.md](l1-fault-lifecycle.md) — FL owns a fault's **identity and status**; this spec owns its **owner**. The two are orthogonal: the same identity can change owner as evidence arrives, and FL-11 (the recording pipeline's own failures are faults attributed to their source) is this discipline applied to one specific component — generalized here.
- [l1-error-reporting.md](l1-error-reporting.md) — ERR carries a report **inward**, from the user to this product. This spec decides when a fault should travel **outward** instead, and ERR-1's consent gate governs that act unchanged.
- [l1-change-attribution.md](l1-change-attribution.md) — CA ranks *which measured series moved* around an event and explicitly produces a shortlist, never a cause (CHA-5). This spec produces a **verdict about responsibility**, which is a different question over different evidence; CA's shortlist is one input to it.
- [l1-completion-verification.md](l1-completion-verification.md) — CMP-7 already holds the symmetric claim to evidence: a dismissal ("pre-existing", "not ours") is a claim needing the same proof as a completion. This spec is what that proof looks like when the dismissal is *ownership* rather than *pre-existence*.
- [l1-doctor.md](l1-doctor.md) — HEAL-4 (non-destructive diagnosis) and HEAL-3 (escalate the risky) are the operational half; OWN-9 states the read-only boundary as an invariant of the diagnostic act itself.
- [l1-foreign-agent-invocation.md](l1-foreign-agent-invocation.md) — FAI governs commissioning a delegate; this spec governs the verdict when the delegate's run fails, which FAI-10's exhaustion path assumes but does not classify.
- [l1-signal-silencing.md](l1-signal-silencing.md) — SIL-6 forbids proposing a silence before a diagnosis exists; the ownership verdict is the part of that diagnosis which makes a silence legitimate (a known third-party defect) or illegitimate (our own, still unfixed).
- [l1-confidentiality-flow.md](l1-confidentiality-flow.md) — OWN-10's captured process memory is the highest-confidentiality artifact the system routinely produces, and it is produced by diagnosis rather than by any data path CF models.

## 1. Motivation

Left unspecified, the same five misattributions recur:

- **The assembler blamed for the assembled.** A component the product installs, launches, or wraps fails on its own terms, and the failure is filed, triaged, and repaired as the product's. The repair is a workaround that must be carried forever, and the real defect is never reported to anyone who could fix it.
- **The environment blamed on the component.** A process killed for memory pressure, a disk that filled, a credential that expired, a machine that slept. The component that happened to be running when the environment failed is recorded as the cause, and every future occurrence reinforces a false pattern.
- **The in-process guest blamed on the host.** Third-party code loaded into the product's own address space — an extension, a plugin, a driver, a model runtime — is a common cause and an easy suspect. Named without evidence implicating it, the accusation is indistinguishable from a guess and routes the repair away from the actual defect.
- **The uncertain verdict rendered as certain.** What the evidence proves and what the diagnostician inferred are merged into one narrative. The narrative is plausible, is acted on, and cannot be checked afterwards because its two halves are no longer separable.
- **The gap filled by invention.** An artifact is partially unreadable — unresolvable frames, a truncated log, an unparseable region — and the missing names are supplied from plausibility rather than from evidence. This is the most damaging of the five, because everything around the invented part is real.

## 2. Constraints & Assumptions

- **The system is assembled, not authored.** Most components running under this product were written elsewhere, and their internals are neither visible nor changeable here.
- **Ownership is decidable only against a declared boundary.** Without an enumerated sphere of control, every case is argued from scratch, and arguments made under pressure resolve toward whoever is present.
- **Evidence is incomplete by default.** Symbols are missing, logs rotate, the environment has moved on. A discipline that requires complete evidence produces no verdicts.
- **Diagnosis runs on a live system a person depends on.** It is not a maintenance window, and anything it changes it must be able to undo.
- **A capture of a running process contains whatever that process held.** Memory images, heap dumps, and full-state snapshots carry credentials, tokens, and user documents by construction, not by accident.

## 3. Core Invariants

Rules every Layer 2 implementation MUST NOT violate. They are technology-neutral.

- **OWN-1 (Ownership is a verdict of its own, from a closed set, and *undetermined* is one of its values):** every fault that reaches triage carries an explicit **owner verdict** drawn from a closed set — **ours** / **a named external component's** / **the environment's** / **undetermined** — produced as its own act, separately from the fault's identity and status. *Undetermined* is a real terminal value of that set and MUST NOT be collapsed into *ours* as a default; a system with no way to say "we do not yet know whose this is" has hidden that state inside whichever value absorbs it, and the value that absorbs it is always the one belonging to whoever is present.

- **OWN-2 (The sphere of control is enumerated in advance, not argued case by case):** the product **declares, ahead of any incident, which surfaces it owns** — its own code and commands, its configuration of third-party components, its packaging and defaults, its integration seams. Assembling, installing, launching, or wrapping a component does **not** make that component's internal defects ours; *configuring* it can, and that is precisely why the configuration surface is inside the declared boundary. A boundary produced at incident time is produced under pressure and will be redrawn to match the conclusion already reached.

- **OWN-3 (Environmental causes are excluded before any component is blamed):** resource exhaustion, host-level termination, storage capacity, clock movement, network loss, and credential expiry are **checked and excluded first**, and a fault caused by one of them receives the **environment** verdict. A component terminated by the environment is not a defect in that component, and recording it as one manufactures a false pattern that every recurrence strengthens.

- **OWN-4 (In-process third-party code is flagged, never blamed without evidence implicating it):** third-party code present in the product's own execution context is **noted as present** in the diagnosis — it is a common cause and its presence is material — but the ownership verdict MUST NOT be assigned to it without evidence that it is **actually implicated** in this failure. Presence is context; participation is a claim.

- **OWN-5 (A fault owned elsewhere is routed, never absorbed and never silently worked around):** where the verdict is *a named external component's*, the correct actions are to **say so**, to **name the right destination**, and — where a local mitigation is genuinely needed — to record that mitigation **explicitly as a workaround for a foreign defect**, with the foreign fault it compensates for. Filing outward on the user's behalf is a **separate, consented act** governed by ERR-1, never an automatic consequence of the verdict. A workaround introduced without this record becomes indistinguishable from ordinary code, and it outlives the defect it was compensating for.

- **OWN-6 (A fault's timing is correlated against the environment's own change record before code is blamed):** before attributing a fault to a component's behaviour, the incident's **time is correlated against what changed around it** — component and dependency updates, configuration writes, deployment and build events, the modification times of the artifacts involved. A failure that begins immediately after a change is evidence about the change, and this is the single most under-used input available; skipping it produces verdicts about code that has not moved in months.

- **OWN-7 (What the evidence proves and what the diagnostician inferred are reported as separate things):** a diagnosis states, distinguishably, **what the evidence establishes** and **what is being inferred from it**, and where the cause is genuinely ambiguous it says so rather than assembling confidence out of the gaps. A merged narrative cannot be re-checked when new evidence arrives, and its plausibility is precisely what makes it durable.

- **OWN-8 (Unresolvable evidence degrades the grain of the claim; it is never completed by invention):** where part of the evidence cannot be resolved — unresolvable frames, an unreadable region, a truncated record — the unresolved parts are **reported as unresolved** and **never filled in with plausible names**. Partial evidence remains usable at a **coarser grain** (which component a region belongs to, which layer it came from, whether it sits on a foreground or background path), and reporting the coarser fact is the honest use of it. Fabricating the finer one corrupts a report whose every other part is real.

- **OWN-9 (Diagnosis reads; every mutation it performs is declared in advance and undone):** the diagnostic act **does not fix, tidy, or reconfigure**. It leaves the system as it found it, with exactly two exceptions, both declared before the work begins: artifacts the diagnosis itself created (which it destroys), and a **single explicitly requested** disposition act such as a silence (SIL-6). A diagnosis that repairs as it goes destroys the evidence of what it was diagnosing and makes its own verdict unfalsifiable.

- **OWN-10 (A capture of a live process inherits the confidentiality of everything that process held, and is a transient working artifact — never archive material):** a memory image, full-state snapshot, or equivalent capture taken to diagnose a running component contains **whatever that component held** — credentials, tokens, user documents — and is therefore written to an **unpredictable, private location**, used, and **destroyed at the end of the diagnosis**. It is deliberately **outside** the evidence archive: EA-4's append-only immutability and EA-6's device-local retention are the correct rules for evidence the system means to keep, and they are the wrong rules for an artifact whose safest state is *gone*. What may enter the archive is what was **derived** from the capture — the resolved structure, the verdict, the named facts — never the capture itself.

> An L2 implementation cannot reach RFC until every invariant above is addressed in its Invariant Compliance section.

## 4. Detailed Design

### 4.1 The verdict path

```
fault identified (FL-1)
        │
        ├─ environmental cause present?  ────────► environment            (OWN-3)
        │
        ├─ correlate the incident time against the change record          (OWN-6)
        │
        ├─ does the failing surface fall inside the declared boundary?    (OWN-2)
        │        yes ─────────────────────────────► ours
        │        no, and evidence implicates a named component ─► that component's  (OWN-4)
        │        no, and nothing is implicated ────► undetermined         (OWN-1)
        │
        └─ report: evidence | inference, separated                        (OWN-7)
```

*Undetermined* is not a failure of the procedure. It is what the procedure returns when the evidence stops, and its value is that it stays open instead of resolving to whoever is nearest.

### 4.2 The boundary, and why configuration sits inside it

The enumerated sphere of OWN-2 is not a list of files; it is a list of **surfaces where this product's decisions are expressed**. A component's own defect is theirs. This product's *choice* of that component, its *configuration* of it, its *packaging* of it, and the *contract* it exposes to it are all this product's, and a failure that arises from one of them is ours even though the crash happened in their code. This is the distinction that makes the boundary useful rather than a disclaimer.

### 4.3 Failure modes named

| Mode | Shape | Closed by |
| --- | --- | --- |
| Default absorption | No verdict act; everything is implicitly ours | OWN-1 |
| Boundary drawn at incident time | The line lands wherever the conclusion needed it | OWN-2 |
| Environment blamed on the component | OOM, full disk, expired token filed as a code defect | OWN-3 |
| Guest blamed for the host | In-process third-party code named on presence alone | OWN-4 |
| Silent workaround | A foreign defect compensated for in code nobody can later attribute | OWN-5 |
| Timeless diagnosis | Cause sought in code that has not changed, ignoring what did | OWN-6 |
| Merged narrative | Evidence and inference indistinguishable, unfalsifiable later | OWN-7 |
| Invented completion | Plausible names supplied where evidence ran out | OWN-8 |
| Repairing diagnostician | The evidence destroyed by the act of diagnosing | OWN-9 |
| Archived secrets | A process capture retained under append-only immutability | OWN-10 |

## 5. Implementation Notes

- OWN-2's boundary is a maintained artifact and belongs beside the component inventory rather than in incident tooling. It changes when the product's integration surface changes — a new provider, a new extension class, a newly configured default — and a boundary that is never edited is a boundary nobody is using.
- OWN-6 is cheap to implement and disproportionately effective: the change record usually already exists (update history, deployment records, configuration audit, artifact timestamps) and needs only to be *consulted at the right moment*, which is before the verdict rather than after it is questioned.
- Where the verdict is *a named external component's* and a report will travel outward, the outward artifact discloses **machine authorship** — the model and harness that produced it — and where the producing agent cannot establish those with certainty it says so plainly rather than inventing a version string. An uncertain self-identification is a fact about the report, not a blank to be filled.

## 6. Drawbacks & Alternatives

- **OWN-1's *undetermined* leaves faults unowned.** By design. An unowned fault is visible as unowned; a falsely-owned one is invisible as anything.
- **OWN-2 requires maintaining a boundary nobody is asked for until an incident.** Held: the alternative is deriving it under pressure, which reliably produces the boundary that justifies the conclusion already reached.
- **OWN-5 slows the obvious fix.** Accepted. The obvious fix is usually right; recording it as a foreign-defect workaround costs one line and is the only thing that lets it be removed when the defect is fixed upstream.
- **OWN-9 forbids the diagnostician from repairing what they can plainly see.** Accepted, and it is the invariant most resented in the moment. A diagnosis that repairs is a diagnosis that cannot be re-run, and the second occurrence has no evidence left.
- **Alternative — infer ownership from where the failure surfaced:** rejected. The surface is where the symptom appeared, and in an assembled system it is almost never where the defect lives.
- **Alternative — treat every foreign failure as environmental:** rejected. It is the mirror of default absorption, and it disowns exactly the class OWN-2 places inside the boundary: our configuration of their component.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[FAULT]` | `.design/main/specifications/l1-fault-lifecycle.md` | Fault identity and status; FL-11 is this discipline applied to one component |
| `[REPORT]` | `.design/main/specifications/l1-error-reporting.md` | ERR-1 consent gate governing any outward filing |
| `[VERIFY]` | `.design/main/specifications/l1-completion-verification.md` | CMP-7 — a dismissal is a claim needing the same evidence |
| `[DOCTOR]` | `.design/main/specifications/l1-doctor.md` | HEAL-4 non-destructive diagnosis, the operational half of OWN-9 |
| `[ARCHIVE]` | `.design/main/specifications/l1-evidence-archive.md` | EA-4/EA-6 — the retention rules OWN-10 deliberately routes around |
| `[SILENCE]` | `.design/main/specifications/l1-signal-silencing.md` | SIL-6 — the disposition an ownership verdict legitimizes or forbids |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.0 | 2026-09-04 | Core Team | Initial concept — the ownership verdict as an act distinct from fault identity (FL) and from metric attribution (CHA-5's shortlist), claiming the question an assembled system asks before any repair: *whose defect is this?* Mined from an external desktop-environment distribution whose crash-diagnosis guidance is unusually strict about it — an enumerated sphere of control ("a crash in a program it merely installs is not its bug unless its own packaging or configuration is implicated"), resource exhaustion excluded first ("a process killed by the OOM killer is not a bug in that process"), in-process third-party code "worth flagging — but do not pin blame on it without evidence that it is actually implicated", unsymbolized frames left unnamed ("never invent function names to fill the gap; an unsymbolized stack still has shape"), and "leave the system as you found it". Ten invariants: a closed owner verdict whose *undetermined* value must not collapse into *ours* (OWN-1); the sphere of control enumerated in advance, with **configuration of a third party inside the boundary even though the crash is in their code** (OWN-2); environmental causes excluded before any component is blamed (OWN-3); in-process guests flagged, not blamed on presence (OWN-4); a foreign fault routed and any local mitigation recorded as a foreign-defect workaround, outward filing consent-gated per ERR-1 (OWN-5); incident time correlated against the environment's own change record before code is blamed (OWN-6); evidence and inference reported separately so the verdict stays falsifiable (OWN-7); unresolvable evidence degrading the claim's grain rather than being completed by invention (OWN-8); diagnosis read-only with its two declared exceptions (OWN-9); and a live-process capture treated as a transient, maximally-confidential working artifact deliberately routed **around** the evidence archive, since EA-4 append-only immutability is the wrong rule for an artifact whose safest state is gone (OWN-10). Concept-only; no L2 yet. |
| 1.0.1 | 2026-10-22 | Core Team | Citations updated for the requirement-ID prefix renames that make each prefix name one specification (RULES §1, 1.8.0, the person's decision of 2026-10-22): `CA-n` of `l1-change-attribution` is now `CHA-n`. No requirement was added, removed or reworded. |
