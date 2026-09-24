# Invocable Registry & Dispatch Contract

**Version:** 1.1.1
**Status:** Stable
**Layer:** implementation
**Implements:** l1-surface-parity.md

## Overview

The concrete realization of the SP-11 action catalog in this project's stack: **one runtime registry of invocables** owned by the Rust core, **one `Invocation → Outcome` dispatch** through which every action is reached, and three surfaces — the `cronus` command line, the terminal UI, and the desktop shell — that **render their command surface as a projection of that registry** instead of restating it.

The registry is simultaneously the `contribute` extension point of the EP-2 taxonomy: core-supplied invocables and extension-supplied invocables enter through the same published door (EP-12), which is what makes a plugin's command reachable from a surface the plugin has never heard of.

## Related Specifications

- [l1-launch-handoff.md](l1-launch-handoff.md) — `[ADDED v1.1.0]` LH-5 is why the command line is projected in two halves (§4.7.1): the launcher may not resolve a name an extension can define, so only the semantic half is generated from the registry.
- [l1-surface-parity.md](l1-surface-parity.md) — Parent concept. SP-11 is this spec's subject; SP-1/SP-2 are the rules it makes structural rather than reviewable.
- [l1-extension-points.md](l1-extension-points.md) — EP-2 (`contribute` kind), EP-4 (collision rule), EP-9 (lazy activation), EP-11 (reserved core namespace), EP-12 (no privileged registration door), EP-13 (registration as a reversible effect).
- [l1-input-binding.md](l1-input-binding.md) — IB-1 makes the declared binder list the single source of the advertised schema; IB-2…IB-5 supply the rejection model this dispatch returns.
- [l1-architecture.md](l1-architecture.md) — INV-3 (command parity) becomes a structural property here; INV-9 (shipped-surface honesty, incl. the v1.4.0 declared-retirement rule) is enforced at the projection.
- [l2-surface-conformance.md](l2-surface-conformance.md) — Sibling. This spec makes the surfaces agree by construction; that one proves it (SP-6).
- [l2-cli.md](l2-cli.md) · [l2-tui.md](l2-tui.md) · [l2-application-shell.md](l2-application-shell.md) — The three consuming surfaces.
- [l2-crate-topology.md](l2-crate-topology.md) — The tier model that decides where the descriptor types may live.
- [l2-extension-registry.md](l2-extension-registry.md) — Extension manifests and lifecycle; the origin of contributed invocables.
- [l2-core-library.md](l2-core-library.md) — The facade crate that owns the capability contract.
- [l1-capability-reachability.md](l1-capability-reachability.md) — REA-1/REA-5: every invocable declares agent-reachable or human-only; the `reach` field (§4.2).
- [l1-security.md](l1-security.md) — SEC-10: the verbs that write the authority plane are human-only.

## 1. Motivation

**The same mapping is written by hand once per surface, and there are three of them.** Shell input reaches the core through a command enum; slash input reaches it through a separately maintained catalog; the desktop's WebView reaches it through a per-capability IPC command list. Each is internally consistent, and no import gate, type check, or per-surface test can see that they describe different products. This is SP-2 in its pure form: three re-derivations of one fact.

**A compile-time surface cannot host a runtime contribution.** A command enum is fixed when the binary is built, and so is the desktop's IPC handler list. An extension installed afterwards has nowhere to attach, whatever its manifest declares. Extension manifests already carry a capability list that no code consumes, and an extension can therefore reach *Active* while adding nothing a user can invoke. Any design that keeps a compile-time enum as the source of truth forecloses the plugin story permanently — the constraint decides the architecture before cost enters the discussion.

**Parity that is asserted against a hand-copied list is not asserted at all.** The terminal UI proves its parity against a constant mirroring the command-line verb set, maintained by hand. When the command line grew, the mirror did not, and the check stayed green while the two surfaces diverged by eight verbs. A duplicated oracle degrades silently; it is the failure mode SP-6 exists to close, and it must be replaced by a projection rather than repaired as a list.

**The drift reaches the specification layer too.** The command-line spec's own parity table lists verbs the product does not have and omits every group it does. A table maintained by hand alongside the thing it describes is the same defect at a different altitude, and the remedy is the same: stop maintaining a copy, and derive.

**One decision point is also the only way the security property becomes real.** Output redaction is applied by the terminal UI and by the desktop bridge, and not at all by the command line. Three surfaces, two implementations, one omission — and all of them fed an empty secret list. A single dispatch boundary is the one place where INV-7 can be satisfied once instead of remembered three times.

## 2. Constraints & Assumptions

- **The ports tier is dependency-free by construction.** Its manifest carries an empty dependency section and its own specification states the property as structural. Serialization support for descriptor and outcome types is therefore an **optional feature**, off in the default build.
- **Compile-time command surfaces remain in use, as generators rather than sources.** The command line keeps its argument parser and the desktop keeps its IPC transport; both are *built from* the registry at startup rather than declaring it.
- **All three surfaces link the core in-process.** The command line, the terminal UI, and the desktop's Rust shell each construct the engine directly. There is no core↔frontend process boundary; the desktop's inter-process seam lies **inside** that frontend, between its shell and its WebView.
- **The desktop shell is a separate build workspace,** deliberately detached so WebView dependencies stay out of the engine workspace. No single workspace-level test can link all three projections.
- **The registry is startup-assembled and then read-mostly.** Registration happens during composition and on extension activation/deactivation; dispatch is the hot path.
- **This spec does not implement the five unshipped command groups.** Their surface treatment is prescribed here (INV-9); their behavior is out of scope.

## 3. Invariant Compliance (Layer 2 only)

| L1 Invariant | Implementation |
| --- | --- |
| SP-1 One decision point per observable behavior | Every action a user can reach through more than one surface is an **invocable** registered once and dispatched once. A surface owns argument syntax, rendering, and view state, and nothing else. Where a surface needs a decision the registry does not expose, the remedy is a new invocable or a new binder — never a local computation. The current counter-example is explicit: the command line computes the board's on-disk location itself, a domain fact it must instead receive. |
| SP-2 Derived facts are consumed, never recomputed | The advertised argument schema, the help text, the completion list, the verb set, and the grouping are all **projections of one descriptor**. A surface that recomputes any of them is defective even while it agrees. The three present re-derivations (parser enum, slash catalog, IPC command list) collapse into one, and their replacements are pinned by SP-5 tombstones. |
| SP-3 Divergence is a tracked defect class | Each collapsed mapping is recorded as a finding in the inventory owned by [l2-surface-conformance.md](l2-surface-conformance.md) §4.1, naming its sites, the divergence reachable from it, and this registry as the replacing primitive. |
| SP-4 A finding is repaid only when all four hold | Repayment is defined and tracked in the sibling spec: copies deleted, deletion pinned, adversarial fixture landed, every consumer registered. This spec supplies the primitive; it does not itself close the findings. |
| SP-5 Deletion is pinned; ledgers move one way | The hand-copied verb mirror, the per-capability IPC command list, and the stale specification parity table are entered as tombstones when removed. The tombstone list only grows. |
| SP-6 Parity is proven by one corpus through real projections | Delegated to [l2-surface-conformance.md](l2-surface-conformance.md). This spec's contribution is to make each surface's projection a **callable function of the registry**, so a corpus can drive the real one rather than a stand-in. |
| SP-7 A new consumer registers before it ships | A surface becomes a surface by implementing the projection interface (§4.7) and running the shared corpus in its own test target. The desktop's separate workspace makes this a per-surface obligation rather than a central test. |
| SP-8 Legitimate difference is named, with its reason | The descriptor's `locus` field (§4.8) is the machine-readable form of this rule. Host-owned facilities — the desktop's shell settings are the standing example — are declared `HostOnly` with their reason, not omitted. |
| SP-9 Extract before the second implementation exists | The registry is extracted before the desktop's generic dispatch and before any further surface. Serialization is designed in at the start (§4.5) precisely because retrofitting it after the WebView projection exists is the expensive branch of the ladder. |
| SP-10 Converge first, correct second | Adopting the registry is behavior-preserving by construction: a verb's observable result must not change in the same commit that moves it onto the registry. Known-wrong behaviors uncovered during migration — the swallowed store error that reports emptiness, the ignored output-format flag, the unescaped hand-built structured output — are recorded as residuals at the invocable that owns them and corrected in separate, disclosed changes. |
| SP-11 Shared vocabulary, declared exposure | The registry **is** the catalog. Every invocable carries identity, human-readable name, summary, locus, and declared binders. Menus, help, completion, slash discovery, and agent-facing instructions render from it. A surface that deliberately does not expose an invocable declares the exclusion (§4.8); silence is not an available way to say "not here". `[MODIFIED v1.1.0]` The locus set gains `Installation` (§4.8), which is what stops installation verbs from being modelled as `Semantic` and offered inside a session where they have no meaning. |
| SP-12 Projectable and executable faces, declared per member | `[ADDED v1.1.0]` The handler is not a field of the descriptor — the dispatcher holds it, joined to the descriptor only by identity (§4.2). Serialization is derived in the **outbound direction only**, so a wire payload cannot construct a descriptor and no serialization site has a host handle available to leak. The split is structural rather than a rule each site must remember. |
| REA-1 (l1-capability-reachability) Reachability declared | The descriptor's `reach` field (§4.2); `HumanOnly` verbs are absent from agent-facing projections and refused for an agent caller; authority-plane verbs are `HumanOnly`. Pending realization. |
| SP-13 Unrecognized and unavailable are different answers | `[ADDED v1.1.0]` Resolution is a distinct step returning `Found`/`Unknown` (§4.5); `Outcome` exists only for an invocation that resolved, and carries `Unavailable` for one that ran and could not answer. The three surfaces act on `Unknown` in three different ways — fall through, usage error, refresh the stale catalog — none of which an outcome variant could serve. |

## 4. Detailed Design

### 4.1 Placement in the crate tiers

```plaintext
ports tier      descriptor types, Invocation, Outcome, Rejection   (zero-dep; serialization optional)
      ▲
domain tier     the registry itself; core invocable definitions
      ▲
facade          composition root: registers core invocables through the public seam
      ▲
   ┌──┴──────────────┬────────────────────┐
  CLI              TUI              desktop shell ──IPC──▶ WebView
```

Descriptor and envelope types live in the ports tier because all three surfaces and the domain must name them without any of them depending on each other. The registry's behavior lives in the domain tier: it is pure logic with no I/O. Composition — deciding *which* invocables exist in a given build — belongs to the facade, which is already the sole owner of the capability contract.

**Serialization.** The ports tier gains an optional serialization feature, disabled by default so the tier's dependency-free property holds for the ordinary build. The desktop shell enables it; it already carries that dependency. The rejected alternative is a shell-owned data-transfer type, which would reintroduce exactly the hand-written mapping this spec removes.

### 4.2 The invocable descriptor

`[REFERENCE]` — shape, not implementation:

```rust
pub struct Invocable {
    pub id: InvocableId,        // qualified: "core:board.list", "<ext-id>:verb"
    pub name: &'static str,     // human-readable, catalog-facing
    pub summary: &'static str,  // one line; feeds help, slash discovery, completion
    pub group: &'static str,    // noun namespace, per the project command grammar
    pub locus: Locus,           // Semantic | ClientLocal | HostOnly | Installation
    pub binders: Vec<Binder>,   // ordered, typed; the single schema source (IB-1)
    pub stability: Stability,   // Shipped | Retired { superseded_by }
    pub reach: Reach,           // AgentReachable | HumanOnly (REA-1 of l1-capability-reachability)
}
```

`[ADDED v1.1.1]` **Reachability is declared on the descriptor** (`l1-capability-reachability` REA-1). The catalog is also what agent-facing instructions render from (SP-11), and REA-1 makes an undeclared capability agent-reachable by default — so without the field every verb in the registry would be one the agent may initiate. A `HumanOnly` invocable is omitted from every agent-facing projection, and dispatch refuses an agent-initiated invocation of it before any handler runs (REA-5: it becomes an instruction to the person, never an attempted call). Every verb that writes the authority plane — autonomy level, trust and permission rules, sandbox and egress policy, credential grants, user administration, channel and ingress configuration — is `HumanOnly` (`l1-security` SEC-10); a contribution cannot register an authority-plane verb at all (EP-7). Pending realization: the shipped descriptor has no `reach` field yet.

The descriptor is the **whole** advertised contract. Nothing about an invocable may be known to a surface that is not reachable from its descriptor, because anything else is a fact a surface would have to hold privately — and a privately held fact is the first step of a fork.

`[ADDED v1.1.0]` **The descriptor is the projectable face, and it is the only face that crosses a boundary** (SP-12). The handler is held separately by the dispatcher and is not a field of `Invocable`; the two are joined only by identity. This is not a stylistic split — it is what makes the projectable/executable boundary *structural*. A registry whose entry owned its handler would need every serialization site to remember to omit it, and the first site that forgets ships a host handle to a WebView. Here there is nothing to omit: `Invocable` derives serialization in the outbound direction only, and no inbound path can construct one from a wire payload.

`[ADDED v1.1.0]` **A contributed descriptor is validated and detached at registration** (EP-14). Descriptor text supplied by a contribution is a contributor-controlled quantity, and the boundary it must be constrained at is the one it *enters* through, not the one it eventually leaves through:

| Field | Constraint checked at registration |
| --- | --- |
| `id` tail | Matches the identity grammar (§4.4); non-empty; no separator character |
| `name`, `summary` | Non-empty after trimming; bounded length |
| `group` | Non-empty; matches the group grammar |
| `binders` | Bounded count; each binder's own name and description bounded and non-empty |

A descriptor failing any check is **refused, naming the field and the constraint**. It is never silently truncated, defaulted, or repaired: a repaired declaration is one whose author will never learn it was invalid, and the repair becomes an undocumented part of the contract. What the registry retains is a **normalized owned copy** — the contribution keeps no reference through which it could alter what the catalog later renders. Core-supplied descriptors pass through the same checks; a core descriptor that violates them is a bug that must fail at bootstrap rather than reach a surface.

`stability` carries the INV-9 obligation in both directions. `Shipped` is the only value that appears on a default surface. `Retired` keeps the migration path discoverable: the verb is off the shipped surface and out of completion, and an invocation of it resolves to a message naming its replacement — never a silent unknown-command failure, and never a permanent alias that leaves two verbs for one capability. An action the core cannot yet perform is **not registered at all**; it has no descriptor, so no surface can advertise it, and the "parses then answers *not implemented*" shape becomes unrepresentable rather than discouraged.

### 4.3 Registration: one door

Core invocables and contributed invocables register through the same published function. There is no privileged path, per EP-12: a seam exercised only by third parties is one nobody notices has become insufficient, and the core running through it fails loudly the moment the published surface stops being enough.

Core-supplied contributions differ in exactly the three ways EP-12 sanctions — eager loading where ordering demands it, implicit trust, and survival of the switches that disable third-party contributions — and in no other way. Composition and failure isolation apply to them unchanged.

`[ADDED v1.1.0]` **Registration returns the exact effect that reverses it.** A successful `register` yields a handle whose disposal removes that registration and its attached handler; there is no `unregister(id)` a caller could forget to call, mismatch, or call for an entry it did not register. This is EP-13's *ownership is automatic, never remembered* made structural: the unwind is produced **by the seam**, at the moment of registration, so a component that drops its handles has completed its teardown by construction. A contribution's disposal is what makes it replaceable in place — the whole point of EP-13 — and the failure it prevents is the one that is invisible until the second load, when a reloaded extension's stale registration is still resolving and answering.

### 4.4 Identity and collisions

Names are qualified by their contributor's identity (EP-11). The core owns a **reserved identity no contribution may claim**, which is what guarantees a contribution cannot shadow a core name *whatever the core adds later* — a guarantee first-registration-wins cannot make. Identities are validated at load: a well-formed identity may not produce an ambiguous qualified name.

`[ADDED v1.1.0]` **The grammar is enforced at construction, not assumed.** An identity is `<qualifier>:<tail>`, where both halves match a closed character grammar that excludes the separator, and both are non-empty. This is stated because the failure it prevents is silent: an identity type that accepts any string parses `"a:b:c"` and `"noqualifier"` without complaint, and the ambiguity surfaces much later as a lookup that resolves to the wrong entry or to none. A malformed identity is refused at the point it is built, so no unvalidated identity can exist to be registered.

Collisions on an unqualified verb are resolved by a **declared rule**, never by load order (EP-4). The rule this registry declares, in full:

- **Qualified identities never collide.** EP-11's namespace split makes this structural — two contributions cannot produce one qualified id — so the only contested space is the **bare form** (the tail alone).
- **The core's tail wins the bare form.** A contributed invocable whose tail matches a core one keeps its qualified name and does not take the bare form.
- **The loser is shadowed, not displaced.** It remains registered, remains resolvable by its qualified identity, and reappears in the bare form if the shadowing entry is ever removed. This is the distinction EP-4 now requires a rule to state, and it is the one a reader otherwise guesses wrong.
- **A shadowed bare form is reported, never silent.** The notice names the losing entry, the winner, and the qualified name the loser is still reachable by. Silence here is a specific defect: the contribution's author observes only that their verb "does not work", with nothing in the system that would tell them why or that it partly does.
- **Two sources claiming one identity** resolve by declared source order — the first loads, the later is refused with a notice naming it.

### 4.5 Dispatch

`[REFERENCE]`:

```rust
pub struct Invocation { pub id: InvocableId, pub args: ArgValues, pub caller: Surface }

// Resolution comes first and is a separate question (SP-13).
pub enum Resolved<'a> { Found(&'a Invocable), Unknown }

// An Outcome exists only for an invocation that resolved.
pub enum Outcome {
    Value(OutcomeValue),
    Stream(StreamHandle),
    Rejected(Rejection),            // a declared binder failed; body never ran
    Unavailable { reason: String }, // resolved, ran, could not answer
}
```

`[ADDED v1.1.0]` **Resolution is asked and answered before dispatch, and its negative answer is not an `Outcome`** (SP-13). An invocation naming nothing the registry knows has no outcome at all, because nothing ran — and the surfaces need that distinction to behave correctly, in opposite directions:

| Surface | On `Unknown` | Why an `Outcome` would be wrong |
| --- | --- | --- |
| Terminal UI | The line was never a command; hand it on as ordinary input | An error outcome makes every message beginning with a slash-like token an error |
| Command line | Report *no such command*, with the near-miss suggestion, at a usage exit code | An error outcome is indistinguishable from a command that ran and failed |
| Desktop | The catalog the client holds is stale; refresh it | An error outcome sends the user a failure for a client-side staleness problem |

Folding `Unknown` into `Unavailable` was the shape this spec carried at v1.0.0, and it is wrong for a reason that only appears at the surfaces: `Unavailable` means *this action exists and could not answer*, which is an incident; `Unknown` means *no such action*, which for one surface is not even an error. One outcome cannot serve both without one of the two surfaces behaving incorrectly.

`Outcome` is **structured data, never rendered text**. This is what lets one dispatch serve a text renderer, a structured-output renderer, a set of terminal widgets, and an IPC payload without any of them re-deriving the others' content. The existing push-channel seam on the desktop bridge — message plus a one-shot close, with reconnection owned by the host's connection lifecycle rather than a frontend timer — is the transport for `Stream`; this spec adds no second streaming mechanism.

### 4.6 Binding and rejection

An invocable declares an ordered list of typed binders, and that declaration is the single source from which its advertised schema is derived, so what a caller is told and what the runtime enforces cannot drift (IB-1). Every binder completes before the body begins, making *did not run* a structural fact rather than a self-report (IB-2).

A rejection travels the **normal result channel** as a structured value (IB-3), naming one of four distinguishable modes — **absent**, **unreadable**, **malformed**, **ill-shaped** — plus its location within the input (IB-4). Optional means absent and never invalid: a value that is present and fails to bind still rejects, with mode and location intact (IB-5).

Two consequences for the surfaces. A rejection is rendered, not thrown: the command line maps it to an exit code and a located message, the terminal UI to inline feedback, the desktop to a typed payload. And the classic quiet failure — a store error reported as an empty result with a success code — is unrepresentable, because emptiness and unavailability are different `Outcome` values rather than two readings of one printed line.

### 4.7 Surface projection

Every surface implements one interface: given the registry, produce this surface's command surface.

| Surface | Projects which loci | Projection | Replaces |
| --- | --- | --- | --- |
| Command line | `Semantic` + `Installation` | Argument parser built at startup from descriptors; help and completion from the summary and binders | Hand-written command enum, for the semantic half only |
| Terminal UI | `Semantic` + `ClientLocal` | Slash catalog and its discovery listing | Hand-written catalog + hand-copied verb mirror |
| Desktop shell | `Semantic` + `ClientLocal` | One generic dispatch IPC command plus the existing subscription channel; the WebView's client is generated from the catalog it receives | Per-capability IPC command list and its hand-written client |

`[ADDED v1.1.0]` **The projected set is a filter on locus, not the whole registry**, and the surfaces genuinely differ in which loci they take. This is what stops the projection from becoming absurd in either direction — a terminal UI offering `/completion` to emit a shell script, or a command line refusing to expose a verb because a session-scoped surface has no use for it. It also bounds what the command line's generated parser covers: `Installation` verbs are declared in the frontend that owns them (§4.7.1), and only the `Semantic` set is generated from descriptors.

`[ADDED v1.1.0]` **A surface's own extras are still descriptors, never a second list.** Where a surface adds an action that exists nowhere else — the terminal UI's pane focus, the desktop's panel toggle — it registers a `ClientLocal` invocable into the same registry rather than keeping a private table beside the projection. A surface holding one private table is a surface that will hold two, and the second is where the verb the other surfaces never learn about lives.

#### 4.7.1 The command line's two halves

`[ADDED v1.1.0]` The command line is the one surface whose verbs come from two places, and the split is by locus. Generating **all** of it from the registry was the v1.0.0 reading and it does not survive contact with the launcher contract: an `Installation` verb must be answerable when the composition it would configure is precisely what has not come up, so it cannot be projected from a registry that only exists after composition. Conversely, hand-declaring the semantic half is what reintroduces the fork this spec exists to remove.

| Half | Source | Examples |
| --- | --- | --- |
| Semantic verbs | Generated from `Semantic` descriptors at startup | The product's domain operations, including every verb an extension contributed |
| Installation verbs | Declared by the frontend itself, in its own closed grammar | Workspace initialization, configuration, extension management, diagnostics, completion generation |

The second half is the launcher's grammar and obeys [l1-launch-handoff.md](l1-launch-handoff.md): it is closed, small, self-owned, and contains no name an extension can define (LH-1/LH-5). The first half is resolved after composition, which is when the extensions that contribute to it exist.

**Installation verbs are still catalog entries, and they are declared exactly once.** SP-11 forbids expressing a boundary by silence, so an installation verb must appear in the catalog for the other surfaces to *declare* that they deliberately do not offer it — a terminal UI that simply lacks `config` is indistinguishable from one that has not implemented it yet. But a verb declared both in a launcher grammar and again as a descriptor is two statements of one fact, which is the fork this spec exists to close. The resolution is a single direction of derivation:

```text
[REFERENCE]
  The frontend declares its installation verbs ONCE, in its own source.
      ├── the launcher's parser is built from that declaration (pre-composition)
      └── the same declaration registers Installation descriptors into the
          catalog at composition (post-composition, for honesty and exclusions)

  Not: a hand-written parser beside a hand-written descriptor list.
```

The pre-composition parser and the catalog entry are two **consumers** of one declaration, not two declarations. This keeps the launcher answerable before the registry exists while leaving no verb the catalog cannot account for — and it is why a surface's declared non-exposure (§4.8) can be checked rather than assumed.

Parity stops being a property that is checked and becomes one that is **structural**: there is no place for a surface to hold a verb the registry does not have, and no way for it to miss one the registry does have. The conformance corpus then guards the remaining risk — that two projections of the same descriptor disagree in behavior — which is precisely the class §4.2 of the parent spec says no structural gate can see.

### 4.8 Locus and declared non-exposure

| Locus | Meaning | Example |
| --- | --- | --- |
| `Semantic` | Belongs to the core; reachable from every surface and remotely invocable | board, memory, workspace operations |
| `ClientLocal` | Belongs to a surface; the same identity may be implemented per surface | pane focus, panel toggle |
| `HostOnly` | Never remotely invocable; a host-owned facility | the desktop's shell settings slice |
| `Installation` | `[ADDED v1.1.0]` Acts on the product's own installation, not on the user's work; runs to completion without a session | workspace init, configuration, extension management, diagnostics |

`[ADDED v1.1.0]` The fourth locus exists because the first three had no member for a real and populous class, and its absence was pushing that class into `Semantic` — where it does not belong and does visible damage. An installation verb has no meaning inside a live session: *configure the product* is not an action within a piece of work, and a session-scoped surface that offers it is offering a verb whose effect the user cannot situate. The two halves are distinguished by a question with an objective answer: **does this act on the user's work, or on the product that hosts it?** Verbs that appear in both sets under one name are two different actions — selecting a session to start in is not switching session inside a running one — and they stay separate rather than being unified on the strength of a shared word.

The desktop's settings commands are the standing worked example: they are host-owned marshalling rather than core logic, so they legitimately stay outside generic dispatch — and are therefore **declared** `HostOnly` with that reason rather than quietly left out. A capability boundary stated as an exclusion is a scope decision; the same boundary expressed by silence is indistinguishable from a missing feature and will eventually be "fixed" by someone.

### 4.9 Lifecycle

A contribution is bound lazily — when its point is reached or its declared activation trigger fires — so installed-but-unused contributions cost nothing (EP-9), and the live wiring is inspectable at any time: which contributions are registered, under which identities, in what order.

Registration is a **reversible effect owned by the registering component** (EP-13). Deactivating an extension unwinds its invocables through the seam that created them, rather than relying on the extension to remember; teardown completes only once the work it stopped has actually stopped. A verb belonging to a deactivated extension disappears from every projection at once, because every projection is a function of the same registry.

`[ADDED v1.1.0]` **The registry is mutable after bootstrap, so a projection built once is a snapshot that goes stale.** Registration and disposal happen while surfaces are live — an extension activating on a trigger, a reload replacing a contribution in place — and a surface holding a projection built at startup will render a catalog that no longer describes the registry. The registry therefore **announces its own change**, and each surface refreshes the projection it owns:

- The announcement is a **notification, not a decision**: an observer cannot veto or alter the mutation, and the mutation is complete before observers run.
- **Observer failures are contained individually.** One failing observer neither aborts the mutation nor starves the observers after it. A surface that fails to refresh is a surface rendering a stale catalog — a real defect, logged as one — and it is strictly better than a registry that could not complete a registration because a UI could not repaint.
- The desktop's catalog crosses a process boundary and is therefore always a **copy** that can be stale between announcements; `Unknown` resolution (§4.5) is exactly the signal that tells it to refresh, which is why that outcome must be distinguishable rather than folded into a generic failure.

### 4.10 Contribution safety

A `contribute` point must declare what happens when a contribution misbehaves, who may attach to it, and how a user tells a contributed action from a core one. All three are properties of this registry, not of the surfaces projecting it.

**Failure policy (EP-6).** A contributed invocable's error, panic, or overrun is contained at the point and never reaches the kernel. Dispatch is time- and step-bounded; an invocable that exceeds its bound is terminated and its invocation resolves as **`Unavailable`, attributed to the contribution** — not as a core fault, not as a hang, and not as `Rejected`, which is reserved for an invocation whose body never ran (§4.6, IB-2): a body that ran and failed must not be reported as one that was refused before it started. A failing contribution is logged and audited, and repeated failure is grounds for deactivation — which unwinds its registrations by §4.9 rather than leaving a verb that always fails. *Undefined behavior on contribution failure is forbidden*: every dispatch terminates in an `Outcome`.

**Grant-gated reach (EP-7).** Attaching to the invocable point is a **default-deny, permissioned** capability declared in the contribution's manifest. An extension cannot register a verb it did not declare, and registering an invocable whose effects are security-relevant may require a specific grant beyond the general one. Point reach is capability-scoped, never ambient — an *Active* extension is not thereby entitled to contribute commands.

**Attribution and the interrupt ceiling (EP-10).** Every contributed invocable carries a **core-drawn attribution marker** naming its extension wherever it occupies a user-visible surface: help, completion, discovery listings, palettes, and its rendered output. The marker is drawn by the projection and is **not composable by the contribution**, because a contributed verb that can present itself as the product converts the user's trust in the product into a capability an extension holds. A contributed invocable may interrupt ordinary work and MUST NOT outrank a decision about the session itself — a trust grant, a destructive confirmation, a save-or-discard.

> These three, with EP-11's namespace reservation (§4.4), are what make a contributed verb *safe to be reachable*. Reachability without them is the plugin story with its failure modes left unspecified.

### 4.11 Relationship to extension-authored command definitions

The extension system already defines a **command definition format** — an extension-authored, templated command with argument tokens, documented in `l2-extension-registry`. That format is not a second command concept and does not survive as a parallel path: it is one **producer** of invocables. Loading an extension turns each of its command definitions into a descriptor that registers through the door in §4.3, exactly as a core invocable does (EP-12), and it is thereafter indistinguishable in reachability — it appears in every surface's projection — and fully distinguishable in provenance, through the qualified identity of §4.4 and the attribution marker of §4.10.

The alternative, letting that format keep its own resolution path, would recreate the fork this spec closes: two kinds of command, discoverable in different places, with only one of them reaching the terminal UI and the desktop.

### 4.12 Redaction at the boundary

Output masking is applied **once**, at the dispatch boundary, through the core redaction path. No surface re-implements it and no surface may skip it.

This is a correction of a live asymmetry, and the correction has two independent halves. The first is placement: today two of three surfaces mask and one does not. The second is that all three are fed an empty secret list, so the property is presently inert everywhere it exists. Moving redaction to the boundary fixes the first half; the second requires the core to expose its secret store to the dispatch path and is tracked as a distinct obligation, not as something this move has already achieved.

### 4.13 Journaling a dispatch

`[ADDED v1.1.0]` EP-9 requires the live wiring to be inspectable; that covers *what is registered*. What actually **ran** is a separate record, and it is the one an operator needs when reconstructing what a session did.

A dispatch that resolved is journaled as a **pair** — one record when the invocation enters its handler, one when it settles — joined by an identity minted at dispatch. The pair, not a single record, is what makes an unsettled dispatch visible: a lone entry record is a dispatch that never returned, which is precisely the state a single combined record cannot represent because it is only written on completion.

| Rule | Reason |
| --- | --- |
| Only a **resolved** invocation is journaled | An unknown name entered no handler and did nothing; journaling it fills the record with typing mistakes |
| The pairing identity is unique **across process restarts** over one resumed journal | A per-process counter repeats after a restart and silently pairs a new dispatch with an old one |
| A failure to write the **entry** record fails the dispatch loudly | A dispatch that runs unrecorded is the one case the journal exists to prevent |
| A failure to write the **settlement** record on an already-failing dispatch is contained | The handler's own failure must stay the reported one; a journal problem must not replace it |
| An invocable may **decline to journal its raw input** | Its arguments may carry a secret, or an authoritative domain record may already own the payload |

The last row is the INV-7 half of this section and it is a *declaration on the descriptor*, not a redaction pass over the journal. Redaction (§4.12) masks values it has been told about; an invocable whose entire argument is a credential cannot rely on that, because the store may not know the value. Declaring that this invocable's input is not journaled removes the class rather than filtering it — and, unlike a redaction rule, it is stated at the invocable by the person who knows what the argument is.

## 5. Implementation Notes

Ordering is constrained by SP-9 — the primitive precedes the second consumer — and by the rule that convergence never travels with correction (SP-10).

1. **Descriptor, envelope, and rejection types** in the ports tier, with the optional serialization feature. No consumer yet.
2. **Registry and dispatch** in the domain tier; core invocables registered from the facade through the public door.
3. **Command line onto the projection.** Behavior-preserving. Two things land with it and neither is a behavior change: the unshipped groups leave the default surface (INV-9), and residuals are recorded at the invocables that own them.
4. **Corpus and first registrations** — the command line registers as a consumer here, before the second projection exists.
5. **Terminal UI onto the projection**; the hand-copied verb mirror is deleted and tombstoned; the UI registers with the corpus as it lands.
6. **Single entry point** — the terminal UI becomes a verb of the one binary rather than a second one.
7. **Desktop shell onto generic dispatch**; the per-capability command list is deleted and tombstoned; host-owned settings stay, now declared.
8. **Disclosed corrections**, each separately: the swallowed store error, the ignored output-format flag, the unescaped structured output, and the empty secret list.

## 6. Drawbacks & Alternatives

- **A runtime surface loses compile-time exhaustiveness.** A statically derived command enum is checked by the compiler; a registry is checked by its own tests. The trade is forced rather than chosen — a compile-time surface cannot accept a contribution that arrives after the build — and the conformance corpus is what buys the assurance back.
- **Startup does work that used to be a constant.** Building the parser from descriptors costs time on every invocation of a tool whose whole appeal is being cheap to run. The registry is read-mostly and the cost is bounded by the descriptor count; if it stops being negligible, the projection is cacheable, and that is an optimization rather than a design change.
- **An optional feature on a dependency-free tier is a compromise.** It keeps the default build honest and gives one consumer what it needs, but the tier can no longer be described as unconditionally dependency-free — only as dependency-free by default. The alternative, a shell-owned transfer type, was rejected because it restores the hand-written mapping this spec exists to remove.
- **Alternative — grow the capability trait into the full contract.** Rejected. It is a compile-time surface and forecloses contributed invocables exactly as the command enum does, while also requiring every surface to be recompiled for every capability.
- **Alternative — let each surface keep its own mapping and add a linting check.** Rejected by §4.2 of the parent: a structural check cannot see two correct-shaped answers that differ, which is the entire defect class.
- **Alternative — generate all surfaces from one declaration.** Rejected as over-reach, on the parent spec's own reasoning: it buys parity by surrendering the platform-idiomatic presentation each surface exists to provide. The catalog takes the part that pays — shared vocabulary — and leaves presentation to the surface.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[PARITY]` | `.design/main/specifications/l1-surface-parity.md` | Parent; SP-1…SP-11, and §4.2's account of what each check cannot see |
| `[POINTS]` | `.design/main/specifications/l1-extension-points.md` | EP-2/EP-4/EP-9/EP-11/EP-12/EP-13 — the seam rules this registry realizes |
| `[BINDING]` | `.design/main/specifications/l1-input-binding.md` | IB-1…IB-5 — binder declaration and the rejection model |
| `[ARCH]` | `.design/main/specifications/l1-architecture.md` | INV-3 parity, INV-7 secrets, INV-9 honesty incl. declared retirement |
| `[TOPOLOGY]` | `.design/main/specifications/l2-crate-topology.md` | Tier rules governing where descriptor types and registry logic may live |
| `[CORPUS]` | `.design/main/specifications/l2-surface-conformance.md` | The sibling that proves what this spec makes possible |
| `[LAUNCH]` | `.design/main/specifications/l1-launch-handoff.md` | LH-1/LH-5 — the launcher grammar that bounds what §4.7.1 may generate |

## Document History

| Version | Date | Notes |
| --- | --- | --- |
| 1.1.1 | 2026-09-23 | Consistency pass (2026-09-23): Descriptor gains `reach: AgentReachable \| HumanOnly` (REA-1: an undeclared capability is agent-reachable by default, and the catalog feeds agent-facing instructions) — authority-plane verbs are HumanOnly, refused for agent callers (SEC-10); pending realization. §4.10: a contributed invocable that overran or panicked resolved as `Rejected`, which §4.6 reserves for a body that never ran (IB-2) — now `Unavailable`, attributed to the contribution. |
| 1.1.0 | 2026-09-06 | Amendment after a study of how comparable products actually organize a command surface. Six changes, each closing a gap between this spec and its own L1 parents. **§4.2** — the descriptor is the projectable face and the handler is held separately, so the executable/projectable split is structural rather than a rule every serialization site must remember (SP-12); contributed descriptor text is validated and detached into a normalized owned copy at registration, refused rather than repaired (EP-14). **§4.3** — registration returns the exact effect that reverses it, which is EP-13's *ownership is automatic, never remembered* made real; the previous shape had no disposer at all. **§4.4** — the identity grammar is enforced at construction (EP-11 said *validated at load*; nothing enforced it), and the EP-4 collision rule is now actually declared: qualified ids cannot collide, the core wins the bare form, the loser is **shadowed rather than displaced** and stays reachable by qualified identity, and the shadowing is reported rather than silent. **§4.5** — resolution is split from outcome: `Unknown` is not an `Outcome`, because the three surfaces act on it in three incompatible ways (fall through, usage error, refresh a stale catalog) and v1.0.0's folding of it into `Unavailable` forced one of them to behave incorrectly (SP-13). **§4.7.1** — the command line is projected in two halves: `Semantic` verbs generated from descriptors, `Installation` verbs declared in the frontend's own closed grammar, because an installation verb must be answerable when the composition it configures is exactly what has not come up (LH-1/LH-5); §4.8 gains the `Installation` locus this requires. **§4.9/§4.13** — the registry announces its own mutation with individually contained observers, since a projection built at startup goes stale the moment an extension activates; and a resolved dispatch is journaled as a paired entry/settlement record, with an invocable able to decline journaling its raw input, which removes a secret-bearing argument class that redaction cannot reach. |
| 1.0.0 | 2026-09-05 | Initial spec. Realizes the SP-11 action catalog as a **runtime invocable registry** in the core, dispatched once as `Invocation → Outcome`, with every surface rendering a **projection** rather than a restatement — making INV-3 parity structural instead of asserted. Registration uses **one published door** for core and contributed invocables (EP-12), qualified identities over a reserved core namespace (EP-11), declared collision resolution (EP-4), lazy binding (EP-9), and reversible registration (EP-13), which together make a plugin-contributed verb reachable from surfaces that predate the plugin. Argument schemas derive from ordered typed binders as their single source (IB-1); rejections are typed first-class outcomes with four located modes (IB-3/IB-4), making the empty-versus-unavailable confusion unrepresentable. `Outcome` is structured data, so one dispatch serves text, structured output, terminal widgets, and IPC without re-derivation; the existing push-channel seam carries streams. `locus` declares host-only and client-local actions as exclusions rather than omissions (SP-8/SP-11), and `stability` carries INV-9 in both directions — unshipped actions have no descriptor, retired ones keep a discoverable migration path. Redaction moves to the dispatch boundary, with the inert-empty-secret-list half recorded as a separate obligation rather than claimed as fixed. Records the three constraints that decided the design: dependency-free ports tier (serialization as an optional feature), compile-time command surfaces that cannot host runtime contributions, and all three frontends linking the core in-process with the desktop's IPC seam lying inside that frontend. Post-Update Review added §4.10 **contribution safety** — the three seam rules a `contribute` point must declare and this spec had left implicit: bounded, contained failure resolving to a rejection attributed to the contribution (EP-6); default-deny, grant-gated point reach, so an *Active* extension is not thereby entitled to contribute verbs (EP-7); and a **core-drawn attribution marker the contribution cannot compose away**, without which a contributed verb is indistinguishable from a core one and converts trust in the product into a capability an extension holds (EP-10) — and §4.11 reconciling the extension system's existing **command definition format**, which becomes one *producer* of invocables registering through the same door rather than a parallel resolution path that would reach only one surface. |
