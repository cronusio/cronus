# Model Error Recovery

**Version:** 1.0.3
**Status:** Stable
**Layer:** implementation
**Implements:** l1-routing.md, l1-doctor.md

## Overview

The runtime error-recovery pipeline for model API calls: a structured taxonomy of failure modes, a priority-ordered classification pipeline that produces structured recovery action hints, and the retry/fallback/compress/rotate loop that acts on them.

## Related Specifications

- [l1-routing.md](l1-routing.md) - Routing invariants (fallback cascade, RTG-2).
- [l1-doctor.md](l1-doctor.md) - Self-healing and escalation model.
- [l2-model-router.md](l2-model-router.md) - Model selection and fallback cascade source.
- [l2-agent-session.md](l2-agent-session.md) - Turn loop that calls the classifier per iteration.
- [l2-context-management.md](l2-context-management.md) - The conservative budget applied when a context window is unknown (§4.6).

## 1. Motivation

Model API calls fail in many distinct ways — auth, billing, rate limiting, context overflow, server errors, transport timeouts — each requiring a different recovery action. Scattered inline string matching produces inconsistent behavior as new providers are added. A centralized priority-ordered classifier with structured outputs keeps recovery logic consistent, testable, and extensible.

## 2. Constraints & Assumptions

- The classifier runs on every API error synchronously, before any retry action is taken.
- Classification must be fast: pattern matching only, no model calls.
- Provider-specific patterns are encoded explicitly; new providers extend the pattern lists.
- Context overflow recovery always delegates to the context engine (compress + retry), never a bare retry.
- Billing exhaustion and auth failures trigger credential rotation; only after rotation exhausts all credentials does the cascade advance.

## 3. Invariant Compliance (Layer 2 only)

| L1 Invariant | Implementation |
| --- | --- |
| RTG-2 Fallback | `ClassifiedError.should_fallback` triggers cascade advancement in the retry loop — through the router's eligible candidates only (`l2-model-router` selection pipeline): a failure never moves a request off-device that routing would keep local. |
| HEAL-1 Continuous checks | The classifier runs after every API call before any recovery action; the health probe (§4.6) runs before a turn and on each cascade advance. |
| HEAL-2 Safe self-repair | Deterministic, safe recoveries apply automatically and are recorded: retry with backoff, compression before fallback, credential rotation before abort. |
| HEAL-3 Escalate the risky | What recovery must not decide alone is surfaced instead: a provider or content-policy block, a permanent auth failure, an exhausted cascade. |
| HEAL-5 Traceable | Each classified error (kind, provider, model, recovery taken) is appended to the audit log (SEC-7). |

Scope: the `l1-doctor` rows cover only the invariants model-call recovery realizes. HEAL-4, HEAL-6, HEAL-7 and HEAL-8 are realized by `l2-doctor`, which carries the full HEAL-1…HEAL-8 table.

## 4. Detailed Design

### 4.1 Error taxonomy (FailoverKind)

```text
[REFERENCE]
enum FailoverKind {
  // Auth / authorization
  Auth,               // Transient 401/403 — refresh or rotate credential
  AuthPermanent,      // Auth failed after refresh — abort

  // Billing / quota
  Billing,            // 402 or confirmed credit exhaustion — rotate immediately
  RateLimit,          // 429 or periodic quota throttling — backoff then rotate

  // Server-side
  Overloaded,         // 503/529 — provider overloaded, backoff
  ServerError,        // 500/502 — internal server error, retry

  // Transport
  Timeout,            // Connection/read timeout — rebuild client + retry

  // Context / payload
  ContextOverflow,    // Context too large — compress then retry; never bare-retry
  PayloadTooLarge,    // 413 — compress payload
  ImageTooLarge,      // Image exceeds per-image size limit — shrink and retry

  // Model / provider policy
  ModelNotFound,      // 404 / invalid model — fallback to different model
  ProviderBlocked,    // Aggregator policy or privacy guardrail — no fallback; surface to user
  ContentBlocked,     // Provider safety filter — deterministic refusal; no automatic fallback;
                      //   surface to user (a retry elsewhere only on the user's explicit request)

  // Request format
  FormatError,        // 400 bad request — non-retryable; fallback or abort

  // Catch-all
  Unknown,            // Unclassifiable — retry with jittered backoff
}
```

### 4.2 ClassifiedError — structured recovery output

```text
[REFERENCE]
struct ClassifiedError {
  kind: FailoverKind,
  status_code: Option<u16>,
  provider: String,
  model: String,
  message: String,
  // Recovery action hints — checked by the retry loop, not re-derived
  retryable: bool,
  should_compress: bool,           // Compress context before retrying
  should_rotate_credential: bool,  // Advance to next credential in pool
  should_fallback: bool,           // Advance fallback cascade in model-router
}
```

### 4.3 Classification pipeline (priority-ordered)

The classifier checks these stages in order, returning on the first match:

**1. Provider-specific** patterns (highest priority)

- Content-policy blocks: deterministic refusals by provider safety filters. `retryable: false, should_fallback: false` — the refusal is surfaced, never laundered by re-sending the same request to another provider until one accepts it; the user may explicitly retry elsewhere. Must run before status-based classification so a 400 safety block is not downgraded to a generic `FormatError`.
- Provider-specific subscription/entitlement errors (e.g. expired OAuth tokens, model tier restrictions). Classify as `Auth` with `retryable: false, should_fallback: true`.
- Thinking-block signature invalidity: `retryable: true` (strip reasoning blocks and retry).
- Long-context tier gate: `should_compress: true, retryable: true`.
- Grammar pattern rejection (local inference): `retryable: true` (strip unsupported schema fields).

**2. HTTP status code** + **message-aware refinement**

- `401` → `Auth` (`should_rotate_credential: true, should_fallback: true`)
- `402` → disambiguate billing vs transient quota:
  - usage-limit pattern + transient signal ("try again", "resets at") → `RateLimit`
  - confirmed exhaustion → `Billing` (`should_rotate_credential: true, should_fallback: true`)
- `403` → `Billing` if billing pattern in message, otherwise `Auth`
- `404` → `ProviderBlocked` if policy pattern; `ModelNotFound` if model-not-found pattern; else `Unknown`
- `413` → `PayloadTooLarge` (`should_compress: true`)
- `429` → `RateLimit` (`should_rotate_credential: true, should_fallback: true`)
- `400` → see **400 sub-pipeline** below
- `500/502` → `FormatError` if request-validation pattern (non-retryable to prevent retry floods); else `ServerError`
- `503/529` → `Overloaded` (backoff, retry)
- Other `4xx` → `FormatError` (`retryable: false, should_fallback: true`)
- Other `5xx` → `ServerError` (retry)

**400 sub-pipeline (ordered by specificity)**:

1. Multimodal tool content rejected → strip image parts from tool messages, record (provider, model), retry
2. Image too large → `ImageTooLarge` (shrink and retry)
3. Invalid encrypted replay blob → strip replay state, retry
4. Request-validation pattern ("unknown parameter", "unsupported parameter") → `FormatError, retryable: false` — prevents retry floods on deterministic rejections
5. Context overflow patterns → `ContextOverflow, should_compress: true`
6. Provider/model blocked patterns → `ProviderBlocked` or `ModelNotFound`
7. Rate-limit or billing patterns in body → `RateLimit` or `Billing`
8. Large session + generic short body → `ContextOverflow` heuristic (is_large: `prompt_tokens > context_length * 0.4`)
9. Fallback → `FormatError`

**3. Structured error code from response body**
Match known codes: `resource_exhausted` → `RateLimit`; `context_length_exceeded` → `ContextOverflow`; etc.

**4. Message pattern matching (no status code)**
Billing patterns, rate-limit patterns, context overflow patterns, auth patterns, model-not-found patterns, timeout message patterns.

**5. SSL/TLS transient patterns**
Always classify as `Timeout` — SSL alerts mid-stream are transport hiccups, not context overflow. Run before disconnect check to prevent unnecessary compression on flaky TLS.

**6. Server disconnect + large session**
`is_large: prompt_tokens > context_length * 0.6` → `ContextOverflow, should_compress: true`.
Otherwise → `Timeout`.

**7. Transport error type heuristics**
Connection errors, read timeouts, SSL errors → `Timeout` (rebuild client + retry).

**8. Fallback**
`Unknown, retryable: true` with jittered backoff.

### 4.4 Retry loop contract

```text
[REFERENCE]
Given ClassifiedError:
  if should_compress         → context_engine.compress(); then retry
  elif should_rotate_credential → credential_pool.rotate(); then retry
  elif should_fallback       → model_router.advance_cascade(); then retry
  elif retryable             → jittered_backoff(); retry (max 3)
  else                       → abort turn, surface error to user
```

Each classified error is appended to the audit log (kind, provider, model, recovery action taken, timestamp).

### 4.5 Credential pool (multi-key rotation)

The credential pool manages multiple API keys for the same provider, enabling transparent failover without user intervention:

```text
[REFERENCE]
enum CredentialStatus { Ok, Exhausted, Dead }
// Dead: permanently invalid — token revoked, OAuth invalidated.
//       Excluded from rotation unconditionally.
//       Clears only when fresh credentials are written.

struct CredentialEntry {
  id: String,
  status: CredentialStatus,
  last_used: Option<Timestamp>,
  cooldown_until: Option<Timestamp>,
}

impl CredentialPool {
  rotate() -> Option<CredentialEntry>  // Round-robin across Ok entries
  mark_exhausted(id, cooldown)
  mark_dead(id)
}
```

Terminal auth reasons that immediately mark a credential `Dead` (no cooldown retry): token invalidated, token revoked, invalid grant (refresh token rejected), refresh token reused.

### 4.6 Provider health probe

Before a turn begins — and whenever the model router advances the fallback cascade (§4.4) — the provider is probed to confirm it is reachable and able to serve requests. This prevents wasting turn latency on a provider that is already known-dead.

#### ProviderHealthStatus

```text
[REFERENCE]
ProviderHealthStatus {
  ok:            bool,             // true = provider is healthy and ready
  probed:        bool,             // true = a network probe was attempted (false = cached/skipped)
  provider_label: String,          // human-readable provider name for display
  endpoint:      String,           // probe target URL
  detail:        String,           // diagnostic detail (empty on ok)

  // Failure category when ok = false:
  // "unreachable"  — no network path (DNS failure, connection refused, timeout)
  // "unhealthy"    — reachable but returning errors (5xx, model not loaded)
  // "unauthorized" — reachable but credentials rejected (401/403)
  failure_label:  Option<"unreachable" | "unhealthy" | "unauthorized">,

  // Multi-hop display label: identifies which leg of a proxy chain this probe covers.
  // E.g. "vLLM backend" when the user-facing endpoint is a frontend proxy.
  probe_label:   Option<String>,

  // Sub-probes for multi-hop architectures: the top-level probe is the user-facing
  // endpoint; subprobes are its backend components. Each follows the same schema.
  subprobes:     Vec<ProviderHealthStatus>?,
}
```

#### Multi-hop probing

When the configured endpoint is a proxy or aggregator (e.g. an on-device gateway proxying to a local inference backend), a single probe at the proxy surface cannot pinpoint whether the proxy itself or a backend component is the failure point. Subprobes solve this:

```text
[REFERENCE]
ProbeResult {
  surface: ProviderHealthStatus,      // top-level: what the client sees
  subprobes: [
    ProviderHealthStatus { probe_label: "inference backend", ... },
    ProviderHealthStatus { probe_label: "auth sidecar", ... },
  ]
}
```

`probe_label` in each subprobe carries a human-readable component name so the Doctor health report can display exactly which hop in the chain is failing.

#### Context window discovery on model switch

When the fallback cascade advances to a different model, the new model's context window must be re-probed to avoid feeding an incorrect `effective_budget` to the context engine:

```text
[REFERENCE]
resolve_context_window(provider, model) -> u32:     // 0 = unknown
  match provider:
    "ollama"  → warm_model(model); probe_ollama_context_window(model)
    "vllm"    → GET /v1/models → extract max_model_len for this model
    "cloud"   → the model's declared window in models.json, else 0   // not exposed via API
    _         → the declared window if the catalog has one, else 0
```

On discovery failure (probe error, model not found) the window is reported **unknown** (0) and a WARNING is logged. The session continues, and the context engine applies its conservative default for an unknown window (`l2-context-management` §4.1) — an assumed large window would make the engine scale its budget off a window nobody proved, which is exactly what that rule forbids.

## 5. Drawbacks & Alternatives

- **Large pattern list maintenance:** providers change error messages; patterns drift. Mitigated by using narrow provider-specific phrases rather than generic words.
- **Heuristic context overflow (large session + generic 400):** may trigger unnecessary compression on unrelated errors. Justified: a wasted compress is cheaper than a hard abort on a recoverable overflow.
- **SSL alert → Timeout, not ContextOverflow:** a large session with an SSL alert would otherwise trigger compression incorrectly. SSL patterns must run before the disconnect heuristic.
- **Health probe latency:** adding a probe before each turn adds round-trip latency. Mitigated by caching probe results per provider with a short TTL (e.g. 30s) and skipping the probe when the last result was `ok`.
- **Alternative — single retry with no classification:** loses all adaptive recovery (compression, rotation, cascade); rejected.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[ROUTING]` | `.design/main/specifications/l1-routing.md` | Fallback cascade invariants |
| `[DOCTOR]` | `.design/main/specifications/l1-doctor.md` | Self-healing model |
| `[ROUTER]` | `.design/main/specifications/l2-model-router.md` | Cascade and credential pool |
| `[SESSION]` | `.design/main/specifications/l2-agent-session.md` | Turn loop consuming the classifier |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.3 | 2026-09-24 | Core Team | Consistency pass (2026-09-24): A scope note names `l2-doctor` as the carrier of HEAL-4, HEAL-6, HEAL-7 and HEAL-8, which this table does not list. |
| 1.0.2 | 2026-09-23 | Core Team | Consistency pass (2026-09-23): Compliance cited DOC-1/DOC-2 (the doctor prefix is HEAL) and SEC-2 as "Audit" — now HEAL-1/2/3/5 with the audit log under SEC-7; the RTG-2 row confines the cascade to the router's eligible candidates. A provider safety refusal (`ContentBlocked`) triggered automatic fallback, shopping the same request across providers until one accepted it — it is now surfaced, with a retry elsewhere only on the user's explicit request. Context-window discovery failure assumed 131 072 tokens, which makes the context engine budget off an unproven window — an undiscoverable window is now reported unknown and the engine's conservative default applies. |
| 1.0.1 | — | Core Team | Last version before this section was added; earlier revisions are recorded in version control. |
