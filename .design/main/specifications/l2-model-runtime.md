# Model Runtime (Transport & Provider Connectivity)

**Version:** 1.0.3
**Status:** Stable
**Layer:** implementation
**Implements:** l1-model-runtime.md

## Overview

The concrete realization of the local model runtime for the Rust workspace: the **transport
layer** that turns a routing decision into an actual inference call. It supplies the
streaming inference surface the workspace does not yet have. Two provider seams exist today
but neither serves a call: the contract's `ModelProvider` trait is **routing metadata**
(`id`/`health`/`context_window`/`cost`/`latency`/`tier`/`task_fit` — the properties the
router scores, with no generate method at all), and the workflow runtime's `ModelProvider`
trait is a minimal **synchronous** surface (`generate(prompt) -> String`, `analyze` — no
streaming, embeddings, or model introspection) satisfied only by a deterministic test stub.

This spec defines the missing piece: a new **inference trait** in the contract crate (the
streaming call surface), a concrete endpoint-profile provider that implements it *alongside*
the routing-metadata facet, and a host-side adapter that satisfies the nodus generate/analyze
trait by collapsing the stream to a `String` — all over the six federated local REST
providers from the technology-stack catalog and explicitly egress-gated remote APIs.

Model *selection* (router scoring, hardware fit) is specified and implemented and
credential-lane routing is specified (`l2-model-router` RTG-10), while no component can
actually reach a model — the workspace deliberately contains no HTTP transport today.
This L2 defines that transport: synchronous-first, thread-scoped, streaming-capable, and
cancellable, without introducing an async runtime into the core.

## Related Specifications

- [l1-model-runtime.md](l1-model-runtime.md) - The L1 parent (MR-1…MR-16) this spec realizes.
- [l2-technology-stack.md](l2-technology-stack.md) - §4.4 local provider catalog (six REST providers, probe discipline, endpoints) this transport federates to.
- [l2-model-router.md](l2-model-router.md) - Selection, scoring, and hardware-fit that sit *above* this transport; the router picks, this spec calls.
- [l2-model-error-recovery.md](l2-model-error-recovery.md) - Error taxonomy and retry/rotate/fallback applied to transport failures.
- [l2-crate-topology.md](l2-crate-topology.md) - The contract · domain · infra-crate · facade partition this transport crate slots into (DN-2 seam as a crate boundary).
- [l1-security.md](l1-security.md) - Egress gate and secret isolation governing remote backends and credentials.
- [l1-routing.md](l1-routing.md) - RTG-10 credential lanes carried through to the transport's authentication headers.

## 1. Motivation

Every model-consuming subsystem (orchestration, memory generators, triage, compaction,
nodus workflows) targets a provider seam, and every seam currently dead-ends: the contract
trait exposes only routing metadata (no call method at all), and the workflow runtime's
`generate`/`analyze` trait is satisfied only by a deterministic test stub. The router can
*decide* which model should serve a request, record credential lanes, and estimate hardware
fit — and then no seam can perform the streaming call. This is the single gap that keeps the
office's autonomy inert end to end.

The transport must respect two standing architectural facts: the workspace is synchronous by
design (the stack's provider probes are *parallel thread-scoped*, not async), and the
dependency policy is std-first — a third-party crate is added only when strictly necessary.
An HTTP+TLS client is such a necessity; an async runtime is not.

## 2. Constraints & Assumptions

- **No async runtime.** The core stays synchronous. Concurrency is bounded worker threads
  owned by the runtime component; streaming is delivered through a pull-based iterator /
  callback contract, not futures.
- **Minimal transport dependency.** One blocking HTTP client with rustls-based TLS
  (candidate: `ureq`-class), justified under the dependency policy as strictly necessary.
  No websocket/gRPC dependency — the entire provider catalog is REST.
- **Federated local serving.** Cronus does not reimplement weight storage or model serving
  in v1: it federates to installed local servers (the §4.4 catalog) that own acquisition,
  storage, and loading. The Cronus-native content-addressed store remains a later,
  spec-gated extension.
- **nodus stays dependency-free.** The bridge adapter that implements the workflow runtime's
  provider trait lives host-side (facade tier), delegating to the contract provider. nodus
  itself gains no dependency (nodus LP-1 preserved).
- **Remote backends are opt-in.** Any non-loopback endpoint requires the security egress
  gate and a credential from the secret store; loopback providers require neither.

## 3. Invariant Compliance (Layer 2 only)

| L1 Invariant | Implementation |
| --- | --- |
| MR-1 Local-first serving | Default provider set = the six loopback REST providers (stack §4.4); a non-loopback endpoint is constructible only through the egress-gated remote-backend path with an explicit user grant; no silent remote fallback — a failed local call surfaces through error-recovery, never re-routes off-device on its own. |
| MR-2 Provider-abstracted backends | The transport defines a new **inference trait** in the contract crate (`generate_stream`, `embed`, `describe`, `pull`, residency hints) — the streaming call surface neither existing trait provides. A concrete endpoint-profile provider implements this inference trait **and** the routing-metadata `contract::ModelProvider` (so the router can score it) — two facets of one object; adding a provider is a new endpoint profile, never a caller change. The nodus `ModelProvider` (`generate`/`analyze` → `String`) is satisfied by a host-side adapter that calls the inference trait and collapses the stream to a `String`. |
| MR-3 Content-addressed model store | **Delegated (v1):** the federated local server owns manifests/blobs; the transport surfaces each provider's catalog (name, digest, size) through `describe`. A Cronus-native store is a disclosed deferral with an upgrade trigger (self-managed serving backend), per the frugality discipline. |
| MR-4 Verifiable named acquisition | **Delegated (v1):** pull-by-name is forwarded to providers that support it, progress-streamed to the caller; digest reported back post-pull. Acquisition is explicit (a `model pull` capability), never a hidden stall inside a generate call — a missing model fails fast with the acquisition hint. |
| MR-5 Portable model definition | **Delegated (v1):** definition-based customization forwards to the provider's definition mechanism where present; the transport records the resolved definition digest in `describe` output so a customized model stays reproducible. |
| MR-6 Explicit load/unload lifecycle | Load/unload hints map to the provider's residency controls (e.g. keep-alive/TTL parameters); resident-model introspection surfaces via the provider status endpoints; where a provider lacks a control, the transport reports the capability as absent rather than emulating it silently. |
| MR-7 Fit-gated hardware scheduling | The feasibility gate stays in the router's hardware-fit module; the transport contributes honest inputs (model size/quant from `describe`) and refuses a load the gate rejected. |
| MR-8 Streaming inference contract | Generate/chat/embed over the industry-compatible `/v1` shape (and provider-native shapes behind the same trait); incremental output is a pull-based token/event iterator fed by a worker thread; cancellation is a first-class handle (atomic flag + connection drop) honored mid-stream. |
| MR-9 Catalog management | List/details/remove forward to provider catalog endpoints through the same contract surface clients use; no manual file surgery — a provider without a management endpoint reports the gap explicitly. |
| MR-10 Managed server with observability | Federated servers are supervised, not owned: reachability probes (800 ms thread-scoped, per stack §4.4), health + resident state surfaced to the doctor and process-monitor planes; start/stop of provider processes is deferred to the doctor's authorized repair path. |
| MR-11 Thin clients over the server | CLI/TUI/GUI bind `model` verbs to the library methods only (`model list/status/pull/describe`); no frontend holds model state; offline provider ⇒ graceful degraded listing, marked unreachable. |
| MR-12 Versioned, reproducible references | Every completed call records the serving provider, model name, and digest (when the provider reports one) into the operational record, making "which weights answered" a stored fact. |
| MR-13 Multi-device placement | Delegated to the router's hardware-fit scoring; the transport passes through per-device residency reported by the provider where available and never fabricates placement data. |
| MR-14 Calibrated, honest estimates | The transport reports only measured facts (latency, tokens/s observed per call) tagged as measurements; estimates remain the router's output; the two are never merged in one field. |
| MR-15 Degrade before refusing | **Pending.** The router's hardware-fit layer already names placements below the fastest (`CpuOffload`, `CpuOnly`, `l2-model-router` §4.9); considering them before an MR-7 refusal, stating the tier's cost as a calibrated estimate before the run, and recording the tier used on the run's record (MR-12) are to be realized. A placement never changes outputs; one that would is a different configuration, not a tier. |
| MR-16 Feasibility is a running property | **Pending.** The feasibility estimate is to be driven by declared resource characteristics (resident footprint, active fraction, per-token state growth, load-transient peak) carried with the model definition — an absent one lowers confidence, never counts as zero — and crossing the boundary mid-run is to re-enter the managed response (reduce the working set, move to a named tier, or stop cleanly), never a crash or silent truncation. The router's §4.9 memory estimate is the current, single-scalar input. |

> This table must be complete before the spec can reach RFC status.

## 4. Detailed Design

### 4.1 Crate placement and seam wiring

A new infra crate implements the contract seam, mirroring the store-local/auth-local
pattern from the crate topology:

```plaintext
crates/
├── contract/       # ModelProvider (routing metadata, unchanged) + NEW InferenceBackend trait (streaming call surface)
├── model-local/    # NEW: REST transport implementing contract::InferenceBackend
├── domain/         # consumes both traits — no transport dependency
└── core/ (facade)  # wires model-local in; hosts the nodus bridge adapter
```

The contract crate **gains** the inference trait; the routing-metadata `ModelProvider` stays
exactly as-is (the two traits describe different facets — score vs. call — of one provider).
Dependency direction stays inward: `model-local → contract`; `domain` depends on the traits,
never on `model-local`; the facade composes them. Adding this crate extends the topology the
crate-topology spec enumerates: implementation of this spec MUST be accompanied by a
crate-topology amendment registering `model-local` as a peer infra crate (same tier and
rules as `store-local`/`auth-local`) and by the corresponding CI boundary-guard update —
the guard's failure path is part of this spec's acceptance, mirroring how the topology
was landed. The nodus bridge adapter lives in the facade and
implements the workflow runtime's provider trait by delegating to the wired
`contract::InferenceBackend` (the call surface — `contract::ModelProvider` is routing
metadata with no generate method) — nodus keeps zero dependencies, and workflow `gen`
steps run against real models the moment the facade is wired.

### 4.2 Synchronous streaming contract

```plaintext
[REFERENCE] — shape, not code
generate(request, cancel: CancelHandle) -> TokenStream
  TokenStream: blocking pull-iterator of Event { Token | ToolCall | Usage | Done | Error }
  CancelHandle: atomic flag; set → worker drops the connection, iterator yields Error(Cancelled)
```

A worker thread owns the HTTP connection and parses the provider's stream format (SSE or
NDJSON) into events pushed to a bounded channel; the caller pulls. Backpressure is the
bounded channel; a slow consumer never grows memory unbounded. One request = one worker;
total transport concurrency is capped by a small pool sized from the router's
concurrency budget.

The nodus bridge consumes this **same** stream and concatenates its `Token` events into the
`String` that `nodus::ModelProvider::generate` returns — streaming internally, blocking at
the nodus boundary. This is why the nodus trait needs no streaming variant: the blocking
`String` return is a projection of the stream, not a second call path.

An `Error` event — a dropped connection, a provider failure, a cancellation — is never folded
into text. Text that arrived before the error is a fragment, and handing it back as the
model's answer would let a workflow continue on a truncated plan or an empty result as though
the call had succeeded. The call fails, and the step fails into its declared error handler
(`l1-workflow-language` WFL-8). **Pending:** `nodus::ModelProvider::generate` returns a bare
`String` and so has no failure channel; the trait gains a fallible return (nodus stays
dependency-free, nodus LP-1), and until it does the shipped bridge returns the partial text on an
error — a known gap, not the intended behaviour.

### 4.3 Provider endpoint profiles

Each catalog provider is an **endpoint profile**: protocol family (`/v1`-compatible or
provider-native), capability flags (streaming, embeddings, pull, residency control, digest
reporting), and probe rules — data, not code. New providers are added by profile. Probes
follow the stack's discipline: thread-scoped, 800 ms timeout, loopback-only by default,
wildcard binds rejected.

The per-model endpoint **address** is not re-invented here: the router policy already carries
`api_base` per model (and maps the on-device runtime to the `inference.local/v1` virtual
host, per `l2-model-router`). The profile adds only the *how-to-talk* layer — protocol
family, capability flags, probe rules — over that address; it is not a parallel endpoint
registry, and `api_base` stays the single source of truth for *where* a model lives.

### 4.4 Credentials and egress

Loopback profiles carry no credentials. A remote profile resolves its credential from the
secret store at call time (never cached in config), attaches it per the profile's auth
scheme, and is constructible only after the security egress grant for that endpoint. The
credential lane chosen by the router (RTG-10) is carried in the request metadata so the
transport uses the lane the router priced.

The transport does **not** own credential rotation or the credential pool: multi-key rotation
stays in `l2-model-error-recovery`, and per-key/account cooldown stays in `l2-model-router`
(Connection Cooldown). The transport only *attaches* the single credential for the lane the
router already selected, and reports an auth failure back onto the error-recovery taxonomy
(§4.5) so those upstream layers decide rotation — no auth-state machine is duplicated here.

### 4.5 Failure surface

The transport maps wire failures onto the error-recovery taxonomy (connect-refused,
timeout, 4xx, 5xx, malformed-stream, cancelled) and never retries internally — retry,
rotate, and fallback belong to the error-recovery layer so policy stays in one place.

### 4.6 Frontend bindings

| Action | CLI | TUI | Library |
| --- | --- | --- | --- |
| list providers + reachability | `cronus model status` | `/model status` | `models.status()` |
| list available models | `cronus model list` | `/model list` | `models.list()` |
| model details (digest, size, params) | `cronus model describe <name>` | `/model describe <name>` | `models.describe(name)` |
| acquire by name | `cronus model pull <name>` | `/model pull <name>` | `models.pull(name, progress)` |

Per the shipped-surface honesty rule, these verbs appear on a frontend only once bound.

## 5. Implementation Notes

1. Endpoint profiles + probe reuse — the stack §4.4 probe logic is the discovery half; this
   crate adds the call half.
2. `/v1`-compatible generate path with streaming + cancellation — unlocks the largest
   provider subset first.
3. Facade wiring: contract provider into the engine; nodus bridge adapter.
4. Provider-native paths (pull, residency control) per capability flag.
5. Remote profiles behind the egress gate — last, after the local path is proven.

## 6. Drawbacks & Alternatives

- **Federation defers MR-3/4/5 depth:** v1 trusts the local server's store instead of a
  Cronus-native content-addressed store. Accepted as a disclosed deferral: the L1 contract
  is honored at the boundary (digests surfaced, acquisition explicit), and a native store
  remains a clean later extension behind the same trait.
- **Blocking transport limits massive fan-out:** a thread per in-flight stream caps
  practical concurrency well below an async design. Accepted: the product is single-user
  and on-device; the router's concurrency budget is the real ceiling, not the transport.
- **Alternative — adopt an async runtime now:** rejected; it would ripple `async` through
  the synchronous core and contradict the established thread-scoped probe design for no
  present need.
- **Alternative — one bespoke client per provider:** rejected; the catalog converges on
  REST with two protocol families, so profiles-over-one-client is strictly less code.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[CONTRACT]` | `crates/contract/src/lib.rs` | The routing-metadata `ModelProvider` trait, and the crate that gains the new `InferenceBackend` trait this transport implements |
| `[NODUS]` | `crates/nodus/src/executor.rs` | The workflow-runtime `ModelProvider` (`generate`/`analyze` → `String`) the host-side bridge adapter satisfies |
| `[STACK]` | `.design/main/specifications/l2-technology-stack.md` | Provider catalog, endpoints, and probe discipline (§4.4) |
| `[ROUTER]` | `.design/main/specifications/l2-model-router.md` | Selection, hardware-fit, `api_base` address, and per-key cooldown sitting above this transport |
| `[RECOVERY]` | `.design/main/specifications/l2-model-error-recovery.md` | Error taxonomy the failure surface maps onto; owner of multi-key rotation |

## Document History

| Version | Date | Notes |
| --- | --- | --- |
| 1.0.3 | 2026-09-24 | Consistency pass (2026-09-24): §4.2 said nothing about `Error` events in the nodus bridge, and the shipped bridge returns the text received before an error as a successful generation — an error is never folded into text; the call fails into the workflow's error handler (WFL-8). The fallible nodus `generate` this needs is recorded as pending. Bare `LP-n` citations of the nodus portability contract are now written `nodus LP-n`: this workspace's `l1-lookahead-planning` defines LP-1…LP-6 as well, so the bare form pointed a reader at the wrong invariant. No requirement changed. |
| 1.0.2 | 2026-09-23 | Consistency pass (2026-09-23): The nodus bridge was said to delegate to `contract::ModelProvider`, which is routing metadata with no generate method — it delegates to the inference trait this spec defines. The Overview called credential lanes implemented (they are specified, RTG-10). Compliance gains MR-15 and MR-16 (Pending) and the parent range reads MR-1…MR-16. |
| 1.0.1 | 2026-07-16 | Post-Update Review correction (Stable): the initial draft wrongly claimed the transport "implements `contract::ModelProvider`" with generate/embed/describe — but that trait is **routing metadata** (no call method), and the only real generate surface is nodus's synchronous `generate`/`analyze` → `String`. Corrected the seam model: the transport defines a NEW `InferenceBackend` trait in the contract crate; a concrete provider implements it *plus* the routing-metadata facet; the nodus trait is satisfied by a stream-collapsing bridge. Also clarified two seams against Stable neighbors — endpoint profile consumes the router's `api_base` (not a parallel address registry), and credential rotation stays in router/error-recovery (transport only attaches the selected credential). Promoted RFC→Stable. |
| 1.0.0 | 2026-07-16 | Initial RFC — closes the audit-identified transport gap: synchronous streaming REST transport crate (`model-local`) over the federated §4.4 provider catalog; host-side nodus bridge adapter; endpoint profiles; credential-lane + egress-gated remote path; MR-1…MR-14 compliance table (MR-3/4/5 delegated-with-disclosure in v1). |
