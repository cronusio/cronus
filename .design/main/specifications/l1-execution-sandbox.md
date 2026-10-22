# Execution Sandbox Confinement

**Version:** 1.0.2
**Status:** Stable
**Layer:** concept

## Overview

OS-level confinement of the code and commands the agent runs. Where client security states that agent-run execution is sandboxed (SEC-6), this spec elaborates *how* that sandbox confines execution: by denying, by default, across four axes — the operations the code may invoke, the privileges it holds, the resources it consumes, and the filesystem and runtime image it sees. It is the execution-confinement layer of a defense-in-depth model: distinct from network egress policy (which governs *where* the code may connect) and from process integrity (which protects the agent's *own* process), and layered with both so that bypassing one barrier does not defeat the others.

## Related Specifications

- [l1-security.md](l1-security.md) - SEC-6 names sandboxed execution; this spec defines the confinement axes that satisfy it.
- [l1-process-integrity.md](l1-process-integrity.md) - Protects the agent's own process (anti-dump/anti-trace/env); this spec confines the *code the agent runs*. Complementary layers.
- [l2-sandbox-policy.md](l2-sandbox-policy.md) - The network-egress + binary-allowlist slice of sandbox enforcement; this spec covers the syscall/privilege/resource/image slice.
- [l2-execution-workspace.md](l2-execution-workspace.md) - The isolated filesystem environment the sandboxed code runs inside; confinement applies within it.
- [l1-policy-governance.md](l1-policy-governance.md) - Posture tiers may select how strict the confinement profile is.
- [l1-environment-lifecycle.md](l1-environment-lifecycle.md) - The independent lifetime axis: this spec governs *what code may do* once running, that one governs *when the environment exists* and what happens to its work when it ends.
- [l1-execution-locus.md](l1-execution-locus.md) - **Orthogonal and composed**: ES says *how confined* execution is on each axis; LOC says *which world* it happens in. A locus may be confined or unconfined, and selecting one never relaxes an axis (LOC-7).

## 1. Motivation

The whole point of sandboxing agent-run code is that the code is not fully trusted — it may be model-generated, pulled from an extension, or shell commands the agent composed. Network egress policy alone is not enough: a program confined only at the network layer can still exhaust the host's memory, spawn a fork bomb, read files outside its task, abuse a privileged syscall, or escalate via a setuid binary. "Defense-in-depth (sandbox + egress gate)" is only as strong as the *sandbox* half — and that half is the OS-level confinement of what the code may actually do.

That confinement has four independent axes — operations (which low-level calls are permitted), privileges (which capabilities the process holds), resources (how much CPU/memory/processes it may use), and the filesystem-and-image it executes against. Each must deny by default and grant back only the minimum, each relaxation must be explicit and audited, and the controls must be independent so no single bypass collapses the sandbox. When a control cannot be applied, untrusted code must be refused, not run unconfined.

## 2. Constraints & Assumptions

- The code being confined is untrusted by default; confinement is a safety boundary, not a convenience setting.
- Confinement mechanisms are OS-specific (syscall filters, capability sets, control groups, mount namespaces); this spec constrains the required *effects* per axis, not the mechanism.
- This layer composes with, and never replaces, the network egress policy and process integrity — it is one of several independent barriers.
- Some hosts cannot enforce every axis; the spec defines the required behavior (refuse) when a control is unavailable.

## 3. Core Invariants

Rules every Layer 2 implementation MUST NOT violate:

- **EXS-1 (Deny-by-default across axes):** agent-run code executes confined on every axis — operations, privileges, resources, filesystem/image — with each axis denying by default. Capability is granted explicitly and minimally, never assumed (the concrete elaboration of SEC-6).
- **EXS-2 (Operation allowlist):** the low-level operations the sandboxed code may perform are restricted to a task-appropriate allowlist; operations outside it are blocked at the boundary (syscall-filter-equivalent), not merely discouraged in guidance.
- **EXS-3 (Least privilege):** the sandbox drops all ambient privileges/capabilities and grants back only the minimal set the task requires; privilege-escalation primitives (setuid-style elevation, gaining new privileges on exec) are disabled.
- **EXS-4 (Resource bounds):** CPU, memory, process/thread count, and I/O are capped per sandbox. Exceeding a bound throttles or terminates the sandbox — never the host. A runaway or malicious program cannot exhaust host resources.
- **EXS-5 (Filesystem confinement):** the execution root is read-only except for explicitly declared, minimal writable scratch areas; the writable surface is the least the task needs and is scoped to the execution workspace.
- **EXS-6 (Runtime-image integrity):** when the sandbox is provisioned from a packaged runtime image, that image is pinned by content digest and verified before use; a mutable tag is never trusted. A registry compromise or tag force-push cannot silently swap the sandbox image.
- **EXS-7 (Explicit, audited escalation):** any relaxation of any axis — a broader operation set, an added privilege, a writable path, a raised resource bound — is an explicit, recorded decision scoped to the narrowest unit possible, never a silent default (consistent with SEC-7 auditability).
- **EXS-8 (Fail-closed for untrusted code):** if a required confinement control cannot be applied, the sandbox refuses to run the code rather than running it unconfined; the inability is surfaced and auditable. (This is stricter than own-process hardening, which may degrade visibly — untrusted execution defaults to refuse.)
- **EXS-9 (Independent, layered barriers):** execution confinement is independent of and layered with the network egress policy and process integrity. No single control is the sole barrier; a bypass of one layer does not grant what another layer denies.

> L2 specs cannot reach RFC status until all invariants here are addressed in their "Invariant Compliance" section.

## 4. Detailed Design

### 4.1 Confinement Axes

| Axis | Denies by default | Grants back (minimal) | Breach response |
| --- | --- | --- | --- |
| Operations | all but an allowlist of low-level calls | task-appropriate calls | block at boundary (EXS-2) |
| Privileges | all ambient capabilities + escalation | the few capabilities the task needs | no-new-privileges enforced (EXS-3) |
| Resources | unbounded use | capped CPU/mem/procs/IO | throttle/terminate sandbox (EXS-4) |
| Filesystem & image | writes + untrusted image | read-only root + declared scratch + pinned image | read-only enforced; image verified (EXS-5/EXS-6) |

### 4.2 Provisioning Sequence

```text
[REFERENCE]
provision_sandbox(task, profile):
    image := resolve(profile.image_digest)        // EXS-6: digest, never a mutable tag
    verify(image.digest == profile.image_digest) or fail_closed()
    apply_operation_allowlist(profile.ops)         // EXS-2
    drop_all_privileges(); grant(profile.caps)     // EXS-3; set no-new-privileges
    set_resource_bounds(profile.limits)            // EXS-4
    mount_readonly_root(); mount_scratch(profile.writable)  // EXS-5
    for axis in AXES:
        if not enforced(axis): fail_closed("cannot confine " + axis)   // EXS-8
    run(task)                                       // inside l2-execution-workspace dir
```

### 4.3 Escalation & Posture

A task that legitimately needs more (an extra capability, a writable path, a higher limit) requests it; the grant is explicit, minimal, recorded (EXS-7), and may be gated by the policy-governance posture tier (a "restricted" tier grants less back than a "balanced" one). Resource-bound breaches surface as operational-health signals.

### 4.4 Layering

```text
[REFERENCE]
agent-run code is confined by ALL of, independently (EXS-9):
  - execution sandbox (this spec)  — what it may DO (ops/privs/resources/fs/image)
  - network egress policy          — where it may CONNECT
  - process integrity              — protects the supervising agent process itself
A bypass of any one does not relax the others.
```

## 5. Drawbacks & Alternatives

- **Compatibility friction:** strict operation allowlists and capability drops can break legitimate tools; mitigated by EXS-7 explicit per-task escalation and posture tiers, never by a blanket relaxation.
- **Host coverage gaps:** not every OS enforces every axis; EXS-8 makes the safe choice (refuse untrusted code) explicit rather than silently degrading.
- **Alternative — network policy only:** rejected; leaves resource exhaustion, privilege abuse, and out-of-scope filesystem access wide open — the egress gate is only half of defense-in-depth.
- **Alternative — fold into l2-sandbox-policy:** that L2 is scoped to network + binary allowlist; the syscall/privilege/resource/image axes are a distinct, OS-level concern that deserves its own concept so neither half is under-specified.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[SECURITY]` | `.design/main/specifications/l1-security.md` | SEC-6 sandboxed-execution invariant this spec elaborates. |
| `[NET-POLICY]` | `.design/main/specifications/l2-sandbox-policy.md` | The network/binary-allowlist confinement slice this spec complements. |
| `[WORKSPACE]` | `.design/main/specifications/l2-execution-workspace.md` | The isolated filesystem environment confinement applies within. |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.2 | 2026-10-22 | Core Team | Requirement-ID prefix renamed `ES` → `EXS` so that it names one specification in the workspace (the person's decision, design-layer audit 2026-10-22; RULES §1 as amended in 1.8.0): every definition of this spec's invariants and every citation of them across the corpus now reads `EXS-n`. No requirement was added, removed or reworded. |
| 1.0.1 | 2026-08-19 | Core Team | Related Specifications record the new `l1-execution-locus` as **orthogonal and composed**: ES says how confined execution is on each axis, LOC says which world it happens in — a locus may be confined or unconfined, and selecting one never relaxes an axis (LOC-7). Link-only; no invariant added or changed. |
| 1.0.0 | 2026-06-26 | Core Team | Initial spec — OS-level execution confinement of agent-run code: deny-by-default across four axes (operations/privileges/resources/filesystem+image), operation allowlist, least-privilege capability drop, resource bounds, read-only root, runtime-image digest pinning, explicit audited escalation, fail-closed for untrusted code, independent layered barriers (EXS-1…EXS-9). |
