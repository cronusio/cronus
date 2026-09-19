# Execution Sandbox

**Version:** 1.0.1
**Status:** Stable
**Layer:** implementation
**Implements:** l1-execution-sandbox.md, l1-process-integrity.md

## Overview

The enforcement mechanism beneath the sandbox policy: what actually confines a child process the agent starts, on each platform, how the engine hardens itself, and how the sandbox's coverage is stated. `l2-sandbox-policy` is the **schema** — which endpoints, binaries and paths are permitted. This spec is the **mechanism** that makes "not permitted" true, and the honest statement of everything it does not cover. A pattern guard (`l2-tool-security`) is the cheap first layer in front of it; this is the boundary behind (`l1-security` SEC-12).

## Related Specifications

- [l1-execution-sandbox.md](l1-execution-sandbox.md) - The nine confinement invariants this spec realizes (cited here as "ES-n of l1-execution-sandbox": the prefix is also used by `l1-evaluation-suites`, which is unrelated).
- [l1-process-integrity.md](l1-process-integrity.md) - PI-1…PI-8: engine self-hardening and the child spawn path realized here.
- [l1-security.md](l1-security.md) - SEC-6 (sandboxed execution), SEC-8 (embedded vs remote confinement), SEC-10 (the agent cannot write its own policy), SEC-12 (boundary versus heuristic; coverage is enumerated).
- [l2-security.md](l2-security.md) - §4.3 used to carry only the requirement and an open backend question; it now points here. Also §4.4 SSRF guard, applied by the egress proxy below.
- [l2-sandbox-policy.md](l2-sandbox-policy.md) - The policy this backend enforces: named network entries, binary allowlists by absolute path, isolation tiers, and the access-failure classification §4.8 produces.
- [l2-tool-security.md](l2-tool-security.md) - The heuristic guard in front (command analysis, path containment, `self_disruption` category); never counted as boundary.
- [l2-agent-autonomy.md](l2-agent-autonomy.md) - The approval path an expansion request travels; unattended callers get a visible refusal (AG-9).
- [l2-execution-workspace.md](l2-execution-workspace.md) - The worktree/scratch root that is the only default writable area.
- [l1-action-gating.md](l1-action-gating.md) - AG-9 (a gate only where the answer can be given), AG-10 (untrusted-derived arguments at a privileged sink), AG-11 (a reviewer can add friction, never grant).
- [l1-consent-binding.md](l1-consent-binding.md) - CB-1/CB-2 (consent binds the resolved invocation) drive the path-resolution rule; CB-5 (consent is authorization, never containment) is why an approval never substitutes for this spec.
- [l1-interception-model.md](l1-interception-model.md) - INT-2 (check-before-use, TOCTOU-safe) and INT-8 (honest coverage boundary), applied to confinement.
- [l1-capability-reachability.md](l1-capability-reachability.md) - REA-1: the human-only shell surface the agent cannot reach (§4.12).
- [l2-crate-topology.md](l2-crate-topology.md) - Where the port and the per-platform adapters live (§4.14).
- [l2-doctor.md](l2-doctor.md) - The health surface that carries the probe result and the degradation record.

## 1. Motivation

Policy without enforcement is a document. `l2-sandbox-policy` says which binaries may reach which endpoints; nothing in the codebase yet makes an agent-started process unable to do otherwise. No shipped verb executes tools; the one piece of code that would — the quality-gate runner — is dormant, starts project tools by bare name with the engine's whole environment, and does its process I/O inside the tier documented as having none. `check run` deliberately invokes nothing yet, which is the cheap moment to put the runner behind a port. `l1-execution-sandbox` states nine invariants and had no Layer-2 owner; `l2-security` §4.3 restated the requirement in two lines and left the backend as an open question.

Two facts shape the answer. First, **a boundary and a heuristic are different things** (SEC-12). The runtime guard parses and pattern-matches; parsers and pattern lists are defeated one unanticipated input at a time, so the guard can never be the only thing between an adversarial or subverted process and the host. Second, **the platforms differ**, and the project is developed and used on Windows first: a design that quietly assumes Linux namespaces leaves the primary platform with a policy file and no enforcement. This spec therefore names a native backend per platform, built from primitives an ordinary user can use without administrator rights, behind one port — and says plainly where each falls short.

## 2. Constraints & Assumptions

- Rust, one binary (INV-8). The backend is an adapter behind a port: the domain tier holds the policy and contract types and no OS code; platform code lives in an adapter crate (§4.14).
- The backend **only enforces** the policy the host wrote; it can never relax it and never authors it (SEC-10). A confined child and the agent that requested it have no path to the policy file.
- Enforcement is **per spawned child**. The engine process itself is protected by self-hardening (§4.11), not by this confinement.
- Primitives are chosen for availability to a non-administrator, because a control that needs elevation to enable is a control that ships off.
- A host that cannot honor a required confinement control **refuses** to run untrusted code (ES-8 of l1-execution-sandbox); it does not degrade quietly. Own-process hardening may degrade, visibly (PI-6).
- Tests that need a real operating system cannot all run on every developer host; a backend that cannot be exercised in the current environment is reported `unverified`, never `passed`.

## 3. Invariant Compliance (Layer 2 only)

| L1 Invariant | Implementation |
| --- | --- |
| ES-1 of l1-execution-sandbox — Deny-by-default across axes | Every axis (operations, privileges, resources, filesystem, network) starts denied in `prepare()`; the resolved policy adds grants explicitly and minimally (§4.1, §4.5, §4.6). |
| ES-2 — Operation allowlist | Linux: a syscall allowlist (seccomp) that kills on escape primitives; macOS: a generated profile denying by default; Windows: the restricted token and job limits remove the operations a child does not need (§4.1). |
| ES-3 — Least privilege | Restricted token / dropped capabilities / no-new-privileges; setuid-style elevation and privilege-gaining on exec are disabled (§4.1). |
| ES-4 — Resource bounds | Job Object limits / cgroup-or-rlimit per child; exceeding a bound terminates the child tree, never the host (§4.7). |
| ES-5 — Filesystem confinement | Read-only root except declared writable roots; resolve-then-open; anchors and protected files unwritable even inside a writable root (§4.5). |
| ES-6 — Runtime-image integrity | The container backend pins its image by content digest and verifies before use; a mutable tag is refused (§4.1). |
| ES-7 — Explicit, audited escalation | A typed denial leads to a fresh, minimal expansion request through the approval path; never an automatic unconfined retry (§4.8). |
| ES-8 — Fail-closed for untrusted code | `select()` refuses with a typed `SandboxUnavailable` when no backend probes available, and `prepare()` refuses with `Unenforceable{axis}` when a required grant cannot be enforced on this host (§4.1); the only unconfined run, or unconfined axis, is a human-authored, governed, persistently-marked policy value (`sandbox.enabled = false`, or an axis in `accept_unconfined`; §4.2). |
| ES-9 — Independent, layered barriers | Confinement is independent of the egress proxy's policy and of the heuristic guard; a bypass of the guard grants nothing the backend denies (§4.6, §4.9). |
| PI-1 — No crash-dump leakage | Dumps disabled for the engine process at startup (§4.11). |
| PI-2 — No tracer attach | Attach refused where the platform offers a mechanism; the Windows equivalent is a restricted process descriptor, recorded as partial (§4.11). |
| PI-3 — Environment sanitization | Loader-override, preload and interpreter-startup vectors removed before untrusted input is handled (§4.4, §4.11). |
| PI-4 — Early enforcement | The hardening sequence runs before configuration, secrets or input are touched (§4.11). |
| PI-5 — Child inheritance | Every child is spawned from the sanitized environment and cannot regain a stripped vector (§4.3, §4.4). |
| PI-6 — Visible degradation | Every unavailable step is recorded (audit and doctor) with what remains unprotected (§4.11, §4.2). |
| PI-7 — Scope boundary | This spec is one of three layers with secret-at-rest isolation and the process hardening; none is claimed to replace another (§4.9). |
| PI-8 — A child carries only what it was meant to carry | `EnvPlan` is an allowlist plus recorded pass-throughs; secret-shaped and engine-own variables are stripped by default (§4.4). |

Related invariants realized here:

| Invariant | Implementation |
| --- | --- |
| SEC-6 Sandboxed execution / SEC-8 confinement modes | §4.1 (embedded backends and the `remote` backend behind one port); host paths mount only into embedded confinement. |
| SEC-10 Authority self-containment | The policy file and the engine's own configuration are not writable by any child (§4.5); expansion is a request, never a self-grant (§4.8). |
| SEC-12 Boundary versus heuristic | The coverage declaration (§4.9) is the single source for the word "sandboxed"; unconfined paths are labeled, not dressed up. |
| INT-8 Honest coverage boundary | §4.9 enumerates each way work can begin. |
| AG-9 A gate where the answer can be given | An unattended caller is refused visibly, never queued (§4.13). |

## 4. Detailed Design

### 4.1 The port and the backends

```text
[REFERENCE]
port SandboxBackend {
    kind()      -> native_windows | native_macos | native_linux | container | remote
    probe()     -> Available{version, capabilities} | Unavailable{reason}   // executes the runtime; PATH presence is not availability
    prepare(policy: ResolvedPolicy, root: ExecRoot) -> Prepared | PrepareError
    spawn(prepared, spec: SpawnSpec) -> Child | SpawnError
    classify(exit, captured_streams) -> Denial?                              // §4.8
    coverage() -> CoverageDeclaration                                        // §4.9
}

SpawnSpec { argv: [OsString], cwd: Path, env: EnvPlan, stdin, limits: Limits, deadline: Duration }
```

`argv` is an argument vector. There is no string field a shell could parse: a component that wants a shell composes the argv `[shell, "-c", text]`, and that shell is the confined child (§4.3).

| Platform | Default backend | Primitives, applied together | Unavailable when |
| --- | --- | --- | --- |
| Windows | `native_windows` | two profiles behind one backend. **`compat`**: a restricted token (privileges stripped, deny-only groups), a low integrity level, writable roots labeled for it, and a Job Object (kill-on-close, no breakaway, process/memory/CPU/handle limits, UI restrictions) — confines filesystem, privileges and resources and does **not** confine the network. **`strict`**: an AppContainer profile — no network access at all, file access only to the roots granted to its identity — at the cost of compatibility with tools that need the registry, the user profile or shared temporary locations | the token, integrity level, label or job assignment is refused (for example the process already sits in an incompatible job); `strict` also needs its profile to be creatable |
| macOS | `native_macos` | a generated Seatbelt profile run through the system `sandbox-exec` at its fixed absolute path: per-root read/write allowlists, `unlink`/rename denied on writable-root anchors and protected metadata, default-deny network except the proxy | the launcher is absent or the profile does not compile |
| Linux | `native_linux` | user, mount, pid and network namespaces (fresh `/proc`, private `/tmp`, workspace bind), a syscall allowlist that kills on `ptrace` and namespace-escape primitives, Landlock where the kernel has it, no-new-privileges | unprivileged user namespaces are disabled and Landlock is absent |
| any | `container` | a container runtime with the image pinned by content digest (ES-6), read-only root, dropped capabilities, no-new-privileges, resource limits, network none or proxy-only | the runtime does not answer a `version` call |
| any | `remote` | SEC-8 remote confinement: an authenticated separate manager; a client-local path is never mounted into it | the manager is unreachable or unauthenticated |

Windows confinement has no namespace equivalent, and no primitive available without administrator rights filters sockets per endpoint. **Enforceability is therefore a property of the axis, not of the backend.** `prepare()` returns `Unenforceable{axis}` when the resolved policy asks for a grant the backend cannot enforce on this host — per-endpoint network in the Windows `compat` profile (the token cannot filter sockets), or a proxy route in `strict` before the one-time loopback exemption of §4.6 has been performed. An unenforceable axis is refused like an unavailable backend (ES-8 of l1-execution-sandbox) unless the human explicitly accepts that axis as unconfined — the `accept_unconfined` list of the host-written policy file (`l2-sandbox-policy` §4.1, an authority-plane write, SEC-10); the acceptance is recorded, shown by `cronus sandbox status`, and stamped on every surface that runs under it (SEC-12(b)). Windows thus offers two honest postures for the network axis — *denied entirely* (`strict`) or *unconfined and labeled as such* (`compat`) — and never a policy file that says "only these hosts" beside a child that can reach any.

### 4.2 Selection, probing and refusal

```text
[REFERENCE]
select(policy, platform) -> Backend | SandboxUnavailable{tried: [(kind, reason)]}:
    for kind in [policy.backend] + platform_default(platform):        // configured first, then the default
        p := backend(kind).probe()                                    // runs the runtime; a stopped daemon fails HERE, not at first use
        if p is Available: audit("sandbox_selected", kind, p, policy.digest); return backend(kind)
    return SandboxUnavailable{...}                                    // ES-8: refuse, name what was tried, say what to do
```

The only route to an unconfined run is the human-authored policy value `sandbox.enabled = false` (the key the trust dialog already warns about, `l2-security` §4.8) — or, for a single axis, its name in `accept_unconfined` (`l2-sandbox-policy` §4.1); `isolation_compatibility: best_effort` means exactly that and nothing looser. It is a governed escape hatch (`l1-policy-governance` PG-6 — the managed tier can remove it), it is never a default of any profile, the agent cannot set it (SEC-10), and every surface that runs under it carries a persistent *unconfined* marker. Probe results and the selected backend appear in `cronus sandbox status` and the doctor report.

### 4.3 Spawn discipline

- **Argument vectors only.** The engine never builds a shell string to start a confined child, and a backend never shells out to launch one. When the agent's tool is "run a shell command", the shell is itself the confined child; the guard analyses the text *before* spawn (`l2-tool-security` §4.2) and the confinement holds regardless of what that analysis concluded.
- **`--` before operands.** Wherever the engine builds the argv for a program that takes options, operands that originate outside the engine follow a `--` separator so a flag-shaped value cannot be read as an option.
- **Trust the resolved path, never the name.** The executable is resolved to an absolute canonical path *before* any allowlist or trust decision (`l2-sandbox-policy` §4.4), that path is pinned at fork and verified at exec, and the resolver is explicit rather than the operating system's search: no current-directory search, no shim or alias directories, extension resolution on Windows made explicit. The same name resolves differently under a different `PATH`; consent and allowlists bind the resolved identity (CB-1, CB-2).
- **The launcher is not found by search.** A platform launcher (`sandbox-exec`, the namespace helper) is invoked by a fixed absolute path.

### 4.4 Environment

`EnvPlan` is a **plan**, not the parent's environment (PI-8): an allowlist of variables the child needs, plus pass-throughs recorded by name with the reason.

| Removed by default | Why |
| --- | --- |
| Secret-shaped variables — by name (token, key, secret, password, credential, certificate) and by value shape (a private-key block, a provider-issued key, a bearer or session token, a credential inside a URL) | a subverted child must not read what the engine holds |
| The engine's own credentials (model-provider keys, session and pairing tokens) | never inherited, whatever their name |
| Dynamic-loader and preload overrides | code injection into the child and its descendants (PI-3) |
| Interpreter startup hooks and options that load code before the program runs, and shell startup-file variables | code runs before the requested command does |

A child that is by declared design the user's own shell may receive the general environment as an audited exception (PI-8). A stripped variable is logged by name only (SEC-5).

### 4.5 Filesystem confinement

- **Writable roots** are the execution workspace or worktree (`l2-execution-workspace`) and declared scratch — nothing else by default. Everything outside is read-only or not visible.
- **Resolve, then open, then verify the handle.** Containment is decided on the object that will actually be used, not on a path string checked earlier (INT-2). A path whose resolution changes between check and use is a denial, not a race to win.
- **Escape shapes the backends must refuse:** a symlink, junction or reparse-point swapped after the check; a hard link created inside a root to a file outside it; a rename that crosses the root boundary; `..`, drive-relative, UNC and device-namespace paths; alternate data streams; short (8.3) names; comparison on a case-insensitive volume done case-sensitively.
- **Anchors and protected files are unwritable even inside a writable root:** the root directory itself, the repository metadata directory, the sandbox policy file, the engine's configuration and state directory, and standing-instruction files (`l2-agent-constitution`). These are the authority plane and its persistence vectors (SEC-10, AG-10).
- A write outside the declared roots is a typed denial (§4.8), never a silent failure.

### 4.6 Network

Per-endpoint policy from `l2-sandbox-policy` is enforced by making a **host-side egress proxy the only route**: the confined child has no path to a direct socket. The proxy applies the named entries and binary allowlists, the SSRF guard (`l2-security` §4.4) and the audit record. Default is no network at all.

| Platform | How the proxy becomes the only route |
| --- | --- |
| Linux | a network namespace with no interface (deny-all); when the policy grants endpoints, a host proxy reachable through a bind-mounted Unix socket with an in-sandbox forwarder |
| macOS | the profile permits outbound connection to the proxy socket only |
| Windows | `strict` denies the network entirely. Reaching a granted endpoint through the proxy additionally needs a **one-time elevated loopback exemption** for the profile — a setup step performed only if the human wants endpoint grants; without it the grant is `Unenforceable` (§4.1). `compat` does not confine the network at all. |

### 4.7 Resource bounds

CPU time, memory, process and thread count, open handles, output bytes and wall time are bounded per child. Exceeding a bound terminates the **child tree** — never the host — and yields `Denial{axis: resource}`. Output is size-capped with an explicit truncation marker so a flood cannot fill the transcript or the disk.

### 4.8 Denial detection and expansion

```text
[REFERENCE]
Denial { axis: fs | net | proc | priv | resource, target?: String, evidence: String, retriable: bool }

classify(exit, streams):     // exit status, permission-shaped errors on stderr, the backend's own audit signal
    -> Denial? ; unknown failures are NOT reported as denials (no false confinement blame)
```

The tool result carries the typed denial, not a raw diagnostic (SEC-11). An expansion — one more path, one more endpoint — is a **fresh, minimal request** through the approval path at the narrowest unit (ES-7 of l1-execution-sandbox); it is never an automatic retry outside the confinement, and the agent cannot widen the policy itself (SEC-10). Where no answering surface exists (§4.13) the outcome is a visible refusal naming what could not be asked.

### 4.9 Coverage declaration

Every backend returns a `CoverageDeclaration`; the table below is the union the product may quote. Any surface, prompt or document that says work is "sandboxed" derives the word from this table, and **a launch path not listed here is unconfined until it is added** (SEC-12(d)).

| Launch path | Confined by this backend? | What holds it otherwise |
| --- | --- | --- |
| Agent-run child process (tool execution, shell tool) | **yes** | this spec |
| Quality-gate tool runs (`check run`, when enabled) | **yes** when routed through the backend — the runner takes a process port for exactly this | this spec |
| In-process tools (file read/edit, code graph, memory, retrieval) | **no** — they run inside the engine | path containment and the tier gate (`l2-tool-security` §4.2); never described as sandboxed |
| Tool-server (MCP) subprocess | yes when launched through the backend — the default for servers not shipped with the product | admission scanning, hash-bound trust (`l2-security` §4.8), the tier gate |
| Hook commands | yes when routed through the backend — the default for project hooks | the hook trust registry and fingerprint (`l2-security` §4.8) |
| Extension or plugin code loaded in-process | **no** | admission (`l1-extension-marketplace` XM-8, `l1-component-scanning`), the grant (EXT-3); labeled unconfined |
| A command the human types at a human-only surface | **no** — the human's own authority (§4.12) | unreachable by the agent (REA-1) |
| The engine itself | **no** | self-hardening (§4.11) |

A confinement that shares the guarded process's memory, filesystem and credentials — running a plugin "sandboxed" inside the engine, for instance — is **not offered** as a boundary (SEC-12(c)): the row above says *no* and the plugin is gated by admission and grants instead.

### 4.10 Self-disruption protection

A confined child cannot signal, terminate, suspend or debug the engine, its service registration, or its other children: a Job Object without breakaway and a token that lacks the terminate right on the engine (Windows); a pid namespace and a syscall filter denying signals to foreign pids and `ptrace` (Linux); a profile denying process-control outside the child's own tree (macOS). Overwriting or unlinking the engine binary, its service unit or its state is a filesystem denial (§4.5). The guard also classifies such commands as `self_disruption` before spawn (`l2-tool-security` §4.2) — the heuristic in front, the boundary behind.

### 4.11 Engine self-hardening (PI-1…PI-6)

```text
[REFERENCE]
pre_main_hardening():                   // first statements of main — before configuration, secrets or input (PI-4)
    disable_core_dumps()                // PI-1: process-level dump exclusion + zero core-size limit (each platform's own means)
    deny_tracer_attach()                // PI-2: not-dumpable / deny-attach where offered;
                                        //   Windows has no exact equivalent: restrict the process object's descriptor so even
                                        //   same-user callers lose memory-read/write, thread-creation and handle-duplication rights
    scrub_env(loader_and_preload_vectors)   // PI-3
    record_unsupported(step, what_remains)  // PI-6: to the audit log and the doctor surface, naming what is NOT protected
```

A step the platform cannot honor is recorded as **partial** with what remains exposed; the engine still starts (PI-6). This is the opposite default from §4.2, deliberately: hardening the engine may degrade visibly, executing untrusted code may not.

### 4.12 Human-typed versus agent-issued

A command a person types at a human-only surface runs with **their** authority, outside the agent's confinement, and the agent has no path to that surface (REA-1): neither the TUI command bar (which dispatches registry invocables) nor any tool hands the model a shell through it. No surface presents the human's own terminal as covered by the agent sandbox, and the coverage table says so (§4.9).

### 4.13 Unattended callers

A scheduled or background run has no answering surface (AG-9). A denial that would need an expansion is a visible refusal naming what could not be asked. The request may be *recorded* for a person to review later — never auto-granted — and the run neither waits on it nor proceeds past it.

### 4.14 Crate placement and the conformance corpus

- The port and its contract types (`SpawnSpec`, `EnvPlan`, `Denial`, `CoverageDeclaration`, `ResolvedPolicy`) live in the contract/domain tiers with no OS dependency. The per-platform backends live in **one adapter crate** with platform code behind `cfg(target_os)`, in the same tier as the other operating-system adapters; a container or remote backend becomes its own adapter only if it brings third-party dependencies (the crate-minting rule of `l2-crate-topology` §4.4). The facade selects and wires the backend; a frontend never names a backend type (INV-2, INV-10).
- **Escape corpus.** One corpus of attempts runs against every backend: write outside the root; a symlink or junction swapped between check and use; a hard link to an outside file; a rename across the boundary; a secret-shaped variable and an engine key in the parent environment; a direct socket connect; a request to a metadata address; a fork bomb and a memory hog; signalling the engine; opening the policy file or a standing-instruction file for write; a preload variable; an output flood. Each attempt's result is `passed`, `failed` or `unverified`; a backend that cannot run on the current host is `unverified`, never `passed`. Each platform has an always-on lane where CI provides the host, and the usage-simulation harness (`l2-simulation-suite`) may drive the same corpus end to end.

### 4.15 Command surface

| Action | CLI | TUI | Notes |
| --- | --- | --- | --- |
| Show the selected backend, probe results, policy digest and the coverage table | `cronus sandbox status` | `/sandbox status` | read-only; also feeds the doctor report |
| Re-run the probes | `cronus sandbox probe` | `/sandbox probe` | executes each backend's self-test; no policy change |
| Explain the last denial | `cronus sandbox explain <run-id>` | `/sandbox explain <run-id>` | the typed `Denial` and the narrowest expansion request it implies |

Per INV-9 the group appears on the shipped surface only when the port and at least one backend exist; until then it has no descriptor rather than answering "unavailable".

## 5. Drawbacks & Alternatives

- **Three native backends are more work than one.** Accepted: the port, one conformance corpus and one coverage table are shared, so the cost is the per-platform primitive code, not three designs. The alternative — a single container backend — is rejected as the *default*: on Windows and macOS a container runtime means a virtual machine the user did not choose to install, and a default that needs installing is a default that ships off.
- **Windows has no namespace equivalent and no non-administrator socket filter.** The network axis is *denied entirely* (`strict`) or *unconfined and labeled* (`compat`); per-endpoint grants need a one-time elevated setup step or a container or remote backend. The coverage table and `sandbox status` say so, and `Unenforceable{axis}` keeps a policy file from claiming more than the host can enforce.
- **`sandbox-exec` is deprecated but functional.** The probe (§4.2) is what protects a user on a system where it stops working: the backend reports `Unavailable`, selection refuses, and nothing runs unconfined by accident.
- **Landlock and unprivileged namespaces are not universal on Linux.** The backend needs at least one; without either it is `Unavailable` and selection falls to a container backend or refuses.
- **A guard that is good enough is tempting.** Rejected by SEC-12: the guard is a heuristic, useful and legible, and stays in front. Nothing here is weakened by its existence and nothing here is replaced by it.
- **Confining plugin code in-process.** Rejected outright (SEC-12(c)): the engine's own memory, filesystem and credentials are shared with it, so the confinement would be a label. Plugin code is gated by admission and grants and listed as unconfined.
- **Retrying a denied command outside the confinement automatically.** Rejected: it turns every denial into an escalation nobody approved. The retry is a fresh, minimal request a person answers (§4.8).

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[ES]` | `.design/main/specifications/l1-execution-sandbox.md` | The nine confinement invariants realized here |
| `[PI]` | `.design/main/specifications/l1-process-integrity.md` | PI-1…PI-8 — self-hardening and the child spawn path |
| `[SECURITY]` | `.design/main/specifications/l1-security.md` | SEC-6, SEC-8, SEC-10, SEC-12 |
| `[POLICY]` | `.design/main/specifications/l2-sandbox-policy.md` | The policy schema this backend enforces; denial classification |
| `[TOOLSEC]` | `.design/main/specifications/l2-tool-security.md` | The heuristic guard in front |
| `[L2SEC]` | `.design/main/specifications/l2-security.md` | SSRF guard, trust registry, the pointer replacing the old backend TBD |
| `[TOPOLOGY]` | `.design/main/specifications/l2-crate-topology.md` | Tier and crate-minting rules for the port and the adapters |

## Document History

| Version | Date | Author | Notes |
| --- | --- | --- | --- |
| 1.0.1 | 2026-09-19 | Core Team | Correction found while decomposing the spec into tasks: the first revision claimed per-endpoint network enforcement on Windows through a packet-filter rule conditioned on the child's token, which needs administrator rights and contradicts the spec's own constraint that primitives be available without them. Windows now has two named profiles — `compat` (restricted token, low integrity, labeled writable roots, Job Object; the network is not confined) and `strict` (AppContainer; no network at all, granted roots only) — and enforceability is stated as a property of the axis: `prepare()` returns `Unenforceable{axis}` and the host refuses unless the human accepts the axis as unconfined, recorded and stamped on every surface. Linux proxy route restated (no-interface namespace plus a bind-mounted socket). ES-8 compliance row extended. The concrete acceptance channel is the `accept_unconfined` list added to `l2-sandbox-policy` 1.1.0, which also retires that spec's silent-downgrade reading of `best_effort`. Motivation corrected to name the dormant quality-gate runner and §4.9 gained its row. |
| 1.0.0 | 2026-09-19 | Core Team | Initial spec — the execution-confinement mechanism `l1-execution-sandbox` never had a Layer-2 owner for, and the engine self-hardening `l1-process-integrity` left unrealized. One port with a native backend per platform built from primitives available without administrator rights (restricted token + Job Object + low integrity on Windows; a generated Seatbelt profile on macOS; namespaces + syscall filter + Landlock on Linux), plus container and remote backends; selection by probing — running the runtime, not finding it on `PATH` — and refusal, not degradation, when none is available; argument-vector spawning, `--` before external operands, trust by resolved absolute path pinned at fork; environment as an allowlist plan with secret-shaped and engine-own variables stripped; filesystem confinement by resolve-then-open with an explicit escape-shape list and protected anchors; a host-side egress proxy as the only network route; typed denial and a fresh minimal expansion request instead of an unconfined retry; a coverage declaration that enumerates every launch path including the negative ones (SEC-12); self-disruption protection; engine self-hardening with recorded partials; the human-typed/agent-issued line; an unattended-caller rule; crate placement and one escape corpus whose result is `passed`, `failed` or `unverified`. Replaces the open "backend per OS" question in `l1-security` and `l2-security` §4.3. Distilled from a cross-check of eight external agent command-line tools against this corpus: the strongest of them run per-platform native confinement selected by probing, pin the launcher by absolute path, and state plainly which execution paths their sandbox does not cover. |
