# Policy Governance

**Version:** 1.2.1
**Status:** Stable
**Layer:** concept

## Overview

An administrative control layer over configuration. It defines how settings resolve through ordered tiers, lets an administrator principal fix a *managed* tier that lower tiers cannot override, and gives that tier concrete levers to harden a deployment: restrict the loadable extension / hook / permission-rule surface to a vetted set, constrain which extension sources are trusted, and disable safety-reducing escape hatches. The goal is a deployment an operator can lock down once so that neither a user, a project, nor the agent can silently weaken it.

This is distinct from client security (which protects secrets and confines execution) and from authentication (which decides *who* a principal is). Policy governance decides *what configuration and capability surface a principal is permitted*, and which of those decisions are administrator-final rather than user-discretionary. It applies equally to a single user hardening their own machine, a superadmin setting policy for a small team, or an operator managing a fleet.

## Related Specifications

- [l1-security.md](l1-security.md) - Client security (secrets, sandbox, exfiltration); governance composes with SEC-6/SEC-7 and can mandate stricter security posture.
- [l2-multi-user-auth.md](l2-multi-user-auth.md) - Defines the administrator/superadmin principal whose authority sets the managed tier; governance is the policy layer above its flat privilege model.
- [l2-extension-registry.md](l2-extension-registry.md) - Supply-chain checks for extensions; governance restricts which sources and artifacts may load (PG-4/PG-5).
- [l2-plugin-hooks.md](l2-plugin-hooks.md) - Hooks are a governable artifact class; managed-only mode constrains which hooks run.
- [l2-tool-security.md](l2-tool-security.md) - Permission rules and approval gates are governable; governance can fix them and disable bypass.
- [l1-application-shell.md](l1-application-shell.md) - Hosts the layered settings model this spec orders.
- [l1-staged-rollout.md](l1-staged-rollout.md) - [ADDED v1.2.0] Owner of the observe → warn → enforce ratchet (SRO-10) that PG-10 binds the managed tier to when a restriction is newly introduced or tightened.
- [l2-app-ui.md](l2-app-ui.md) - Realizes the per-parameter override indicator and the revert-to-default affordance (PG-9) in the settings surface; the concrete control is L2, the precedence-aware semantics are PG-9.

## 1. Motivation

Configuration that anyone can change is configuration no one can rely on. A user can paste a setting that disables a safety check; a project file can register an unreviewed hook that runs on every action; a convenience flag can switch off approval prompts wholesale. In a shared or hardened deployment these are not edge cases — they are the default failure mode.

The fix is a precedence-ordered configuration model with an administrator-owned tier at the top that is set out-of-band and cannot be overridden from inside the controlled environment. That tier needs more than key/value overrides: it needs the power to shrink the *capability surface* — to say "only hooks I vetted run here," "only these extension sources are trusted," "the bypass-everything mode does not exist on this machine" — and to do so transparently, so a user can always see what is locked and why. Without this layer, every other security invariant is one user edit away from being switched off.

## 2. Constraints & Assumptions

- The managed tier is stored where the controlled principal cannot write it (a system location, a protected store, or an out-of-band channel); enforcing that storage boundary is an implementation concern, not a guarantee this concept can make on its own.
- Governance restricts and fixes configuration; it does not invent new capabilities. Every lever here gates a surface defined by another spec (extensions, hooks, permission rules, sandbox, approval gates).
- Absent any managed policy, behaviour is identical to an unmanaged install — governance is opt-in and additive, never a hidden default restriction.
- The model is the same at every scale; "administrator" may be the sole user acting deliberately, a team superadmin, or a fleet operator.

## 3. Core Invariants

Rules every Layer 2 implementation MUST NOT violate:

- **PG-1 (Layered precedence):** configuration resolves through an ordered tier chain (managed → … → project → user → built-in default); a higher tier's value overrides lower tiers for the same key. Both the effective value and the tier that supplied it are always resolvable.
- **PG-2 (Managed tier is authoritative):** the top *managed* tier is set out-of-band by an administrator principal and MUST NOT be overridden, weakened, merged-away, or removed by any lower tier. A value fixed by the managed tier is final within the controlled environment.
- **PG-3 (Inspectable provenance):** for every effective setting the system can report its originating tier ("setting sources"). Managed restrictions are never applied silently — a user can always discover what is locked and that it is administrator-set.
- **PG-4 (Surface lockdown — managed-only modes):** the managed tier can restrict the *loadable surface* of each governable artifact class — extensions, hooks, permission rules — to an administrator-vetted set. When a class is set "managed-only," user- and project-supplied instances of that class are not loaded. Lockdown is declared per class and is default-off.
- **PG-5 (Trusted-source restriction):** the managed tier can constrain which extension sources (registries / distribution catalogs) are trusted; only artifacts originating from an allowlisted source may be installed or loaded. This composes with — and never bypasses — the extension supply-chain checks.
- **PG-6 (Escape-hatch governance):** any setting that *reduces* enforced safety (e.g. a mode that bypasses approval gates, or weakens the sandbox) is itself a governed value the managed tier can disable. When disabled, the escape hatch is unavailable regardless of user or project intent.
- **PG-7 (Fail-open absence, fail-safe presence):** with no managed policy present, the system imposes no hidden restriction (behaves as unmanaged). With a managed policy present, every restriction it enforces and every action it denies is auditable (composes with SEC-7).
- **PG-8 (Policy integrity):** the managed policy source is integrity-verified (pinned/hash-checked) before it is trusted; a tampered, substituted, or corrupt policy is detected and the system fails safe (treats it as a hard error, not as "no policy"), reusing the pinning discipline already applied to hooks and tool manifests.

- **PG-9 (Per-parameter override visibility & revert-to-default):** the settings surface exposes, per parameter, whether its effective value is the **default** or an **override**, and — when it is an override at a tier the acting principal may write — offers a **revert-to-default** action. Reverting clears the override at the principal's *own writable tier* so the value falls back through the precedence chain (PG-1) to the next-lower tier's value; it never edits a tier the principal may not write. The revert affordance is present **only when there is a principal-writable override to clear**: a parameter already at its default, or one fixed by the managed tier (PG-2), offers no revert — a managed-locked value surfaces as *locked* (PG-3 provenance), never as resettable. A revert is itself a governed, auditable configuration write (PG-7), and "default" means the value the parameter resolves to *absent this principal's override*, not necessarily the built-in baseline (a user-tier revert may fall back to a project value, then to the built-in default).
- **PG-10 (A newly introduced restriction ratchets; it never binds on first contact):** [ADDED v1.2.0] when a managed restriction is **newly introduced or tightened** — a lockdown of a loadable surface (PG-4), a narrowed trusted-source set (PG-5), a lowered cap — it MUST NOT go from absent to blocking in one step. It is first applied in **observe** mode, in which it blocks nothing and records exactly what it *would have* blocked; then in **warn**, in which the affected operation still succeeds while the principal is told the restriction now covers it; and only then in **enforce**. The reason is specific to this tier: a managed restriction is set out-of-band by an administrator (PG-2), applies to everyone at once, and cannot be overridden by the people it breaks (PG-1) — so a mis-sized restriction has the largest blast radius and the fewest escape routes of any change in the system, and the measurement that would have sized it correctly is available for free before it binds. The ratchet governs *introduction only*: an already-enforcing restriction stays enforcing, and PG-7's fail-safe-presence posture is unchanged. Demoting `enforce → warn` remains available as the immediate remedy for a restriction discovered to be mis-sized, without editing any threshold. The full ratchet semantics, including the derived warn threshold, are owned by `l1-staged-rollout` (SRO-10); PG-10 binds the managed tier to them.

> L2 specs cannot reach RFC status until all invariants here are addressed in their "Invariant Compliance" section.

## 4. Detailed Design

### 4.1 Tier Resolution

```text
[REFERENCE]
TIERS (highest → lowest):
  managed   — administrator, out-of-band, un-overridable (PG-2)
  cli       — invocation-time flags
  project   — checked-in project config
  user      — per-user config
  default   — built-in baseline

resolve(key):
    for tier in TIERS:                 // highest first
        if tier defines key: return { value: tier[key], source: tier }   // PG-1, PG-3
    return { value: default[key], source: "default" }
```

A managed-tier value short-circuits resolution: lower tiers are not consulted for that key.

### 4.2 Governable Artifact Classes & Lockdown

Each class names a surface owned by another spec. The managed tier may set each to one of: `open` (default — user/project instances load), `managed-only` (only administrator-supplied instances load), or `off` (the class is disabled entirely).

| Class | Governs | Lockdown effect |
| --- | --- | --- |
| Extensions / plugins | extension registry surface | `managed-only` → only administrator-installed extensions load |
| Hooks | hook system | `managed-only` → user/project hooks are ignored |
| Permission rules | approval-gate allow/ask/deny rules | `managed-only` → only administrator rules apply; user/project rules dropped |
| Extension sources | trusted registries/catalogs | allowlist → installs only from listed sources (PG-5) |
| Escape hatches | safety-reducing modes | `off` → mode unavailable (PG-6) |

### 4.3 Provenance Reporting

A status surface enumerates, per effective setting, its source tier and whether it is locked (managed). This is the user-facing counterpart to PG-3: lockdown is legible, not mysterious. A denied action attributable to policy reports the governing tier in its diagnostic.

### 4.4 Integrity & Fail-Safe

```text
[REFERENCE]
load_managed_policy(source):
    raw := read(source)
    if not integrity_ok(raw):           // PG-8: pinned hash / signature mismatch
        fail_safe("managed policy integrity check failed")   // hard error, not "unmanaged"
    return parse(raw)
```

Distinguishing "no policy" (legitimately unmanaged → PG-7 fail-open) from "policy present but tampered" (→ PG-8 fail-safe hard error) is the security-critical boundary: a substituted policy must never be silently downgraded to "no restrictions."

### 4.5 Per-parameter Override & Revert (PG-9)

The revert is a precedence operation, not a value assignment — it *removes* the
principal's override and lets resolution (§4.1) re-run:

```text
[REFERENCE]
is_overridden(key, principal):                       // PG-9 visibility
    eff := resolve(key)                                // PG-1: value + source tier
    return eff.source is a tier the principal WROTE (not `default`, not a higher locked tier)

revert_to_default(key, principal):
    if managed_fixes(key):        return LOCKED         // PG-2 — not resettable, shown locked (PG-3)
    if not is_overridden(key, principal):  return NOOP  // nothing to clear → no affordance shown
    clear_override(key, principal.writable_tier)        // remove ONLY the principal's own entry
    audit("revert", key, principal)                     // PG-7 governed, auditable write
    return resolve(key)                                 // value falls through to the next-lower tier
```

The UI shows the revert control next to a parameter exactly when `is_overridden` holds
and the parameter is not managed-locked; the concrete control (an inline reset icon that
appears on change) is the L2 settings-surface realization, the precedence-and-lock-aware
semantics above are the L1 contract.

## 5. Drawbacks & Alternatives

- **Lockout risk:** an over-restrictive managed tier can make a deployment unusable, and by PG-2 the user cannot self-rescue. Mitigated by PG-3 provenance (the user sees the cause) and by keeping administration out-of-band (the administrator can correct it where the policy lives).
- **Alternative — single flat settings file:** simplest, but any principal can disable any safety control; rejected — it is exactly the failure mode PG-2 exists to close.
- **Alternative — fold into client security (l1-security):** rejected; security confines *execution and secrets*, governance controls *configuration authority and capability surface*. Different axis, different administrator model; folding them would overload l1-security and force a wide cascade for an orthogonal concern.
- **Alternative — fold into authentication (l2-multi-user-auth):** rejected; auth decides identity, governance decides permitted configuration. Governance consumes the administrator identity auth provides but adds the policy layer auth explicitly defers ("no RBAC … can extend it later").

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[AUTH]` | `.design/main/specifications/l2-multi-user-auth.md` | Defines the administrator/superadmin principal whose authority sets the managed tier. |
| `[REGISTRY]` | `.design/main/specifications/l2-extension-registry.md` | Extension supply-chain surface governed by PG-4/PG-5. |
| `[HOOKS]` | `.design/main/specifications/l2-plugin-hooks.md` | Hook surface governed by PG-4 managed-only mode. |
| `[TOOLSEC]` | `.design/main/specifications/l2-tool-security.md` | Permission-rule / approval-gate surface governed by PG-4/PG-6. |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.0 | 2026-06-26 | Core Team | Initial spec — administrative policy-governance layer: layered precedence with an un-overridable managed tier, surface lockdown (managed-only extensions/hooks/permission rules), trusted-source restriction, escape-hatch governance, inspectable provenance, fail-safe policy integrity (PG-1…PG-8). |
| 1.2.0 | 2026-07-23 | Core Team | Added PG-10 — a newly introduced or tightened managed restriction (surface lockdown PG-4, narrowed trusted sources PG-5, a lowered cap) ratchets **observe → warn → enforce** rather than binding on first contact: observe blocks nothing and records exactly what it would have blocked, warn lets the operation succeed while telling the principal the restriction now covers it, enforce blocks. The reason is tier-specific — a managed restriction is set out-of-band (PG-2), applies to everyone simultaneously, and cannot be overridden by the people it breaks (PG-1), so it has the largest blast radius and the fewest escape routes of any change in the system, while the measurement that would have sized it correctly is free before it binds. Governs introduction only (an already-enforcing restriction is untouched, PG-7's fail-safe-presence posture unchanged), and `enforce → warn` demotion is the immediate remedy for a mis-sized restriction without editing a threshold. Full ratchet semantics including the derived warn threshold are owned by `l1-staged-rollout` SRO-10. |
| 1.1.0 | 2026-07-15 | Core Team | Added PG-9 (per-parameter override visibility & revert-to-default) + §4.5 — the settings surface shows per parameter whether the effective value is default or an override and, for a principal-writable override, offers a revert-to-default that *clears the override at the principal's own writable tier* so the value falls back through the precedence chain (PG-1), never editing a tier the principal may not write; the affordance appears only when there is a principal-writable override to clear — a default-valued or managed-locked (PG-2) parameter offers no revert (locked, not resettable, PG-3); revert is a governed auditable config write (PG-7); "default" = the value absent this principal's override (may be a lower tier's value, not necessarily the built-in baseline). Added l2-app-ui Related (realizes the inline reset-icon affordance; the precedence-and-lock-aware semantics are L1). Additive — L1 stays Stable (C9). |
| 1.2.1 | 2026-10-22 | Core Team | Citations updated for the requirement-ID prefix renames that make each prefix name one specification (RULES §1, 1.8.0, the person's decision of 2026-10-22): `SR-n` of `l1-staged-rollout` is now `SRO-n`. No requirement was added, removed or reworded. |
