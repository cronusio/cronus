# Sandbox Network Policy

**Version:** 1.1.1
**Status:** Stable
**Layer:** implementation
**Implements:** l1-security.md

## Overview

The concrete sandbox network egress policy: a deny-by-default schema (version, filesystem, process isolation, network_policies) with named policy entries each specifying allowed endpoints and the binaries permitted to use them. A preset system adds opt-in access tiers (balanced/open) on top of the restricted baseline. PolicyContext gives the agent runtime visibility into its active policy and verification state.

## Related Specifications

- [l1-security.md](l1-security.md) - SEC-3 (No exfiltration) and SEC-6 (Sandboxed execution) that this spec concretizes.
- [l2-security.md](l2-security.md) - Parent security spec; this spec expands §4.2 (egress gate) and §4.3 (execution sandbox).
- [l2-execution-workspace.md](l2-execution-workspace.md) - Isolated execution environments that the policy is applied to.
- [l2-agent-autonomy.md](l2-agent-autonomy.md) - Approval gate used when an agent requests policy expansion.
- [l2-doctor.md](l2-doctor.md) - Health probe that verifies policy enforcement is active.
- [l2-execution-sandbox.md](l2-execution-sandbox.md) - [ADDED v1.0.1] the mechanism that enforces this policy per platform (§4.3 of `l2-security` now points there); its denial classification (§4.8) feeds this spec's §4.9, and its coverage table states what the policy does not reach.

## 1. Motivation

The egress gate in `l2-security.md §4.2` is a logical gate; this spec provides the concrete enforcement schema. Without named-entry, binary-allowlisted policies, an agent running as an authorized binary gains access to every endpoint the binary could reach — defeating least-privilege. Named entries with per-binary allowlists confine each binary to only the endpoints it legitimately needs. Deny-by-default ensures that messaging platforms, social networks, and other high-risk categories are not reachable unless explicitly added via a preset.

## 2. Constraints & Assumptions

- The policy file is read at sandbox startup; runtime changes require a reload coordinated by the host process.
- Binary paths in allowlists are absolute; a binary not listed in any policy entry is denied all network access.
- Messaging and social platforms are never in the restricted baseline — they require an explicit opt-in preset.
- Policy files are protected by the Config Integrity Shields mechanism (`l2-security.md §4.6`).
- The host process (Cronus daemon) writes and locks the policy file; the sandboxed agent reads it read-only.

## 3. Invariant Compliance (Layer 2 only)

| L1 Invariant | Implementation |
| --- | --- |
| SEC-3 No exfiltration | Deny-by-default; `network_policies` allowlist is the only path to outbound network. |
| SEC-6 Sandboxed execution | Binary allowlisting confines each executable to its named policy entries only. |
| SEC-7 Auditable | Every access failure is classified and logged with `AccessFailureClassification`. |
| SEC-12 Heuristics labeled; coverage stated | This spec is a policy schema, not a boundary by itself: enforcement and its enumerated coverage are `l2-execution-sandbox`. `PolicyContext` (§4.8) is generated from the selected backend's coverage declaration, so the text the model sees about confinement cannot exceed what is enforced (SEC-12(d)). |

## 4. Detailed Design

### 4.1 Policy file schema

The policy file is YAML (or JSON) with the following structure:

```text
[REFERENCE]
SandboxPolicy {
  version: u32,                          // schema version; current = 1

  filesystem_policy: FilesystemPolicy {
    include_workdir: bool,               // allow access to the execution workspace dir
    read_only:  Vec<String>,             // absolute paths mounted read-only
    read_write: Vec<String>,             // absolute paths mounted read-write
  },

  process: ProcessConfig {
    run_as_user:  String,               // e.g. "sandbox" — honored inside a container or remote backend, or through an
    run_as_group: String,               //   unprivileged user-namespace mapping; never by creating or switching to an OS
  },                                    //   account (that needs rights the engine does not hold); otherwise reported "not applied"

  // Optional OS-native isolation compatibility:
  // "strict"       — abort sandbox creation if kernel isolation cannot be applied.
  // "best_effort"  — [MODIFIED v1.1.0] NOT a silent downgrade (ES-8 of l1-execution-sandbox): proceed only on the axes
  //                  the human lists in `accept_unconfined`; each accepted axis is recorded, stamped on every surface
  //                  that runs under it, and shown by `cronus sandbox status`. An axis not listed still refuses.
  isolation_compatibility: "strict" | "best_effort",
  accept_unconfined: Vec<"fs" | "net" | "proc" | "priv" | "resource">,   // [ADDED v1.1.0] only meaningful with best_effort; default empty

  // Named network policy entries. Deny-by-default: any traffic not matched by
  // a listed entry is blocked.
  network_policies: Map<String, NetworkPolicyEntry>,
}
```

### 4.2 Network policy entry

```text
[REFERENCE]
NetworkPolicyEntry {
  name: String,               // human-readable label (same as map key)
  endpoints: Vec<Endpoint>,  // allowed remote endpoints
  binaries: Vec<String>,     // absolute paths of executables allowed to use this entry
}
```

Each entry is independently evaluated per connection request: the originating binary path and the target endpoint must both match for the connection to be permitted.

### 4.3 Endpoint schema

```text
[REFERENCE]
Endpoint {
  host: String,              // hostname or IP; wildcard "*.example.com" allowed
  port: u16,

  // Transport protocol:
  // "rest"      — HTTP/HTTPS REST endpoint.
  // "websocket" — WebSocket upgrade endpoint.
  protocol: "rest" | "websocket",

  // Enforcement mode:
  // "enforce" — traffic not matching this entry's rules is denied.
  // "audit"   — violations are logged but not blocked (useful during policy migration).
  enforcement: "enforce" | "audit",

  // TLS handling:
  // "terminate"   — TLS is terminated at this endpoint.
  // "passthrough" — raw TLS pass-through (transparent proxy).
  // "skip"        — no TLS (plain); use only for localhost or trusted internal networks.
  tls: "terminate" | "passthrough" | "skip",

  // HTTP method-path rules (REST only). Absent = all methods/paths allowed for this endpoint.
  rules: Vec<EndpointRule>?,
}

EndpointRule {
  allow: MethodPathAllow {
    method: String,  // "GET", "POST", "*" for any
    path:   String,  // prefix or exact path; "*" for any
  }
}
```

### 4.4 Binary allowlisting

Each `NetworkPolicyEntry.binaries` lists the absolute paths of executables authorized to use that entry's endpoints. A binary may appear in multiple entries, accumulating the union of allowed endpoints:

```text
[REFERENCE]
effective_egress(process) =
  union of endpoints in all entries where process.canonical_exe_path ∈ entry.binaries

deny rule: any outbound call from a binary not in any entry's binaries list is
  immediately blocked, regardless of the target endpoint.
```

Binary paths are resolved to their canonical absolute path (symlinks resolved) at sandbox spawn time, then compared against the allowlist. A symlink replacement after resolution is rejected.

### 4.5 Deny-by-default baseline

The restricted baseline includes only the endpoints required for the agent runtime's core operation:

- Managed inference (local on-device or trusted internal endpoint)
- Gateway dial-back (IPC between agent and host process)
- Package registry access (for dependency resolution in code execution tasks, when applicable)

Messaging platforms (email, Slack, Discord, Telegram, WhatsApp, WeChat, etc.) and social networks are **never** in the restricted baseline. Their absence is by design — deny-by-default prevents accidental data exfiltration to communication channels. They are available only via explicit opt-in presets (§4.6).

### 4.6 Policy tiers

A tier is a named combination of preset packages applied on top of the restricted baseline:

```text
[REFERENCE]
PolicyTier: "restricted" | "balanced" | "open"

TierPreset {
  name:   String,
  access: "read" | "read-write",
}

TierDefinition {
  name:        PolicyTier,
  label:       String,
  description: String,
  presets:     Vec<TierPreset>,
}
```

| Tier | Presets included |
| --- | --- |
| `restricted` | Baseline only (inference + gateway) |
| `balanced` | `npm-registry`, `pypi`, `huggingface`, `brave-search`, `weather` |
| `open` | Balanced + `public-reference` + messaging platforms (Slack, Discord, Telegram, Jira, email) |

The `open` tier is appropriate only for fully-trusted sandboxes where the user has reviewed and accepted the policy; it must not be the default.

### 4.7 Policy presets

Presets are named, discoverable, opt-in add-ons that can be applied on top of the active tier. Each preset expands the policy by adding endpoints to specific named entries:

```text
[REFERENCE]
PolicyPreset {
  name:                   String,
  description:            String,
  allowed_host_categories: Vec<String>,  // e.g. ["package-registry", "inference"]
  redacted_host_count:    u32,           // number of specific hosts (not enumerated for privacy)
  source:                 "builtin" | "custom",
  verification:           PolicyContextPresetVerification,
}
```

Preset resolution:

1. The active tier's preset list is the starting set.
2. Additional presets may be requested through the approval gate (`l2-agent-autonomy.md §4.6`); an unattended caller gets a visible refusal instead (AG-9).
3. Unknown or unverified presets are listed in `known_unapplied_presets` (§4.8) and not automatically applied.

### 4.8 PolicyContext — agent visibility

The running agent can inspect its current policy state via `PolicyContext`:

```text
[REFERENCE]
PolicyContext {
  sandbox_name:             String,
  tier:                     PolicyTier,
  active_presets:           Vec<PolicyPreset>,
  known_unapplied_presets:  Vec<PolicyPreset>,  // visible but not yet applied
  approval_path:            String,              // command/channel to request policy expansion
  support_boundaries:       Vec<PolicyContextSupportBoundary>,
}

PolicyContextSupportBoundary {
  capability: String,                           // e.g. "network-policy", "filesystem-policy"
  owner:      "host" | "agent" | "external",
  note:       String?,
}

PolicyContextPresetVerification:
  | "verified"             // verified against both registry and active policy
  | "registry-only"        // known in registry but not yet applied to policy
  | "gateway-only"         // applied in policy but not in registry (policy ahead of registry)
  | "gateway-unavailable"  // registry unreachable; verification skipped
```

`PolicyContext` is read-only from the agent's perspective. To request a policy change, the agent emits a structured request through `approval_path`, which routes to the approval gate; the host process is the sole writer of the policy file.

### 4.9 Access failure classification

When an outbound call is blocked or fails in a way that may be policy-related, the failure is classified before being returned to the caller:

```text
[REFERENCE]
AccessFailureKind:
  | "blocked-by-policy"   // definitively blocked by the network policy
  | "missing-approval"    // endpoint exists but requires an approved preset not yet applied
  | "unsupported"         // endpoint is not in any known preset
  | "unknown"             // failure does not match policy patterns

AccessFailureClassification {
  kind:          AccessFailureKind,
  reason:        String,
  next_step:     String,   // human-readable recovery hint
  matched_preset: String?, // which preset would grant access (if identifiable)
  confidence:    "high" | "low",
}
```

Classification rules:

```text
[REFERENCE]
// Only the enforcing layer knows it denied something. Its record is the evidence.
Classification logic (first match wins):
  if the egress proxy / backend recorded a denial for this connection
     (the typed Denial{axis: net} of l2-execution-sandbox §4.8):
       if target_host matches a known_unapplied_preset      → "missing-approval", high, matched_preset set
       else                                                  → "blocked-by-policy", high
  if no denial was recorded and target_host is in no preset  → "unsupported", low
  else                                                       → "unknown", low

// An OS error (EAI_AGAIN, ENETUNREACH, EHOSTUNREACH, ECONNREFUSED, ETIMEDOUT, ENOTFOUND) or a
// remote HTTP 401/403 WITHOUT a recorded denial is a failure of the network or of the remote
// service's own authentication — not of this policy. It is classified "unknown" and its
// next_step never proposes a preset: recommending a policy expansion for a server that is
// simply down would push the human to widen egress for nothing.
```

This matches the backend's own rule that unknown failures are not reported as denials (`l2-execution-sandbox` §4.8): a classification that blames confinement for an ordinary outage invites exactly the authority widening SEC-10 exists to keep deliberate.

Every access failure classification is written to the audit log with the full `AccessFailureClassification` struct.

## 5. Drawbacks & Alternatives

- **File-based policy vs compiled rules:** the YAML/JSON schema is human-readable and auditable; compiled rules would be faster but opaque. Speed is not a concern for policy enforcement (checked at connection setup, not per-packet).
- **Binary allowlisting via absolute canonical path:** a binary replaced at its path between resolution and spawn could bypass the check. Mitigation: pin the canonical path at fork time and verify it has not changed at exec time.
- **`isolation_compatibility: "best_effort"` on constrained hosts:** reduces enforcement guarantees, and `[MODIFIED v1.1.0]` it no longer means "proceed with a warning": a run degrades only on the axes the human names in `accept_unconfined`, each visibly and persistently labeled (`l2-execution-sandbox` §4.1, §4.2). Production deployments should prefer `strict`.
- **Alternative — single allowlist for all binaries:** simpler, but loses per-process least-privilege. Any compromised binary gains the full endpoint set allowed by any entry.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[SECURITY]` | `.design/main/specifications/l1-security.md` | SEC-3/SEC-6 invariants |
| `[PARENT-SEC]` | `.design/main/specifications/l2-security.md` | Egress gate + sandbox this spec expands |
| `[WORKSPACE]` | `.design/main/specifications/l2-execution-workspace.md` | Sandbox environments |
| `[AUTONOMY]` | `.design/main/specifications/l2-agent-autonomy.md` | Approval gate for policy expansion |
| `[BACKEND]` | `.design/main/specifications/l2-execution-sandbox.md` | The enforcement mechanism per platform |

## Document History

| Version | Date | Notes |
| --- | --- | --- |
| 1.1.1 | 2026-09-23 | Consistency pass (2026-09-23): §4.9 access-failure classification no longer labels ordinary network errors (ECONNREFUSED, ETIMEDOUT, ENOTFOUND…) and a remote 401/403 as `blocked-by-policy`/`missing-approval` with high confidence — that nudged users toward widening egress for a server that was merely down, and contradicted `l2-execution-sandbox` §4.8 (unknown failures are not denials); only a recorded proxy/backend denial is a policy block. `run_as_user` is honored only where no elevation is needed. Approval-gate cross-reference corrected (§4.8 → §4.6, AG-9); typo fixed. |
| 1.1.0 | 2026-09-19 | `isolation_compatibility: "best_effort"` reconciled with `l1-execution-sandbox` ES-8 (fail closed for untrusted code): it read as "proceed without kernel isolation; emit a warning", a silent downgrade the L1 forbids and that `l2-execution-sandbox` refuses. It now proceeds only on the axes the human lists in the new `accept_unconfined` field — each recorded, stamped on every surface that runs under it, and shown by `cronus sandbox status`; an axis not listed still refuses. Found while decomposing `l2-execution-sandbox` into tasks. |
| 1.0.1 | 2026-09-19 | Cross-reference only — `l2-execution-sandbox` is the enforcement mechanism this policy schema lacked; Related Specifications and Canonical References extended. No invariant or schema changed. (Document History section introduced at this revision per RULES §5; prior version lineage tracked in `INDEX.md`.) |
