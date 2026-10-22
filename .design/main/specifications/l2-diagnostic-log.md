# Diagnostic Log (Logging Realization)

**Version:** 1.0.1
**Status:** Stable
**Layer:** implementation
**Implements:** l1-diagnostic-log.md, l1-log-legibility.md

## Overview

How the Rust product logs: one structured event model for every crate, a sink installed first thing in `main` of every process, a rotating JSON-lines file per process kind under the OS logs location, a human console projection, a live tail for every surface, a compact feed for agents, and redaction applied where each projection is rendered. Today no crate emits a single log event; this spec is the target the first one is written against.

## Related Specifications

- [l1-diagnostic-log.md](l1-diagnostic-log.md) - DLG-1…DLG-8, the forensic plane this realizes.
- [l1-log-legibility.md](l1-log-legibility.md) - LL-1…LL-9, the event model and its projections.
- [l2-filesystem-layout.md](l2-filesystem-layout.md) - §4.1 `<logs>` location per OS; logs are machine-local and excluded from backups by default.
- [l2-invocable-registry.md](l2-invocable-registry.md) - §4.12 redaction at the boundary and the per-invocable "do not journal input" declaration reused for log fields.
- [l2-tool-receipts.md](l2-tool-receipts.md) - The execution-authenticity record; a different plane, never fed from the log (DLG-1).
- [l2-github-issue.md](l2-github-issue.md) - The issue-report bundle, the log's one egress path besides telemetry (DLG-6).
- [l2-technology-stack.md](l2-technology-stack.md) - The logging crates added to the stack.
- [l2-crate-topology.md](l2-crate-topology.md) - Which tier may install a sink (binaries only) and which may only emit (every library tier).

## 1. Motivation

The engineering rules require descriptive structured events and critical errors logged with enough context to debug a session, yet the code has no logging at all: a failed background turn, a refused migration or a crash leaves nothing behind but what reached a terminal. Without a single specified model, the first logging added would be whatever each crate's author reached for — formatted strings on standard error here, a file there — exactly the divergence the legibility contract forbids.

## 2. Constraints & Assumptions

- Library crates emit events and never install a subscriber; only the `cronus` binary (and the desktop shell's host) install one.
- Event emission costs next to nothing when the event's level is filtered out.
- Logs are machine-local operational data; they are never committed, synced or backed up by default.
- **Dependency added (justified):** the `tracing` family (event facade, subscriber, non-blocking file writer). The standard library offers no structured logging; this family is the ecosystem's common facade, so dependencies that already emit through it are captured without adapters.

## 3. Invariant Compliance (Layer 2 only)

| L1 Invariant | Implementation |
| --- | --- |
| DLG-1 Distinct plane | The log is its own sink. Receipts, the session journal, health scores, telemetry and the dispatch journal (`l2-invocable-registry` §4.13) are written by their owners and never derived from log lines; a log line may *mention* their identifiers for correlation (LL-6). |
| DLG-2 Survives what other planes cannot | The sink is the first thing `main` installs, before argument parsing finishes resolving configuration. A panic hook writes the panic, its location and a backtrace to the log before the default hook runs. Standard error of spawned subprocesses (model servers, tools) is forwarded into the log under the subprocess's name. A native-fault handler is **Pending** per OS; where absent, `log status` reports *native crash capture: unavailable* rather than implying coverage. |
| DLG-3 Session header | The first event of every process is `session.start` with OS, architecture, product version and build, process kind (`cli`, `tui`, `engine`, `desktop`), working directory, arguments (secret-marked ones replaced by `<redacted>`), state root and start time. |
| DLG-4 Own scope full, dependencies clamped | Default filter: `cronus*` targets at `info`, everything else at `warn`. `log.level` (setting) and `CRONUS_LOG` (environment, same filter syntax) override; the dependency floor is a separate value so raising the product's verbosity does not flood the file. |
| DLG-5 Bounded, rotating | One file family per process kind: `<logs>/<kind>.jsonl`, rotated at 10 MiB, 5 rotations kept (`log.max_size`, `log.max_files`). The oldest rotation is dropped silently by design. |
| DLG-6 Local, consent-gated, secret-safe | Nothing reads the log for egress except the issue-report bundle (preview, scrub, consent) and the telemetry opt-in. Backup excludes `<logs>` unless `--include-logs`. |
| DLG-7 Live inspection at parity | `log show [--level] [--grep] [--since] [--kind]`, `log tail`, `log path`, `log status` are `Inspect` installation verbs, answered without composing the office; the terminal UI renders them under `/log …`, the desktop in a log panel over the same core functions. |
| DLG-8 Neutral sink | Writes go through a non-blocking writer with a bounded queue; when the queue is full events are dropped and counted, and the count is written as one `log.dropped` event when space returns. A failed open or rotation retries on the next write and never panics or returns an error into the caller. |
| LL-1 One event, projections | Every fact is one `tracing` event. Projections: the JSON-lines file (complete), the console (human), the live tail (stream), the agent feed (compact). No surface writes a log line of its own. |
| LL-2 Message and fields | Each event carries a stable message (`"backup created"`) and typed fields (`backup.id`, `bytes`, `duration_ms`); messages never embed values that belong in fields. |
| LL-3 Shaped per reader | Console: level, target, message, key fields on one line, payloads folded, `warn` and above by default for one-shot commands. File: every field. Agent feed: §4.4. |
| LL-4 Overload bounded | Level gating (DLG-4), a payload policy (`log.payload = descriptor` by default: sizes and counts, not bodies; `excerpt` and `full` opt-in), and folding of identical consecutive events into one with `repeat`. |
| LL-5 Honest reduction | A truncated field carries `truncated_from` with the original size; a folded event carries `repeat`; dropped events are counted (DLG-8). |
| LL-6 Attribution | Fields always present: `ts`, `level`, `target` (crate and module), `kind` (process), `session` (process run id), and when applicable `workspace`, `card`, `run`, `agent`, `invocation` (the dispatch pairing identity). |
| LL-7 Secret-safe at every projection | Every projection passes its text through the core redaction path with the live secret list before rendering; arguments of invocables declared "do not journal input" are never logged; field names on a denylist (`*token*`, `*secret*`, `*password*`, `authorization`) are always replaced. |
| LL-8 Agent economy | §4.4: tail-windowed, payload-free, folded, capped by a declared token budget. |
| LL-9 Stable schema | Each line carries `v` (schema version, starting at 1); fields are namespaced by subsystem; a schema change bumps `v` and `log show` reads every version it knows. |

## 4. Detailed Design

### 4.1 Installing the sink

```text
[REFERENCE]
main():
  guard := diagnostics::install(process_kind, logs_root())   // first statement; holds the writer alive
  emit session.start header                                   // DLG-3
  install panic hook (write panic + backtrace, then chain to the default hook)
  … parse, compose, run …
  drop(guard)                                                 // flushes the queue on exit
```

`logs_root()` resolves `<logs>` from the same path resolver as every other root; portable mode puts it under the portable directory.

### 4.2 Line format

```text
[REFERENCE]
{"v":1,"ts":"2026-10-22T09:14:03.120Z","level":"INFO","target":"cronus_domain::backup",
 "kind":"cli","session":"7f3c…","msg":"backup created","backup.id":"2026-10-22T09-14-03","bytes":482113,"duration_ms":214}
```

### 4.3 Levels

| Level | Use |
| --- | --- |
| `error` | an operation failed and the person or the office must act; carries enough context to reproduce |
| `warn` | something degraded but work continued (a fallback taken, a retry, a skipped item) |
| `info` | a state change worth knowing a day later (store migrated, engine seat taken, update staged) |
| `debug` | the steps of an operation |
| `trace` | per-item detail; never enabled by default |

### 4.4 Agent feed

`log tail --for-agent --budget <tokens>` returns the last events at `warn` and above (plus `info` for the named `card` or `run`), folded, payload-free, newest last, cut to the budget with a final line stating how many events were left out. It is the only form in which an agent reads logs.

### 4.5 Command surface

| Action | CLI | TUI |
| --- | --- | --- |
| Show / filter | `cronus log show [--level <l>] [--grep <text>] [--since <dur>] [--kind <k>]` | `/log show …` |
| Follow | `cronus log tail [--kind <k>]` | `/log tail` |
| Where the files are | `cronus log path` | `/log path` |
| Sink state, drops, native-crash capture availability | `cronus log status` | `/log status` |

## 5. Drawbacks & Alternatives

- **JSON lines are harder to read raw.** Accepted: `log show` is the reading surface; the file serves machines and offline diagnosis.
- **Alternative — the `log` facade only.** Rejected: it has no structured fields as first-class content (LL-2) and no spans for correlation.
- **Alternative — one file for all processes.** Rejected: the engine and short-lived command-line runs would interleave and contend for one file.
- **Alternative — log to the state tier.** Rejected: the log is ephemeral and machine-local; the state tier is backed up and may be synced.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[DIAG]` | `.design/main/specifications/l1-diagnostic-log.md` | DLG-1…DLG-8 |
| `[LEGIBILITY]` | `.design/main/specifications/l1-log-legibility.md` | LL-1…LL-9 |
| `[CLI-MAIN]` | `crates/cli/src/main.rs` | Where the sink is installed first |
| `[REDACTION]` | `.design/main/specifications/l2-invocable-registry.md` | §4.12 — the redaction path every projection uses |

## Document History

| Version | Date | Notes |
| --- | --- | --- |
| 1.0.0 | 2026-10-22 | Initial realization of `l1-diagnostic-log` and `l1-log-legibility`, from the user's backlog (logging): the `tracing` family as the one event model, the sink installed first in every binary, a session header, a panic hook and forwarded subprocess output, own crates at `info` and dependencies at `warn`, rotating JSON-lines files per process kind under `<logs>`, a non-blocking writer that drops and counts rather than blocks, redaction at every projection, a budgeted agent feed, and the `log` inspect verbs on every surface. All Pending in code (no crate logs today); native crash capture Pending per OS. |
| 1.0.1 | 2026-10-22 | Citations of the parent's requirement IDs follow the rename of its prefix `DL` → `DLG` (RULES §1, 1.8.0). No requirement was added, removed or reworded. |
