# Surface Driver Realization

**Version:** 1.0.0
**Status:** Stable
**Layer:** implementation
**Implements:** l1-surface-driver.md

## Overview

The concrete realization of the surface driver in this project's stack. One **generic driver
library** (`cronus-drive`, no dependency on any Cronus crate) holds the command table, the
session protocol, the spool-directory channel, the step controller and the frame model. One
**stateless client** — the same library behind a `cronus-drive` binary — maps shell verbs one
for one onto commands and is also what the simulation harness opens sessions through. **Dev-only
hosts** compose a surface's real production wiring with a driven backend and register this
product's probes and shortcuts: the terminal interface's host first, as an *example target* of
its own crate so that nothing of the driver is a dependency of the product; the desktop shell's
host later, contract-complete today.

Two lanes carry the work. The **held lane** drives a surface through a development seam: turns
are counted, time is held by domain, the floor sits above the terminal, and a route replays
with no actor in the ordinary `cargo test` gate. The **free lane** drives the shipped binary
under a pseudo-terminal: nothing is held, the floor is the operating system's console, and it
reaches what the seam cannot — including the interactive branches of the command line that a
pipe never takes (§7.1).

## Related Specifications

- [l1-surface-driver.md](l1-surface-driver.md) — Parent concept. DRV-1…DRV-14 are this spec's subject.
- [l2-simulation-suite.md](l2-simulation-suite.md) — The consumer. Sessions open inside its disposable worlds, every session command is appended to its transcript, and pinned routes replay in its always-on lane (§4.10).
- [l2-tui.md](l2-tui.md) — Owns the seams the held lane stands on: the injectable backend and renderer, the pure render function, and the single composition function (§4.6 there).
- [l2-cli.md](l2-cli.md) — The command line, whose one-shot runs the existing process wrapper already drives and whose consent prompts need the free lane (§4.7).
- [l2-application-shell.md](l2-application-shell.md) · [l2-app-ui.md](l2-app-ui.md) — The graphical shell: its named-action registry (AS-6) and the single dispatch function its external triggers funnel into, which is where §4.8 reaches an operating-system-level trigger.
- [l2-invocable-registry.md](l2-invocable-registry.md) — The catalog and `Dispatcher` every shortcut goes through, so a shortcut forks no product logic (DRV-8).
- [l2-crate-topology.md](l2-crate-topology.md) — Where `cronus-drive` sits and why no product crate may depend on it outside `dev-dependencies` (§4.1).
- [l2-execution-sandbox.md](l2-execution-sandbox.md) — §4.4's environment allowlist, which already keeps anything not named out of a child the product starts.
- [l1-process-integrity.md](l1-process-integrity.md) — PI-5 and PI-8: the allowlist rule that is why the enabling signal is an argument and not an inheritable variable (DRV-1).
- [l1-surface-parity.md](l1-surface-parity.md) — SP-13's three resolution outcomes, which a shortcut inherits by going through the product's own dispatch (DRV-8), and its refusal to fold *unavailable* into *empty* (DRV-6).
- [l1-browser-control.md](l1-browser-control.md) — BC-2 and BC-3: the addressing and fail-loud invalidation the graphical shell's structural observation follows (§4.8).
- [l1-acceptance-oracle.md](l1-acceptance-oracle.md) · [l1-invariant-tripwires.md](l1-invariant-tripwires.md) — AO-5 and TW-1…TW-6: the rules the calibration checks and the structural scans are held to (DRV-12, §4.11).
- [l2-workflow-runtime.md](l2-workflow-runtime.md) — §4.2.7 names the interactive question channel as unowned; the free lane is where such a channel would first be observed (§4.7).
- [l1-live-diagnostics.md](l1-live-diagnostics.md) — LD-10: the probes a host registers are published introspection routines, read-only, with a declared authority level and timeout.
- [l1-agent-tool-ergonomics.md](l1-agent-tool-ergonomics.md) — The rules the command surface follows (§4.5, §4.9).
- [l2-quality-pipeline.md](l2-quality-pipeline.md) — Where the calibration suite and the replay lane attach to the gates.

## 1. Motivation

**The only driver that exists spawns a process.** `cronus-sim run` runs the product with an
argument vector and records what it printed. That is the whole command-line half of usage
simulation and it is sound; it cannot press a key, hold a screen still or read a frame, and the
suite's own implementation notes recorded that need before this spec existed. The terminal interface — the
product's default composition (`l2-tui.md` §4.4) — and the graphical shell therefore have no
instrument at all. The keystroke defect recorded in `l2-tui.md` 1.3.0 was found by hand for that
reason, and the instrument that was built to replace hand-testing still cannot reproduce it.

**The seams the terminal interface needs are already there.** The loop is generic over its
terminal backend, its snapshot source and its renderer (`run_with`); the backend's event source
is a trait whose production implementation folds a native event into a small vocabulary; the
render function is a pure function of the view-model. A driven composition is a different
backend and a different renderer handed to the loop that already exists. What is missing is
three small things: a single composition function instead of one inlined in `run()`, the fold
as a function both backends share, and the host that uses them (§7.2).

**Holding the surface is not holding the product.** The terminal loop polls at 50 ms and reads
a snapshot source on every pass; a core call that is slow runs on a worker thread and arrives
on a later pass; and the domain and core read the host clock directly in some twenty places
with no clock to be handed (§7.3). A driver for this product can honestly claim a held
*surface* and nothing wider, and it must say so in every record — `surface-held` in the
vocabulary of the parent — because an actor that believes the product is held will conclude a
race is a defect, or a defect is a race.

**The command line has a branch a pipe never takes.** `cronus ext activate` and `cronus
activation enable` read a consent from standard input and refuse to run at all when standard
input is not a terminal and no acknowledgement flag was given. The process wrapper pipes
standard input, so the prompt, its wording, its cancel path and its refusal are invisible to
usage simulation. A person at a terminal gets all of it (§7.1).

**The host is Windows first.** Git Bash rewrites an argument that starts with `/` into a path
under its own root, and the terminal interface's own commands start with `/`. A freshly
renamed file can be briefly unreadable on Windows. A process cannot be probed for liveness with
signal zero, which ends it there. And a shared temporary root is a per-user area on Windows and
macOS and not on a shared Unix host. Each of these has already cost a studied implementation a
fix, and each is cheaper to design out than to rediscover.

## 2. Constraints & Assumptions

- **The driver is a dev tool and never a product dependency.** `cronus-drive` is linked by the
  simulation harness and by dev-only hosts, and by nothing else (§4.1).
- **`cronus-drive` uses the standard library and `serde_json`.** No async runtime, no network
  crate, and no process spawning outside the client half. The free lane's pseudo-terminal and
  terminal-emulation needs are the only candidate third-party dependencies of the whole driver
  and enter through the dependency policy (§4.7), not by default.
- **Windows is a first-class host.** Every path, retry and liveness check in this spec works
  there first.
- **The harness stays in the ports tier.** `cronus-simulation` consumes the client half of the
  library and names nothing of the domain tier (`l2-crate-topology.md` §4.4(d)).
- **The graphical shell's track is paused by its owner.** This spec binds its future
  realization to the contract and states the preconditions it must keep (§4.8); it does not
  schedule that work.
- **No implementation code lives in this spec.** Message shapes are marked `[REFERENCE]`.

## 3. Invariant Compliance (Layer 2 only)

| L1 Invariant | Implementation |
| --- | --- |
| **DRV-1** Development-only, absent from what ships | No product crate depends on `cronus-drive` outside `[dev-dependencies]`. The terminal host is an **example target** of `cronus-tui` — never built by a release build, listed by no packaging step — and the desktop host lives in the shell's own detached workspace's development surface (§4.8). The enabling signal is a **launch argument of the host** (`--drive-session <dir>`), never an environment variable, so there is nothing for a child to inherit (`l1-process-integrity.md` PI-5, PI-8). *Verified by* a manifest tripwire (`cronus-drive` is a `[dependencies]` entry of no product crate) and a **release-shaped run**: the shipping `cronus` binary is started with the host's argument and with a populated session directory, and the directory stays untouched and no session is read, written or opened. |
| **DRV-2** Private, no endpoint | `cronus-drive` has no `std::net` type and no network crate (tripwire). The session directory is created by the client with exclusive creation and owner-only access, inside the world when used for simulation and otherwise under the user's private runtime area (`$XDG_RUNTIME_DIR` where it exists, otherwise the per-user data directory) — **never** under a shared temporary root (§7.7). Before every read and write the host and the client check that the directory is not a symbolic link or reparse point, that it is owned by the current user and, on Unix, grants nothing to group or others; on Windows ownership is by construction (a path under the user's profile) and the check is the profile prefix and the absence of a reparse point. A failed check refuses the session with `channel_not_private`, naming what failed. |
| **DRV-3** Input parity at a declared floor | The terminal host injects **native-shaped** events through the same fold function the production backend uses (§4.6), so a class the fold drops stays dropped and the platform's press-and-release rule is exercised, not bypassed. The floor — *input at the decoded native event, before the fold; output as the rendered cell grid, before it becomes bytes* — is written into `ready.json` and every record. A `key` or `paste` the fold does not consume answers `folded: dropped_at_floor` rather than success. Text is carried byte-exact: through an argument, `--stdin` or `--file`, never rewritten; a leading-`/` argument under an MSYS-style shell is detected and either undone when unambiguous or refused with guidance (§4.9). *Verified by* the calibration fixture's round trip of a string with a leading slash, non-ASCII characters and a carriage return. |
| **DRV-4** Held time, honest domains, measured determinism | The held lane holds the surface's loop by blocking inside the backend's event poll; the other domains are listed as free in `ready.json` (`free_domains`). Class starts `surface-held`. After **every** step the host compares turns granted with `tick` calls made and `redrawn` outcomes with `draw` calls made; a mismatch downgrades the class to `turn-counted` with the measured difference, and nothing raises it again in that session. `step --until` validates its condition once before the first turn so a malformed one is immediate guidance (`bad_args`, nothing ran), and a condition that errors at run time ends the step with the error `condition_failed`. The free lane reports class `free`. *Verified by* the calibration fixture's counters, its held-for-a-while stillness check, and a deliberate pairing fault. |
| **DRV-5** Two tiers | The tier is a start argument (`--tier operator` or `--tier inspector`), written to `session.json` and `ready.json`, and fixed. The host builds its command table **by tier**: an operator-tier host never registers a probe or a shortcut, `commands` lists only that tier's commands, and a request for another is guidance — `unknown_command`, answering with the session's own listing, not a refusal (ATE-3, ATE-13). `until` conditions are parsed against the tier's grammar (§4.5). *Verified by* a test that asks an operator-tier host for every inspector command and a registry tripwire that the operator build path never calls the registration of a probe. |
| **DRV-6** Presented, with its turn | The frame model carries `turn` (current), `drawn_at_turn`, `stale`, `unavailable{reason}` and `truncated{cut}` (§4.6). The capturing renderer stamps each frame as it is drawn; a frame is `stale` whenever `drawn_at_turn < turn` *and* the view-model now differs from the one last drawn (the host compares them; the terminal loop draws inside the turn that changed the view, so the held lane should never meet this, and a graphical shell, whose frame completes after the turn that changed the view, will). A surface with no projection reports the panel's own *unavailable* (`l2-tui.md` §4.1) as part of what was presented, not as an empty cell run. |
| **DRV-7** Non-interference | The terminal host opens no terminal and no window, so there is no pointer, focus or screen to take. The graphical host is created without taking focus (§4.8). The driver's files live in the session directory; the product's durable state is written only by the product, and by a shortcut through its own dispatcher (§4.5). |
| **DRV-8** Shortcuts | A shortcut is `shortcut <id> <args>` at the inspector tier, executed through the registry and `Dispatcher` the surface composed — the path the command bar uses — so resolution, binding, masking and the three resolution outcomes (SP-13) are the product's own. The host labels the record entry `via: shortcut`, writes the same label into every later record entry until the actor takes surface input again, and its evidence stream (§4.10) excludes a shortcut-dispatched invocation from coverage. |
| **DRV-9** One table, thin bindings, four outcomes | `table.rs` is the only place a command is declared; the client's verbs and any tool-protocol binding are generated from it (§4.9). Every answer says whether the command **ran** and whether it was **guided** (nothing ran; success-shaped, with its repairs), or ended in a product **error**; the fourth outcome, *unreachable*, is the client's and has no answer. Exit status: `0` ran or guided (`ran` says which; `--strict` makes a guided answer exit `3` for scripts), `1` the product answered with an error, `2` the client could not reach, start or understand the session. `intent` is a required, first-ordered argument of every mutating command (ATE-10). Guidance and errors carry a stable `code`, the subject and `repairs` (ATE-14); a known command misused answers `wrong_form` listing its forms (ATE-2, ATE-13). |
| **DRV-10** Safe failure, no late effects, bounded life | Handlers run under a panic guard: a panic in a registered probe or shortcut answers `internal`, changes nothing the host owns and leaves the session serving. Every request carries `expires_at`; the host discards a request it picks up after that moment, appends the discard to `record.jsonl` and answers it as guidance (`expired`, nothing ran); the client withdraws an unclaimed request itself on timeout. The idle bound defaults to 600 s (§6). A host ends with `closed.json` naming `asked`, `idle`, `channel_removed` (its session directory was deleted — the world was torn down) or `product_exited`, and releases held input first. *Verified by* the error matrix (§4.11): a snapshot of turns, held input and view before and after each malformed request is identical, and an expired request is shown not to run. |
| **DRV-11** The hold is not an oracle | The hold is a block inside the backend's poll. It invokes no registry action, no pane action and no product suspend, and a tripwire asserts the hold path calls no `Dispatcher` entry. Scenario guidance (§4.10): a check of the product's own pause, idle or background behavior runs with that mechanism engaged and `step` advancing, and is never concluded from a held frame. |
| **DRV-12** Calibrated before trusted | `crates/drive/tests` holds the **metronome** calibration surface — a loop whose frame is a known function of its commands (a turn counter, a bar one cell per turn, echoed text, a resize counter) — and runs it through the same host machinery. Structural checks are narrow, one-rule, named tripwires (TW-1…TW-6): no network type, no process spawn outside the client half, no Cronus dependency in the manifest, no probe registration reachable from an operator build. Each check is paired with a deliberate-breakage run that must fail it (AO-5). |
| **DRV-13** Reach | Geometry is a start argument and `resize` a command; locale and color capability are the world's environment, recorded in `ready.json`. The terminal host accepts `--fixture <name>` to render the real view with an injected projection (an *unavailable* panel, a narrow terminal, wide glyphs) — fixtures live under the example target, not in `cronus-tui` (§4.6). No catalog of presentable states exists yet (§7.8), so until one is declared an unreachable state or condition is recorded by hand as USM-8's gap finding. |
| **DRV-14** Recorded, replayable | The host appends `record.jsonl` *before* answering — one line per request with its arguments, intent, label, answer, and turn before and after — and the harness copies it into the transcript at `finish`. The record header states surface, floor, lane, tier, presentation conditions, held and free domains, class, product version and world. Replay is the `[[drive]]` step kind of `crates/simulation/tests/replays` (§4.10); a frame assertion names the cells it claims and masks declared volatile regions. |

## 4. Detailed Design

### 4.1 Layout

```plaintext
crates/drive/                         # cronus-drive — generic; no Cronus dependency
├── Cargo.toml                        # std + serde_json only
├── src/
│   ├── lib.rs
│   ├── table.rs                      # the command table: names, tiers, argument/result/error shapes
│   ├── protocol.rs                   # request, response, ready, closed, record lines (§4.4)
│   ├── spool.rs                      # the channel, both ends: atomic write, ordering, expiry, privacy checks
│   ├── step.rs                       # step controller: counting, until-conditions, bounds (pure)
│   ├── frame.rs                      # frame model: rows, styled cells, geometry, freshness, truncation
│   ├── session.rs                    # host side: hold, serve, end reasons, idle bound, panic guard
│   ├── client.rs                     # client side: start detached, attach, send, withdraw, stop, liveness
│   └── bin/cronus_drive.rs           # the stateless shell client
└── tests/
    ├── metronome/                    # the calibration surface (DRV-12)
    ├── matrix.rs                     # the error matrix, before/after equality
    └── tripwires.rs                  # one narrow named check per rule

crates/tui/examples/driven.rs         # the terminal host — dev-only; cronus-drive is a dev-dependency here
crates/simulation/                    # gains `session` verbs over cronus_drive::client (§4.10)
apps/desktop/                         # the graphical host — deferred (§4.8)
```

`cronus-drive` is minted under `l2-crate-topology.md` §4.4(d): it is the shared instrument library
of two verification harnesses and cannot share a compilation unit with what it checks. It depends
on **nothing** of the product, which is DRV-9's generic core made checkable. The dependency
direction is one rule with one tripwire: `cronus-drive` appears in the `[dependencies]` of
`cronus-simulation` and of no product crate, and in the `[dev-dependencies]` of the frontend
crates that carry a host.

### 4.2 Enablement, hosts and the session directory

```plaintext
cronus-drive start [--host <path>] [--tier operator|inspector] [--size CxR] [--idle-quit <secs>]
                   [--session <name>] [--fixture <name>] -- <host arguments>
  -> creates the session directory (exclusive, owner-only, verified), writes session.json,
     starts the host detached with --drive-session <dir> [--tier ..] [--size ..] [--idle-quit ..],
     waits for ready.json, fails with the host log's tail if the host exits first
```

The host binary is found by `--host`, then `CRONUS_DRIVE_HOST`; `cronus-sim` resolves it as it
resolves the product binary (`product::resolve_binary`, extended to search
`target/<profile>/examples/`, §7.5) and hands the path to the library, so the generic client
never learns a Cronus path. A client refuses a live session of the same name, refuses a host
whose `ready.json` carries a different protocol number, naming both, and clears what a finished
session left before it starts a new one.

A session directory holds:

| File | Written by | Meaning |
| --- | --- | --- |
| `session.json` | client | Host pid, command line, start time, lane, tier. |
| `ready.json` | host | `{protocol, pid, surface, lane, tier, floor, class, reason?, held_domains, free_domains, geometry, conditions, product_version, commands}`; its presence means the host is held and listening. |
| `req-<seq>-<id>.json` · `resp-<id>.json` | client · host | One request and its answer, each written under a temporary name and renamed. |
| `record.jsonl` | host | One line per request, written before the answer (DRV-14). |
| `closed.json` | host | `{reason, exit_code?, turn}` — written as the session ends. |
| `host.log` | client | The host's standard output and error. |
| `frames/` | host | Frames saved without an explicit path. |

### 4.3 The channel

A request is one file, `req-<seq>-<id>.json`, where `<seq>` is microseconds since the epoch and
`<id>` is unique. The host serves requests oldest first, one at a time, writes the answer, then
removes the request. A writer creates `<name>.tmp` and renames it, and a reader ignores `*.tmp`,
so no reader sees half a file. While held the host polls the directory about every 2 ms.

```plaintext
PSEUDO-CODE: host.serve_pending()

    FOR each request file, oldest first:
        verify the channel is still private                       -- DRV-2, before every use
        parse the request                                         -- unparsable -> guidance bad_request
        IF now > request.expires_at:
            append "discarded" to record.jsonl; answer guidance expired; CONTINUE   -- DRV-10
        record the command, then run it under the panic guard     -- record before answer (DRV-14)
        write the answer
        IF the command is a step with turns left: release the loop and RETURN
```

Reading a file that has only just appeared retries for up to about a second, because a freshly
renamed file can be refused on Windows for a moment. Nothing outside the session directory is
read or written by the channel.

### 4.4 Messages

```plaintext
[REFERENCE] Messages

request   : { "id": "<unique>", "cmd": "<name>", "args": { ... },
              "intent": "<why this call, in the actor's words>", "expires_at": <unix ms> }
response  : { "id": "<same>", "ran": true,  "result": { ... }, "guidance": null, "error": null,
              "turn": <int>, "class": "<exact|surface-held|turn-counted|free>" }        -- ran
          | { "id": "<same>", "ran": false, "result": null,
              "guidance": { "code": "<code>", "subject": "<what>", "message": "<text>", "repairs": [ ... ] },
              "error": null, "turn": <int>, "class": "<...>" }                           -- guided: nothing ran
          | { "id": "<same>", "ran": true,  "result": null, "guidance": null,
              "error": { "code": "<code>", "subject": "<what>", "message": "<text>" },
              "turn": <int>, "class": "<...>" }                                          -- the product's error

guidance codes : bad_request, unknown_command, wrong_form, bad_args, expired
error codes    : condition_failed, product_error, internal
client-side    : channel_not_private, host_gone, timeout, protocol_mismatch   -- outcome *unreachable*, exit 2

record line : { "seq": <int>, "id": "<id>", "cmd": "<name>", "args": { ... }, "intent": "<why>",
                "via": "surface" | "shortcut", "answer": { ... },
                "turn_before": <int>, "turn_after": <int>, "at": <unix ms> }
closed.json : { "reason": "asked" | "idle" | "channel_removed" | "product_exited",
                "exit_code": <int|null>, "turn": <int> }
```

A response carries the turn counter and the class in force *now*, so every answer says what it
is entitled to promise (DRV-4) and an observation can be compared with the turn it reflects
(DRV-6).

### 4.5 The command table

| Command | Tier | Mutates | Arguments | Result |
| --- | --- | --- | --- | --- |
| `ping` · `status` · `commands` | operator | no | — | liveness · surface, lane, tier, class and reason, held and free domains, turn, geometry, floor · this tier's commands with help |
| `step` | operator | yes | `turns`, or `until` with `bound` | `{turns_run, drawn, frame_changed, drawn_at_turn, until_met?}`; the condition is checked before every turn, the first included |
| `key` | operator | yes | `name`, `mods?` | `{delivered, folded: consumed \| dropped_at_floor, turn}` |
| `type` | operator | yes | `text` | `{delivered, turns}` — one key event per turn |
| `paste` | operator | yes | `text` | `{delivered, folded}` — a paste is *not* typing; the fold decides |
| `resize` | operator | yes | `cols`, `rows` | `{geometry}` |
| `frame` | operator | no | `rows?`, `styles?`, `exact?` | the frame model (§4.6) |
| `geometry` | operator | no | — | `{cols, rows}` |
| `probes` · `probe` | inspector | no | — · `name`, `args?` | the catalogue · a read-only snapshot, redacted at the product's boundary |
| `view` | inspector | no | — | the view-model the surface renders from |
| `shortcut` | inspector | yes | `id`, `args` | the product's own dispatch outcome |
| `quit` | operator | yes | — | `{quitting}` — ends the session (reason `asked`) |

`until` conditions are parsed against the tier's grammar:

| Tier | Conditions |
| --- | --- |
| operator | `frame_contains <text>`, `frame_lacks <text>`, `row_contains <row> <text>`, `exited` |
| inspector | the operator conditions, and `probe_equals <name> <path> <value>` |

Every command marked *Mutates* takes `intent` — why this call, in the actor's words — as its first
argument (ATE-10). The record keeps it as the actor's testimony, never as evidence (USM-5).

A product-registered command may not take a name in this table; registration of one is refused
and logged, and the session continues. Product-specific commands are *only* probes and
shortcuts, which is what keeps the generic core generic (DRV-9).

### 4.6 The terminal-interface host (held lane)

The host is `crates/tui/examples/driven.rs`. It composes the production wiring through the
single composition function `l2-tui.md` §4.6 requires, and calls the loop entry the product
calls, with a driven backend and a capturing renderer.

- **A turn is one iteration of the loop**: one backend poll answered, one `tick`. The loop takes
  at most one event per iteration, so `type "abc"` is three turns and a `step 5` with nothing
  queued is five idle turns.
- **The driven backend** implements the backend seam. `enter` and `leave` touch no terminal;
  `size` is the session geometry; `poll_event` blocks inside the session's serve loop while the
  surface is held, and while a step has turns left returns the next injected event, or an idle
  result at once, never sleeping the loop's 50 ms tick — so the loop's idle tick becomes a
  logical turn and a step runs as fast as the loop does.
- **Injection is native-shaped.** A command builds the terminal library's own event value and
  passes it through the fold function shared with the production backend. What the fold drops
  (key release, focus, mouse, paste, an unmapped key) is reported `dropped_at_floor`; the
  platform's press-and-release pairing reaches the fold as it would from a device.
**The capturing renderer** draws each view-model with the same pure render function into an
off-screen cell buffer of the session geometry, and stamps it with the turn it was drawn at.
The frame model:

```plaintext
[REFERENCE] frame
{ "turn": 12, "drawn_at_turn": 12, "stale": false,
  "geometry": [100, 30],
  "rows": [ "…", "…" ],                       -- right-trimmed unless exact
  "cursor": [x, y] | null,
  "styles": [ { "row": 3, "col": 0, "len": 12, "fg": "…", "bg": "…", "mods": ["bold"] } ],   -- on request
  "unavailable": null | { "reason": "…" },
  "truncated": null | { "rows_cut": 0 } }
```

A wide glyph occupies the cells it occupies and the cell after it is reported as covered, not
as a blank.

- **The floor, declared.** Input enters after terminal byte decoding, at the fold; output is the
  cell grid before it becomes bytes. Not exercised in this lane: entering and leaving raw mode,
  the terminal's decoding of escape sequences, the bytes of the frame diff, the real terminal's
  glyph widths. They stay covered by the backend's own lifecycle tests and by the free lane
  (§4.7). A verdict that rests on them is outside a held-lane session's reach (DRV-3).
- **Domains.** Held: the loop. Free: worker threads (a slow core call arrives on a later turn),
  storage, inference, the host clock (§7.3). A snapshot still in flight shows as an unchanged
  frame; the actor steps `until` the frame shows it. Class `surface-held`.
- **Fixtures.** `--fixture <name>` starts the same render over an injected projection — an
  unavailable panel with its reason, a narrow terminal, content with wide glyphs — so a state no
  play reaches can be looked at (DRV-13). Fixtures live in the example target and are never
  exposed by the library.
- **Probes and shortcuts** are registered by the host for the inspector tier only: the view-model
  as `view`, the registry's catalogue as a probe, and every dispatch through the surface's own
  `Dispatcher` as the shortcut path. Probe results pass the product's redaction boundary before
  they leave the host; a probe that cannot be redacted is not registered.
- **Ending.** When the loop returns the host writes `closed.json` with `product_exited` and the
  exit status; when the actor sends `quit` it ends with `asked`. A step the product's exit
  interrupts is answered with the turns it ran and `exited: true`, never left unanswered.

### 4.7 The terminal lane and the command line (free lane)

The command line has two input profiles, and they are different *surfaces* to the code:

| Profile | Standard input is | The product takes | Driven by |
| --- | --- | --- | --- |
| **piped** | a pipe | the non-interactive branch: a consent flow refuses without its acknowledgement flag | the existing process wrapper, unchanged |
| **terminal** | a pseudo-terminal | the interactive branch: the disclosure, the prompt, the cancel path | the free lane, below |

The **terminal lane** starts the shipped binary inside a pseudo-terminal at the session
geometry, feeds it bytes as a terminal's keys would, and reconstructs the screen from the byte
stream with a terminal emulator. It serves two purposes with one mechanism: the interactive
consent flows of the command line (`ext activate`, `activation enable`, and the interactive
channel `l2-workflow-runtime.md` §4.2.7 leaves without an owner), and the *real* terminal
interface — its raw mode, byte decoding and diffing — as the floor-covering lane for §4.6.

- **Floor:** the operating system's console. Nothing is held; class `free`.
- **A turn** ends when the process has produced no output for `quiet_ms` (default 150) or has
  exited. Every answer states `quiet_ms`, so an actor knows the turn boundary is a measurement
  and not a count, and uses `until` for anything that matters.
- **Observation** is the same frame model, reconstructed; `exited` and the exit status are
  operator-tier observations.
- **Dependencies.** A pseudo-terminal on Windows is not reachable from the standard library,
  and a terminal emulator is a few hundred lines for the subset this product emits (cursor
  addressing, color and attribute selection, erase). Both are candidates under the dependency
  policy and neither is added by default: each must be justified against the std-only
  alternative when this lane is built. <!-- TBD: pseudo-terminal crate versus direct platform calls; terminal-emulation crate versus an owned subset -->
- **Order.** The held lane for the terminal interface is built first — it needs no new
  dependency, replays deterministically and closes the keystroke gap. This lane follows and
  closes the consent-flow gap.

### 4.8 The graphical shell

The shell is the one surface whose realization this spec does not schedule; it specifies what
the realization must satisfy so that the track, when it resumes, builds against a contract.

- **Host.** The shell itself, composed with the driver only in a development surface of its own
  detached workspace — a development-only target or feature of that workspace, which the
  engine workspace and the release `tauri build` never enable. DRV-1's release-shaped run
  applies to the shell's release artifact exactly as to the binary's. <!-- TBD: development target versus feature, decided by the shell's track -->
- **Turn.** One presented frame of the embedded view. Exact frame control depends on the
  platform's embedded view offering a virtual time source; until it is shown to, the shell's
  class is the lower one it measures (`turn-counted` or `free`), and `ready.json` says so.
- **Input** goes through the embedded view's own input path by an in-process call into the
  view's developer protocol — no port, no endpoint (DRV-2) — which the page cannot tell from a
  device. It is **never** synthesized at the operating system, because that takes the
  co-present person's devices (DRV-7).
- **Operating-system triggers** — a global hotkey, a tray action — are below that floor. They
  are reached only as shortcuts into the single dispatch function `l2-app-ui.md` §4.11 says every
  external trigger funnels into, labeled as shortcuts (DRV-8).
- **Observation.** Operator tier: the window's last composited frame as an image, the geometry,
  and the **accessible structure** — roles and accessible names, addressed and invalidated as
  `l1-browser-control.md` BC-2 and BC-3 do. Inspector tier: the shell's view store projection
  and the bridge's catalogue. Where the shell exposes no structure, the answer is
  `unavailable{reason}`, never an empty tree (DRV-6).
- **Co-presence.** The driven window is created without activation and without taking focus.
- **Preconditions the shell must keep:** every actionable element carries a role and an
  accessible name (§7.4 — nothing requires it today); named actions dispatch through one
  registry (AS-6, present); animation and relative-time content read a clock the host can
  supply, or are declared volatile.

### 4.9 The client and its bindings

The client is stateless: each invocation writes one request, waits for its answer, prints one
JSON object and exits. Verbs map one for one onto the command table; the table generates them,
so a verb that has no command, or the reverse, cannot be written.

| Verb | Does |
| --- | --- |
| `start` · `stop` · `sessions` | Open a session (§4.2) · send `quit`, wait briefly, end the host if it stays, keep the logs · list sessions and whether each host is alive. |
| `status` · `ping` · `commands` | The command of that name. |
| `step N` · `step --until <cond> --max M` | `step`. |
| `key` · `type` · `paste` · `resize` | The command of that name. |
| `frame` · `geometry` | The command of that name. |
| `probes` · `probe` · `view` · `shortcut` | Inspector-tier commands; absent from an operator session's help. |
| `raw <json>` | A request as written. |

Every mutating verb — `start`, `stop`, `step`, `key`, `type`, `paste`, `resize`, `shortcut` —
takes `--intent <why>`; a call without it is guidance (`wrong_form`) that shows the form.

Output is one JSON object on standard output. Exit status: **0** the command ran or was guided
(`ran` says which; `--strict` turns a guided answer into **3** for a script that must not mistake
*nothing ran* for success), **1** the product answered with an error, **2** the client could not
start, reach or understand the session (DRV-9). `--session` and `--timeout` are accepted before
or after the verb. A request that gets no answer within `--timeout` is withdrawn by the client if
the host has not yet claimed it, and fails with status 2 and `timeout`; a command the host has
already begun is bounded and completes, and its outcome is read from `record.jsonl`. A host
process that is gone with no `closed.json` is reported as status 2 `host_gone`, carrying the last
`record.jsonl` line and the tail of `host.log` — a crash is evidence, not a missing answer.

**Argument fidelity.** Text for `type` and `paste` may come from an argument, `--stdin` or
`--file`. When `MSYSTEM` is set and an argument begins with the shell's own root path followed by
what was typed, the client has been handed a rewritten argument: it restores it when the
original is unambiguous and otherwise answers `wrong_form` guidance naming the stdin form.
It never forwards a rewritten argument silently. Escapes (`\n`, `\t`, `\e`) are interpreted only
under `--escapes`.

**Liveness and Windows.** The client asks the operating system whether the host process exists
and never sends signal zero, which ends a process on Windows. The detached host is started with
the platform's detach flags and its output in `host.log`.

**Tool-protocol projection.** A stdio server that exposes the command table as tools is a
second binding of the same table: one tool per command, names and schemas generated from the
table, the same answers and the same codes, the same intent field. It is not built first (§6).
It is a client of the same channel, so a host has exactly one transport.

### 4.10 Composition with the simulation harness

`cronus-sim` gains a `session` family over the client library:

```plaintext
cronus-sim session start <world-id> --surface tui|cli [--size CxR] [--lane held|free]
cronus-sim session <world-id> <command> ...          -- every driver command, passed through
cronus-sim session stop <world-id>
```

- **The wrapper rule extends.** Every product invocation passes through the wrapper (USM-5); so
  does every session command. The host's `record.jsonl` is copied into the transcript as
  entries of kind `drive` at `finish`, and frames are saved under the run directory. A verdict
  cites a drive entry by index like any other.
- **The world owns the session.** The session directory is inside the world; tearing the world
  down deletes it, which the host sees as `channel_removed` and ends. `finish` stops sessions
  before it tears the world down.
- **Bounds.** Each driver command counts as a step toward the scenario's `bound`; turns do not.
- **A simulation is operator-tier.** Every scenario's actor is a user, whatever it may consult,
  so `session start` opens operator-tier sessions only and has no `--tier` option for a scenario
  run; the inspector tier belongs to debugging by someone who may know the implementation, and a
  verdict that cites an inspector-tier observation is void as simulation evidence (DRV-5).
- **Evidence stream and coverage.** The host also writes, apart from `record.jsonl`, the
  invocable identity of every dispatch with `via: surface | shortcut`. The actor never sees it
  at the operator tier; the harness reads it. `cronus-sim coverage` can then check a scenario's
  declared `covers` against what was *reached by the surface*, and a shortcut path counts for
  nothing (USM-8, DRV-8).

**Replay lane.** A pinned route may hold `[[drive]]` steps, executed in order against a fresh
world with no actor:

```plaintext
[REFERENCE] replay steps
[[drive]]  op = "type"    text = "/board list"
[[drive]]  op = "step"    turns = 3
[[drive]]  op = "expect"  row_contains = { row = 5, text = "Board" }
           mask = [ { rows = [2, 3], reason = "relative time labels" } ]
```

An `expect` asserts the claim the route was pinned for, never whole-frame equality, and a
region the product derives from the clock or from another process is masked with its reason.
Replay runs in `cargo test -p cronus-simulation` with the other replays and costs no inference.

**Pause-like behavior (DRV-11).** A scenario that checks the product's own pause, idle or
background behavior states so, and its obligation is decided on a stepping session with the
mechanism engaged.

### 4.11 Verification

| Layer | What is checked | How |
| --- | --- | --- |
| **Pure logic** | Step counting, `step 0`, an `until` that holds now runs none, a bound ends a condition that never holds, a condition that errors ends the step | unit tests in `step.rs` |
| **Calibration** | The metronome: exact counts after steps of 1, 7 and 600; nothing moves while held for a settle interval; frame fidelity (the bar's cell count equals the turns); typed text round-trips byte-exact; a resize is reflected; a deliberate pairing fault downgrades the class | integration test through the host machinery |
| **Error matrix** | Each malformed, unknown, misused and expired request returns its documented code; a snapshot of turn, held input and view before and after is identical; the host then steps exactly | `tests/matrix.rs` |
| **Privacy** | A directory with loose access, a symlink, a reparse point and one owned by another user are each refused with `channel_not_private` | `tests/` on each host OS |
| **Tripwires** | No network type; no process spawn outside the client half; no Cronus dependency; no probe registration reachable from an operator build; no hold-path dispatch | `tests/tripwires.rs`, one narrow named check per rule |
| **Release-shaped** | The shipping `cronus` binary, given the host's argument and a populated session directory, reads, writes and opens nothing | an integration test building and running the real binary |
| **Real host** | The terminal host drives the real composition: `type "/help"`, `step`, `frame` shows the echoed prompt; `key Esc` ends the loop and `closed.json` says `product_exited` | an integration test, headless |

Every check above is paired with a deliberate-breakage run that must fail it, recorded when the
check is built (DRV-12, AO-5): a step one turn too many, a hold that lets a turn through, a frame
that is the previous one, an expired request that runs, an inspector command reachable from an
operator host, a loosened directory mode.

## 5. Implementation Notes

1. **`step.rs` and `frame.rs` first**, pure, with no surface in them, and tested alone.
2. **`spool.rs` with its privacy checks and expiry, then `session.rs`**, against the metronome — so
   the channel, the hold and the error matrix exist and are green before any product code is touched.
3. **The client and its verbs**, generated from `table.rs`.
4. **Two refactors in `cronus-tui`, no behavior change** (§7.2): one composition function used by
   the product entry and any host; the native-event fold as a function both backends use. The
   existing `cronus-tui` tests stay green unchanged.
5. **The terminal host** with its first probes, then the real-host test.
6. **Harness integration** (`session` verbs, `drive` transcript entries, `[[drive]]` replay) and
   the first scenario that needs it — the command-bar key, whose companion artifact the keystroke
   defect never had.
7. **The terminal lane** (§4.7) once its dependencies are decided; **the graphical host** when its
   track resumes, against §4.8.

## 6. Drawbacks & Alternatives

- **[DR] A dev-only example host, rather than a Cargo feature of the product binary.** A feature
  puts a dormant enable path in every shipped binary and is additive across a workspace build, so
  one consumer enabling it re-enables it for all — the two reasons `l2-crate-topology.md` §7
  rejects features as a boundary. An example target is absent by construction. Cost: the host's
  entry procedure is not the product's, which the shared composition function and the real-host
  test contain. (Override: a `drive` feature behind the same table, if a host must be the shipped
  binary.)
- **[DR] A spool directory, rather than a socket or an inherited pipe.** No endpoint at all
  (DRV-2), nothing to bind, and the protocol is readable with a text viewer; it needs only the
  standard library on every host. A pipe held by a parent would need a resident parent, which the
  stateless client deliberately is not. Cost: polling every few milliseconds while held, and the
  Windows read retry.
- **[DR] A launch argument enables a host, not an environment variable.** A variable is inherited
  by every process the product starts and survives in the shell; an argument is neither. It costs
  nothing and removes the need to scrub.
- **[DR] A stateless shell client first, a tool-protocol projection second.** The shell client
  works in every agent host and in CI with no registration; a projection generated from the table
  adds no behavior and can follow without reshaping anything. The study that this spec draws on
  made the same call for its own facility after trying a tool server first. (Override: build the
  projection first if an agent host is found where the shell route is unavailable.)
- **[DR] A generic library with no Cronus dependency**, so the instrument cannot depend on what it
  checks, and so a registered probe is the *only* way product knowledge enters.
- **[DR] The first claim is `surface-held`, not `exact`.** Reaching `exact` needs a clock the core
  can be handed and a model provider a world can script (§7.3); promising it earlier would make
  the class a statement about the driver's hopes.
- **[DR] An idle bound of 600 s.** Long enough for an actor to think between commands, short enough
  that a forgotten host does not hold a world through a working session. (Override: `--idle-quit`.)
- **[DR] Guidance exits 0 by default, and `--strict` makes it 3.** `l1-agent-tool-ergonomics.md`
  ATE-2 and ATE-13 make an unresolvable call success-shaped so an early rejection does not teach an
  agent to abandon the tool; a script that must not mistake *nothing ran* for success opts in.
- **Alternative — put the driver in `cronus-simulation`.** Rejected: it would fuse the scenario
  discipline to the instrument, give every host a dependency on the harness, and leave the
  debugging use with no home outside a scenario.
- **Alternative — pseudo-terminal only.** Rejected as the only lane: no hold, so no exact steps and
  no replay without an actor, plus a dependency before any value. Accepted as the free lane.
- **Alternative — a `Debug` dump of the view-model as the frame.** Rejected: it is not what a person
  saw, and its text is not stable across unrelated changes.
- **Drawback — `ready.json` and `record.jsonl` are one more contract.** Accepted; the protocol
  carries its own number and a client refuses a mismatch.
- **Drawback — concurrent actors are not prevented.** Two actors on one session interleave, oldest
  request first; each command is atomic and every request carries its id, arrival and intent in the
  record, so the interleaving is visible rather than forbidden. A session has one actor by
  convention, and a scenario run has exactly one by construction.

## 7. Findings Surfaced During Analysis

Recorded because each bears on the driver and each is independently actionable.

- **7.1 — The process wrapper's floor is a pipe, and the command line branches on a terminal.**
  `ext activate` and `activation enable` ask for a typed consent and refuse to proceed when standard
  input is not a terminal and no acknowledgement flag is given. The wrapper pipes standard input, so
  the interactive branch — the wording a person reads, the cancel path, the case-insensitive
  confirmation — is unreachable by usage simulation, and the coverage report cannot say so.
- **7.2 — The terminal interface composes inside `run()` and folds native events inside its
  production backend.** Both are what a host needs shared; neither is shared. The refactors change
  no behavior (§5 step 4).
- **7.3 — The product has no clock it can be handed.** The domain and core read the host clock
  directly in some twenty places. A held surface is the honest ceiling until a clock port exists;
  the `exact` class is not reachable before it.
- **7.4 — The graphical-shell specifications carry no accessibility requirement.** The cheapest
  faithful structured observation of a graphical surface is its roles and accessible names, and
  nothing requires the shell to expose them (§4.8).
- **7.5 — `product::resolve_binary` searches `target/<profile>/` and its `deps/` only**, so an
  example target is not found; extending it to `examples/` is part of §4.2.
- **7.6 — A window the actor opens can take the developer's focus.** The terminal host opens no
  window; the graphical host must be created without activation (§4.8).
- **7.7 — Worlds are created non-exclusively with default permissions under the platform temporary
  root, with a name derived from a timestamp.** On a per-user temporary area (Windows, macOS) that is
  private; on a shared Unix temporary root it is not. A session inside one must therefore check its
  own directory (DRV-2), and the world builder's creation should be exclusive and owner-only —
  recorded against `l2-simulation-suite.md` USM-2.
- **7.8 — No catalog of presentable states exists.** `cronus-sim coverage` complements *actions*
  against the invocable catalog and has nothing to complement screens or presentation conditions
  against, so DRV-13's gap rule is applied by hand until one is declared.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[SEAMS]` | `crates/tui/src/terminal.rs` | The backend seam and the production backend's fold — the input floor of §4.6. |
| `[LOOP]` | `crates/tui/src/app.rs` | The loop entry generic over backend, source and renderer, the pure render function and the production composition. |
| `[WRAPPER]` | `crates/simulation/src/bin/cronus_sim.rs` | The process wrapper whose verb family §4.10 extends. |
| `[WORLD]` | `crates/simulation/src/world.rs` | Where a world and its environment are built; the session directory lives inside it. |
| `[RESOLVE]` | `crates/simulation/src/product.rs` | Binary resolution, extended to host targets (§4.2). |
| `[CONSENT]` | `crates/cli/src/commands.rs` | The terminal-dependent consent branches of §7.1. |
| `[DISPATCH]` | `apps/desktop/tauri/src/bridge.rs` | The shell's shared dispatcher bridge, the shortcut path of §4.8. |

## Document History

| Version | Date | Notes |
| --- | --- | --- |
| 1.0.0 | 2026-10-03 | Initial spec — the realization of `l1-surface-driver.md`: a generic zero-Cronus-dependency library (`cronus-drive`) holding the command table, the spool-directory channel with its privacy checks and request expiry, the step controller and the frame model; a stateless client generated from the table; a dev-only terminal host built as an example target over the loop's existing backend and renderer seams, with native-shaped injection through a shared fold, a capturing renderer, a declared floor and class `surface-held` verified on every step; a free lane (pseudo-terminal, class `free`) for the interactive branches a pipe never takes and as the floor-covering lane; the graphical shell bound to the contract with its preconditions and its realization deferred; and composition with the simulation harness — `session` verbs, `drive` transcript entries, a host evidence stream that keeps shortcuts out of coverage, and `[[drive]]` replay steps. Eight findings recorded (§7), among them that the command line's interactive consent flows are unreachable by the present wrapper, that the product has no clock it can be handed, and that no catalog of presentable states exists. Post-Update Review PASS-WITH-REWRITES, rewrites applied: four outcomes (ran, guided, error, unreachable) so an unresolvable call is success-shaped guidance as the tool-ergonomics rules require, a calibration surface renamed away from any studied project's vocabulary, and several clarifications (floor, staleness, protocol number, crash evidence). |
