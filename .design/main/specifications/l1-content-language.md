# Content Language

**Version:** 1.0.1
**Status:** Stable
**Layer:** concept

## Overview

Two language questions are routinely conflated. The first — *what language does the
interface speak?* — is solved: user-visible strings live in resource bundles and the
active locale selects among them. The second is unowned: **what language does the model
write in when it generates content?**

That second question decides the quality of everything durable the office produces. A
memory narrative, a session summary, a wiki page, a report, an issue title, a commit
message — each is model-written text that will be *searched, deduplicated, merged, and
read* long after the turn that produced it. When its language is left to emerge from
whatever the prompt happened to look like that session, the corpus silently fragments:
lexical search stops crossing its own records, near-duplicates in two languages fail to
deduplicate, consolidation merges nothing, and the archive reads like two different
projects filed in one drawer.

The failure is invisible at write time and expensive at read time, which is why it needs a
policy rather than a habit. **Content language** is that policy: the language of generated
content is **declared per artifact kind**, a durable corpus holds **one stable language**,
the language *of record* is separate from the language *of display*, quoted material is
never translated, and a query that finds nothing because it was asked in another language
is reported as a language gap rather than as an honest empty result.

The keyed-on-artifact-kind part is the load-bearing one, and it is what no locale setting
can express: the same workspace legitimately wants conversation in one language and
technical artifacts in another. A single global locale forces one of the two to be wrong.

## Related Specifications

- [l1-memory-model.md](l1-memory-model.md) / [l1-memory-intelligence.md](l1-memory-intelligence.md) — the corpus most exposed to language drift: MEM-4's authored-vs-learned source-of-truth split is where CL-3's "one stable language per kind" lands, and MI-4 conflict adjudication cannot recognize a cross-language duplicate at all.
- [l1-memory-consolidation.md](l1-memory-consolidation.md) — MC-6's redundancy→merge action is silently defeated by a bilingual corpus: two records saying the same thing in two languages are not detected as redundant, so the corpus grows a permanent parallel branch.
- [l1-user-model.md](l1-user-model.md) — UM-4 (explicit overrides inferred) is exactly how CL-7 resolves an unstated preference; language is a user attribute like any other and carries the same provenance and confidence discipline.
- [l1-intent-resolution.md](l1-intent-resolution.md) — an inferred language policy is a **recorded assumption** (IR-3), surfaced at a checkpoint (IR-6), never a silent guess.
- [l1-generation-shaping.md](l1-generation-shaping.md) — the sibling output-side lever on a **different axis**: shaping governs verbosity and effort (how much, how hard), this governs the language the output is written in. Both steer the generator; neither may trade correctness (GSH-4).
- [l1-project-wiki.md](l1-project-wiki.md) / [l1-notes.md](l1-notes.md) / [l1-knowledge-base.md](l1-knowledge-base.md) — durable authored corpora subject to CL-3 and to the CL-8 retrieval-gap rule.
- [l1-search.md](l1-search.md) — where the cross-language gap becomes visible: an empty result set caused by a language mismatch is a different answer from "nothing matched".
- [l1-data-lineage.md](l1-data-lineage.md) — a translation is a **derived** artifact with provenance (CL-4/CL-9), not an in-place rewrite of the record it came from.
- [l1-security.md](l1-security.md) — CL-5's never-translate rule covers identifiers, paths, and error strings: translating them destroys both searchability and evidentiary value.
- [l1-workflow-language.md](l1-workflow-language.md) — the nodus projection (§4.6): the declarative configuration surface already carries the policy field, and value provenance already carries the derived-translation relation; no new language primitive is required.

## 1. Motivation

**Language drift is a retrieval defect that looks like a preference.** Lexical matching does
not cross languages, and stemming, tokenization, and script normalization are all
language-specific. A corpus written half in one language and half in another has, for search
purposes, been cut in half — and neither half knows the other exists. Nothing in the system
reports this: every write succeeded, every query returned a well-formed answer, and the
answer was simply missing everything filed in the other language.

**Deduplication and consolidation fail silently in the same way.** Redundancy detection,
supersession, and merge all rest on recognizing that two records say the same thing. Across
languages they cannot, so the corpus accumulates a parallel branch that maintenance passes
will never collapse. The cost compounds: every later summarization, every abstraction hub,
every "have we seen this before" query works on half the evidence.

**One locale cannot express the real policy.** The common, sensible arrangement is *split by
artifact kind*: talk to me in my language, write the code and its commit messages in the
project's technical language. A single locale setting must answer both with one value, so it
is either wrong for the conversation or wrong for the artifacts — and the resolution invented
at implementation time becomes an accidental convention nobody chose.

**Rewriting records to change a display preference destroys history.** Once language is
treated as a display concern, the natural implementation translates in place, and the
original wording — the user's own words, the exact phrasing of a decision — is gone. What
should have been a derived view becomes a lossy mutation of the record.

**Translating what must not be translated is the other half of the failure.** An error
string, an identifier, a file path, a quoted user sentence: each is evidence whose value is
its exactness. A generator told "write in language X" will helpfully translate them unless
told not to, and the result is a record that can no longer be matched against the artifact it
describes.

## 2. Constraints & Assumptions

- This spec governs **model-generated content**. Interface string localization (resource
  bundles, locale selection for user-visible UI text) is a separate, already-solved concern
  and is not re-specified here.
- The unit the policy is keyed on is the **artifact kind** (conversation turn, memory record,
  summary, wiki page, report, code comment, commit message, issue title), not the user and
  not the session.
- The office is **local-first**: language policy is local configuration and user-model data;
  nothing here authorizes egress for translation.
- Translation quality is **out of scope**. This spec governs *which* language content is
  written in, whether a translation is derived or in-place, and how a language gap is
  reported — never how well a translation reads.
- A model may fail to comply with a language instruction. The spec assumes instruction is
  the weakest enforcement and requires the compliance to be checked rather than trusted
  wherever the surface can check it.

## 3. Core Invariants

Rules every Layer 2 implementation MUST NOT violate:

- **CL-1 Generated-content language is declared, never emergent**: every surface that
  produces model-written text resolves its output language from a **declared policy**. An
  undeclared language is not a neutral default — it is whatever the prompt's own language,
  the model's training bias, or the current provider happens to produce, and it changes
  without notice when any of those change. "Whatever it wrote" is not a policy.

- **CL-2 The policy is keyed on artifact kind, not on a single global locale**: different
  artifact kinds in the **same** workspace may declare different languages, because the real
  requirement is usually split — conversational output in the person's language, technical
  artifacts (identifiers, code comments, commit messages, technical documentation) in the
  project's declared technical language. A model that forces one language per user or per
  workspace makes one of those two wrong by construction.

- **CL-3 A durable corpus holds one stable language per kind**: content written into a
  **searchable, mergeable corpus** (memory records, wiki pages, summaries, ledger narratives)
  is written in that corpus's declared language and does **not** vary by session, by model,
  or by the language the triggering request happened to use. This is a retrieval-integrity
  requirement, not an aesthetic one: language drift silently halves lexical recall, defeats
  duplicate detection, and blocks consolidation merges — none of which reports an error.

- **CL-4 Language of record and language of display are separate**: changing a display
  preference MUST NOT rewrite stored content. A translation is a **derived projection**
  carrying its provenance (source record, direction, producer) and living beside the record;
  the record itself is never mutated in place. Supersede-don't-mutate applies to language
  exactly as it applies to any other correction.

- **CL-5 Quoted and technical material is reproduced, never translated**: verbatim excerpts,
  the user's own words, tool output, error strings, log lines, identifiers, symbol names,
  file paths, URLs, and command text are carried through **as-is**, whatever the surrounding
  policy language. Their value is exactness — a translated error message no longer matches
  the error, and a translated identifier no longer matches the code.

- **CL-6 Enforced by construction where possible, checked where not**: where a surface can
  make language structural — a fixed template, an enumerated field, a typed extraction
  schema, a field the generator does not author — it does. Where compliance depends on the
  model following an instruction, the produced content is **checked** against the declared
  language and a violation is recorded and surfaced, never assumed away. An instruction the
  system never verifies is a hope, not a policy.

- **CL-7 Unstated preference is grounded, then assumed on the record**: the policy resolves
  in order — an **explicit declaration**, then the **user model's** stated preference, then
  the declared default. Where resolution is inferential it is a **recorded, correctable
  assumption**, surfaced at a natural checkpoint rather than silently applied forever. A
  language inferred once from a single message and then treated as settled is the
  archetypal silent guess.

- **CL-8 A cross-language retrieval gap is reported, never rendered as an empty result**:
  when a query's language differs from the corpus's, the system either **bridges** the gap
  (semantic matching, script/transliteration normalization, query translation as a derived
  query) or **states** that the gap exists. Returning "no results" for a query that would
  have matched in the corpus's language is a false negative presented as a fact — the
  absence-is-not-good-news discipline applied to language.

- **CL-9 A language change is forward-only and its migration is explicit**: changing a
  corpus's declared language applies to content written **after** the change; existing
  records are not silently rewritten. Converting an existing corpus is an **explicit,
  audited migration** that produces derived records with provenance (CL-4) and is reversible
  by discarding the derivations — never an implicit consequence of editing a setting.

- **CL-10 Measured by consistency, not asserted**: language consistency is computed from the
  corpus — share of records matching the declared language per kind, cross-language duplicate
  rate, and the rate of empty results whose query language differed from the corpus. A claim
  that the corpus is language-consistent is made only with those numbers; unmeasured, the
  drift is exactly the kind that stays invisible until retrieval quality has already decayed.

> L2 specs cannot reach RFC status until all invariants here are addressed in their "Invariant Compliance" section.

## 4. Detailed Design

### 4.1 Resolution (CL-1 / CL-2 / CL-7)

```text
[REFERENCE]
language_for(artifact_kind, principal, workspace):
    if declared(artifact_kind, workspace):    return declared_value        // explicit wins
    if user_model.states_language(principal): return stated               // UM-4
    if workspace.default_for(kind_class):     return workspace_default    // e.g. technical vs conversational
    a := assume(kind, evidence)                                            // IR-3
    record(a); surface_at_checkpoint(a)                                    // IR-6, correctable
    return a.value
```

The resolution is per **(artifact kind, workspace)** — never per session and never per
request — which is what makes CL-3's stability achievable at all. A per-request resolution
would reintroduce drift by design: the same corpus would take the language of whichever
request happened to write into it.

### 4.2 The two-axis policy (CL-2)

| Kind class | Typical policy | Why |
| --- | --- | --- |
| Conversational output | the person's language | it is addressed to them, read once, not indexed as a corpus |
| Durable corpus records | one declared corpus language | retrieval, dedup, and merge all depend on stability (CL-3) |
| Technical artifacts | the project's technical language | they live beside code and are read by contributors and tools |
| Quoted / evidentiary material | **no** policy — reproduced as-is | exactness is the value (CL-5) |

The fourth row is not a language choice; it is the absence of one, and it must be stated
explicitly because a generator instructed to write in a language will otherwise apply it
uniformly — including to the material whose whole purpose is to match something else
byte-for-byte.

### 4.3 Record versus display (CL-4 / CL-9)

```text
[REFERENCE]
render(record, display_language):
    if record.language == display_language:   return record.text
    t := derived_translation(record, display_language)   // provenance: source, direction, producer
    return t.text                                        // record.text is never mutated
```

The record keeps its own language forever; a display in another language is a *view*. This
is what makes a language-preference change cheap and reversible: discarding derivations
restores the original state exactly, because the original was never touched.

A corpus-wide migration is the same operation applied in bulk, plus an audit entry — and it
is still additive, so a migration that turns out badly is undone by dropping the derived set
rather than by restoring a backup.

### 4.4 The retrieval gap (CL-8)

Three honest responses to a query whose language does not match the corpus, in order of
preference:

1. **Bridge it** — semantic matching, script/transliteration normalization, or a derived
   translated query, with the bridging disclosed in the result.
2. **State it** — "no lexical match; the corpus is in {language} and this query is in
   {language}", with the option to bridge.
3. **Never** — return a bare empty result set, which asserts *nothing like this exists* when
   the truth is *nothing like this was found the way you asked*.

This composes the memory store's multi-script search fallback: where the primary matcher is
language- or script-sensitive and yields nothing, the fallback path exists precisely so that
a script mismatch does not masquerade as absence.

### 4.5 Why instruction alone is insufficient (CL-6)

A generator's language compliance is a soft property of a prompt, and soft properties fail
quietly under distribution shift: a new model, a longer context, a prompt that quotes a lot
of another language. The three enforcement tiers, strongest first:

```text
[REFERENCE]
tier 1  structural — the field is not model-authored (enumerated value, template, schema)
tier 2  checked    — model-authored, then verified against the declared language; violation recorded
tier 3  instructed — model-authored, unverified: acceptable only where nothing durable depends on it
```

Durable corpus writes (CL-3) belong at tier 1 or 2. Tier 3 is legitimate for conversational
output, where a wrong-language turn is immediately visible to the person reading it and costs
one correction — not a permanent, unsearchable record.

### 4.6 nodus projection

The workflow layer needs **no new primitive** for this:

1. **The policy is a configuration field.** The validated declarative configuration surface
   already carries typed, defaulted, range-checked fields validated before any value becomes
   visible to a workflow — a content-language field is an ordinary member of it, and the
   host renders and validates it exactly like the rest.
2. **A translation is a derived value, and provenance already carries derivation.** Value
   provenance travels with values through the runtime; a translated value is derived from
   its source and inherits that relation, which is precisely CL-4's record-versus-projection
   distinction expressed in machinery the language already has.
3. **Schema and vocabulary prose are technical artifacts.** A host schema's command
   descriptions and a macro's documentation are model-facing *and* contributor-facing text,
   and CL-2 classifies them with the technical artifacts — one declared language, stable
   across the vocabulary, so a workflow authored against a schema reads consistently
   whatever the author's own language.

The judgement half stays host-side: which kinds exist, what each declares, and whether to
bridge or state a retrieval gap are host policy, consistent with how every other policy
concern maps onto the provider surface.

## 5. Implementation Notes

1. Store the record's language **on the record**, not derived from its content at read time:
   detection is probabilistic and re-detecting on every read makes CL-10's consistency
   metric unstable.
2. Make the never-translate set (CL-5) a property of the **field**, not a heuristic over the
   text — an identifier field is never translated because of what it is, not because a
   classifier guessed it looked like code.
3. Wire the CL-8 gap detection into the same path as the memory store's script fallback
   rather than adding a parallel one; one place that knows "this looked like a language
   mismatch" is enough.
4. CL-10's metrics belong with the existing practice-analytics detectors — corpus language
   consistency is a health signal, computed from the record, not a new store.

## 6. Drawbacks & Alternatives

- **Per-kind policy is more configuration than one locale.** True, and it is the minimum that
  expresses the actual requirement (§1). The default may still be a single value; the model
  merely permits the split rather than requiring it.
- **Checking model compliance costs something on every durable write.** Bounded by CL-6's
  tiering: the check applies where content is durable, which is exactly where a wrong-language
  record is expensive and permanent.
- **Forward-only language change leaves a mixed corpus behind.** Accepted and preferred to the
  alternative: an implicit bulk rewrite that destroys the originals. CL-9 makes the migration
  available, explicit, audited, and reversible.
- **Alternative — one global locale for everything.** Rejected by CL-2: it cannot express the
  common conversation-versus-technical split, so the split gets invented ad hoc at each
  surface.
- **Alternative — always write the corpus in one fixed language and translate on display.**
  Rejected as a *requirement*: it is a legitimate configuration under CL-3/CL-4, but as a rule
  it forces every user's own words through a lossy transform before they are stored, which
  CL-5 exists to prevent.
- **Alternative — detect language per record and let the corpus be mixed.** Rejected by CL-3:
  detection makes the mixture *legible*, not *searchable*; dedup and merge still fail, and the
  parallel branch still grows.
- **Alternative — treat it as part of interface localization.** Rejected: resource bundles
  select among strings someone already wrote; nothing in that mechanism decides what language
  a model writes a new memory record in, nor what happens to it at retrieval.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[MEMORY]` | `.design/main/specifications/l1-memory-model.md` | The corpus CL-3 protects; MEM-4 source-of-truth split. |
| `[MEM-INTEL]` | `.design/main/specifications/l1-memory-intelligence.md` | Conflict adjudication and recall that cross-language drift defeats. |
| `[CONSOLIDATION]` | `.design/main/specifications/l1-memory-consolidation.md` | MC-6 redundancy→merge, silently defeated by a bilingual corpus. |
| `[USER-MODEL]` | `.design/main/specifications/l1-user-model.md` | Where a stated language preference lives (UM-4 precedence). |
| `[INTENT]` | `.design/main/specifications/l1-intent-resolution.md` | IR-3/IR-6 recorded-assumption path for an inferred policy. |
| `[SEARCH]` | `.design/main/specifications/l1-search.md` | Where the CL-8 language gap must be visible. |
| `[LINEAGE]` | `.design/main/specifications/l1-data-lineage.md` | Provenance for derived translations (CL-4/CL-9). |
| `[WORKFLOW-LANG]` | `.design/main/specifications/l1-workflow-language.md` | The nodus surface the policy projects onto (§4.6). |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.0 | 2026-08-05 | Core Team | Initial spec — the language of **model-generated content**, the question interface localization does not answer and nothing else owned: declared per surface rather than emerging from the prompt, the model's bias, or the provider (CL-1); keyed on **artifact kind** rather than one global locale, because the real requirement is usually split — conversation in the person's language, technical artifacts in the project's — and one locale makes one of them wrong by construction (CL-2); one stable language per durable corpus kind, a **retrieval-integrity** requirement since drift silently halves lexical recall, defeats duplicate detection, and blocks consolidation merges without reporting anything (CL-3); language of record separate from language of display, a translation being a derived projection with provenance and never an in-place rewrite (CL-4); quoted and technical material — user words, tool output, error strings, identifiers, paths — reproduced never translated, since exactness is their value (CL-5); enforced structurally where the field is not model-authored, checked where it is, with instruction-only acceptable solely where nothing durable depends on it (CL-6); unstated preference grounded through declaration → user model → default and otherwise a recorded correctable assumption (CL-7); a cross-language retrieval gap bridged or stated but never rendered as a bare empty result, which asserts absence where the truth is *not found the way you asked* (CL-8); language changes forward-only with an explicit, audited, reversible migration (CL-9); and consistency measured from the corpus rather than asserted (CL-10). Nodus projection needs no new primitive — the validated configuration surface carries the policy field, value provenance already expresses the derived-translation relation, and schema/vocabulary prose classifies with the technical artifacts. Concept-only. |
| 1.0.1 | 2026-10-22 | Core Team | Citations updated for the requirement-ID prefix renames that make each prefix name one specification (RULES §1, 1.8.0, the person's decision of 2026-10-22): `GS-n` of `l1-generation-shaping` is now `GSH-n`. No requirement was added, removed or reworded. |
