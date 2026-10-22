# Execution Locus

**Version:** 1.1.2
**Status:** Stable
**Layer:** concept

## Overview

The model of **the world a capability acts in, and the rule that capabilities sharing a world are substituted together**. A *locus* is one coherent execution world: a filesystem, a process table, a network position, and the identity that acts in them, all mutually consistent. The claim this spec makes is narrow and load-bearing: **the locus is the unit of substitution, never the individual capability.**

The reason is that the world-touching capabilities are not independent. A file reader, a shell, a persistent terminal, a language server, a file watcher, and a code-intelligence indexer all observe and mutate the *same* filesystem and the *same* process table. Substituting one of them — pointing the shell at a remote machine while the file reader stays local — does not produce a remote-capable system. It produces a system whose tools disagree about what exists: the agent reads a file that the command it just ran cannot see, and the failure surfaces three steps later as a nonsensical result rather than as a configuration error.

Modeled correctly, the payoff is the opposite and it is large: because the world-touching capabilities are bound to a locus rather than to an implementation, pointing the locus at a remote sandbox, a container, or another machine moves **all of them together, with no forked implementations** — the shell, the terminal, the language server, and the indexer follow, because each one asks the locus rather than the host.

## Related Specifications

- [l1-execution-sandbox.md](l1-execution-sandbox.md) - **Orthogonal and composed** (LOC-7): the sandbox says *how confined* execution is on each axis; the locus says *which world* it happens in. A locus may be confined or not; confinement never identifies a locus, and selecting a locus never relaxes an axis.
- [l1-environment-lifecycle.md](l1-environment-lifecycle.md) - Provisioning, status, and end-of-life of an environment **instance** (EL-1…EL-10). That spec owns the instance's existence over time; this owns which capabilities are bound to it and how they move. An instance is a locus; a locus need not be an instance (the operator's own machine is a locus nobody provisioned).
- [l1-architecture.md](l1-architecture.md) - INV-4's hub-and-spoke topology and INV-8's sanctioned process boundaries; a locus boundary is one of the boundaries INV-8 sanctions and never a licence for new ones.
- [l1-code-execution.md](l1-code-execution.md) / [l1-browser-control.md](l1-browser-control.md) / [l1-code-intelligence.md](l1-code-intelligence.md) - Three consumers of the world; each must resolve its locus rather than assume the host.
- [l1-reproduction-recipe.md](l1-reproduction-recipe.md) - A reproduction is only valid within a stated locus; LOC-5's stamping is what makes the statement checkable.
- [l1-tool-receipts.md](l1-tool-receipts.md) / [l1-change-attribution.md](l1-change-attribution.md) - An effect's record names where it happened; a receipt without a locus is ambiguous the moment more than one locus exists.
- [l1-scoped-capability-layers.md](l1-scoped-capability-layers.md) - How a per-actor world is selected: locus selection is a composition-time contribution (LOC-8), landing in the actor's scope layer like any other.
- [l1-process-monitor.md](l1-process-monitor.md) - Renders the confined execution hosts as named processes; LOC-5's observability is what lets that view name the locus each one serves.
- [l1-remote-liveness-verdict.md](l1-remote-liveness-verdict.md) - [ADDED v1.1.1] RLV-8 states the rule this spec's substitution model assumes but does not itself enforce: two live locus registrations MUST NOT name the same underlying resource. RLV also supplies what a locus's liveness-reporting capability must return once connectivity to it is lost — a question LOC-6's binary refuse-or-serve does not by itself resolve for a remote locus whose channel, rather than its capability set, is what became unavailable.

## 1. Motivation

Cronus is designed to run work somewhere other than the operator's machine — in a sandbox, in a provisioned environment, on a hub while the operator sits at a spoke. Every one of those is a change of *world*, and the change is only sound if the whole set of world-touching capabilities changes with it.

Three failures follow from treating each capability as independently swappable.

**Split-world incoherence.** The agent runs a command in one world and reads a file in another. Nothing errors. The tools return results that are individually correct and jointly impossible: a build that succeeded produced no artifact, a file the agent just wrote is missing, a process it started does not appear. The agent, reasonably, concludes the tool is broken and falls back to a manual workaround — [l1-agent-tool-ergonomics.md](l1-agent-tool-ergonomics.md) ATE-1's failure, arriving from a configuration defect rather than a tool defect.

**Provider forks.** If remoteness is a property of each capability, then every capability needs a remote variant: a remote shell, a remote file reader, a remote terminal, a remote indexer. That is N implementations of one idea, drifting independently, each with its own view of paths, identity, and errors. Binding them to a locus instead makes remoteness one implementation that all of them consume.

**Unattributable evidence.** A cached observation, an index, a receipt, or a test result carries a claim about the world. When more than one world exists, an observation that does not name its world is not evidence — it is a fact about *somewhere*. This is invisible while there is one locus and silently wrong from the day there are two.

The resolving idea is to make the locus a **declared, first-class, observable identity** that world-touching capabilities are bound to and that moves as one.

## 2. Constraints & Assumptions

- A locus is defined by the **coherence** of what it contains, not by its location: two capabilities are in one locus when a mutation by either is observable by the other. Local versus remote is a property some loci have, not what a locus is.
- Loci are **few and long-lived** relative to operations. This models a world an operation runs in, not a per-call routing decision (LOC-8).
- The set of world-touching capability kinds is **open** — new ones are added over time — which is precisely why the binding must be to a locus rather than enumerated per kind.
- Nothing here grants authority. A locus is not a permission and selecting one never relaxes a confinement axis (LOC-7).
- Cross-locus operation is expected and normal; what is forbidden is cross-locus operation that is *implicit*.

## 3. Core Invariants (Layer 1 only)

Rules every Layer 2 implementation MUST NOT violate:

- **LOC-1 (Every world-touching capability declares its locus; none assumes the host):** a capability that reads or mutates a filesystem, spawns or observes a process, holds a terminal, watches for change, or indexes any of these **declares the locus it acts in** and resolves that locus at use. No such capability may default to the host it happens to be running on. A capability that assumes the host is one that cannot be moved, and it is the one that silently breaks every other capability's world.

- **LOC-2 (The locus is the unit of substitution; co-resident capabilities move together):** capabilities bound to one locus are substituted **as a set**. Pointing a locus elsewhere moves every capability bound to it; substituting one capability's world while its co-residents stay is forbidden, because the result is a set of tools that individually succeed and jointly describe an impossible world. There is no per-capability locus override.

- **LOC-3 (A handle is locus-bound; crossing is an explicit transfer, never a reinterpretation):** any reference produced by a locus — a path, a process identity, a port, a handle, a working root — is **valid only within that locus** and carries its locus with it. Using such a reference in a different locus is an **explicit transfer** with a named mechanism and a recorded cost, never an implicit reinterpretation of the same string against a different world. A path that silently means two different things in two loci is the split-world failure in its smallest form.

- **LOC-4 (Moving the locus must not fork its consumers):** a consumer of a world-touching capability is written **once**, against the capability's contract, and works in every locus that supplies it. Introducing a new kind of locus MUST NOT require a parallel implementation of the consumers — remote variants of a shell tool, a file tool, an indexer. Where a locus cannot supply a capability, the answer is LOC-6's visible refusal, not a forked consumer.

- **LOC-5 (The active locus is observable and stamped onto what it produces):** which locus an operation ran in is **inspectable at any time** and is **recorded on every artifact that makes a claim about a world** — effect receipts, observations, cached indexes, test and reproduction results, diagnostics. An observation, index entry, or receipt that does not name its locus is not evidence about any world, and MUST NOT be treated as evidence about the current one. Where an artifact's locus differs from the active one, it is presented as an artifact from elsewhere, never silently as a local fact.

- **LOC-6 (A capability absent from the active locus refuses visibly; it never falls back to another):** where the active locus supplies no provider for a required capability, the outcome is a **visible, reasoned refusal naming the locus and the capability**. Falling back to another locus — most temptingly, to the host — is forbidden: it produces exactly the split world the model exists to prevent, and it does so at the moment the system appears to be recovering gracefully. The refusal follows the ergonomics rules for an unavailable capability: absent from the affordance surface where the absence is static, and refused with a legible reason where it is discovered at use ([l1-agent-tool-ergonomics.md](l1-agent-tool-ergonomics.md) ATE-2/ATE-3).

- **LOC-7 (Confinement composes with a locus; it never defines one):** the confinement axes of [l1-execution-sandbox.md](l1-execution-sandbox.md) apply **within** a locus and are orthogonal to it. A locus may be tightly confined or entirely unconfined; "sandboxed" is not a locus and "remote" is not a confinement level. Selecting or moving a locus MUST NOT relax any axis, and confining a locus MUST NOT change which world it is. Conflating the two produces the reasoning that a remote world is inherently safe, which is a security claim nobody made.

- **LOC-8 (Locus selection is a composition decision, not a per-call argument):** which locus an actor's capabilities are bound to is decided when the actor's world is composed, and it is stable for the operations that world serves. It is not a per-call parameter that a model or a tool argument may set. Two reasons bind it: a per-call locus makes coherence unenforceable — nothing can guarantee two calls in one task shared a world — and it hands the choice of *where effects land* to the least accountable participant in the system.

- **LOC-9 (A locus change invalidates world-derived state rather than silently reusing it):** state derived from a world — indexes, caches, read-before-write records, discovered inventories, resolved paths — is **keyed by locus** and is invalidated, re-derived, or explicitly carried over when the locus changes. Silently reusing world-derived state across a locus change is forbidden; it is the mechanism by which a stale local index answers questions about a remote world with total confidence.

- **LOC-10 (A locus declares the properties that change how it must be used):** a locus declares the operational facts a correct consumer needs — whether its world is shared with the operator's own, whether it survives the operation that created it, its round-trip cost class, and what happens to its contents when it ends. These are not implementation details: a consumer that batches its calls, one that streams, and one that preserves a residue behave differently by necessity, and a locus that hides these properties forces every consumer to be tuned for the locus it was written against, which reintroduces LOC-4's fork through the back door.

- **LOC-11 (One decider per application; every surface is *delivered* the default locus, never derives it):** `[ADDED v1.1.0]` the application-wide default locus is resolved in exactly **one** place and distributed to every window, panel, and surface. No surface computes it independently, and exactly **one** user gesture changes it — every other surface reads. Where N surfaces each derive the default from the same inputs, they drift the moment those inputs change at different times, and the product develops several simultaneous opinions about where work runs, each internally consistent. This is LOC-8's composition-time selection given a cardinality: composition-time is *when*, and a single decider is *how many*.

- **LOC-12 (Two distinct binding disciplines — a preference that follows, and a binding that never moves):** `[ADDED v1.1.0]` a surface's association with a locus is one of exactly two kinds, and which one it is MUST be declared rather than left to behaviour. A **preference** (a pin) resolves to its target while that target can serve and to the application default while it cannot, so a surface whose preferred locus becomes unavailable **auto-follows** and returns on its own when the locus is usable again; nothing is lost because a preference holds no locus-bound state. A **binding** is fixed for the container's lifetime: any container holding live locus-bound handles (LOC-3) — an open session, a terminal, a running view — binds its locus at creation and **never** migrates, because re-pointing it would silently reinterpret every handle it holds against a different world. Moving such a container to another locus is a **clone**: a new container is created there and the original is left intact. Availability for a binding is established when the container is created, not polled continuously — a bound container whose locus later becomes unreachable reports that state (LOC-6) rather than silently re-binding.

> L2 specs cannot reach RFC status until all invariants here are addressed in their "Invariant Compliance" section.

## 4. Detailed Design

### 4.1 What is in a locus

```plaintext
locus  ── one coherent world ──────────────────────────────────┐
  filesystem      · paths mean one thing                       │
  process table   · a process started here is visible here     │
  network position· what is reachable, from what address       │
  acting identity · whose permissions the effects carry        │
───────────────────────────────────────────────────────────────┘
        ▲             ▲            ▲            ▲          ▲
   file access   shell/exec   persistent    watchers   language
                              terminals                servers,
                                                       indexers

    all bound to the locus (LOC-1) → all move together (LOC-2)
```

The membership test is behavioral, not topological: **two capabilities are in one locus when a mutation by either is observable by the other.** A container sharing a mounted directory with the host is one locus for that directory and a different one for its process table — which is exactly the kind of partial sharing that must be *declared* (LOC-10) rather than discovered by an agent whose file write did not appear where its command looked.

### 4.2 The failure LOC-2 prevents

| Step | Split world (each capability swapped independently) | One locus (LOC-2) |
| --- | --- | --- |
| agent runs a build | remote shell: succeeds | in-locus shell: succeeds |
| agent lists artifacts | local file reader: empty | in-locus reader: artifacts present |
| agent concludes | "the build silently failed" | correct result |
| agent's next action | rebuild, or fall back to manual steps | continues |
| what a reviewer sees | a plausible, wrong narrative | nothing to review |

The right column is not "better error handling" — it is the absence of the error. Nothing in the left column errors; every component behaved as written. That is what makes split-world incoherence expensive: it consumes an agent's turns, produces a confidently wrong account, and leaves no failure to attribute.

### 4.3 Substitution without forks

LOC-4 is what turns the model from bookkeeping into leverage. A tool is authored once against the capability contract:

```plaintext
[REFERENCE] shape only — not an implementation
    tool "run_tests":
        shell  := resolve(Shell,      in: active_locus)   // LOC-1
        files  := resolve(FileAccess, in: active_locus)   // same locus, by construction
        result := shell.run("<test command>")
        report := files.read("<report path>")             // guaranteed same world (LOC-2)
```

The tool contains no notion of local or remote. Introducing a new kind of locus supplies new providers for `Shell` and `FileAccess`; the tool is unchanged, and so are the terminal, the watcher, and the indexer. This is the property the model is for: **one substitution, whole-product effect.**

The corollary is LOC-6. When a new locus cannot supply a capability — no persistent terminal, no language server — the honest outcome is that the capability is unavailable *in that locus*, stated as such. A fallback to the host would keep the tool "working" and reintroduce split-world incoherence at the exact seam the locus was introduced to close.

### 4.4 Evidence and the locus stamp

LOC-5 and LOC-9 are one idea seen from two sides: **a claim about a world is only meaningful with the world named.**

| Artifact | Without a locus stamp | With one |
| --- | --- | --- |
| effect receipt | "this file was written" — where? | attributable, and checkable against the world it names |
| code index / cache | answers confidently about a world it may not have seen | re-derived or explicitly carried on a locus change (LOC-9) |
| read-before-write record | may authorize a write against a file it never read *here* | scoped to the world the read happened in |
| reproduction recipe | reproducible "on some machine" | reproducible, with the world part of the recipe |

The stale-index case is the one that bites hardest, because an index is *designed* to answer without touching the world. Keyed by locus, it declines to answer for a world it has not indexed; unkeyed, it answers wrongly and fast.

### 4.5 Demarcation

| Question | Owner |
| --- | --- |
| How confined is execution, on each axis? | `l1-execution-sandbox` (EXS-1…EXS-9) |
| Does this environment instance exist, and what happens when it ends? | `l1-environment-lifecycle` (EL-1…EL-10) |
| Which world does this capability act in, and what moves with it? | this spec |
| Which locus does *this actor* use? | composed into the actor's world (`l1-scoped-capability-layers` SCL-8), per LOC-8 |

An environment instance is a locus; the operator's own machine is a locus that no lifecycle provisioned. The distinction matters because LOC-1 binds *every* world-touching capability, including the ones that run when no environment has been provisioned at all — that default host is a locus with a name, not the absence of one.

### 4.6 Selection across many surfaces (LOC-11 / LOC-12) [ADDED v1.1.0]

LOC-8 settles *when* a locus is chosen. It leaves open two questions that a product with many windows answers badly by default: **how many things decide**, and **what happens to a surface whose locus goes away**.

```text
[REFERENCE]
default_locus        := resolved ONCE per application, delivered to every surface   (LOC-11)
                        changed by exactly one gesture; no surface writes it

surface_association  := preference(target) | binding(locus)                          (LOC-12)

resolve(preference(t)) := t if t can serve, else default_locus     // auto-follows, and returns
resolve(binding(l))    := l, for the container's whole lifetime    // never re-pointed

move a bound container elsewhere := CLONE into the new locus, original left intact
```

**Why one decider.** Each surface deriving the default from shared inputs is locally correct and collectively wrong: the inputs change at different moments in different windows, and the product ends up running work in two worlds while every window believes it is showing the current one. Centralising the decision costs a delivery mechanism and removes the entire class.

**Why a preference follows and a binding does not.** The difference is whether the surface holds locus-bound state. A picker or a filter holds none — its association is a hint, and following the default when its preference cannot serve is strictly better than showing a dead surface. An open session, a terminal, or a running view holds handles that LOC-3 makes valid only in the locus that issued them; re-pointing such a container does not move it, it reinterprets its handles against a world where they mean something else or nothing. Hence **clone-not-migrate**: the honest way to "open this elsewhere" is a second container in that locus, which also keeps LOC-5's stamping truthful — each container's artifacts carry the one world they were produced in.

**Why availability is checked at binding time and not continuously.** Polling every bound container's locus is both expensive and misleading: a transient unreachability would tear down live work that would have recovered. The check belongs where the commitment is made; afterwards, unavailability surfaces through LOC-6's visible refusal on the next operation, which is where the user can act on it.

## 5. Drawbacks & Alternatives

- **Declaring a locus is friction on every world-touching capability.** Every file, process, and terminal capability gains a resolution step it did not have. Accepted: the alternative is that each one independently assumes the host, and the assumption is invisible until a second locus exists.
- **LOC-6's refusal is less helpful than a fallback, in the moment.** A user whose remote locus lacks a language server would rather get local results than nothing. Refused because those results describe a different world, and the user has no way to know that from the output. Where carrying over is genuinely correct, LOC-9's *explicit* carry-over is the sanctioned form.
- **LOC-8 forbids a useful per-call escape.** "Just run this one command locally" is a real request. It is served by composing a second actor bound to the local locus, which keeps the effects attributable, rather than by a parameter that makes any single call's world unknowable from the outside.
- **Rejected — remoteness as a per-capability flag.** The intuitive design, and the direct cause of both split-world incoherence and N forked providers.
- **Rejected — treating the sandbox as the locus.** Convenient, since confinement and remoteness often arrive together, and wrong: it makes "confined" and "elsewhere" the same axis, so relaxing one appears to relax the other, and an unconfined remote world becomes inexpressible.

## nodus-relevance mapping

Nodus already carries the environment as an extension role; the locus rules sharpen what that role must guarantee across a workflow's steps.

| Element | nodus seam | Note |
| --- | --- | --- |
| One world per run (LOC-2) | NE-7 instance isolation, LP-18 environment-liveness seam | A run's effectful steps share one environment instance; two steps of one workflow acting in different worlds is the split-world failure at DSL grain. |
| Declared locus (LOC-1) | LP-8 capability manifest | The environment role is declared and resolved before the run; no step may assume an ambient host. |
| Visible refusal (LOC-6) | NE-10 capability-declared fail-fast, LP-21 declared refusal | A workflow needing an environment the host cannot supply fails before running, naming what is missing. |
| Locus-stamped evidence (LOC-5) | NE-3 trajectory projection, NE-12 archivable candidate result | A graded run's result is a claim about the world it ran in; the archive carries it. |
| Confinement orthogonal (LOC-7) | LP-11 per-effect authorization | Authorization gates an effect; the environment decides where it lands. The two are separate declarations on the same step. |
| Declared locus properties (LOC-10) | NE-6 environment profile, NE-13 fixed resource budget | The profile is where a locus's operational facts already live; round-trip cost class belongs beside the budget. |

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[SANDBOX]` | `.design/main/specifications/l1-execution-sandbox.md` | EXS-1…EXS-9 — the orthogonal confinement axes LOC-7 composes with. |
| `[ENV-LIFECYCLE]` | `.design/main/specifications/l1-environment-lifecycle.md` | EL-1…EL-10 — instance existence over time; demarcated in §4.5. |
| `[ARCHITECTURE]` | `.design/main/specifications/l1-architecture.md` | INV-4 hub-and-spoke, INV-8 sanctioned process boundaries. |
| `[SCOPES]` | `.design/main/specifications/l1-scoped-capability-layers.md` | SCL-8 — where LOC-8's composition-time selection lands. |
| `[ERGONOMICS]` | `.design/main/specifications/l1-agent-tool-ergonomics.md` | ATE-2/ATE-3 — the shape of LOC-6's refusal. |
| `[RECEIPTS]` | `.design/main/specifications/l1-tool-receipts.md` | The effect record LOC-5 stamps. |
| `[NODUS-ENV]` | `.design/nodus/specifications/l1-nodus-environment.md` | NE-6/NE-7/NE-10 — the DSL-grain environment role. |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.1.2 | 2026-10-22 | Core Team | Citations updated for the requirement-ID prefix renames that make each prefix name one specification (RULES §1, 1.8.0, the person's decision of 2026-10-22): `ES-n` of `l1-execution-sandbox` is now `EXS-n`. No requirement was added, removed or reworded. |
| 1.1.1 | 2026-09-01 | Core Team | Links-only — added `l1-remote-liveness-verdict` to Related Specifications: RLV-8 states the same-underlying-resource-one-registration rule this spec's substitution model assumes; RLV covers what a locus's liveness capability returns when its channel, not its capability set, becomes unavailable. No invariant changed. |
| 1.1.0 | 2026-08-20 | Core Team | Amendment — LOC-11 one decider per application: the default locus is resolved in exactly one place and delivered to every surface, changed by exactly one gesture, because N surfaces deriving it from shared inputs drift the moment those inputs change at different times and the product develops several simultaneous, internally consistent opinions about where work runs (LOC-8 gave the *when*; this gives the *how many*). LOC-12 two declared binding disciplines: a **preference** resolves to its target while that target can serve and to the default while it cannot, so a surface auto-follows and returns on its own — safe precisely because a preference holds no locus-bound state; a **binding** is fixed for its container's lifetime because the container holds LOC-3 handles, and re-pointing it would reinterpret every handle against a different world rather than move it, so relocation is **clone-not-migrate** (a second container elsewhere, original intact), which also keeps LOC-5 stamping truthful. Availability for a binding is established at creation, not polled — continuous polling would tear down live work on transient unreachability, while LOC-6's visible refusal on the next operation surfaces it where the user can act. New §4.6. Additive: no existing invariant weakened; L1 stays Stable (C9). Distilled from an adoption pass over an external multi-provider agent-orchestration desktop client with several concurrently reachable execution hosts. |
| 1.0.0 | 2026-08-19 | Core Team | Initial spec — the execution locus: one coherent world (filesystem, process table, network position, acting identity), with **the locus as the unit of substitution, never the individual capability**. Every world-touching capability declares its locus and none assumes the host (LOC-1); co-resident capabilities move as a set, since a shell in one world and a file reader in another produce tools that individually succeed and jointly describe an impossible world — a failure that errors nowhere, consumes the agent's turns, and yields a confidently wrong account (LOC-2); handles are locus-bound and crossing is an explicit transfer, never a reinterpretation of the same string against a different world (LOC-3); moving the locus must not fork the consumers, which is the leverage the model exists for — one substitution, whole-product effect (LOC-4); the active locus is observable and stamped onto every artifact that claims something about a world, an unstamped observation being evidence about nowhere (LOC-5); a capability absent from the active locus refuses visibly and never falls back, since a host fallback recreates the split world at the exact seam the locus was introduced to close (LOC-6); confinement composes with a locus and never defines one, conflating them producing the unmade claim that a remote world is inherently safe (LOC-7); locus selection is a composition decision rather than a per-call argument, because a per-call locus makes coherence unenforceable and hands the choice of where effects land to the least accountable participant (LOC-8); world-derived state is keyed by locus and invalidated or explicitly carried across a change, which is what stops a stale local index from answering confidently about a remote world (LOC-9); and a locus declares the operational properties that change how it must be used — sharing, persistence, round-trip cost, end-of-life disposition — since hiding them tunes every consumer to one locus and reintroduces the fork (LOC-10). Closes the previously-unmodeled execution-host dimension; demarcated from `l1-execution-sandbox` (how confined) and `l1-environment-lifecycle` (does the instance exist) in §4.5. Distilled from an adoption pass over an external plugin-framework-based agent-harness reference. Concept-only. |
