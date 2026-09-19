# Process Integrity & Hardening

**Version:** 1.1.0
**Status:** Stable
**Layer:** concept

## Overview

Hardening of the agent's own running process so that secrets held in memory cannot be exfiltrated through crash dumps, debugger attachment, or environment-based code injection. Where secret isolation keeps credentials out of files, backups, and logs, and the sandbox confines what agent-run *code* may touch, process integrity protects the *live process itself* — the address space where decrypted secrets, tokens, and model keys necessarily exist while in use. It is the in-memory, anti-tamper complement to those two.

## Related Specifications

- [l1-security.md](l1-security.md) - SEC-1 keeps secrets out of files/logs; this spec keeps them out of crash dumps and other processes' reach while in memory.
- [l1-storage-model.md](l1-storage-model.md) - STO-6 secret isolation at rest; process integrity is the runtime complement.
- [l2-sandbox-policy.md](l2-sandbox-policy.md) - The sandbox confines agent-run code; process integrity protects the supervising process itself.
- [l2-execution-sandbox.md](l2-execution-sandbox.md) - [ADDED v1.1.0] the spawn path where PI-5/PI-8 are applied to every child, and the engine self-hardening sequence that realizes PI-1…PI-4 and PI-6.

## 1. Motivation

A coding agent necessarily holds live secrets in memory: a decrypted API key, an OAuth token, a model-provider credential. File-level isolation does nothing for those — they are in the process address space the moment they are used. Three commonplace local vectors can lift them out: a **core dump** writes the entire address space to disk on a crash; a **debugger or tracer** attaching to the process reads (or rewrites) its memory live; and an **inherited environment variable** such as a library-preload or dynamic-loader override injects attacker code into the process or any child it spawns. None of these require privilege escalation or a sandbox escape — they are available to any local process running as the same user.

Closing them is cheap and early: disable dumps, refuse tracer attach, and strip the dangerous environment vectors before the process loads its first secret. The cost is negligible; the protected surface is every credential the agent will ever hold.

## 2. Constraints & Assumptions

- These protections defend against *local same-user* inspection and accidental disk exposure; they are not a defense against a privileged (root/admin) adversary, which is out of scope.
- Hardening steps are OS-specific in mechanism; this spec constrains the required *effects*, not the syscalls.
- Hardening must not break legitimate operation — child tools and sandboxes still run; only the dangerous inherited vectors are removed.

## 3. Core Invariants

Rules every Layer 2 implementation MUST NOT violate:

- **PI-1 (No crash-dump leakage):** the process disables core/crash dumps of its own address space, so an in-use secret is never written to disk by a fault.
- **PI-2 (No tracer attach):** the process refuses debugger/tracer attachment (the platform's ptrace-equivalent), so another same-user process cannot read or rewrite its memory at runtime.
- **PI-3 (Environment sanitization):** dangerous inherited environment influences — library-preload and dynamic-loader override vectors (e.g. preload/insert-libraries variables) — are removed before any untrusted input is processed, preventing code injection via the environment.
- **PI-4 (Early enforcement):** hardening is applied as early as possible in process startup — before any secret is loaded or any untrusted input is handled — so there is no unhardened window in which the process is exposed.
- **PI-5 (Child inheritance):** processes the agent spawns (tools, shells, sandboxes) inherit the sanitized environment and MUST NOT silently re-expose a stripped injection vector to themselves or their descendants.
- **PI-6 (Visible degradation):** where the host OS cannot honor a hardening step, the system records the gap in an auditable way and continues with reduced protection rather than failing to start — but the reduced posture is surfaced, never silent.
- **PI-7 (Scope boundary):** process integrity protects the live process; it does not replace secret-at-rest isolation (SEC-1 / STO-6) or the execution sandbox (SEC-6). The three are layered defenses, each covering what the others cannot.
- **PI-8 (A child carries only what it was meant to carry — the engine's own credentials are not inherited by default):** [ADDED v1.1.0] PI-5 says a child does not regain a stripped *injection* vector; this is its counterpart for *secrets*. A process the agent spawns inherits an explicit allowlist of the environment plus deliberate, recorded pass-throughs — not the parent's whole environment. **Secret-shaped variables** (by name — token, key, secret, password, credential, certificate — **and by value shape** — a private-key block, a provider-issued key, a bearer or session token, a credential embedded in a URL) and **the engine's own credentials** (the model-provider keys and session tokens it uses to do its own work) are stripped by default (SEC-1); the user's *general* environment may be passed to a child that is by declared design the user's own shell, as an audited exception, never as the default for agent-run code. Stripping is the default and pass-through is the exception, so a new secret-bearing variable is safe by omission rather than by someone remembering to list it. A stripped variable is recorded by name only, never by value (SEC-5), so the operator can see what a child did not receive.

> L2 specs cannot reach RFC status until all invariants here are addressed in their "Invariant Compliance" section.

## 4. Detailed Design

### 4.1 Hardening Sequence

```text
[REFERENCE]
pre_main_hardening():           // runs before main(), before any secret/input (PI-4)
    disable_core_dumps()        // PI-1
    deny_tracer_attach()        // PI-2 (platform ptrace-equivalent)
    scrub_env(["<preload-var>", "<loader-override-var>", ...])   // PI-3
    record_unsupported_steps()  // PI-6: note any step the OS refused
spawn_child(cmd):
    env := sanitized_env()      // PI-5: children inherit the scrubbed environment
    env := allowlist(env) + passthrough(cmd)   // PI-8: an explicit allowlist, not the parent's whole environment
    env := strip_secret_shaped(env)            // PI-8: by name and by value shape; the engine's own credentials never pass by default
    exec(cmd, env)
```

### 4.2 Layered Defense Map

| Vector | Without PI | With PI |
| --- | --- | --- |
| Crash → core dump on disk | full address space (secrets) dumped | dumps disabled (PI-1) |
| Local debugger attaches | reads/writes live memory | attach refused (PI-2) |
| Malicious preload/loader env | injects code into process + children | vectors stripped, not inherited (PI-3/PI-5) |
| Agent-run child reads the engine's own keys from its environment | a subverted tool exfiltrates the provider key it inherited | secret-shaped and engine-own variables not inherited by default (PI-8) |

### 4.3 Degradation Reporting

When a step cannot be applied (unsupported OS, missing capability), PI-6 requires it be recorded and surfaced (e.g. in the health/doctor surface and security audit log) so the operator knows the true posture rather than assuming full hardening.

## 5. Drawbacks & Alternatives

- **Debugging friction:** PI-2 also blocks *legitimate* debugging of the agent process; mitigated by a developer build/flag that relaxes hardening explicitly and auditably, never by default.
- **Not a privileged-adversary defense:** a root/admin attacker can override these; accepted and stated (scope boundary). PI raises the bar against the common same-user vectors, not against full host compromise.
- **Alternative — rely only on secret-at-rest isolation:** rejected; it leaves the entire in-use window (decrypted secrets in memory) exposed to dumps, tracers, and env injection.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[SECURITY]` | `.design/main/specifications/l1-security.md` | Secret isolation and sandboxed execution that PI complements in-memory. |
| `[STORAGE]` | `.design/main/specifications/l1-storage-model.md` | Secret-at-rest isolation (STO-6); PI is its runtime counterpart. |
| `[EXEC-SANDBOX]` | `.design/main/specifications/l2-execution-sandbox.md` | The Layer-2 realization: engine self-hardening and the child spawn path (PI-1…PI-8). |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.1.0 | 2026-09-19 | Core Team | Added PI-8 (a child carries only what it was meant to carry). PI-3/PI-5 kept injection vectors out of the environment; nothing kept the engine's own *credentials* out of it, so every agent-run child inherited the provider keys and session tokens the engine uses for its own work and a subverted tool could read them from its environment. PI-8: a child inherits an explicit allowlist plus recorded pass-throughs; secret-shaped variables (by name and by value shape) and the engine's own credentials are stripped by default, pass-through is the audited exception (so a new secret-bearing variable is safe by omission), a child that is by design the user's own shell may receive the general environment as a declared exception, and a stripped variable is recorded by name only. §4.1 spawn sequence and §4.2 map extended. Distilled from a cross-check of eight external agent command-line tools: subprocess environment filtering by variable name and by value pattern, with a separate hard blocklist for code-injection variables, recurred across the strongest of them. |
| 1.0.0 | 2026-06-26 | Core Team | Initial spec — process integrity & hardening: disable crash dumps, refuse tracer attach, scrub injection-vector environment variables, early enforcement, child inheritance, visible degradation, layered-defense scope boundary (PI-1…PI-7). |
