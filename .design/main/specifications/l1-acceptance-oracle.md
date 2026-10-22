# Acceptance Oracle Integrity

**Version:** 1.1.1
**Status:** Stable
**Layer:** concept

## Overview

An acceptance oracle is the artifact that decides whether a unit of work is done. It is also **the only artifact in the verification stack that nothing checks**. Everything downstream is faithful: the check runs, the evidence is recorded, the roll-up aggregates, the gate holds — and all of it certifies, with perfect mechanical fidelity, whatever the oracle happened to say. A criterion that cannot fail therefore converts the entire apparatus into a machine for manufacturing confidence, and it does so *silently*, because every layer beneath it is working exactly as designed.

The canonical shape of the failure is a criterion reading *"the payment flow works end to end"* bound to a check that prints a success word and exits successfully. It is syntactically valid. It runs. It returns what it promised. It passes the gate, passes re-verification, passes the roll-up, and appears in the report as a met requirement. Nothing in the system is broken; the oracle is simply empty.

This concept names the discipline that closes it: **an acceptance criterion must be capable of failing, and that capability is reviewed before the work starts rather than discovered after the report.** It is the authoring-time sibling of completion verification — that layer demands fresh evidence from the authoritative check, this one asks whether the check is authoritative at all.

## Related Specifications

- [l1-completion-verification.md](l1-completion-verification.md) — the run-time sibling. CMP-4 forbids substituting a *weaker but real* check for the right one (a linter for a build); this spec forbids a check that is **structurally incapable of failing**, and moves the enforcement moment to authoring time.
- [l1-requirement-checklists.md](l1-requirement-checklists.md) — tests whether the *requirements* are well written; this spec tests whether the *criteria that judge the work* can fail. The two are adjacent and non-overlapping: RQ audits the statement of need, AO audits the instrument that measures it.
- [l1-loop-governance.md](l1-loop-governance.md) — LG owns *who may change* the oracle (never the actor being judged, no criteria drift); this spec owns *whether the oracle is worth obeying* in the first place. Ownership and integrity are independent failures: a properly-owned empty oracle is still empty.
- [l1-claim-verification.md](l1-claim-verification.md) — grounds claims about *facts* against sources; a criterion is not a claim about the world but an instrument for deciding one.
- [l1-invariant-tripwires.md](l1-invariant-tripwires.md) — the standing mechanical enforcement a rule gets after it is stated; the falsifiability review (AO-7) is a tripwire applied to criteria themselves.
- [l1-evaluation-suites.md](l1-evaluation-suites.md) — behavioural evaluation over graders; an evaluation's graders are acceptance oracles and are subject to this contract.
- [l1-iterative-refinement.md](l1-iterative-refinement.md) — ITR-6 frozen criteria; freezing a criterion that cannot fail freezes the wrong thing, so AO-1 is the precondition for ITR-6 being worth anything.
- [l1-outcome-confidence.md](l1-outcome-confidence.md) — estimates what was never checked; a criteria set that cannot fail is one of the strongest possible signals feeding that estimate.
- [l1-quality-standards.md](l1-quality-standards.md) — the definition-of-done gates whose content this contract governs.
- [l1-negative-specification.md](l1-negative-specification.md) — stating what must *not* happen; AO-5 is the measurement discipline such statements require before they may be trusted.
- [../../nodus/specifications/l1-nodus-language.md](../../nodus/specifications/l1-nodus-language.md) — NL-14 grader-gated refinement: the host-supplied grader and its fixed rubric are acceptance oracles, and a rubric that admits no failing verdict makes `~UNTIL +grade` terminate on its first iteration forever.

## 1. Motivation

The project already enforces a strong evidential chain: a completion claim names its check, the check is run fresh, delegated results are verified independently, dismissals need baselines. That chain has one unexamined link at its head. Every guarantee it offers is *conditional on the criterion being meaningful*, and the criterion is prose — the layer this project treats as its weakest everywhere else.

Left unnamed, the failure arrives by ordinary, well-intentioned routes:

- **The tautology.** A criterion is hard to check, and the check that gets written observes something adjacent and cheap rather than the outcome named.
- **The indistinct signal.** A check greps its own output for `passed`, and the failure path prints `2 tests passed, 3 failed`. The gate goes green on a failing run.
- **The unverified absence.** A criterion asserts an error no longer occurs. The check looks in the wrong file, or matches a pattern that matches nothing anywhere, and reports the same clean result it would report if the fix were real.
- **The number that proves itself.** The brief says the report must list 47 accounts; the check looks for the string `47` in output the brief's own number was interpolated into.
- **The activity title.** *"Improve error handling"* cannot be failed by anyone, because some improvement always occurred.

None of these is caught by running the check, re-running it, running it in a fresh environment, or having an independent party run it. They are all invisible to execution, because execution is not where they live. They are visible to **reading**, cheaply, at the moment the criterion is written — and mechanically detectable in their most common forms.

## 2. Constraints & Assumptions

- **A criterion is prose plus an instrument, and the two can disagree.** No system can infer that unrestricted natural language and an arbitrary executable mean the same thing. This contract narrows the gap; it does not close it.
- **Some outcomes have no mechanical oracle.** Judged criteria are legitimate and permanent. The discipline is that they are *declared* as judged, not that they are eliminated.
- **The review is advisory in the specific and mandatory in the aggregate.** A finding is a prompt to sharpen a criterion, never proof that an outcome is wrong; but skipping the review altogether is not permitted where the criteria gate real completion.
- **Review must not execute.** An audit of an oracle that runs the oracle has crossed into a different trust boundary and has different consequences; the two acts stay separate.
- **This is for work whose quiet incompleteness is expensive.** A trivial edit or a factual answer does not acquire criteria; the ceremony is justified by consequence, not applied by default.

## 3. Core Invariants

Rules every Layer 2 implementation MUST NOT violate. They are technology-neutral.

- **AO-1 (Criteria precede the work and are what completion is judged against):** acceptance criteria for a unit are authored **before** implementation begins, and completion is judged against **that** set. Criteria written after the artifact exists, or revised to match what was built, describe the outcome rather than requiring it — the same goalpost move LG forbids, arriving through the front door instead.

- **AO-2 (One observable outcome per criterion — never an activity):** a criterion names a **state of the world a stranger could check** ("a malformed record is rejected with the documented error"), never an activity ("improve the importer", "handle edge cases", "review the schema", "address the feedback"). An activity is satisfied the moment some of it has occurred, so it cannot be failed, and a criterion that cannot be failed is not a criterion.

- **AO-3 (Runnable or judged — declared, never silently either):** each criterion declares itself **runnable** (bound to a check that decides it, together with what that check must show) or **judged** (decided by a named reviewer against stated evidence). A **half-bound** criterion — an instrument with no stated expectation, or an expectation with no instrument — is **malformed and MUST be refused**, never quietly downgraded into a judged one. Silent downgrade is how a set loses its mechanical guarantees without anybody deciding to give them up.

- **AO-4 (Success is signalled only by what success can produce):** a runnable criterion's pass signal MUST be **distinctive of success** — emitted only after every assertion has passed. Vocabulary that failure output uses as readily as success output ("ok", "done", "passed", "complete", "0") is not a pass signal. Passing requires **both** a successful termination **and** the signal: a failed instrument whose error text happens to contain the expected token has not passed, and a successful termination with no signal has not passed either.

- **AO-5 (An absence claim requires a positive control):** a criterion asserting that something does **not** occur is trusted only after the same check has been demonstrated to **fail against a case where it does occur**. Untested, an absence check is indistinguishable from one that is looking in the wrong place, matching a pattern that matches nothing, or silently misconfigured — and it reports the same clean result in every one of those cases. This is the single most common way a green result carries no information.

- **AO-6 (A supplied figure is not its own proof):** where a criterion states a quantity, the instrument MUST **derive that quantity from the source of truth** and apply the acceptance rule to what it derived. Looking for the number that was supplied in the brief proves that the number was copied correctly and nothing else.

- **AO-7 (Falsifiability review before the criteria are worked to):** before a criteria set governs real work, it passes a **falsifiability review** — a distinct act that asks, of each criterion, whether it could fail. The review **MUST NOT execute** the criteria's instruments. It is **advisory per finding** (a finding sharpens a criterion; it does not condemn an outcome) and **mandatory as a step**. The mechanically detectable classes MUST be caught here rather than certified at report time: fixed-output instruments, indistinct pass signals, unmeasured figures, activity-shaped titles, path-shaped expressions read as patterns, and a set that is mostly judged.

- **AO-8 (A mostly-judged set is a disclosed condition, not a default):** where most criteria in a set are decided by human judgement, **that ratio is itself surfaced** with the result. Judgement is legitimate and sometimes unavoidable; what is not legitimate is a checklist of unrunnable statements carrying the *appearance* of mechanical verification while providing none of its properties. Whoever reads the outcome must be able to see which it was.

- **AO-9 (Judged criteria carry evidence proportional to consequence — and are never assumed low-risk):** a judged criterion records the **exact artifact, location, measurement, or decision** that settles it, and the highest-consequence ones take independent review. Ambiguous evidence leaves the criterion **unmet**. Crucially, a criterion MUST NOT be treated as low-stakes merely because no command decides it: checkability and consequence are **independent axes**, and the outcomes hardest to check mechanically are frequently the ones that matter most.

- **AO-10 (An empty or malformed set is refused, never satisfied):** a criteria set that is empty, structurally invalid, or carries duplicate identities is an **error**, not a pass. *Nothing to check* MUST NEVER reduce to *everything checks out*. Reading an absent or broken contract as a satisfied one is the highest-leverage way a verification stack lies, because it converts the failure of the mechanism into a certificate of success.

- **AO-11 (Criteria carry stable explicit identities):** each criterion has an **explicit, unique, stable identifier**, qualified by its owning set when cited across a hierarchy. Identifiers derived from position — line numbers, ordinals, order of appearance — change meaning the moment an unrelated criterion is inserted above them, which silently re-points every report, handoff, and cross-reference that named one.

- **AO-12 (A measurable criterion names its counterfeits and its repair precedence):** [ADDED v1.1.0] a criterion reduced to a **cheap mechanical measurement** — a size that must not exceed a bound, a count that must reach a floor, a string that must be present — is satisfiable by acts that defeat the property it stands for. The criterion therefore carries, alongside its measurement, **an enumerated list of the ways of passing it that do not count** (clipping or hiding the content instead of fitting it, moving the excess out of the measured region, degrading the thing being measured until it complies) and a **declared repair precedence** naming which changes are tried first and which are last. Both halves are needed: the counterfeit list makes an illegitimate pass reviewable rather than arguable, and the precedence keeps the cheapest repair — which is nearly always the one that damages the underlying property — from being the first one reached for. This is not the same failure AO-1…AO-11 close: those govern a criterion that **cannot fail**, while this governs one that can fail, does fail, and is then satisfied by attacking the measurement instead of the work. A criterion whose counterfeits nobody can name is a criterion nobody has examined.

- **AO-13 (A threshold belongs to a class; an item whose measure is undefined for its class is excluded, not scored):** [ADDED v1.1.0] where a criterion applies one bound to a **heterogeneous population**, the bound is wrong for most of it: the legitimate outliers fail loudly and the real defect sits inside the tolerance the outliers forced. A measurable criterion over such a population therefore **classifies each item first** — by its declared role, its computed kind, and the provenance of how it came to be that way — and applies **the bound belonging to that class**. Two consequences bind. An item for which the measure is **not defined** (no meaningful denominator, no comparable baseline, a deliberately different shape) is **excluded from scoring and recorded as excluded**, never scored badly by a formula that does not apply to it — a check that fires on the intentional cases teaches its own dismissal exactly as ATE-2 describes. And an **explicit authored override is a repair control, never an exemption**: recording *why* an item takes an unusual shape explains its provenance and does not raise, waive, or satisfy the bound it must still meet (composing ORI-8 — legitimate overlap is resolved on the content, never on the threshold).

> L2 specs cannot reach RFC status until all invariants here are addressed in their "Invariant Compliance" section.

## 4. Detailed Design

### 4.1 The two ways an oracle lies

| Failure | Shape | Caught by |
| --- | --- | --- |
| **Wrong instrument** | The check is real and can fail, but it measures something weaker than the claim | l1-completion-verification CMP-4 |
| **Empty instrument** | The check runs and passes, but nothing it does could ever produce a failure | **this spec (AO-4/AO-5/AO-6/AO-7)** |

They are distinct and need distinct remedies. A weak proxy is caught by asking *"does this check prove that claim?"* — a question about semantics, answerable at report time by comparing claim to check. An empty instrument is caught by asking *"could this check ever fail?"* — a question about the instrument alone, answerable at authoring time without knowing the claim at all. The second question is cheaper, earlier, and partly mechanical, which is why it gets its own layer.

### 4.2 The falsifiability review (AO-7)

```text
[REFERENCE]
review(criteria_set):
    if criteria_set is empty or malformed or has duplicate ids:
        return REFUSE                                    // AO-10 / AO-11 — never "all met"
    findings := []
    for c in criteria_set.live:                          // handed-off criteria are excluded from quality review
        if c.runnable and instrument_is_fixed_output(c):  findings += TAUTOLOGICAL
        if c.runnable and signal_is_indistinct(c):        findings += WEAK_SIGNAL
        if c.runnable and expectation_shaped_ambiguously(c): findings += AMBIGUOUS_EXPRESSION
        if c.judged and title_states_a_quantity(c):       findings += UNMEASURED_FIGURE
        if title_names_an_activity(c):                    findings += ACTIVITY_NOT_OUTCOME
        if c.half_bound:                                  return REFUSE            // AO-3 — malformed, not judged
    if judged_share(criteria_set) > declared_threshold:   findings += MOSTLY_JUDGED  // AO-8
    return findings                                       // advisory per finding; the step itself is mandatory
    // NEVER executes an instrument — AO-7
```

Two properties make this useful rather than decorative. It is **lexical and honest about being lexical**: it reads the criteria, so it produces prompts rather than verdicts, and it says so. And it is **usable as a criterion of its own** — a criteria set can require its own falsifiability review to pass, which is the cheapest possible way to make the discipline self-enforcing rather than remembered.

### 4.3 The classes, and why each one passes today

| Class | What it looks like | Why nothing downstream catches it |
| --- | --- | --- |
| Tautological instrument | Emits a fixed success token; asserts nothing | It runs, terminates successfully, and prints what was expected |
| Indistinct signal | Expects a word that failure output also prints | The signal genuinely appeared in the output |
| Unverified absence | Asserts something is gone; never shown able to detect it present | A wrong path and a real fix produce identical output |
| Self-proving figure | Expects the number the brief supplied | The number is genuinely there — it was interpolated in |
| Activity title | Names work rather than a resulting state | No reading of the title admits a failing state |
| Ambiguous expression | A path-shaped expectation read as a pattern, matching more than intended | It matches, which is all the mechanism asks |
| Mostly-judged set | Human decisions wearing a checklist's clothes | Every criterion is legitimately met by someone's word |

Every row here is green at report time. That is the point: the classes are defined precisely by their invisibility to execution.

### 4.4 Where the review sits in the quality family

| Layer | Asks | Subject | Moment |
| --- | --- | --- | --- |
| l1-requirement-checklists | Are the requirements complete, clear, measurable? | the **requirements** | before planning |
| **this spec** | Can the criteria that judge the work **fail**? | the **instrument** | before implementation |
| l1-completion-verification | Was the authoritative check run, fresh, and does it prove this claim? | the **evidence** | at the completion claim |
| l1-outcome-confidence | What did nobody think to check? | the **trajectory** | at the completion claim |
| l1-evaluation-suites | Does behaviour hold across a graded corpus? | the **behaviour** | continuously |

The four verification-side layers form one chain, and this one is its head. A weakness here propagates through all of them with no further symptom, which is why it is worth a layer of its own rather than a bullet inside one of the others.

### 4.5 Judged criteria, honestly

Judged criteria are not a degraded form of runnable ones. Three rules keep them from becoming a loophole:

1. **Declared, never defaulted** (AO-3) — a criterion becomes judged by decision, never by an instrument going missing.
2. **Evidence is specific** (AO-9) — the artifact, the location, the measurement, the decision. Not "reviewed" and not a pasted log.
3. **Unrunnable is not low-risk** (AO-9) — a set's most consequential outcome is often exactly the one no command decides, and the temptation to spend the least review on the criterion that received the least automation runs precisely backwards.

## 5. Implementation Notes

1. **Criteria representation** — explicit stable identifiers, a declared runnable/judged kind, the instrument and its expected signal for runnable ones, the evidence slot for both; a validating reader that refuses empty, duplicate-identified, and half-bound sets (AO-3/AO-10/AO-11).
2. **The review as a first-class step** — a distinct, non-executing pass emitting findings by class with the criterion each attaches to, and an explicit strict mode for contexts where advisories should block.
3. **Self-application** — ship the review usable as a criterion, so a criteria set can require its own falsifiability as its first entry.
4. **Signal guidance at authoring time** — surface the distinctive-token rule (AO-4) where criteria are written, since the indistinct-signal class is the one authors reproduce most reliably.
5. **Positive-control record** (AO-5) — an absence criterion carries, as part of its evidence, the demonstration that its check failed against a known-present case; without that record the criterion stays unmet.

## 6. Drawbacks & Alternatives

- **A lexical review produces false alarms.** A fixed-output-looking command may chain a real verifier; an expectation word may genuinely be distinctive in one project's vocabulary. Accepted and designed for: findings are advisory per item (AO-7), and their job is to make an author look again, not to overrule them.
- **It cannot detect a semantically empty check that looks busy.** An instrument that performs elaborate work and asserts nothing passes the review. Accepted: this layer raises the floor mechanically and leaves the ceiling to reading. The alternative — attempting to prove instrument-to-prose equivalence — is not achievable and would license overconfidence if claimed.
- **Alternative — audit the oracles by running them.** Rejected (AO-7): executing an inherited instrument to learn what it does is precisely the act the consent layer exists to gate, and it moves an authoring-time reading task into a run-time execution risk.
- **Alternative — require every criterion to be runnable.** Rejected (AO-3/AO-9): it would push genuinely judged outcomes into fake instruments, which is the tautology class arriving by policy. Declaring and disclosing the judged share (AO-8) preserves the information instead of destroying it.
- **Alternative — fold this into completion verification.** Rejected: that layer's power comes from being evidential and run-time. This one is textual and authoring-time, its findings are advisory rather than binary, and merging them would blunt both — a set of prompts inside a gate either hardens into false failures or softens the gate.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[VERIFY]` | `.design/main/specifications/l1-completion-verification.md` | CMP-4 proxy-check rule; the run-time sibling of this contract |
| `[CHECKLIST]` | `.design/main/specifications/l1-requirement-checklists.md` | Requirement-quality audit; the adjacent, non-overlapping layer |
| `[LOOP]` | `.design/main/specifications/l1-loop-governance.md` | Oracle ownership and criteria immutability |
| `[TRIPWIRE]` | `.design/main/specifications/l1-invariant-tripwires.md` | Mechanical standing enforcement, applied here to criteria |
| `[QUALITY]` | `.design/main/specifications/l1-quality-standards.md` | Definition-of-done gates whose content this governs |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.0 | 2026-08-25 | Core Team | Initial concept — the acceptance criterion is the one artifact in the verification stack that nothing checks, and every layer beneath it faithfully certifies whatever it says. Distinguished sharply from CMP-4 (which forbids a *weaker but real* proxy check): this contract forbids an instrument **structurally incapable of failing**, and moves enforcement to authoring time. Criteria precede the work and are what completion is judged against (AO-1); one observable outcome per criterion, never an activity that cannot be failed (AO-2); runnable-or-judged declared, with a half-bound criterion refused rather than silently downgraded (AO-3); success signalled only by what success can produce, requiring both successful termination and a distinctive signal (AO-4); an absence claim trusted only after its check is demonstrated to fail against a known-present case — the most common way a green result carries no information (AO-5); a supplied figure derived from the source of truth, never restated as its own proof (AO-6); a mandatory-as-a-step, advisory-per-finding, **non-executing** falsifiability review catching the mechanically detectable classes before work starts, usable as a criterion of its own (AO-7); a mostly-judged set disclosed as a condition rather than passing as mechanical verification (AO-8); judged criteria carrying consequence-proportional evidence and never assumed low-risk, since checkability and consequence are independent axes (AO-9); an empty or malformed set refused — *nothing to check* never reducing to *everything checks out* (AO-10); stable explicit criterion identities, never position-derived (AO-11). Nodus relevance: NL-14 graders and their fixed rubrics are acceptance oracles. Concept-only. |
| 1.1.0 | 2026-09-04 | Core Team | Amended — AO-12 and AO-13, from an external artifact-generation tool whose measurable criteria are published together with the exact ways of cheating them. **AO-12**: a criterion reduced to a cheap mechanical measurement is satisfiable by acts that defeat the property it stands for, so it carries an enumerated **counterfeit list** (clip, hide, move the excess outside the measured region, shrink the thing being measured) and a **declared repair precedence** naming which changes are tried first and which last — the counterfeit list makes an illegitimate pass reviewable rather than arguable, and the precedence keeps the cheapest repair, which is nearly always the one that damages the property, from being reached for first. Distinct from AO-1…AO-11, which close a criterion that *cannot fail*; this closes one that can fail, does, and is then satisfied by attacking the measurement. **AO-13**: one bound over a heterogeneous population is wrong for most of it — the legitimate outliers fail loudly while the real defect sits inside the tolerance they forced — so items are classified first (declared role, computed kind, provenance) and scored against their class's bound; an item whose measure is **undefined for its class** is excluded and recorded as excluded rather than scored by a formula that does not apply, since a check firing on intentional cases teaches its own dismissal (ATE-2); and an explicit authored override is a **repair control, never an exemption** — it explains provenance and does not waive the bound (ORI-8). |
| 1.1.1 | 2026-10-22 | Core Team | Citations updated for the requirement-ID prefix renames that make each prefix name one specification (RULES §1, 1.8.0, the person's decision of 2026-10-22): `IR-n` of `l1-iterative-refinement` is now `ITR-n`. No requirement was added, removed or reworded. |
