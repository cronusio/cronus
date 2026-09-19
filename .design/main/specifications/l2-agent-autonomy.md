# Agent Autonomy

**Version:** 1.2.0
**Status:** Stable
**Layer:** implementation
**Implements:** l1-security.md, l1-orchestration.md

## Overview

The concrete autonomy ladder and its enforcement machinery: a three-tier `AutonomyLevel` that controls how much a running agent can do without pausing for human approval, a per-call risk classifier that maps every tool operation to a `CommandRiskLevel`, an `ActionTracker` that enforces rolling hourly caps, and an approval gate lifecycle that parks high-impact calls until the user decides.

## Related Specifications

- [l1-security.md](l1-security.md) - SEC-6 sandbox, SEC-7 audit.
- [l1-orchestration.md](l1-orchestration.md) - ORC-9 approval gate.
- [l2-tool-security.md](l2-tool-security.md) - Tool guard that produces `SuspendedPermission` escalated into this gate.
- [l2-scheduler.md](l2-scheduler.md) - Cron context bypasses the interactive approval path.
- [l2-security.md](l2-security.md) - Secret handling; audit log destination.
- [l2-orchestration.md](l2-orchestration.md) - Orchestrator triggers approvals for sub-manager promotions and agent hires.
- [l1-action-gating.md](l1-action-gating.md) - [ADDED v1.2.0] AG-9 (a gate is only a gate where the answer can be given) governs the unattended rows in §4.3/§4.6; AG-10 decides when "Allow always" is not offered; AG-11's refusal-loop bounds are §4.5.
- [l1-consent-binding.md](l1-consent-binding.md) - [ADDED v1.2.0] CB-1/CB-3: an allow-rule binds the resolved invocation and lapses on any change (§4.6).
- [l2-execution-sandbox.md](l2-execution-sandbox.md) - [ADDED v1.2.0] an approved call still runs confined; approval is authorization, never containment (CB-5).

## 1. Motivation

An autonomous agent can issue shell commands, write files, and call external APIs. The blast radius of an unchecked agent is unbounded. The autonomy ladder constrains it at design time (what tier is this agent?) and at runtime (how many actions this hour? does this specific call need a human?). A single enforcement point — `SecurityPolicy::gate_decision` — ensures every code path through the engine passes the same check.

## 2. Constraints & Assumptions

- The autonomy level is set per agent instance; it cannot be elevated by a model-produced tool call.
- The approval gate is interactive-only, and an unattended context has **no answering surface** (AG-9): a `Prompt` cannot be delivered there and is **never converted into Allow** to keep the run moving. `Allow` cells stay `Allow`; every `Prompt` cell resolves to a visible refusal naming what could not be asked; destructive is refused (§4.3).
- `ActionTracker` counts are session-scoped; process restart resets them.
- Always-forbidden paths are unconditional and cannot be overridden by any approval.

## 3. Invariant Compliance (Layer 2 only)

| L1 Invariant | Implementation |
| --- | --- |
| SEC-6 Sandbox | Every shell/code tool call passes `gate_decision` before execution; always-forbidden patterns are blocked before any sandbox spawn. |
| SEC-7 Auditable | Every gate decision (Allow/Prompt/Block) appends to the audit log with reason and tool context; promotions and auto-allowed (rule-suppressed) calls are logged with the matched rule id (§4.6). |
| ORC-9 Approval gate | `GateDecision::Prompt` parks the call in an `ApprovalRequest` with a 10-minute TTL; outcome is recorded. |
| SEC-9 Learnable promotion | An explicit "Allow always" writes a durable, scoped `AllowRule` (§4.6): scope defaults to the narrowest offered, the key is `(risk_class-safe action signature)`, the rule is revocable (§4.7) and its max scope / persistence is clamped by policy governance. `destructive`-class and always-forbidden paths are non-promotable — "Allow always" is not offered for them, so they always re-prompt. |
| SEC-9(g) Interpreters not promotable beyond the exact invocation | A shell, interpreter, launcher or evaluator target is promotable only at `action` scope keyed to the resolved invocation; wider scopes are not offered and a wider rule already stored for such a target is inert (§4.6). |
| SEC-12 Heuristics labeled; coverage stated | The risk classifier and the gate matrix decide *whether a human is asked*; they are heuristics and never containment — a call the gate allows still runs confined (`l2-execution-sandbox`), and no text here describes an allowed call as sandboxed (§4.2). |
| AG-4 (l1-action-gating) Unknown fails to friction | Classification is by the tool's declared effect and resolved parameters, never by words inside a free-text command; an unrecognized or unparseable command classifies `write` at minimum (§4.2). |
| AG-9 A gate only where the answer can be given | Background and cron contexts declare no answering surface; every `Prompt` resolves to a visible refusal, nothing is auto-allowed to keep moving (§4.3, §4.6 step 5). |
| AG-10 What steered an act (privileged sinks) | A request carrying `untrusted_spans` is answered per call: "Allow always" is not offered, because the rule would freeze an outside-chosen argument (§4.6 step 3). |
| AG-11 Reviewer guard rails (refusal loops bounded) | `RefusalTracker` caps consecutive refusals, consecutive unavailable results and total refusals per session, and routes the next call to a human at a cap (§4.5). |
| CB-1 / CB-3 Consent binds the resolved invocation | An `AllowRule` for a shell-class tool keys on the resolved invocation and lapses on any change (§4.6). |

## 4. Detailed Design

### 4.1 Autonomy level

```text
[REFERENCE]
AutonomyLevel: "supervised" | "semi_autonomous" | "autonomous"
```

| Level | Behavior |
| --- | --- |
| `supervised` | Every tool call that is not `read`-class requires explicit approval before execution. |
| `semi_autonomous` | `read` and `write` calls auto-proceed; `network`, `install`, and `destructive` calls require approval. |
| `autonomous` | All calls auto-proceed except `destructive`-class, which always require approval. Always-forbidden paths are blocked unconditionally regardless of level. |

The level is stored in the agent's runtime config and loaded at session start. It cannot be changed from within a running session by a model-produced action.

### 4.2 Command risk classification

Every tool call is classified into one of five risk levels before the gate runs:

```text
[REFERENCE]
CommandRiskLevel: "read" | "write" | "network" | "install" | "destructive"
```

| Class | Examples |
| --- | --- |
| `read` | File reads, directory listings, memory recall, web fetch (GET) |
| `write` | File writes/edits, database updates, environment variable writes |
| `network` | HTTP POST/PUT/PATCH, WebSocket connections, DNS lookups for non-read URLs |
| `install` | Package manager calls, extension installs, binary downloads |
| `destructive` | File deletes, process kills, config resets, `rm`, `DROP TABLE`, data purges |

Classification is performed by `ToolOperation::classify(tool_name: &str, params: &ToolParams) -> CommandRiskLevel`, from the tool's **declared effect class and its resolved parameters** — never from words appearing inside a free-text command. A command string is analysed by the guard (`l2-tool-security` §4.2: segments, wrappers, substitution, resolved executable) and that analysis feeds the class; an unrecognized tool, or a command the analysis cannot parse, classifies as `write` at minimum (conservative, AG-4) and as `destructive` whenever the analysis says so. A keyword classifier over command text ("contains *list* or *get* or *show*, therefore read") is **not conformant**: a word inside an argument would auto-allow the call, and the class decides whether a human is asked at all.

### 4.3 SecurityPolicy

`SecurityPolicy` is assembled once at agent session start from the agent's `AutonomyConfig` and the workspace directory. It is immutable for the session lifetime.

```text
[REFERENCE]
AutonomyConfig {
  level: AutonomyLevel,
  max_actions_per_hour: u32,            // default 60; 0 = unlimited
  workspace_dir: PathBuf,               // all file paths validated against this boundary
  always_forbidden_extra: Vec<String>,  // operator-added patterns (appended to built-ins)
}

SecurityPolicy {
  level: AutonomyLevel,
  max_actions_per_hour: u32,
  workspace_dir: PathBuf,
  forbidden_patterns: Vec<Regex>,  // compiled from always_forbidden_extra + built-ins
}

SecurityPolicy::gate_decision(
  class: CommandRiskLevel,
  level: AutonomyLevel,
  context: ExecutionContext,   // "interactive" | "background" | "cron"
) -> GateDecision

GateDecision: Allow | Prompt | Block { reason: String }
```

Gate matrix (interactive context):

| Class | supervised | semi_autonomous | autonomous |
| --- | --- | --- | --- |
| `read` | Allow | Allow | Allow |
| `write` | Prompt | Allow | Allow |
| `network` | Prompt | Prompt | Allow |
| `install` | Prompt | Prompt | Allow |
| `destructive` | Prompt | Prompt | Prompt |

Background/cron context (no answering surface, AG-9): every `Allow` cell of the matrix stays `Allow`; every `Prompt` cell **resolves to a visible refusal** — `Block { reason: "needs_approval_unattended", asked: <what could not be asked> }`, recorded and surfaced to the operator — and is never converted into Allow; `destructive` → Block unconditionally. A background agent that needs `network` or `install` therefore runs at a level whose row is `Allow` for that class (`autonomous`), which is the operator's explicit, authority-plane choice (SEC-10), or is refused: `supervised` and `semi_autonomous` background runs do not gain those classes by being unattended. A durable rule (SEC-9) whose exact identity matches may still allow a non-destructive call (§4.6).

### 4.4 Always-forbidden paths

The following are blocked before the gate runs — no autonomy level and no approval can override them:

- Shell patterns: `rm -rf /`, `sudo rm -rf`, `mkfs`, `dd if=/dev/`
- Any file path resolving outside `workspace_dir` after symlink expansion
- Any file path matching `.env` or secrets-tier patterns (see `l2-security.md §4.1`)

A call matching an always-forbidden pattern returns `GateDecision::Block` immediately and is logged as `HARD_BLOCKED` in the audit trail.

### 4.5 ActionTracker

`ActionTracker` enforces the rolling hourly cap independently of the gate decision.

```text
[REFERENCE]
ActionTracker {
  session_counts: HashMap<CommandRiskLevel, u32>,
  hourly_window:  RollingWindow,   // sliding 60-minute window of action timestamps
  max_actions_per_hour: u32,
}

impl ActionTracker {
  record(class: CommandRiskLevel) -> Result<(), ActionCapError>
  // Pushes a timestamp; evicts entries older than 60 min.
  // If count after push >= max_actions_per_hour: Err(ActionCapError).

  session_total() -> u32
  hourly_count()  -> u32
}
```

`ActionCapError` produces `GateDecision::Block { reason: "hourly_action_cap_exceeded" }` and is surfaced to the user as a soft stop. The cap slides continuously — no forced cooldown period.

#### Refusal tracking (AG-11f)

The rate cap limits how *often* an agent acts; it does not notice an agent stuck retrying against a refusal. `RefusalTracker` counts refusals per session — consecutive refusals of one action family, consecutive `unavailable` results from an automated reviewer or guard, and total refusals — with default caps of 3, 2 and 20. <!-- TBD: tune with field data --> Reaching a cap **routes the next call to a human approval regardless of autonomy level** and tells the agent to stop retrying that family; it never converts a refusal into permission and never silently drops the run. The counters are audited with the rest of the gate (SEC-7).

### 4.6 Approval gate lifecycle

When `gate_decision` returns `Prompt` in an interactive context, the tool call is parked as an `ApprovalRequest`:

```text
[REFERENCE]
ApprovalRequest {
  id:           String,              // UUID
  tool_name:    String,
  tool_kind:    String,              // "file" | "shell" | "network" | "install" | "other"
  target:       Option<String>,      // primary affected resource (path, command, URL)
  summary:      String,              // human-readable single-sentence description
  paths:        Vec<String>,         // up to 5 affected paths
  risk_class:   CommandRiskLevel,
  ttl_secs:     u32,                 // default 600 (10 minutes)
  created_at:   Instant,
  requires_user_confirmation: true,
}
```

#### Lifecycle

1. `ApprovalRequest` is created and sent to the interactive session's approval UI.
2. The agent suspends execution of the specific tool call; other turn logic is unaffected.
3. The user chooses: Allow once / Allow for session / **Allow always (scope)** / Deny / Deny and explain. "Allow always" is offered only when the call is promotable (SEC-9f) — it is suppressed for `destructive`-class and always-forbidden calls, which can only be allowed once. It is also suppressed for a call whose target is an interpreter, shell, launcher or evaluator at any scope wider than `action` (SEC-9g), and for a call carrying `untrusted_spans` (AG-10): a rule made from it would freeze an argument the outside chose. The approval UI marks those spans (CB-2).
4. **TTL expiry**: no response within `ttl_secs` → auto-Deny; agent receives `ApprovalDenied { reason: "timeout" }`. The tool result phrases it as **not answered in time — not a refusal**: the agent may continue differently or ask again later, and is never told the action is forbidden, because an unanswered prompt is not a policy decision.
5. **Unattended contexts (background/cron) — AG-9**: there is no approval UI to answer, so a `Prompt` outcome is not parked; it resolves at once to a visible refusal (`ApprovalDenied { reason: "unattended_no_surface", asked }`) that is recorded and shown on the operator's surface. Nothing is auto-allowed to keep the run moving (AG-4). `destructive` and always-forbidden calls are denied. A durable `AllowRule` (below) whose exact identity matches MAY allow a non-destructive call; destructive and always-forbidden calls are never matched.
6. All outcomes (Allow / Deny / Timeout / Promotion) are written to the audit log.

"Allow for session" grants approval for calls with the same `(tool_name, target)` pair for the remainder of the session without re-prompting; for a shell-class tool the target is the resolved invocation, exactly as for a durable rule (CB-1). This approval is not persisted across sessions.

#### Durable allow-rules (SEC-9)

"Allow always" promotes the approval into a durable `AllowRule` that survives process restart and suppresses re-prompting for genuinely-equivalent later calls. It realizes the SEC-9 discipline — explicit act, explicit minimal scope, narrow key, revocable, governed, non-promotable dangerous classes:

```text
[REFERENCE]
AllowScope: "action" | "action_class" | "office" | "global"   // narrowest offered is the default

AllowRule {
  id:          String,             // UUID; the handle for revocation (§4.7)
  key:         String,             // stable action signature: (tool_name, normalized_target) for "action" scope;
                                   //   tool_name for wider scopes — EXCEPT program carriers (SEC-9g), which have no wider scope.
                                   //   For a shell-class tool, normalized_target is the RESOLVED INVOCATION (CB-1): executable's
                                   //   canonical path, argument vector, working directory, environment-plan identity — not the
                                   //   command text. Any change to a bound element lapses the rule (CB-3).
  scope:       AllowScope,
  office_id?:  String,             // bound for "office" scope
  created_by:  String,             // approver identity (SEC-7)
  created_at_ms: u64,
}

promote(request, decision):                                   // step 3 "Allow always"
    if request.risk_class == Destructive
       or matches_always_forbidden(request):
        refuse()                                              // SEC-9f — option was never offered
    if request.untrusted_spans:                               // AG-10: an outside-chosen argument is not frozen into a rule
        refuse()
    if is_program_carrier(request.target) and decision.scope != "action":
        refuse()                                              // SEC-9g — interpreter/shell/launcher/evaluator: exact invocation only
    scope := decision.scope or narrowest_offered(request)     // SEC-9b
    cap   := policy.max_promotion_scope()                     // SEC-9e governed clamp
    if scope > cap or policy.promotion_disabled():
        return                                                // fail-closed: no rule; keep prompting
    rule_store.put(AllowRule{ key: action_key(request, scope), scope, ... })
    audit("promotion", rule)

gate_decision(request):                                        // consulted before Prompt
    if request.risk_class != Destructive
       and not matches_always_forbidden(request)
       and rule_store.matches(request, exact_only = has_untrusted_spans(request)):   // SEC-9c equivalence match; AG-10: a steered call matches only an exact-identity rule
        audit("auto_allowed", matched_rule); return Allow      // SEC-9d suppressed prompt logged
    ... // fall through to the normal ladder / Prompt
```

The rule store is part of the state tier (never version-controlled, SEC-1). Governance clamps (`max_promotion_scope`, `promotion_disabled`) come from the managed policy tier; when they tighten below an existing rule's scope, the rule is inert until re-approved (fail-closed), not silently honored.

### 4.7 Command surface

| Action | CLI | TUI | Library (no code) |
| --- | --- | --- | --- |
| get current level | `cronus agent autonomy get` | `/agent autonomy get` | `agent.autonomy.level() -> AutonomyLevel` |
| set level | `cronus agent autonomy set --level <supervised\|semi\|auto>` | `/agent autonomy set …` | `agent.autonomy.set_level(level) -> void` |
| set hourly cap | `cronus agent autonomy set --max-actions-per-hour <N>` | `/agent autonomy set …` | `agent.autonomy.set_cap(n) -> void` |
| view action counts | `cronus agent autonomy status` | `/agent autonomy status` | `agent.autonomy.status() -> ActionTrackerStatus` |
| list durable allow-rules | `cronus agent autonomy rules list` | `/agent autonomy rules list` | `agent.autonomy.rules() -> Vec<AllowRule>` |
| revoke a durable allow-rule | `cronus agent autonomy rules revoke <id>` | `/agent autonomy rules revoke <id>` | `agent.autonomy.revoke_rule(id) -> void` |

### 4.8 Approval record manager

The approval manager tracks in-flight and recently-resolved approval records. Separating record *creation* from *registration* (async wait) avoids a race between the approval UI dispatching a decision and the command being retried before the manager is ready:

```text
[REFERENCE]
RESOLVED_ENTRY_GRACE_MS = 15_000   // keep resolved records 15 s post-decision

ApprovalRecord {
  id:                          String,            // UUID
  request:                     ApprovalRequest,   // the parked tool call (see §4.6)
  created_at_ms:               u64,
  expires_at_ms:               u64,

  // Optional caller-binding fields (prevent replay by a different client)
  requested_by_conn_id?:       String,
  requested_by_device_id?:     String,
  requested_by_client_id?:     String,
  requested_by_device_token?:  bool,

  // Set when the decision arrives
  resolved_at_ms?:             u64,
  decision?:                   ApprovalDecision,   // "allow" | "deny"
  consumed_decision?:          ApprovalDecision,   // set when the consuming call reads it (single-use)
  resolved_by?:                String,             // identity string of the approver
}
```

Lifecycle operations:

```text
[REFERENCE]
ApprovalManager {
  // Synchronous: generate a new record (no async state yet)
  create(request: ApprovalRequest, timeout_ms: u64) -> ApprovalRecord

  // Async: register record and return a future that resolves with the decision.
  // Idempotent: if the same id is already pending, returns the SAME future.
  // Error: if the id is already resolved, panics (caller bug).
  register(record: ApprovalRecord, timeout_ms: u64) -> Future<ApprovalDecision?>

  // Accept a decision (from the UI or background bypass).
  resolve(id: String, decision: ApprovalDecision, resolved_by: Option<String>) -> bool

  // Called internally after TTL elapses.
  expire(id: String) -> void
}
```

After `resolve()` is called, the entry stays in the manager for `RESOLVED_ENTRY_GRACE_MS` (15 s) before being removed. This window allows a `register()` call that races with the decision (e.g. the command is retried before the future is polled) to find the already-resolved record and return the decision immediately rather than blocking.

The `consumed_decision` field is set when the tool call reads the decision, preventing a second tool-call attempt from replaying an already-consumed approval ID.

## 5. Drawbacks & Alternatives

- **10-minute TTL may feel short**: the timer is visible to the user; they can re-trigger the action if it expires. Extending it increases the window for stale approvals.
- **Hourly cap is session-scoped**: process restart resets counts. For multi-session continuity, a persistent ledger (e.g. in the budget engine) would be needed — deferred to the budget engine spec.
- **Unattended runs need an explicit level, not a bypass (v1.2.0):** an earlier revision auto-allowed every non-destructive class in background and cron contexts, including `network` and `install`, which delivered by default what the ladder withholds from an attended `supervised` or `semi_autonomous` agent. A scheduled job that legitimately needs those classes runs at `autonomous` by the operator's explicit choice, or is refused visibly; the ladder is the same ladder attended or not.
- **Keyword classification is gone from the spec (v1.2.0):** a classifier that reads intent from words in a command line mis-tiers by construction — the words `list`, `get` or `read` inside an argument would grant the lowest tier. Classification comes from declared effect and resolved parameters, backed by the guard's parse.
- **`write`-class auto-allowed in `semi_autonomous`**: file writes are common and this avoids approval fatigue. The tool guard still runs for every write call (path containment, hard blocks).
- **Alternative — per-tool allowlists instead of risk classes**: more granular but requires maintenance as new tools are added. The risk-class model is more stable; per-tool overrides can be layered on top.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[SECURITY]` | `.design/main/specifications/l1-security.md` | SEC-6/SEC-7 invariants; SEC-9 learnable promotion realized in §4.6 |
| `[ORC]` | `.design/main/specifications/l1-orchestration.md` | ORC-9 approval gate |
| `[TOOLSEC]` | `.design/main/specifications/l2-tool-security.md` | Tool guard escalation |
| `[SCHED]` | `.design/main/specifications/l2-scheduler.md` | Cron context — bypass rules |
| `[POLICY-GOV]` | `.design/main/specifications/l1-policy-governance.md` | Clamps promotion max scope / persistence (SEC-9e) |
| `[GATING]` | `.design/main/specifications/l1-action-gating.md` | AG-4 unknown→friction, AG-9 unattended, AG-10 steered arguments, AG-11 refusal bounds |
| `[CONSENT]` | `.design/main/specifications/l1-consent-binding.md` | CB-1/CB-3 — the rule binds the resolved invocation |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.2.0 | 2026-09-19 | Core Team | Reconciled with `l1-action-gating` (AG-4, AG-9, AG-10, AG-11), `l1-consent-binding` and `l1-security` SEC-9(g). **Unattended contexts no longer bypass the gate:** the earlier text auto-allowed every non-destructive class — including `network` and `install` — in background and cron contexts, which contradicts AG-4 (boundary-crossing and value-bearing acts are never auto) and AG-9 (never downgrade to a mechanism that skips the question to keep moving); every `Prompt` cell now resolves to a visible refusal naming what could not be asked, `Allow` cells stay `Allow`, and a background agent that needs those classes runs at `autonomous` by the operator's explicit choice. **Classification is by declared effect and resolved parameters, never by words inside a command line**; an unparseable command is `write` at minimum. **Allow-rule identity:** for a shell-class tool the key is the resolved invocation (CB-1) and lapses on change (CB-3); interpreters, shells, launchers and evaluators are promotable only at `action` scope (SEC-9g); a call carrying `untrusted_spans` is never promotable (AG-10). TTL expiry is phrased to the agent as *not answered in time, not a refusal*. New `RefusalTracker` (caps 3/2/20, cap → human approval; AG-11f). Compliance rows added for SEC-9(g), AG-4, AG-9, AG-10, AG-11, CB-1/CB-3. The dormant reference implementation of the classifier and gate differed from this spec on exactly these points (keyword classification with four coarse levels, no execution context); that is recorded as pending realization, not as a change to the design. |
| 1.1.0 | 2026-07-02 | Core Team | Realized SEC-9: approval gate step 3 gains "Allow always (scope)" (offered only for promotable calls); new §4.6 durable `AllowRule` store (scope ladder action/action_class/office/global, stable-signature key, revocable, governance-clamped, fail-closed, destructive/always-forbidden non-promotable); `gate_decision` consults the rule store before Prompt and audits auto-allowed calls; §4.7 `rules list` / `rules revoke <id>` command surface; SEC-9 Invariant-Compliance row. |
| 1.0.1 | 2026-06-26 | Core Team | ApprovalRecord manager: create/register separation, RESOLVED_ENTRY_GRACE_MS=15s, idempotent register, caller-binding replay guard. |
| 1.0.0 | 2026-06-24 | Core Team | Initial spec — autonomy ladder, CommandRiskLevel classifier, SecurityPolicy gate matrix, ActionTracker hourly cap, approval gate lifecycle. |
