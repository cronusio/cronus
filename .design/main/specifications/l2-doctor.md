# Doctor

**Version:** 1.1.1
**Status:** Stable
**Layer:** implementation
**Implements:** l1-doctor.md

## Overview

The concrete self-healing service: the health checks it runs, which problems it auto-repairs versus escalates, how it recovers after a crash, and the `doctor` command.

## Related Specifications

- [l1-doctor.md](l1-doctor.md) - The model this implements.
- [l2-scheduler.md](l2-scheduler.md) - Periodic checks run as a routine.
- [l2-github-issue.md](l2-github-issue.md) - Unrepairable issues may be reported with consent.
- [l2-cli.md](l2-cli.md) - Command grammar standard.

## 1. Motivation

The model needs concrete checks and a clear safe/risky split so the office self-heals without risk.

## 2. Constraints & Assumptions

- Checks are read-only; repairs are explicit and logged.
- A scheduled routine runs checks unattended; `cronus doctor` runs them on demand. Whether the unattended routine may repair is the user's setting (HEAL-7), not the routine's.

## 3. Invariant Compliance (Layer 2 only)

| L1 Invariant | Implementation |
| --- | --- |
| HEAL-1 Continuous checks | A scheduled routine runs the check suite; `doctor` runs it on demand. |
| HEAL-2 Safe self-repair | Deterministic fixes (re-index, unstick obviously-finished cards, prune dangling sessions) run with `--fix`. |
| HEAL-3 Escalate risky | Ambiguous/destructive fixes are reported with a recommended action, not applied. |
| HEAL-4 Non-destructive | Checks read-only; repairs snapshot or are reversible. |
| HEAL-5 Traceable | Every check/repair writes to logs. |
| HEAL-6 Self-recovery | On startup, a recovery pass reconciles interrupted runs/sessions to a consistent state. |
| HEAL-7 User-governed healing authority | **Pending.** A `healing` setting with at least `observe` and `heal` (default) is to live on the human-write-only authority plane: under `observe`, `--fix` and the scheduled routine surface every would-be repair as a recommendation and apply none, and each suppressed repair is recorded. Checks and crash recovery (HEAL-1/4/6) are outside the setting's reach. Today `--fix` alone gates repair. |
| HEAL-8 Build-parity skew | **Pending.** The `[build-parity]` probe (§4.3) is to compare content-derived build identities — never file timestamps — of the halves that meet at a seam, report skew as its own named condition with the restart that clears it, and never repair it automatically; it is constitutive, so the HEAL-7 setting cannot disable it. |

## 4. Detailed Design

### 4.1 Check suite

| Check | Repair (safe) | Escalate (risky) |
| --- | --- | --- |
| store/index consistency | rebuild index from source text | corrupt source data |
| stuck `running` cards | re-queue clearly-abandoned cards | ambiguous in-progress work |
| dangling sessions | prune per session-routing | active-looking sessions |
| config validity | restore missing defaults | conflicting user config |
| disk/resource pressure | clear cache | low disk needing user action |
| crash recovery | resume from durable state | divergent partial writes |

### 4.2 Extensibility

Third-party packages (channel connectors, plugins, workspace extensions) can contribute their own health checks without modifying the core. Two registration paths are supported:

**Programmatic registration** (within the same process):

```text
[REFERENCE]
register_doctor_contribution(id: String, fn: DoctorCheckFn) -> void
// id should be namespaced, e.g. "myplugin.cron"
// fn: (ctx: DoctorRunContext) -> String[]
// Returns informational lines (empty list = nothing to report)
```

**Extension declaration** (third-party packages): an extension declares its checks in its manifest (`l2-extension-registry`), naming each check's id and the configuration keys it needs to read:

```text
[REFERENCE]
doctor_checks:
  - id: "myplugin.cron"
    reads_config: ["schedules"]
```

`DoctorRunContext` is passed to each extension check:

```text
[REFERENCE]
DoctorRunContext {
  cfg: ConfigView,       // read-only view of the keys the check declared — not the whole config
  cli_base_url: String,  // base URL for local service health checks
  timeout: f32,          // per-check timeout in seconds
  deep: bool             // true when --deep flag was passed
}
```

Extension execution order: manual registrations first (alphabetical by id), then manifest-declared checks (alphabetical by id). A third-party check is third-party code: it runs under the extension sandbox with the permissions its extension was granted (`l2-sandbox-policy`), so a panicking, erroring, or hanging check is contained there — it logs a warning and is skipped, never aborting the rest of the suite. An extension check reports; it never repairs. A repair is the core's own act, under HEAL-2/HEAL-3 and the user's HEAL-7 setting.

<!-- [ADDED] v1.1.0 -->
**Concurrent probe execution.** Checks and runbook probes are read-only and independent, so the suite executes them concurrently under a bounded cap (default 4) with the existing per-check `timeout` applied individually; suite wall-clock approaches the slowest probe instead of the sum. The report is assembled after all probes settle and is always rendered in the canonical (alphabetical/runbook) order above — execution order is a scheduling detail, output order is deterministic. Probes that touch the same exclusive resource (e.g. a repair dry-run over one database) declare an exclusivity key and serialize against each other only. `--fix` repairs never run concurrently with probes: the suite settles first, then repairs apply one at a time (HEAL-2 logging preserved).

### 4.3 Extended check runbook

Beyond the check suite in §4.1, the doctor runs a structured runbook that surfaces environmental and installation-layer issues the abstract checks can't reach. Each runbook step is a named probe; output is a pass/warn/fail line with a remediation hint on non-pass.

```text
[REFERENCE]
Runbook probes (in execution order):

[prereqs]
  For each tool an enabled capability needs (git for a version-controlled workspace;
  the platform's service manager when background activation is on):
    pass  — tool found in PATH, version meets minimum
    warn  — optional tool absent; the capability degrades as its spec states
    fail  — required tool missing; remediation = install command

[config]
  The state tier's configuration files (app.json, config.json, routing.json, models.json)
    pass  — each present, parseable, and carrying its required fields
    warn  — optional fields absent; defaults apply
    fail  — a file missing or unparseable; remediation = restore defaults (HEAL-2) or,
            when user content would be lost, a recommendation (HEAL-3)

[token]
  The daemon's bearer token and the internal loopback token (l2-security §4.5)
    pass  — present, of full length, readable only by the user
    warn  — missing; regenerated on the next daemon start
    fail  — the token file is readable by other accounts or its directory is not writable

[state-tree]
  The state tier matches l2-filesystem-layout §4.3
    pass  — every office's directories present; each database opens and reports a schema
            version this build understands (STO-9)
    warn  — a rebuildable cache (a wiki.db projection, a search index) is absent — rebuilt on use
    fail  — a database from a newer build, or one that fails its integrity check

[build-parity]
  HEAL-8: the build identity each running half loaded (frontend, engine, attached clients)
    pass  — all identities agree
    fail  — skew; names the halves that disagree and the restart that clears it (never auto-repaired)

[daemon]
  The always-on engine, when background activation is enabled:
    pass  — registered with the OS supervisor and listening on its configured bind
    warn  — not running; activation is off or the process was stopped on purpose
    (not fail — the engine may be intentionally stopped)

[skills]
  Shipped skill packs present in the program tier and counted:
    pass  — preset skill store exists, N packages found (report N)
    warn  — preset store absent or empty; remediation = repair the installation

[model-providers]
  Local model providers (l2-technology-stack §4.4), probed in parallel:
    pass  — at least one loopback provider answers
    warn  — none answers; local-first routing has no on-device candidate and says so
    (not fail — a cloud-only office is a valid, user-authorized configuration)

[search]
  A web search provider for deep research is configured (an extension):
    pass  — at least one provider registered and on the egress allowlist
    warn  — none; deep research reports that it cannot search rather than failing later
```

Each runbook probe is registered as an extension check (same mechanism as §4.2); operators can add custom probes for workspace-specific dependencies.

### 4.4 Command surface

| Action | CLI | TUI | Library (no code) |
| --- | --- | --- | --- |
| run checks | `cronus doctor` | `/doctor` | `doctor.check() -> Report` |
| run + safe repair | `cronus doctor --fix` | `/doctor --fix` | `doctor.repair() -> Report` |

## 5. Drawbacks & Alternatives

- **False positives unstick real work:** mitigated by conservative "clearly abandoned" criteria. <!-- TBD: abandonment criteria for stuck cards -->
- **Alternative — fix everything automatically:** rejected (HEAL-3).

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[DOCTOR]` | `.design/main/specifications/l1-doctor.md` | Invariants this implements |
| `[CLI]` | `.design/main/specifications/l2-cli.md` | Command grammar standard |

## Document History

| Version | Date | Notes |
| --- | --- | --- |
| 1.1.1 | 2026-09-23 | Consistency pass (2026-09-23): HEAL-7 (user-governed healing authority) and HEAL-8 (build-parity skew) were unmapped — Pending rows. The §4.3 runbook described another product's installation (a four-zone vault, a console build, a "bridge" extension, a foreign service's API URL) that exists nowhere in Cronus — rewritten against Cronus's state tier, tokens, daemon, model providers, search provider, and a build-parity probe. Third-party checks ran in-process with the whole configuration and used a Python packaging syntax — they are manifest-declared, sandboxed, see only declared config keys, and report without repairing. |
| 1.1.0 | 2026-07-04 | Concurrent probe execution (§4.2): read-only checks/probes run under a bounded cap with per-check timeouts; deterministic report ordering after settle; exclusivity keys for same-resource probes; `--fix` repairs stay serialized after the suite settles. History table added with this entry. |
