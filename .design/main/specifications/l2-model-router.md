# Model Router

**Version:** 1.0.5
**Status:** Stable
**Layer:** implementation
**Implements:** l1-routing.md

## Overview

The concrete model router: how Cronus chooses which model answers a prompt — local-first with cloud fallback, difficulty- and cost-aware, with a fallback cascade and a semantic cache. Policy lives in `routing.json`; the model catalog in `models.json`.

## Related Specifications

- [l1-routing.md](l1-routing.md) - The router pattern this implements.
- [l2-technology-stack.md](l2-technology-stack.md) - Local execution options and cloud providers (llama.cpp through FFI is the mobile, foreground-only path).
- [l2-model-runtime.md](l2-model-runtime.md) - The loopback REST providers that serve local inference on a desktop or server host, and MR-1's rule that a failed local call never re-routes off-device on its own.
- [l1-architecture.md](l1-architecture.md) - Hub-and-spoke (local on capable host) and security (INV-7).
- [l2-cli.md](l2-cli.md) - Command grammar standard for routing commands.
- [l2-model-error-recovery.md](l2-model-error-recovery.md) - Error taxonomy and credential pool (multi-key rotation) that interacts with the fallback cascade.

## 1. Motivation

The router pattern needs concrete signals and tools for model selection: estimate difficulty, check local feasibility against hardware, compare cost/capability, and fall back resiliently — defaulting to on-device for privacy and cost.

## 2. Constraints & Assumptions

- Local models run via the on-device runtime — the loopback REST providers of `l2-model-runtime` on a desktop or server host, llama.cpp through FFI on mobile (foreground only); cloud via provider APIs, reached only under authorized egress (SEC-3).
- Policy is read from `<state>/routing.json`; the catalog from `<state>/models.json`.
- Decisions run on the hot path and must be fast; a semantic cache fronts the router.
- Default policy is local-first (chosen product stance).

## 3. Invariant Compliance (Layer 2 only)

| L1 Invariant | Implementation |
| --- | --- |
| RTG-1 Multi-signal | Score by difficulty, cost, token count, capability match, latency, quota headroom, local feasibility. |
| RTG-2 Fallback | Ordered cascade: subscription -> API key -> cheap -> free; on error/unavailable, advance. |
| RTG-3 Short-circuit cache | A semantic cache is checked before routing; a hit returns without a model call. |
| RTG-4 Scope resolution | N/A for models (applies to context router); model policy is global with per-task overrides. |
| RTG-5 Configurable | All weights/thresholds/cascade live in `routing.json`. |
| RTG-6 Privacy-preserving | Local-first: prefer an on-device model when hardware-fit says it can handle the task; else cloud. |
| RTG-7 Bounded & traceable | Each decision logs chosen model + reason; bounded by run budget (orchestration). |
| RTG-8 Lifecycle | N/A (session lifecycle handled by the context router). |
| RTG-9 Function-scoped model roles | **Pending.** Housekeeping functions (titling, triage, decomposition, summarization, curation, vision, approval checks) resolve through per-role bindings in `routing.json` (`aux_roles{}`), each with an economical default and an ordered fallback, honoring privacy routing; a housekeeping call never takes the premium user-facing route by default. |
| RTG-10 Credential-lane routing | **Pending.** The §4.2 cascade orders lanes (subscription before metered key); the cache-warmth rule — a continuing session stays on the lane it used — and the scope-safety rule are not yet realized. |
| RTG-11 User-adjustable effort | **Partial.** Requests carry an optional reasoning-effort field where the provider supports it (§4.12); the office-autonomous default and the human-set effort envelope are pending. |

## 4. Detailed Design

### 4.1 Signals and decision

```mermaid
graph TD
    REQ[prompt + task tags] --> CACHE{semantic cache hit?}
    CACHE -->|yes| RET[return cached]
    CACHE -->|no| DIFF[estimate difficulty]
    DIFF --> FIT{local model fits? hardware-fit}
    FIT -->|yes + capable| LOCAL[route to on-device model]
    FIT -->|no| TIER[cloud: difficulty-threshold + cost]
    LOCAL --> FBL{on failure: cloud_fallback enabled?}
    FBL -->|yes| FB[fallback cascade over eligible providers]
    FBL -->|no| ERR[surface through error recovery]
    TIER --> FB
```

- **Difficulty threshold:** a cheap predictor decides weak-vs-strong tier (RouteLLM-style).
- **Hardware-fit:** estimates whether an on-device model of sufficient capability fits memory/perf (llmfit-style) before choosing local.
- **Cost/quality dial:** a single `cost_quality_tradeoff` knob biases the choice when multiple candidates qualify (OpenRouter-style).

**Selection pipeline — how the mechanisms of this spec compose.** They are stages of one decision, applied in this order, each narrowing the set the next one sees:

1. **Eligibility.** Privacy routing and egress authorization (RTG-6, SEC-3) and hardware fit (§4.9) decide which candidates may serve the request at all; a cloud candidate is eligible only where the policy authorizes cloud for this office.
2. **Tier.** The difficulty estimate (§4.1) or, where configured, the semantic router pool (§4.6) picks the capability tier.
3. **Ranking within the tier.** Cloud candidates by the multi-factor score (§4.8, with the active mode pack); on-device candidates by the per-use-case score (§4.10); `cost_quality_tradeoff` biases both.
4. **Stickiness and exploration.** LKGP and the exploration draw (§4.8) choose only among the candidates stages 1–3 left.
5. **Failure.** The fallback cascade (§4.2) advances only through candidates that were eligible in stage 1.

### 4.2 Fallback cascade

`subscription -> api-key -> cheap -> free`, switching on quota exhaustion, error, or unavailability (OmniRoute-style), and only through providers the user configured and authorized for this office. Local sits ahead of the cascade when local-first applies; a **local failure continues into the cloud part of the cascade only where the routing policy explicitly enables cloud fallback** (`local_first.cloud_fallback`, off unless the user turns it on). Otherwise the failure surfaces through error recovery instead of re-routing the request off-device on its own (`l2-model-runtime` MR-1, SEC-3).

### 4.3 Semantic cache

Before routing, the request is matched against a semantic cache; a sufficiently-similar prior answer short-circuits the call (GPTCache-style), with a configurable similarity threshold and eviction. The cache is scoped to the office and the requesting user: an entry never answers a request from another office or user (`l1-office-model` OFF-1), since a hit returns stored content verbatim. <!-- TBD: cache similarity threshold + TTL/eviction defaults -->

### 4.4 Policy & catalog files

```text
[REFERENCE]
routing.json: { strategy, cost_quality_tradeoff, fallback[], semantic_cache{}, aux_roles{},
                local_first{ enabled, max_local_params_b, cloud_fallback /* default false */ } }
models.json:  [ { id, name, provider, model, baseUrl, context_window? } ]   // absent window = unknown
```

### 4.5 Command surface

Routing commands conform to the CLI grammar standard (see `l2-cli.md` §4.4).

| Action | CLI | TUI | Library (no code) |
| --- | --- | --- | --- |
| list models | `cronus model list` | `/model list` | `models.list() -> Model[]` |
| show policy | `cronus route policy` | `/route policy` | `router.policy() -> Policy` |
| explain a routing decision | `cronus route explain "<task>"` | `/route explain …` | `router.explain(task) -> Decision` |

### 4.6 Semantic router pool

The difficulty threshold in §4.1 uses a cheap heuristic (prompt length, token count, task tags). For workloads with a catalog of candidate models where cost varies significantly, a lightweight semantic router can replace or supplement the heuristic by predicting each model's accuracy on the current prompt and selecting the cheapest option that meets a tolerance threshold.

#### Router pool configuration

```text
[REFERENCE]
RouterPoolConfig {
  routing: RouterPoolRouting {
    // Routing algorithm:
    // "prefill" — use a fine-tuned checkpoint to predict P(correct) via prefill encoding.
    method: "prefill",

    // Path to the fine-tuned router checkpoint (relative to state dir or absolute).
    checkpoint: String,

    // Accuracy-cost tolerance: how far below the best-accuracy model's score a
    // cheaper model may fall and still be selected. 0.20 = 20 percentage points.
    tolerance: f32,   // default 0.20

    // Embedding model used to encode prompts for the checkpoint.
    encoder: String,              // e.g. a small open-weights encoder

    // Runtime backend for running the encoder.
    encoder_backend: "transformers" | "onnx",
  },

  models: Vec<RouterPoolModelEntry>,
}

RouterPoolModelEntry {
  name:                    String,
  display_name:            String,
  litellm_model:           String,          // provider-prefixed model ID for API calls
  cost_per_m_input_tokens: f64,             // USD per million input tokens
  cost_per_m_output_tokens: f64,            // USD per million output tokens
  api_base:                String,          // endpoint URL for this model
}
```

When the on-device runtime is used for the managed inference endpoint, the virtual hostname `https://inference.local/v1` maps to the local inference process, decoupling the policy file from the actual host address.

#### Selection algorithm

```text
[REFERENCE]
select_model(prompt, pool_config) -> RouterPoolModelEntry:
  embedding = encoder.encode(prompt)
  scores    = checkpoint.predict(embedding)  // P(correct) per model, same order as pool_config.models

  best_p    = max(scores)
  threshold = best_p - pool_config.routing.tolerance

  candidates = [(model, score) for (model, score) in zip(models, scores) if score >= threshold]

  // Among candidates, pick the cheapest by estimated total cost
  // (input cost dominates for most routing workloads, so rank on input cost as primary signal)
  return min(candidates, key = model.cost_per_m_input_tokens)
```

If the checkpoint is unavailable or the pool config is absent, the router falls back to the heuristic difficulty estimator (§4.1).

#### Configuration location

`RouterPoolConfig` is stored in `<state>/router-pool.json` (alongside `routing.json`). The catalog in `models.json` is the source for the full model list; `router-pool.json` references a subset of those models that are candidates for semantic routing.

### 4.7 Three-layer provider resilience

Three independent mechanisms with different scope. Keep them separate when debugging routing failures.

```text
[REFERENCE]
Layer               | Scope                              | Skips when …
────────────────────────────────────────────────────────────────────────────
Circuit Breaker     | Whole provider (e.g. "anthropic")  | Provider-level errors (408/500/502/503/504) exceed threshold
Connection Cooldown | Single API key / account           | That key returns account-level errors (429, auth failures)
Model Lockout       | Provider + connection + model      | That model returns per-model errors (404, per-model quota)
```

#### Circuit Breaker (four states)

```text
[REFERENCE]
CircuitBreaker states: CLOSED → DEGRADED → OPEN → HALF_OPEN → CLOSED

  CLOSED    — normal; all requests pass through
  DEGRADED  — elevated failure rate; requests pass but alerting fires
  OPEN      — provider blocked; routing skips it
  HALF_OPEN — probe allowed after reset timeout; success → CLOSED, failure → OPEN

Thresholds per provider class:
  | Class   | Degraded at | Opens at    | Reset timeout |
  | oauth   | 5 failures  | 8 failures  | 60 s          |
  | api-key | 7 failures  | 12 failures | 30 s          |
  | local   | derived     | 2 failures  | 15 s          |

degradation_threshold: default 60% of failure_threshold (configurable)

Adaptive backoff:
  max_backoff_multiplier   = 16   // escalates on repeated OPEN→HALF_OPEN→OPEN cycles
  backoff_escalation_count = 3    // cycles before backoff escalates
  openCycleCount tracked per breaker for diagnostics

Per-kind thresholds: failure_kind = rate_limit | quota_exhausted | transient
  Each kind may carry a separate threshold and cooldown; transient triggers immediate OPEN

Transition history: last 20 { from, to, timestamp, failure_count, reason }

Trip codes (provider-level only): [408, 500, 502, 503, 504]
Do NOT trip for: 401, 403, 429 — those belong to Connection Cooldown

Lazy recovery: getStatus() refreshes OPEN → HALF_OPEN when reset_timeout expires;
               no background timer required
```

#### Connection Cooldown

When one key fails, other keys for the same provider keep serving.

```text
[REFERENCE]
Connection fields:
  rate_limited_until: Timestamp  // connection skipped while this is in the future
  test_status: "unavailable"     // set on failure; cleared by clear_account_error() on success
  backoff_level: u8              // incremented on each recoverable failure

Default cooldowns:
  oauth base:   5 s
  api-key base: 3 s
  api-key 429:  prefer upstream Retry-After / reset headers when present
  backoff:      base_cooldown_ms * 2 ** backoff_level

Terminal states (NOT cooldowns — persist until credentials change or operator reset):
  "banned" | "expired" | "credits_exhausted"
  Do NOT overwrite terminal states with transient cooldown state.

Anti-thundering-herd guard: concurrent failures on the same connection do NOT
  double-increment backoff_level or extend the cooldown redundantly.
```

#### Model Lockout

When only one model fails, the connection continues serving other models.

```text
[REFERENCE]
Scope: provider + connection + model triple

Trigger examples:
  - Per-model quota (429)
  - Missing model (404) on local providers
  - Provider-specific permission failures

API:
  lock_model(provider, connection, model, expires_at)
  clear_model_lock(provider, connection, model)
  list_model_lockouts() -> [ {provider, connection, model, reason, expires_at} ]
```

#### Resilience debugging guide

```text
All keys for a provider skipped → check circuit breaker state AND each connection's rate_limited_until
Provider excluded after reset window → use getStatus() instead of reading raw state field
One key fails, others should work → connection cooldown (not circuit breaker)
Only one model fails → model lockout (not connection cooldown)
State should self-recover but doesn't → check future-timestamp + lazy-read path
```

### 4.8 Multi-factor candidate scoring

When multiple model candidates qualify, a weighted 9-factor score ranks them. All weights sum to 1.0; `validateWeights()` enforces this at startup.

#### Score factors

```text
[REFERENCE]
Factor            | Default | Description
──────────────────────────────────────────────────────────────────────────────
health            | 0.22    | Circuit breaker: CLOSED=1.0, HALF_OPEN=0.5, OPEN=0.0
quota             | 0.17    | Remaining quota / rate-limit headroom [0..1]
cost_inv          | 0.17    | Inverse blended cost (60% input + 40% output, normalized)
latency_inv       | 0.13    | Inverse p95 latency normalized across pool
task_fit          | 0.08    | Task-type fitness: coding|review|planning|analysis|debugging|docs
specificity_match | 0.08    | Match between request specificity and model tier
stability         | 0.05    | Variance-based stability (low latency stdDev + error rate)
tier_priority     | 0.05    | Account tier: Ultra=1.0, Pro=0.67, Standard=0.33, Free=0.0
tier_affinity     | 0.05    | Affinity between candidate tier and recommended tier
```

Candidates with score < 0.2 are temporarily excluded (5 min initial, progressive backoff, max 30 min).

In incident mode (>50% of candidates OPEN), exploration is disabled and stability is maximized.

#### Mode packs

Four pre-defined weight profiles for common optimization goals:

```text
[REFERENCE]
Mode pack     | Primary signal     | Weight | Goal
──────────────────────────────────────────────────
"ship-fast"   | latency_inv        | 0.32   | Lowest latency
"cost-saver"  | cost_inv           | 0.37   | Cheapest per token
"quality"     | task_fit           | 0.37   | Best task fit + stability
"offline"     | quota              | 0.37   | Max quota headroom

Usage: routing.json `strategy_mode` = "ship-fast" | "cost-saver" | "quality" | "offline"
```

#### LKGP (Last-Known-Good-Path)

Routes to the most recently successful candidate. Falls back to the next-best scored candidate on failure. Session-sticky with health safety: if the LKGP candidate is OPEN or DEGRADED, scoring resumes.

#### Reset-aware tiebreaker

When scores are within 0.05 of each other, prefer the candidate whose quota resets soonest:

```text
[REFERENCE]
reset_boost(candidate):
  if candidate.quota_resets_at is set:
    time_to_reset_ms = max(0, candidate.quota_resets_at - now_ms())
    return 1.0 - (time_to_reset_ms / MAX_RESET_WINDOW_MS)   // [0..1]
  else: 0.5                                                   // neutral
```

#### Exploration (bandit)

5% of requests (configurable `exploration_rate`) route to a random candidate **among those already eligible** (selection-pipeline stage 1, §4.1): exploration never moves a request off-device that privacy routing would keep local, never reaches a provider the office has not authorized, and never exceeds the run budget. Exploration is disabled in incident mode.

### 4.9 Hardware-Fit Layer

The `FIT` decision node in §4.1 is implemented as a hardware-fit evaluator that maps each on-device candidate to a `FitLevel` and selects the best `RunMode`.

#### FitLevel taxonomy

```text
[REFERENCE]
Level    | Condition                                    | Routing behaviour
─────────────────────────────────────────────────────────────────────────────
Perfect  | GPU mode; recommended_ram ≤ available VRAM  | Always route local
Good     | GPU mode tight, or CPU offload with headroom | Route local
Marginal | Minimum met but tight; CPU-only always here  | Route local only when no cloud candidate and policy allows degraded
TooTight | required_mem > available mem                 | Skip local → cloud cascade
```

`Perfect` is only achievable in GPU mode. `CpuOffload` caps at `Good`. `CpuOnly` always caps at `Marginal`.

#### RunMode taxonomy

```text
[REFERENCE]
Mode             | Description                                              | TPS factor
─────────────────────────────────────────────────────────────────────────────────────────
Gpu              | All weights in VRAM                                      | 1.0
TensorParallel   | Weights sharded across multiple same-model GPUs          | 0.9
MoeOffload       | MoE experts in RAM, active experts loaded per-token      | 0.8
CpuOffload       | Weights in RAM; hot layers in VRAM                       | 0.5
CpuOnly          | Weights in RAM; no GPU                                   | 0.3
```

`CpuOffload` is skipped on unified-memory platforms (Apple Silicon, NVIDIA Grace/ATS): the GPU and CPU share the same physical pool, so there is no distinct offload path.

#### TPS estimation

Bandwidth-based (preferred when GPU bandwidth is known):

```text
[REFERENCE]
max_tps = bandwidth_GB_s / model_size_GB × 0.55 × run_mode_factor
model_size_GB = params_B × bytes_per_param(quantization)
```

Fixed-constant fallback when GPU bandwidth is unknown:

```text
[REFERENCE]
Backend    | tok/s constant
─────────────────────────
Metal/MLX  | 250
Metal/llama.cpp | 160
CUDA       | 220
ROCm       | 180
CpuArm     | 90
CpuX86     | 70
Ascend     | 390
```

#### Memory estimation

```text
[REFERENCE]
memory_total_GB = model_weights + kv_cache + overhead(0.5 GB)

model_weights = params_B × bpp(quantization)

kv_cache (precise, when n_layers/n_kv_heads/head_dim known):
  bytes = 2 × n_layers × n_kv_heads × head_dim × context × dtype_bytes
  kv_cache_GB = bytes / 1_073_741_824

kv_cache (fallback):
  kv_cache_GB = 0.000008 × params_B × context × kv_quant_scale

kv_quant_scale: fp16=1.0, fp8=0.5, q8_0=0.5, q4_0=0.25, TurboQuant≈0.17
```

KV cache context cap for planning: `min(model_advertised_context, 8192)` — the full advertised window is used for actual inference but a capped estimate avoids inflating planning budgets for 128k+ models.

#### FitLevel thresholds

```text
[REFERENCE]
GPU path:      Perfect if recommended_GB ≤ available; Good if available ≥ required × 1.2; else Marginal
CpuOffload:    Good if available ≥ required × 1.2; else Marginal
CpuOnly:       Always Marginal

VRAM pressure penalty (GPU mode):
  utilization 50–80% → no penalty (sweet spot)
  utilization 80–90% → fit_score penalty −30 pts
  utilization > 90%  → linear cache-thrash penalty, floor 0.30
```

### 4.10 Per-Use-Case Scoring Weights

When multiple on-device candidates qualify (FitLevel ≥ Good), composite score = Σ(weight_i × score_i) over four dimensions, with weights tuned per use-case.

#### Score dimensions (0–100 each)

```text
[REFERENCE]
quality  — base_quality + quant_quality_penalty + generation_bonus
           generation_bonus = (gen - 1.0) × 3.0, capped +9 (gen 4.0)
           quant penalties: F16/Q8_0=0, Q6_K=−1, Q5_K_M=−2, Q4_K_M=−5, Q3_K_M=−8, Q2_K=−12

speed    — derived from TPS estimate, normalized to [0,100]
           optimal range ≈ 25–60 tok/s maps to high scores

fit      — VRAM/RAM utilization sweet spot 50–80% = 100
           < 50%  → 60–100 (under-utilised)
           80–90% → 70 (light pressure)
           > 90%  → 50 (cache thrash risk)

context  — model_context_tokens vs requested, capped at 8 192 for KV planning
```

#### Weight table

```text
[REFERENCE]
Use case    | quality | speed | fit   | context
────────────────────────────────────────────────
General     | 0.45    | 0.30  | 0.15  | 0.10
Coding      | 0.50    | 0.20  | 0.15  | 0.15
Reasoning   | 0.55    | 0.15  | 0.15  | 0.15
Chat        | 0.40    | 0.35  | 0.15  | 0.10
Multimodal  | 0.50    | 0.20  | 0.15  | 0.15
Embedding   | 0.30    | 0.40  | 0.20  | 0.10
```

Use-case inference priority: explicit tag on task > `use_case` field in model catalog > name heuristics (e.g. "code" in name → Coding; "deepseek-r1" → Reasoning; "embed"/"bge" → Embedding).

### 4.11 Hardware Planning Protocol

When the selected model is `TooTight` or the user requests a fit report, the router produces a `PlanEstimate` — not a hard error — so the user can act.

#### Plan structure

```text
[REFERENCE]
PlanEstimate {
  model_name, provider, quantization, context, kv_quant,
  current: { fit_level, run_mode, estimated_tps },  // current hardware status

  run_paths: [                                        // evaluated in preference order
    { path: Gpu,        feasible, minimum: HW, recommended: HW, estimated_tps, fit_level, notes },
    { path: CpuOffload, feasible, minimum: HW, recommended: HW, estimated_tps, fit_level, notes },
    { path: CpuOnly,    feasible, minimum: HW, recommended: HW, estimated_tps, fit_level, notes },
  ],

  upgrade_deltas: [                                   // gaps vs current hardware
    { resource, add_GB, target_fit, description },    // e.g. "+4.2 GB VRAM -> Good"
  ],

  kv_alternatives: [                                  // "what-if" memory savings table
    { kv_quant, memory_required_GB, kv_cache_GB, savings_fraction, supported, note },
  ],
}

HardwareEstimate { vram_gb, ram_gb, cpu_cores }
```

Preference order for preferred path: Gpu → CpuOffload → CpuOnly. `CpuOffload` is `feasible: false` on unified-memory systems (Apple Silicon, NVIDIA Grace).

#### TurboQuant gating

TurboQuant KV compression (~0.34 bytes/element vs 2.0 for fp16) is gated on vLLM + CUDA. It only compresses full-attention layers in hybrid architectures (models that mix self-attention with linear/Mamba-style state-space layers see partial savings proportional to their full-attention fraction). Always shown in the `kv_alternatives` table; marked `supported: false` on non-CUDA backends with an explanatory note.

### 4.12 Post-Processing Action Pipeline

After primary model output is produced, an optional post-processing step routes the output through a configurable LLM provider to refine, reformat, or expand it before delivery to the user.

#### RAII finish guard

Any async processing pipeline must notify the coordinator when it finishes — including failure or panic paths. Wrap coordinator notification in a drop guard:

```text
[REFERENCE]
struct FinishGuard(AppHandle);

impl Drop for FinishGuard {
    fn drop(&mut self) {
        if let Some(coordinator) = self.0.try_state::<Coordinator>() {
            coordinator.notify_finished();
        }
    }
}
// Instantiate at the top of the processing function; coordinator is always notified.
```

#### Prompt template system

Post-processing prompts are templates containing a `${output}` placeholder. At runtime, strip the placeholder and send the primary output as the user message — do not interpolate it into the system prompt:

```text
[REFERENCE]
build_system_prompt(template):
  return template.replace("${output}", "").trim()

// Primary output → user message (never into system prompt).
// Rationale: prevents prompt injection when output contains LLM control sequences.
```

#### Output sanitization

Strip invisible Unicode characters from LLM output before storing or displaying it:

```text
[REFERENCE]
strip_invisible(s):
  remove: U+200B (zero-width space), U+200C (zero-width non-joiner),
          U+200D (zero-width joiner),  U+FEFF (BOM / zero-width no-break space)

These are sometimes inserted by LLMs as padding artefacts and can corrupt
clipboard pastes or downstream text processing.
```

#### Structured output schema

When the post-processor must return structured data, use the `json_schema` response format with `strict: true`:

```text
[REFERENCE]
response_format: {
    type: "json_schema",
    json_schema: {
        name:   "result_schema",
        strict: true,
        schema: { ... }    // JSON Schema object
    }
}
// Supported by OpenAI-compatible endpoints.
// For providers that do not support it: fall back to prompt-based JSON extraction.
```

#### Provider-specific authentication

```text
[REFERENCE]
Anthropic provider:
  x-api-key: <api_key>
  anthropic-version: 2023-06-01
  (NOT Authorization: Bearer)

All other providers (OpenAI, OpenRouter, Groq, Cerebras, …):
  Authorization: Bearer <api_key>

Detection: use the provider's stable ID from the catalog, not URL pattern matching.
```

#### Reasoning model configuration

For providers and models that support extended thinking, attach optional reasoning config fields; skip them entirely (via `skip_serializing_if = "Option::is_none"`) for models that do not:

```text
[REFERENCE]
ChatCompletionRequest:
  reasoning_effort: Option<String>       // "low"|"medium"|"high"  (o-series style)
  reasoning:        Option<{             // alternative providers' reasoning field
    effort:  Option<String>,
    exclude: Option<bool>,
  }>
```

## 5. Drawbacks & Alternatives

- **Difficulty estimation cost:** a wrong estimate mis-tiers; mitigated by the fallback cascade catching failures.
- **Local capability ceiling:** on-device models cap at smaller sizes; local-first yields to cloud when hardware-fit fails (honest about limits).
- **Semantic router cold-start:** the encoder must load on first use; add to the warm-up sequence to avoid latency on the first request.
- **Alternative — always cloud:** rejected as default; loses privacy/cost benefits of the personal-server model.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[ROUTING]` | `.design/main/specifications/l1-routing.md` | Invariants this implements |
| `[STACK]` | `.design/main/specifications/l2-technology-stack.md` | Local/cloud execution options |
| `[CLI]` | `.design/main/specifications/l2-cli.md` | Command grammar standard |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.5 | 2026-09-23 | Core Team | Consistency pass (2026-09-23): A local failure entered the cloud part of the cascade unconditionally, re-routing a request off-device that privacy routing kept local (contradicting l2-model-runtime MR-1 and SEC-3) — cloud fallback after a local failure is now an explicit `local_first.cloud_fallback` opt-in (default off), otherwise the failure surfaces through error recovery. The composition of eligibility, tier, ranking, stickiness/exploration and cascade was unstated, letting exploration and the cascade pick candidates privacy or egress had excluded — a selection pipeline now orders them, with exploration and fallback confined to eligible candidates. The semantic cache is scoped per office and user (OFF-1). Local execution restated to match l2-model-runtime (loopback REST providers on desktop/server, FFI only on mobile). Compliance gains RTG-9 (Pending, `aux_roles{}`), RTG-10 (Pending) and RTG-11 (Partial); `models.json` gains an optional declared `context_window`. |
| 1.0.4 | — | Core Team | Last version before this section was added; earlier revisions are recorded in version control. |
