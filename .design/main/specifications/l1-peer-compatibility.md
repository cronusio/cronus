# Peer Compatibility

**Version:** 1.2.0
**Status:** Stable
**Layer:** concept

## Overview

The model of **how two independently released halves of one product keep working against each other when they are not the same age**. A *peer* is any component that speaks a contract it does not solely own: the core and a frontend, an engine and a mobile client, an office and a remote office, a host and an extension, a runtime and a workflow bundle. Peers ship on different cadences, are updated by different people, and — for anything the user installed rather than the product upgraded — may never be the same version at the same time.

The claim this spec makes is that **version skew is the normal operating state, not a fault**, and that compatibility across it must be *implemented and machine-verified* rather than asserted in a changelog. A single protocol version number cannot express this: it makes one capability's breaking change into every capability's breaking change, and it turns "is this release compatible?" into a human recollection. The alternative developed here is a versioned surface of independently evolving capability lines, executable transforms between adjacent versions, a structural fingerprint that decides whether a claimed compatible change actually is one, and a negotiation that separates the small set of capabilities a connection cannot live without from the large set that may simply be absent.

One invariant carries most of the weight: **the newer peer does all the adapting; the older peer never learns anything new.** Everything else follows from taking that seriously.

## Related Specifications

- [l1-surface-parity.md](l1-surface-parity.md) — The sibling failure. Parity keeps two *simultaneous* surfaces from disagreeing; this keeps two *differently aged* peers from disagreeing. SP's conformance corpus and PCO-13's pinned oracle are the same discipline on different axes.
- [l1-extension-points.md](l1-extension-points.md) — EP-5 already versions a seam and refuses a mismatched contribution at activation. This spec supplies what EP-5 leaves open: per-point version *lines*, the transforms that bridge them, and the machine check that a claimed compatible bump is one.
- [l1-acp.md](l1-acp.md) — ACP-2 capability declaration is the negotiation surface this spec structures: PCO-2 replaces the single `version` field with per-method lines, PCO-8 splits the declaration into floor and optional.
- [l1-record-evolution.md](l1-record-evolution.md) — The persistence twin. Same problem, different medium: a record outlives its writer the way a peer outlives its release. Read together; the two version systems are independent (PCO-1).
- [l1-doctor.md](l1-doctor.md) — HEAL-8 names build-parity skew as a constitutive check; this spec is the contract that check evaluates, and PCO-14 is what it reports.
- [l1-deployment-neutrality.md](l1-deployment-neutrality.md) — Every topology this spec must survive: local-only, desktop hub with thin clients, remote office. Each adds a peer pair that upgrades separately.
- [l1-multi-device-sync.md](l1-multi-device-sync.md) — Devices are peers of each other; SY-3's reconnect is where skew is discovered.
- [l1-invariant-tripwires.md](l1-invariant-tripwires.md) — PCO-5/PCO-9/PCO-13 are tripwires; TW-10's delta-gating and TW-7's reasoned exemptions govern them.
- [l1-agent-tool-ergonomics.md](l1-agent-tool-ergonomics.md) — ATE-2/ATE-3: the shape of the refusal PCO-8's `unsupported` degrade produces at a surface.
- [l1-staged-rollout.md](l1-staged-rollout.md) — A staged rollout *is* deliberate skew; this spec is what makes it safe to have two cohorts live at once.
- [l1-nodus-portability.md](../../nodus/specifications/l1-nodus-portability.md) — LP-8's capability manifest is presence-based; LP-23 adds the version dimension defined here.

## 1. Motivation

**A single protocol version couples everything to its most volatile part.** One number over the whole surface means the capability that changes shape every month sets the compatibility floor for the twenty capabilities that have not changed in a year. Peers are then forced to upgrade in lockstep for reasons that have nothing to do with what they use, and the number stops being consulted because it is always wrong in the conservative direction.

**Compatibility asserted in prose is compatibility nobody can check.** "This is a minor release; it is backward compatible" is a claim about a structural diff, made by the person least able to see it — the author, who knows what they *meant* to change. The diff itself is mechanical and can be computed. Where it is not computed, mislabelling a breaking change as a minor is caught by nothing: every peer in the repository is rebuilt together and passes.

**The dangerous direction is the one that looks safe.** Adding a value to an enum is intuitively additive. It is additive on the *inbound* slot — an old peer never sends the new value. On the *outbound* slot it is a break: the old peer decodes what it receives, meets a value its schema refuses, and fails on a payload it did not ask for and cannot opt out of. The same edit is safe or fatal depending only on which way the data flows, and nothing about the edit itself says which.

**Schema tolerance is routinely mistaken for compatibility.** A new field declared optional, or backed by a fallback value, makes the *parse* succeed. It says nothing about whether the older peer's payloads ever carry the key, and nothing about whether every consumer of that data actually runs the parse before touching the field. A field typed as always-present, delivered as absent through a path that bypassed the tolerant decode, is a runtime fault that the compatibility gate credited as safe.

**Skew is discovered at the worst moment.** Two peers meet at connect time, in the user's hands, after both are installed. A failure discovered then must be legible — which capability, which versions, and *which side to upgrade* — because the user is the only one who can act, and "incompatible" without a direction is a dead end.

**The check must run against what shipped, not against the last commit.** Comparing a working tree to its parent proves nothing about the release a user is running from six months ago. The peer population in the field is a set of released artifacts, and the compatibility question is asked once per member of that set.

## 2. Constraints & Assumptions

- The set of peer pairs is open and grows: every new surface, client, extension host, or remote topology adds one.
- Peers cannot be assumed to upgrade together. Anything the user installs — a mobile client, an extension, a self-hosted office — upgrades on the user's schedule or not at all.
- Some transports fail closed for the whole connection; others fail per capability at use. This spec does not choose the transport; it requires that the compatibility verdict *match* whichever behaviour the transport actually has (PCO-10).
- "Version" here always means a contract's own schema version, never a distribution version. The two are independent numbers (PCO-1), and a shared name for them is the root of most confusion in this area.
- The contract surface is machine-readable. A contract that exists only as prose, or only as hand-written parsing code, cannot be fingerprinted and is outside what this spec can protect.

## 3. Core Invariants (Layer 1 only)

- **PCO-1 (Contract version, distribution version, and persistence version are three independent systems):** the number governing *what two peers negotiate* is distinct from the number governing *which build a component depends on*, and both are distinct from the number governing *what a stored record means*. None implies another: a distribution bump may change no contract, and a contract change is gated by this spec's rules regardless of how the artifact is versioned for shipping. Conflating them produces both false alarms and silent breaks, and MUST NOT be done.

- **PCO-2 (The surface is a set of independently versioned capability lines, never one protocol version):** each named capability — method, event, seam, extension point — carries its own `{major, minor}` line and evolves alone. A breaking change to one capability MUST NOT raise the floor for any capability that did not change. A single version over the whole surface is forbidden: it makes compatibility a property of the release rather than of what the peers actually use.

- **PCO-3 (The newer peer adapts; the older peer never learns):** when two peers differ on a capability, the entire burden of transformation falls on the one holding the newer version. The older peer sends and receives exactly what it always did, with no knowledge that a newer version exists, no conditional branches, and no update. This asymmetry is what makes N-way skew tractable: each peer needs bridges only to versions *older* than its own, so the work is linear in a peer's own history rather than quadratic in the population.

- **PCO-4 (Compatibility is an executable transform, not a claim):** where two adjacent versions of a capability differ, the bridge between them exists as a **declared, callable transform** registered beside the contract — request and response, in the direction the holder of the newer version needs. A version chain with a hole in it is an incompatibility, discovered when the chain is walked, not a documentation gap. "Backward compatible" is not a sentence a spec or a changelog may assert on its own; it is a path that either exists in the registry or does not.

- **PCO-5 (The version label is verified against the structural diff, never trusted):** whether a change is compatible or breaking is decided **mechanically**, by comparing normalized structural fingerprints of the two contract versions, and the declared version bump MUST agree with that verdict. A change that removes or renames a field, narrows an accepted set, changes a type, alters a default or transform so two readers disagree on meaning, or adds a required field is breaking and cannot ship as a compatible bump. The author does not adjudicate their own diff.

- **PCO-6 (Direction decides severity; the same edit is safe one way and breaking the other):** growth of a value set — a new enum value, a new union variant, a new field key — is classified by **which peer emits it**. On a slot the *older* peer emits, growth is safe: an old peer never produces the new value, and a new peer that sends one to an old peer fails per call with a nameable cause. On a slot the *newer* peer emits toward an older one, the same growth is breaking: the older peer decodes what it is handed unconditionally, and every payload carrying the new value fails with no opt-out. Every compatibility verdict MUST be direction-aware; a direction-blind rule is wrong in one of the two directions by construction.

- **PCO-7 (Parse tolerance is not compatibility):** a field being optional, defaulted, or fallback-backed makes a *parse* succeed and proves nothing else. It does not establish that an older peer's payloads ever carry the key, nor that every consumer runs that parse before reading the field. A new key on a peer-facing outbound slot is therefore treated as breaking on its own terms, and MUST NOT be credited as safe because its schema tolerates absence.

- **PCO-8 (A floor that is fatal, and an optional surface that degrades):** the declared surface is split. The **floor** is the small set of capabilities without which the relationship is meaningless; disagreement there fails the connection closed, with no partial mode. Everything else is **optional**: a peer that does not advertise an optional capability is not an error, and each such capability declares its own degrade — either *unsupported*, which the consuming surface renders as a visible, reasoned absence, or *fallback*, which names an older floor capability plus the adapters that express the request in its terms and lift its answer back. Absence is negotiated per capability, never inferred from a version number, and a newly added capability MUST NOT be placed in the floor.

- **PCO-9 (The oracle's input is the released population, not the previous commit):** compatibility is evaluated against **surfaces captured from released artifacts**, one comparison per release still considered live. A working tree checked only against its own parent proves nothing about the peer a user is actually running. The captured surfaces are immutable inputs; they are never edited to make a check pass.

- **PCO-10 (Severity mirrors the transport's real failure behaviour):** each incompatibility class is graded by what the shipped transport actually does with it — a whole-connection fail-closed handshake yields a fatal verdict that no exception may waive; a per-capability check at use yields a feature-outage verdict. A gate whose severities do not correspond to observed behaviour teaches its users to ignore it: in one direction by nuisance, in the other by false safety.

- **PCO-11 (What the checker cannot verify is declared as a reviewed claim, never inferred):** where compatibility depends on a property of the *emitter* that no structural diff can see — most commonly that a peer consults the negotiated version and withholds newer values from older peers — the exemption is recorded **on the contract itself** as an explicit declaration, and is understood as a human claim under review rather than a machine finding. Absent that declaration, the structural rule stands. Silence is never read as the claim.

- **PCO-12 (Exceptions are standing policy, not per-change judgement):** a class of difference that is genuinely safe for a structural reason is encoded as an **identifier-agnostic policy rule** stating the reason, applied uniformly. Per-change sign-off is forbidden: it converts a mechanical gate into a review queue, where the cost of refusing an exception rises with the deadline and the gate erodes to nothing.

- **PCO-13 (A static gate that mirrors a runtime behaviour is pinned to it by test):** where a build-time checker reproduces the logic of a runtime negotiation, the two MUST be tied together by tests asserting the checker's verdicts against the *real* negotiation oracle. An unpinned mirror drifts, and a drifted mirror is worse than none: it reports confidently on behaviour the product no longer has.

- **PCO-14 (An incompatibility names the capability, the two versions, and which side to upgrade):** the verdict a user or operator receives is never a bare refusal. It identifies each capability that could not bridge, both peers' versions of it, and — derived from which side is older *on each* — a concrete direction to act in. Where both sides are older on different capabilities, both directions are stated. An incompatibility with no named remedy is a dead end for the only participant who can resolve it.

- **PCO-15 (A content or meaning change on a structurally unchanged surface is a compatibility change the structural diff cannot see):** [ADDED v1.1.0] PCO-5's structural fingerprint compares the **shape** of a contract across versions — fields, types, required-ness. It is blind by construction to a distinct, equally-breaking class: a slot whose **declared shape never moves** while what is populated into it, or whether it is populated at all, changes — a field the emitter quietly stops filling, a value whose meaning, units, or nullability shifts without a type change, content that starts being derived from a different source, or a message that starts or stops being sent on an existing path with no new opcode. None of these produce a structural diff, so PCO-5's mechanical gate passes them by default, and PCO-7's parse-tolerance rule does not apply either — the field is present and well-typed, it is simply saying something different now. Because no structural signal exists for the checker to key on, this class is declared the same way PCO-11 declares an emitter behavior no structural diff can see: **on the contract itself**, as a reviewed claim that a given content/meaning change is compatible or that it requires the same negotiation machinery a structural change would (PCO-2's independently-versioned capability line, PCO-8's floor/optional split). Absent that declaration, a content change that an older reader would misinterpret is breaking, and silence is never read as the compatible case — inheriting PCO-11's rule that an undeclared exemption is no exemption.
- **PCO-16 (A temporary compatibility branch is born with its removal condition and its siblings):** `[ADDED v1.2.0]` a code path kept only because some still-supported peer lacks a capability — a feature probe and its fallback — is declared either **permanent** (the capability will never exist there) or **temporary**, and a temporary branch carries a **named removal condition** stated against the supported population (PCO-9: *every peer version still supported provides X*), shared verbatim by **every** site that serves the same condition, so all of them are found and removed together when it holds. Two failures it forbids: removing one site and leaving its siblings, which strands a half-removed fallback that nothing exercises; and removing a branch because the newest peer — or the one at hand — supports the capability, which breaks every older peer still in the supported range. A probe that **fails** — a timeout, an unreadable answer — is reported as a failure and never read as *capability absent*: the two lead to different branches, and a network blip silently selecting the degraded path is indistinguishable, afterwards, from a peer that really lacks it.

> L2 specs cannot reach RFC status until all invariants here are addressed in their "Invariant Compliance" section.

## 4. Detailed Design

### 4.1 The shape of a versioned surface

```text
[REFERENCE]
surface := { capability_name -> capability_line }

capability_line := {
  majors: {
     major_n: {
        latest_minor           : m
        versions               : { minor -> { contract, upgrade_from_previous_minor } }
        downgrades_from_latest : { older_major -> transform_pair }
     }
     ...
  }
  degrade? : unsupported | fallback{ to: floor_capability, adapt_request, adapt_response }
}

contract := { request_shape, response_shape }      // machine-readable, fingerprintable
canonical(capability) := highest installed minor of the highest installed major
```

A peer's **declaration** is one canonical version per capability it holds — nothing more. It is not a feature list, not a build number, and not a set of flags: it is exactly the information the other side needs in order to decide whether it can bridge.

### 4.2 Negotiation, and why the asymmetry is the whole trick

For each capability present in either declaration, a peer asks one question of *its own* registry: *can I bridge from my canonical to theirs?*

```text
[REFERENCE]
can_bridge(mine, theirs):
    mine == theirs                  -> yes          (nothing to do)
    mine.major <  theirs.major      -> yes          (PCO-3: I am older; they adapt, I do nothing)
    mine.major == theirs.major:
        mine.minor <  theirs.minor  -> yes          (older again; not my problem)
        else                        -> the minor `theirs` must exist in my line
    mine.major >  theirs.major      -> a downgrade bridge to `theirs.major` must exist
```

Three of the five arms are "yes, because I am the older one." That is PCO-3 in operational form, and it is what keeps the cost bounded: **a peer only ever needs bridges backwards through its own history.** It never needs to know what a future version will look like, and a peer released years ago is required to do nothing at all. The population can contain any number of versions and no peer's obligation grows with it.

The corollary is a design constraint, not a convenience: because the older peer cannot be changed, every difference must be expressible as a transform the *newer* side can perform without the older side's cooperation. A change that only works if both sides do something is a major, by definition.

### 4.3 Direction, and the asymmetry inside a single change

PCO-6 is the invariant most often violated by well-meaning changes, so it is worth stating as a table. Read "outbound" as *toward a peer that may be older*.

| Change to a contract | On an inbound slot (older peer emits) | On an outbound slot (older peer receives) |
| --- | --- | --- |
| Add an optional field | safe — old peer omits it, new peer defaults | **breaking** (PCO-7) — old peer's payload never carries it, and a consumer typed as if it does gets nothing |
| Add an enum value / union variant | safe — old peer never sends it | **breaking** — old peer's decode refuses the value it is handed |
| Remove a field | breaking | breaking |
| Narrow an accepted set | breaking | breaking |
| Widen an accepted set | safe | **breaking** unless emission-gated (PCO-11) |

The right-hand column has one escape and it is PCO-11's: the emitter consults the negotiated version and withholds the new value from peers that predate it. That is a real and useful pattern — but no fingerprint can see it, so it is a **declaration on the contract**, reviewed as a claim about behaviour. It is not something a checker infers, and its absence is never read as its presence.

### 4.4 Floor and optional, and what "degrade" actually buys

Splitting the surface (PCO-8) is what turns "we cannot add capabilities without breaking old peers" into "we can add capabilities freely."

```text
[REFERENCE]
declaration := { floor: {cap -> version}, optional: {cap -> version} }

on connect:
    check(floor)     -> any disagreement = fatal, connection refused, PCO-14 message
    optional         -> evaluated per capability, at first use, never at connect

on use of an optional capability the peer does not advertise:
    degrade = unsupported -> surface renders a reasoned absence (ATE-2/ATE-3),
                             every other capability keeps working
    degrade = fallback    -> express the request in the floor capability's terms,
                             lift its answer back into the newer shape
```

Two consequences are load-bearing. First, **a new capability must never enter the floor** — the floor is fail-closed on its name set, so adding a name to it retroactively makes every older peer incompatible over a capability it has no need of. Second, degrading is *per capability and per peer*: in a topology with several peers, one peer's missing capability degrades that peer's contribution while the others continue at full fidelity. The user sees a partially-featured participant, not a dead session.

The `fallback` form deserves emphasis because it is the one that costs design effort: it requires that the newer capability's request be *expressible* in the older one's terms and its answer *liftable* back. Where that is impossible, the honest degrade is `unsupported`. Inventing an approximation that silently answers a different question is worse than absence — it is the class of failure PCO-14 exists to make visible, arriving instead as a plausible wrong answer.

### 4.5 The gate: what is compared, and against what

```text
[REFERENCE]
for each released_surface R in live_release_set:
    for each capability C in (working_surface ∪ R):
        classify(diff(working_surface[C], R[C]), direction_of_each_slot)
            -> fatal    : floor name-set difference, or unbridgeable canonical pair
            -> breaking : a released peer loses something it shipped with
            -> advisory : growth that cannot break a released peer's decode
    apply standing policy exceptions (PCO-12)

fatal    -> never waivable
breaking -> blocks; resolved by opening a new major with downgrade bridges
advisory -> reported, does not block
```

Three properties of this gate matter more than its details:

**It compares against released artifacts (PCO-9), not against history.** The set of live releases is the actual peer population; anything else measures the wrong thing. The captured surfaces are generated, immutable, and committed alongside the change so the diff is reviewable — and they are never hand-edited to turn a red into a green, which is the single most damaging thing anyone can do to a gate like this.

**Its severities are copied from observed behaviour (PCO-10), not chosen for convenience.** A capability whose transport refuses the whole connection on mismatch produces a fatal that no policy may waive; a capability checked per use produces a feature outage that may, with a reason, be accepted. Mismatched severity is how gates die: too strict and it gets bypassed, too lax and it certifies breaks.

**It is pinned to the runtime oracle (PCO-13).** The build-time checker and the connect-time negotiation are two implementations of one rule, and a rule with two implementations diverges. Tests asserting the checker's verdict against the real negotiation for the same inputs are what keep the mirror honest — the same discipline the parity corpus applies across simultaneous surfaces, applied here across time.

### 4.6 Reporting an incompatibility

PCO-14 is small and disproportionately valuable. The refusal a user sees is assembled, not templated:

```text
[REFERENCE]
incompatible(capabilities) ->
    per capability : name, my_version, their_version, why (missing | unbridgeable)
    aggregate      : which side is older on each  ->  { upgrade_A?, upgrade_B? }
```

Deriving the direction from *which side is older per capability* rather than from a global assumption is what makes it correct in the awkward case — a peer ahead on one capability and behind on another. Both directions are then stated, which is honest and actionable; a single guessed direction would send the user to upgrade the half that was already newer.

### 4.7 Demarcation — three neighbours that are not this

| Neighbour | Its question | Why it is not this |
| --- | --- | --- |
| [l1-surface-parity.md](l1-surface-parity.md) | Do two surfaces *of the same age* compute the same answer? | Parity is about re-derivation between simultaneous consumers. Skew is about one consumer being older. A product can be perfectly parity-clean and still fail every skewed connection. |
| [l1-extension-points.md](l1-extension-points.md) | Where may an extension attach, and under what contract? | EP defines the seam and its lifecycle; this defines how a seam's contract evolves and how two ages of it meet. EP-5's refusal is PCO-8's `unsupported` degrade at the extension seam. |
| [l1-record-evolution.md](l1-record-evolution.md) | What does a stored record mean to a reader that did not write it? | Same shape, different medium and different failure. A peer that refuses is offline; a record that is misread is corrupted. PCO-1 keeps their version systems separate. |

### 4.8 Nodus relevance

| Element | nodus seam | Note |
| --- | --- | --- |
| Per-capability version lines (PCO-2) | LP-8 capability manifest | The manifest is presence-based today; LP-23 adds the `{major, minor}` dimension, so a host that *has* a role at an older contract is distinguishable from one that lacks it. |
| Newer side adapts (PCO-3) | runtime ↔ workflow bundle | A bundle authored against an older language level runs unchanged on a newer runtime; the runtime holds the bridges. A bundle never carries forward-compatibility code. |
| Floor vs optional (PCO-8) | LP-8 fail-fast pre-run | Today every unmet capability is fatal pre-run. The floor/optional split lets a workflow declare which capabilities it can proceed without, degrading that branch instead of refusing the run. |
| Degrade declarations (PCO-8) | `~PICK` / effect-class fallbacks | The `fallback` form is a declared alternative step expressed in terms of a floor capability, not an improvised substitution. |
| Verdict names the side (PCO-14) | `NODUS:*` typed pre-run diagnostic | "Workflow requires Storage v2; host provides Storage v1 — upgrade the host, or lower the declared level." |

## 5. Drawbacks & Alternatives

- **Per-capability lines are more bookkeeping than one version number.** Accepted. The bookkeeping is mechanical and mostly generated; the alternative is a floor set by the most volatile capability, which is a cost paid by every user on every release rather than by one author once.

- **Executable transforms are code that exists only for old peers.** They accumulate, and most of them run for a shrinking population. This is the cost of PCO-3, and it is bounded deliberately: a major line may be retired, at which point its bridges are deleted together with support for it, and peers below that line are refused with a PCO-14 message rather than served silently wrongly.

- **The gate will refuse changes that are, in fact, safe.** PCO-7 in particular is stricter than the schema requires, and will block additions that would have worked. That asymmetry is intentional: the cost of a false refusal is one conversation and a major bump; the cost of a false pass is a shipped peer failing in the field on payloads it cannot influence.

- **Alternative — require peers to upgrade together.** Rejected as unavailable rather than undesirable. Nothing the user installs can be forced to move, and a topology where a remote office, a mobile client and an extension must all update in the same hour is not a topology anyone operates.

- **Alternative — tolerate everything, decode leniently everywhere.** Rejected: blanket leniency turns a version mismatch into a silently wrong value, which is the failure this spec exists to convert into a legible refusal. Tolerance is a decision made per slot, with a direction and a reason ([l1-record-evolution.md](l1-record-evolution.md) governs where it is right).

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[PARITY]` | `.design/main/specifications/l1-surface-parity.md` | SP-6 cross-consumer agreement — the same-age sibling of this problem; PCO-13's pinning is its discipline across time. |
| `[EXT-POINTS]` | `.design/main/specifications/l1-extension-points.md` | EP-5 — the seam version contract this spec refines into lines, bridges and degrades. |
| `[ACP]` | `.design/main/specifications/l1-acp.md` | ACP-2 capability declaration — the negotiation surface PCO-2/PCO-8 restructure. |
| `[RECORDS]` | `.design/main/specifications/l1-record-evolution.md` | The persistence twin; PCO-1 keeps the two version systems distinct. |
| `[DOCTOR]` | `.design/main/specifications/l1-doctor.md` | HEAL-8 build-parity skew — where a PCO-14 verdict surfaces as a constitutive check. |
| `[TRIPWIRES]` | `.design/main/specifications/l1-invariant-tripwires.md` | TW-7/TW-10 — how PCO-5/PCO-9/PCO-13 are enforced without becoming unadoptable. |
| `[ERGONOMICS]` | `.design/main/specifications/l1-agent-tool-ergonomics.md` | ATE-2/ATE-3 — the shape of the `unsupported` degrade at a surface. |
| `[NODUS-PORT]` | `.design/nodus/specifications/l1-nodus-portability.md` | LP-8/LP-23 — the manifest that gains this spec's version dimension. |

## Document History

| Version | Date | Author | Change |
| --- | --- | --- | --- |
| 1.2.0 | 2026-10-22 | Core Team | Added PCO-16 — a temporary compatibility branch is born with a named removal condition stated against the supported population and shared by all its sibling sites, so they are removed together and never because one peer supports the capability; permanent divergence is declared as such; a failed probe is a failure, never *capability absent*. Mined from an external command-line tool serving a range of self-hosted server versions, which tags each temporary feature gate with a shared cleanup identifier and propagates detection errors. |
| 1.1.0 | 2026-09-01 | Core Team | Added PCO-15 — a content or meaning change on a structurally unchanged surface is a compatibility change PCO-5's structural fingerprint cannot see by construction: a field the emitter quietly stops populating, a value whose meaning/units/nullability shifts without a type change, or a message that starts/stops flowing on an existing path with no new opcode. None of these produce a diff for PCO-5 to key on, and PCO-7's parse-tolerance rule does not apply either since the field is present and well-typed. Declared on the contract itself as a reviewed claim, the same way PCO-11 declares emitter behavior no structural diff can see; undeclared, the change is breaking and silence is never read as the compatible case. Distilled from an adoption pass over an external desktop multi-agent orchestrator's client/host wire-compatibility documentation. |
| 1.0.0 | 2026-08-20 | Core Team | Initial spec — peer compatibility under version skew: three independent version systems kept separate (PCO-1); the surface as independently versioned capability lines rather than one protocol version, so one volatile capability cannot set the floor for all (PCO-2); the newer peer adapts and the older peer never learns, which bounds each peer's obligation to its own history instead of the population (PCO-3); compatibility as a declared executable transform rather than a changelog claim, so a hole in the chain is an incompatibility and not a documentation gap (PCO-4); the version label verified against a normalized structural fingerprint, removing the author's ability to adjudicate their own diff (PCO-5); direction-decides-severity — the same value-set growth is safe inbound and fatal outbound, because the older peer decodes what it is handed unconditionally (PCO-6); parse tolerance is not compatibility, since optionality proves the parse succeeds and nothing about whether the payload carries the key or whether every consumer runs the parse (PCO-7); a fail-closed floor plus an optional surface that degrades per capability as unsupported or as an adapted fallback to a floor capability, with new capabilities barred from the floor (PCO-8); the oracle's input as surfaces captured from released artifacts rather than the previous commit, immutable and never edited to make a check pass (PCO-9); severities copied from the transport's observed failure behaviour so the gate is neither nuisance nor false safety (PCO-10); emitter properties no fingerprint can see recorded as reviewed declarations on the contract, with silence never read as the claim (PCO-11); exceptions as identifier-agnostic standing policy rather than per-change sign-off, which erodes under deadline (PCO-12); a static mirror pinned to the runtime oracle by test, since a drifted mirror reports on behaviour the product no longer has (PCO-13); and an incompatibility that names capability, both versions, and which side to upgrade — derived per capability so a peer ahead on one and behind on another gets both directions (PCO-14). Demarcated from surface parity (same-age divergence), extension points (seam definition) and record evolution (stored meaning) in §4.7; nodus mapping supplies LP-23. Concept-only. Distilled from an adoption pass over an external multi-provider agent-orchestration desktop client whose client and host ship as separately released artifacts. |
