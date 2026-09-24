# Extension Points

**Version:** 1.3.1
**Status:** Stable
**Layer:** concept

## Overview

The technology-agnostic model of **where and how the core is extended** — the seam side of the plugin architecture. Where [l1-extensions.md](l1-extensions.md) defines the *artifact* (what a plugin is, its kinds, trust, and load lifecycle), this concept defines the *seam*: the **extension point**, a named, versioned place in the core at which an extension may contribute behavior, and the uniform grammar by which every aspect of the project — the office, the board, automation, memory, the wiki, roles, model serving, navigation, version control, security — exposes such seams.

The design follows the **microkernel (plugin) architecture** proven across the industry (editor contribution points, platform extension points, action/filter hook systems, tapped build pipelines): a small, stable core that owns behavior and publishes a *closed taxonomy* of extension-point kinds; extensions attach only at declared points, never by reaching into internals; and the core mediates every contribution and composes competing ones deterministically. The result is the mechanism the request asks for — plugins for **all aspects** of the project, **scalable** (N plugins cost nothing until their point is reached) and **flexible** (any subsystem gains extensibility by declaring points in the one shared grammar), without the core fragmenting into a dozen bespoke plugin systems.

Extensions and extension points are duals: an *extension* carries one or more **contributions**; each contribution binds to exactly one **extension point** whose kind it matches. This spec owns the extension-point half.

## Related Specifications

- [l1-extensions.md](l1-extensions.md) - The artifact model (kinds, registry, lifecycle, default-deny trust, sandbox, manifest, attestation — EXT-1…11) that an extension carrying contributions obeys; extension points are the seams those artifacts attach to. The two compose: EXT owns the *what*, this owns the *where/how*.
- [l2-plugin-hooks.md](l2-plugin-hooks.md) - A concrete realization of the *observe* and *decide* point kinds at actor-lifecycle and bus-event boundaries (preStop/postStop/on-event); one instance of this model, not its whole.
- [l1-tool-composition.md](l1-tool-composition.md) - Toolkits are a *command/contribution* case (a tool added to the tool surface) plus the composition discipline this spec generalizes across all point kinds.
- [l1-architecture.md](l1-architecture.md) - The layered core is the microkernel that owns the points (EP-1); command parity (INV-3) makes a contributed CLI verb, TUI slash, and library method one operation.
- [l1-security.md](l1-security.md) / [l1-action-gating.md](l1-action-gating.md) - Attaching to a point is a permissioned, sandboxed capability (EP-7); a *decide* point that gates an effect inherits the action-gating discipline.
- [l1-automation-pipeline.md](l1-automation-pipeline.md) - Pipeline trigger/action nodes are *command/contribution* and *provide* points; an automation extends the office through this model.
- [l1-roles.md](l1-roles.md) / [l1-model-runtime.md](l1-model-runtime.md) - Roles and model providers are *provide* points — the core selects one contributed implementation by a declared policy.
- [l1-office-fabric.md](l1-office-fabric.md) - A lens is a *provide*/*contribution* point on the fabric; a plugin can contribute a new lens through the same grammar.
- [l1-extension-marketplace.md](l1-extension-marketplace.md) - Distributes the extensions whose contributions bind here; distribution never bypasses the point contract.
- [l1-surface-parity.md](l1-surface-parity.md) - [ADDED v1.1.0] SP-1's "one decision point per observable behavior" is what EP-12's dogfooding rule protects at the seam: a host that keeps a private registration path has two decision points for one behavior, and the public one is the one that rots.
- [l1-attention-steering.md](l1-attention-steering.md) - [ADDED v1.1.0] AST-6's attention economy is the budget EP-10's interrupt ceiling rations; a contribution that can interrupt without a ceiling spends a resource that belongs to the human, not to the point.
- [l1-scoped-capability-layers.md](l1-scoped-capability-layers.md) - [ADDED v1.2.0] The visibility half of this model: EP declares *where* a contribution attaches, SCL declares *whose view it lands in*. EP-11 prevents a contribution shadowing a core name; SCL-4 is the deliberate scoped opposite, a same-name replacement inside one actor's view only. SCL-2 is what makes EP-13's ownership automatic.
- [l1-composition-layering.md](l1-composition-layering.md) - [ADDED v1.2.0] EP-4 composes contributions at one point; LAY composes the *set of contributors* one level up, and LAY-7 is what keeps a distributed unit's insertions overridable by the consumer.

## 1. Motivation

Cronus already lets capabilities plug in (skills, MCP servers, plugins, connectors — EXT). But "add a capability" is only half of an extensibility story. The other half is **the set of seams the core exposes** — the places a contribution can actually change behavior, across every subsystem. Today that half is implicit and uneven: one subsystem grew a narrow runtime hook surface (actor lifecycle + bus events), others have no declared seam at all, and there is no shared answer to "at which points, in what shape, may an extension contribute — and what happens when two extensions contribute to the same one?"

Three failures follow from leaving the seam side unmodeled:

- **Bespoke, non-uniform extensibility.** If each subsystem invents its own way to be extended, the project accretes a dozen incompatible plugin mechanisms; a plugin author learns a different contract per aspect, and the core cannot reason about extensibility as one thing. The best-practice answer is a *single contribution grammar* every aspect uses.
- **Undefined composition.** When two plugins want to influence the same decision — reorder the same board, both answer the same routing question, both transform the same prompt — an unmodeled system resolves it by accident (load order, last-writer-wins), which is neither deterministic nor safe. World-class plugin systems make composition a first-class, *declared* discipline: side-effect hooks fan out, value hooks form a pipeline, provider hooks select one, decision hooks bail on the first answer.
- **Fragile core / no evolution path.** Without the seam being a *versioned contract*, either the core cannot change (every internal edit risks breaking plugins) or it breaks them silently. The proven remedy is to treat each point as a semantically-versioned API surface: stable within a compatible range, migrated explicitly across a breaking one.

The resolving idea is to lift the microkernel pattern to a first-class concept: a stable core that publishes **declared, versioned extension points** in a **closed kind-taxonomy**, mediates every contribution, composes them **deterministically**, and activates them **lazily**. Extensions attach only there. That is what makes plugins reach *all aspects* while the core stays coherent, safe, and able to grow.

## 2. Constraints & Assumptions

- An extension point is a property of the **core/subsystem**, declared by it; extensions discover points, they do not invent them. A contribution cannot attach where no point is declared.
- The point-kind taxonomy is **closed** (a small fixed set); adding a *new kind* is a core change, not something a plugin does. Subsystems add *points*, never new kinds.
- Contributions are **untrusted** and reach only what their manifest declares; all trust, sandbox, and grant rules of [l1-extensions.md](l1-extensions.md) / [l1-security.md](l1-security.md) apply unchanged. This spec adds the *where*, not a new trust model.
- Composition must be **deterministic**: the same installed set and inputs yield the same result. Ordering is *declared*, never incidental to install or load order.
- The model is **additive to l1-extensions**: it does not change what an extension is; it names the seams contributions bind to and how they compose.
- A non-technical client never wires points by hand; the office manages contributions and asks only at permission gates (OFF-6). The point model is an internal architecture the office operates on the client's behalf.

## 3. Core Invariants (Layer 1 only)

Rules every Layer 2 implementation MUST NOT violate:

- **EP-1 (Declared seams only — no reaching into internals):** the core is extended **only** at named extension points a subsystem explicitly declares. A contribution attaches to a declared point through that point's contract and MUST NOT reach a subsystem's internal state or call another subsystem's code directly (no monkey-patching, no back doors). The seam is the only door; where no point is declared, behavior is not extensible.

- **EP-2 (Closed, uniform point-kind taxonomy):** every extension point is exactly one of a **closed set of kinds**, and the *same* set is used by every aspect of the project. The kinds are: **observe** (react to an event with side effects, cannot change the outcome), **transform** (receive a value and return a possibly-modified one, chainable), **provide** (supply a named implementation the core selects among), **contribute** (add a new invocable surface — a command/verb, tool, node, menu item, lens), and **decide** (answer a decision, able to short-circuit it). A point declares its kind; a contribution MUST match it. No subsystem invents a private, non-conforming extension mechanism (anti-sprawl).

- **EP-3 (Core-mediated invocation):** the core invokes every contribution at its point; a contribution never invokes another contribution directly and never holds the kernel's internals. A contribution influences behavior solely through its point's declared input/output contract, so one contribution can neither corrupt another nor the core. The core is the sole mediator.

- **EP-4 (Deterministic composition & declared precedence):** when several contributions bind the same point, the core composes them **deterministically** by the point kind's discipline — *observe*: all run, isolated, in a deterministic order; *transform*: a stable-order pipeline where each output feeds the next; *provide*: exactly one selected by a declared policy with deterministic tie-breaking; *contribute*: all added, collisions on the same invocable name resolved by a declared rule; *decide*: first non-abstaining decision in declared order wins. Ordering is **declared** (explicit priority and/or before/after constraints), never a function of install or load order. Same points + same inputs ⇒ same composition. `[MODIFIED v1.3.0]` A *contribute* point's declared collision rule MAY resolve by **scope** rather than by precedence — two entries sharing a name in different scopes are not a collision at all, and the narrower one **shadows** the wider one within its scope instead of displacing it. Shadowing and displacement are different outcomes and the rule must say which it means: the shadowed entry remains registered, remains reachable by its qualified identity, and reappears the moment the narrower scope ends, whereas a displaced one is simply gone. A rule that leaves this unstated will be read as displacement by whoever debugs it and as shadowing by whoever wrote it.

- **EP-5 (Versioned seam contract — stable, explicitly evolved):** every extension point is a **versioned contract** (its kind, input/output shape, and guarantees). The core MUST NOT break a published point within a compatible version range; a breaking change is a new point version with an announced migration, never a silent shape change. A contribution declares the point version it targets; a target the core cannot satisfy is **refused before activation** (composing EXT-2), never silently mis-bound to an incompatible seam.

- **EP-6 (Fail-isolated contributions — the kernel survives a bad plugin):** a contribution's error, timeout, or misbehavior is contained at its point under the point's **declared failure policy** — *observe*/*transform* skip the offender and continue (fail-forward), *provide*/*decide* fall through to the next candidate — and is logged and audited. A contribution MUST NOT be able to crash the core, hang a point unbounded (points are time/step-bounded), or silently corrupt an outcome. Every point declares its failure policy; "undefined behavior on plugin failure" is forbidden.

- **EP-7 (Least-privilege, declared reach):** a contribution reaches only the points and capabilities its manifest declares; attaching to a point is itself a **default-deny, permissioned** capability (composing EXT-3/EXT-4/EXT-6). A plugin cannot bind a point it did not declare, and a point MAY require a specific grant to contribute to (e.g., a *decide* point that gates a security-relevant effect). Point reach is capability-scoped, never ambient.

- **EP-8 (Every aspect extensible through the one model):** extensibility is uniform across the project — each subsystem exposes its seams as declared points of the common taxonomy (EP-2), so no aspect is extended through a privileged, non-conforming mechanism, and a **new subsystem gains extensibility by declaring points**, not by adding a parallel plugin system. The architecture scales by adding points within the one model; the set of *aspects* is open, the set of *kinds* is closed.

- **EP-9 (Lazy activation & observable wiring):** a contribution is bound and its host extension loaded **lazily** — only when its point is reached or its declared activation trigger fires (composing EXT-2) — so installed-but-unused contributions cost nothing (scalability). The complete live wiring — which contributions bind which points, in what order, which provider is selected, which decision won — is **inspectable and auditable** at any time (composing EXT-8), so the office and the client can see exactly how the core is currently extended.

- **EP-10 (Attributed contribution, and an interrupt ceiling):** `[ADDED v1.1.0]` any contribution that occupies a **user-visible surface** — a prompt, a dialog, a notice, a rendered region — carries a **durable attribution marker** naming the contributing extension, drawn by the core and not composable by the contribution. Two things follow. A contribution **cannot present itself as the product**: a prompt that could impersonate the host turns the trust the user has in the product into a capability an extension holds. And a contribution's interruption sits under a **declared ceiling** — it MAY interrupt ordinary work, and MUST NOT outrank or pre-empt a decision **about the session itself** (a trust grant, a save-or-discard, a destructive confirmation). Interruption requests are queued in arrival order and settled by request identity, so a duplicated confirmation cannot spill onto whatever was queued behind it.

- **EP-11 (Reserved core namespace; a contribution's identity is the namespace it owns):** `[ADDED v1.1.0]` every named thing a contribution introduces — an invocable name, a surface id, a configuration table, an event name — is **qualified by the contributing extension's identity**, and the core's own names live under a **reserved identity no contribution may claim**. The split is structural, not conventional: it guarantees a contribution can never shadow a core name **whatever the core adds later**, which a first-registration-wins rule alone does not. Identities are validated at load (a well-formed name that cannot make a qualified id ambiguous), and where two sources claim one identity the **first in the declared source order** loads and the later is refused with a notice naming it — one identity cannot own two configuration tables.

- **EP-12 (The core contributes through its own public seam):** `[ADDED v1.1.0]` where the core ships an implementation of something a contribution could also supply — a provider, a surface, an invocable, a transform — that implementation **registers through the published point contract**, not through a private path. There is no privileged registration door. This is the only mechanism that keeps a seam honest: a contract exercised solely by third parties is a contract nobody notices has become insufficient, while a core implementation running through it fails loudly the moment the published surface stops being enough. Core-supplied contributions MAY differ in exactly three declared ways — they are loaded eagerly where ordering demands it, they are implicitly trusted, and they survive the switches that disable third-party contributions — and in no other way; the failure-isolation and composition rules (EP-4/EP-6) apply to them unchanged.

- **EP-13 (Registration is a reversible effect; teardown reaches quiescence, not merely requests it):** `[ADDED v1.2.0]` every contribution a component makes — a contributed invocable, a provider, a listener, a transform — **and** every resource it acquires outside the point contract is a **reversible effect owned by the registering component**, unwound when that component is deactivated, reloaded, or loses a capability it required. Three rules make the unwind real rather than nominal. **Ownership is automatic, never remembered**: a registration made through the seam is unwound *by* the seam, and a resource acquired outside it is wrapped so it unwinds identically — an unwind that depends on a component remembering to unregister is one that will be incomplete, and its omissions are invisible until the second load. **Teardown awaits quiescence**: deactivation completes only once the work it stopped has actually stopped; a teardown that issues a stop and returns leaves orphans that outlive the component that owned them and are attributable to nothing. **Ordering is declared where it matters**: unwinding proceeds in reverse registration order, but independent asynchronous unwinds may proceed **concurrently**, so a sequence-sensitive teardown is registered as **one** effect owning its internal order — never as several effects that happen to have been registered in the right sequence. Observation registries are closed **before** the thing they observe is stopped, so a late completion arrives at no listener rather than at a half-torn-down one. This is what turns EXT-2's *deactivate* from a label into a state, and it is the precondition for replacing a contribution in place without restarting the core.

- **EP-14 (Contributed metadata is validated and detached at the seam — and so is what a contribution returns):** `[ADDED v1.3.0]` everything a contribution **declares about itself** — its name, its human-readable text, its argument schema, the bounds of each — is validated against the point's contract **at registration**, and what the core retains afterwards is a **normalized copy the contributor can no longer reach**. The same holds in the other direction: a contribution's **return value** is validated and detached at the boundary before any consumer sees it. Two failures this closes, and neither is visible from the contribution's own side. A descriptor that passed validation and was then **mutated in place** leaves the core rendering text no check ever saw — the validation happened, and it proved nothing about what is now displayed. A malformed return that flows through unchecked reaches a renderer, a log, or a wire **wearing the core's authority**, and the resulting defect is attributed to the core rather than to the extension that produced it. Validation **refuses** the registration or the return, naming what was wrong; it never silently repairs, truncates, or substitutes a default, because a repaired declaration is one whose author will never learn it was invalid. Bounds are part of the contract, not a downstream concern: an unbounded string in a descriptor is a contributor-controlled quantity that must be constrained where it enters, not where it is finally serialized.

> L2 specs cannot reach RFC status until all invariants here are addressed in their "Invariant Compliance" section.

## 4. Detailed Design

### 4.1 The microkernel stance

The core owns behavior and publishes seams; everything pluggable enters through a declared point the core invokes (EP-1, EP-3).

```mermaid
graph TD
    subgraph CORE["Stable core (microkernel)"]
        SUB1["Subsystem A<br/>declares points"]
        SUB2["Subsystem B<br/>declares points"]
        MED["Point mediator<br/>invoke · compose · isolate"]
    end
    SUB1 -->|declared point| MED
    SUB2 -->|declared point| MED
    EXT1["Extension · contribution"] -->|binds declared point only| MED
    EXT2["Extension · contribution"] -->|binds declared point only| MED
    MED -->|deterministic composition EP-4| SUB1
    MED -->|fail-isolated EP-6| SUB2
```

An extension never touches a subsystem directly; it hands a contribution to the mediator, which invokes it at the point and folds its result back under the kind's composition rule.

### 4.2 The point-kind taxonomy (EP-2)

One closed grammar, reused by every aspect. Each kind carries its own composition and failure discipline (world-practice analog shown for orientation only; the contract is the columns, not the analog).

| Kind | Contribution does | Composition (EP-4) | Failure policy (EP-6) | Analog |
| --- | --- | --- | --- | --- |
| **observe** | reacts with side effects; cannot change the outcome | all run, isolated, deterministic order | skip offender, continue | action hook / event listener |
| **transform** | maps a value → possibly-modified value | stable-order pipeline, chained | skip offender (pass value through) | filter hook / waterfall tap |
| **provide** | supplies a named implementation | exactly one selected by declared policy | fall through to next candidate | provider / strategy |
| **contribute** | adds a new invocable surface | all added; name collisions resolved by declared rule | reject the malformed contribution | contribution point |
| **decide** | answers a decision, may short-circuit | first non-abstaining answer in declared order wins | fall through to next decider | bail hook / chain of responsibility |

Choosing the *kind* is the design act that keeps composition safe: a plugin that must not change an outcome is given an *observe* point (it physically cannot), a plugin that legitimately shapes a value gets *transform*, and a plugin that answers a gate gets *decide* under a grant (EP-7).

### 4.3 Composition & precedence

Precedence is always **declared**, never inherited from load order (EP-4).

```text
[REFERENCE]
Each contribution declares: point-id, target point-version, priority?, before/after?
The core topologically orders contributions on a point by (priority, before/after constraints),
breaking any residual tie by a stable deterministic key (e.g., extension identity), NOT by load order.

observe   → run every contribution in that order, each isolated (one failing does not stop the rest)
transform → thread the value through the order: v = c_k(...c_2(c_1(v)))
provide   → evaluate the selection policy over candidates in that order; bind exactly one
decide    → ask each in order; the first that returns a decision (not "abstain") wins; else core default
contribute→ register each invocable; on same-name collision apply the declared rule (namespaced / refused)
```

Determinism is the load-bearing property: the same installed set and inputs must always compose the same way, so behavior is reproducible and auditable (EP-9) rather than an artifact of what happened to load first.

### 4.4 The seam as a versioned contract (EP-5)

A point is an API the ecosystem depends on. It is semantically versioned:

```text
[REFERENCE]
point "board.card.ordering" v1  (kind: transform; in: card list; out: reordered card list)
  compatible change  → v1.x  (add optional context; old contributions keep binding)
  breaking change    → v2     (shape changes; v1 contributions do NOT auto-bind)
                              core publishes v2 alongside v1 for a migration window, then retires v1
A contribution targeting v1 that meets only a v2 core is refused at activation (EP-5), surfaced — never silently bound to v2.
```

This is what lets the core evolve without breaking the plugin ecosystem, and lets plugins written once keep working — the scalability guarantee the request calls for.

### 4.5 Aspects that expose extension points

"For all aspects" is concrete: every subsystem publishes seams in the one grammar. Representative (non-exhaustive; new subsystems add points the same way, EP-8):

| Aspect | Example point | Kind |
| --- | --- | --- |
| Office / orchestration | on-hire, on-delegate; a custom staffing strategy | observe; provide |
| Kanban board | card transition, column ordering; a custom column/board | observe/transform; contribute |
| Automation pipeline | a new trigger source or action node | contribute; provide |
| Memory | pre-store enrichment, recall re-ranking | transform |
| Wiki (knowledge lens) | page-render augmentation, grounding source | transform; provide |
| Model serving | a model provider selected by policy | provide |
| Roles | a role definition offered to the office | provide/contribute |
| Navigation / office fabric | a new lens or sidebar subsystem | contribute/provide |
| Version control | pre-commit quality gate | decide |
| Security / effects | an authorization decision on an effect | decide (grant-gated, EP-7) |
| CLI / tool surface | a new verb / tool | contribute (parity INV-3) |

Each row is the *same* mechanism, differing only in point kind — a board reorder is a *transform*, a model provider is a *provide*, a commit gate is a *decide*. The office never learns a per-aspect plugin dialect.

### 4.6 Relationship to l1-extensions and l2-plugin-hooks

- **l1-extensions (the dual).** EXT owns the artifact: kinds, registry, discover→grant→activate lifecycle, sandbox, manifest, attestation. This spec owns the seam a contribution binds. An extension's manifest (EXT-9) declares *which points* its contributions target and *at what version* (EP-5) — the manifest is where the two halves meet.
- **l2-plugin-hooks (one realization).** The existing hook system is exactly the *observe* and *decide* kinds instantiated at actor-lifecycle and bus-event points, with its fail-forward and deterministic-order rules being the EP-4/EP-6 disciplines for those points. It is one conforming instance; this L1 generalizes its narrow surface to the whole core. (Aligning that L2 to also declare this parent is a downstream planning reconciliation, mirroring how EXT-10/EXT-11 were carried by their L2 implementers.)

### 4.7 Scalability & flexibility

- **Scalability.** Lazy activation (EP-9) means installed-but-unused contributions are free; the cost is paid only when a point is reached. Determinism (EP-4) keeps behavior stable as the installed set grows. Versioned seams (EP-5) let the ecosystem grow without lockstep upgrades.
- **Flexibility.** Any aspect becomes extensible by declaring points in the one taxonomy (EP-2/EP-8); the five kinds cover the full range from passive observation to authoritative decision, so a subsystem can open exactly the amount of extensibility it should — no more (a *decide* point is a deliberate, grant-gated choice), no less (an *observe* point costs a subsystem almost nothing to publish).

## 5. Drawbacks & Alternatives

- **Taxonomy rigidity.** A closed five-kind set (EP-2) cannot express every conceivable seam. Accepted deliberately: the closure is what keeps composition and safety tractable; a genuinely new kind is a considered core change, not a plugin's prerogative. The five kinds span observe→transform→provide→contribute→decide, which cover the field in practice.
- **Declared-ordering burden.** Requiring declared precedence (EP-4) is more work than "just run them," but incidental load-order composition is exactly the non-determinism world-class systems eliminate; the burden buys reproducibility and auditability.
- **Versioning overhead.** Treating every point as a versioned contract (EP-5) is heavier than an ad-hoc seam, justified by the alternative — a core that either cannot change or breaks plugins silently.
- **Alternative — keep bespoke per-subsystem hooks.** Rejected (EP-8): it fragments extensibility into incompatible dialects and defeats "one mechanism for all aspects."
- **Alternative — let plugins patch internals directly (open core).** Rejected (EP-1/EP-3): direct reach makes plugins mutually corrupting and the core impossible to evolve; the mediated seam is the whole point of the microkernel pattern.
- **Alternative — fold this into l1-extensions.** Rejected: EXT is already a large spec about the artifact and its trust; the seam model spans every subsystem's composition and versioning and deserves its own altitude (the same reasoning that keeps tool-composition and marketplace as siblings of EXT, not sections of it).

## nodus-relevance mapping

Nodus already embodies this model's principles at the DSL grain; the main-workspace concept generalizes them to the whole host.

| Element | nodus seam | Note |
| --- | --- | --- |
| *provide* points (EP-2) | host-supplied providers: `ModelProvider`, `AuditProvider`, `SchemaProvider`, `StorageProvider` (nodus LP-2/LP-8) | The runtime declares provider seams the host fills — *provide*-kind points, selected/injected by the host. |
| Closed kind taxonomy (EP-2) | closed flag/validator/type registries (NL vocab) | Nodus already forbids open-ended private mechanisms; a capability is declared, not improvised. |
| Versioned seam contract (EP-5) | vocabulary schema versioning + `@needs` selective disclosure (NL-16) | A workflow targets a declared, versioned vocabulary slice; incompatible use is a fail-fast validation error. |
| Least-privilege reach (EP-7) | declared capability resolved before run (LP-8) | Binding a seam is a pre-declared, pre-validated capability, never ambient. |
| Deterministic composition (EP-4) | deterministic execution/rendering (NL-6) | Same inputs + same declared set ⇒ same result. |

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[EXTENSIONS]` | `.design/main/specifications/l1-extensions.md` | The artifact model this pairs with; the manifest is where a contribution names its target points (EP-5, EXT-9) |
| `[PLUGIN-HOOKS]` | `.design/main/specifications/l2-plugin-hooks.md` | A conforming realization of the *observe*/*decide* kinds (§4.6) |
| `[ARCH]` | `.design/main/specifications/l1-architecture.md` | The layered core (microkernel) that owns the points; command parity for *contribute* (INV-3) |
| `[SECURITY]` | `.design/main/specifications/l1-security.md` | Default-deny, sandbox, grants a point contribution inherits (EP-7) |
| `[ACTION-GATING]` | `.design/main/specifications/l1-action-gating.md` | The gate discipline a security-relevant *decide* point composes |
| `[TOOL-COMP]` | `.design/main/specifications/l1-tool-composition.md` | Toolkits as a *contribute*+composition case this generalizes |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.3.1 | 2026-09-24 | Core Team | Consistency pass (2026-09-24): Bare `LP-n` citations of the nodus portability contract are now written `nodus LP-n`: this workspace's `l1-lookahead-planning` defines LP-1…LP-6 as well, so the bare form pointed a reader at the wrong invariant. No requirement changed. |
| 1.3.0 | 2026-09-06 | Core Team | EP-14 added — **contributed metadata is validated and detached at the seam, and so is what a contribution returns**. EP-5 versions the seam's *shape* and EP-6 contains a contribution's *failure*, but nothing governed the contributor-controlled **data** that crosses the seam in both directions. Two failures follow and neither is visible from the contribution's side: a descriptor validated and then mutated in place leaves the core rendering text no check ever saw, and an unchecked return reaches a renderer, a log, or a wire wearing the core's authority, so the defect is attributed to the core. Validation refuses and names; it never silently repairs, because a repaired declaration is one whose author never learns it was invalid. Bounds belong at entry, not at serialization. EP-4 amended: a *contribute* point's collision rule MAY resolve by **scope**, where the narrower entry **shadows** rather than displaces — the two outcomes differ (a shadowed entry stays registered, stays reachable by qualified identity, and returns when the scope ends) and the rule must state which it means. |
| 1.2.0 | 2026-08-19 | Core Team | EP-13 added — **registration is a reversible effect; teardown reaches quiescence, not merely requests it**. EP-1…EP-12 governed where a contribution attaches, how it composes, and how it is named, but never what happens when it goes away — leaving EXT-2's *deactivate* a label rather than a state, and making in-place replacement of a contribution unspecifiable. Three rules make the unwind real: **ownership is automatic, never remembered** (the seam unwinds what the seam registered, and resources acquired outside it are wrapped to unwind identically, because an unwind depending on a component's memory is incomplete and its omissions are invisible until the second load); **teardown awaits quiescence** (a stop that is issued and returned from leaves orphans that outlive their owner and are attributable to nothing); and **ordering is declared where it matters** — reverse registration order, but independent asynchronous unwinds proceed concurrently, so a sequence-sensitive teardown is **one** effect owning its internal order rather than several that happen to have been registered in sequence — with observation registries closed **before** the observed thing is stopped, so a late completion arrives at no listener instead of a half-torn-down one. Links the new `l1-scoped-capability-layers` (SCL-2 — the registration site drives visibility and lifetime as one fact, which is what makes ownership automatic) and `l1-composition-layering` (LAY-10 — transactional recomposition depends on this unwind being complete). Distilled from an adoption pass over an external plugin-framework-based agent-harness reference. Additive; no existing invariant weakened. |
| 1.1.0 | 2026-08-13 | Core Team | Added EP-10/EP-11/EP-12 — the three rules that govern a contribution's *presence* rather than its invocation, none of which EP-1…EP-9 reached. **EP-10 (attributed contribution + interrupt ceiling)**: a contribution occupying a user-visible surface carries a core-drawn attribution marker it cannot compose away, because a prompt able to impersonate the host converts the user's trust in the product into a capability an extension holds; and its interruption sits under a declared ceiling — it may interrupt ordinary work, never a decision *about the session itself* — with requests queued in arrival order and settled by request identity so a duplicated confirmation cannot spill onto the next one. **EP-11 (reserved core namespace)**: every name a contribution introduces is qualified by the contributor's identity and the core's names live under a reserved identity nobody may claim, which is what guarantees no shadowing *whatever the core adds later* — a guarantee first-registration-wins does not provide; identity collisions resolve by declared source order with a notice, since one identity cannot own two configuration tables. **EP-12 (the core contributes through its own public seam)**: a core-shipped provider/surface/invocable registers through the published contract with no privileged door, because a contract exercised only by third parties is one nobody notices has become insufficient; core contributions differ in exactly three declared ways (eager load, implicit trust, surviving the third-party disable switch) and in no other, with EP-4/EP-6 applying unchanged. Links `l1-surface-parity` (SP-1 — a private registration path is a second decision point) and `l1-attention-steering` (AST-6 — the attention budget EP-10 rations). |
| 1.0.0 | 2026-07-24 | Core Team | Initial spec — the extension-point (seam) model, dual to l1-extensions: the core is extended only at declared, mediated seams (EP-1/EP-3); a closed, uniform point-kind taxonomy (observe / transform / provide / contribute / decide) used by every aspect (EP-2); deterministic composition under declared precedence (EP-4); versioned seam contracts with pre-activation refusal of incompatible targets (EP-5); fail-isolated contributions under declared failure policies (EP-6); least-privilege, declared, grant-gated point reach (EP-7); every aspect extensible through the one model, new subsystems add points not parallel systems (EP-8); lazy activation and observable/auditable live wiring (EP-9). Microkernel/contribution-point best-practice synthesis; composes l1-extensions (artifact) / l2-plugin-hooks (one realization) / l1-security / l1-architecture. Main-only host architecture concept. |
