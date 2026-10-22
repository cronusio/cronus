# Engine Endpoint & Work Lock

**Version:** 1.1.1
**Status:** Stable
**Layer:** implementation
**Implements:** l1-background-activation.md

## Overview

The concrete realization of BA-11 — one engine per state root, frontends attach and never duplicate — and of the gate that rests beside it: **an installation change that recomposes the office is applied only while no work is running over the state root, and no work can begin while it is applied.**

Four pieces, each answering one question and none doing another's job:

- a **seat**: the exclusive, operating-system-held claim on a state root, so at most one process is the engine for it;
- an **endpoint**: a loopback-only, token-guarded channel by which the other long-lived processes over the same state root dispatch their invocations to the engine instead of running them themselves;
- a **work lock**: a reader–writer lock the operating system holds, taken shared by every piece of running work in any process and exclusive by a change that recomposes — the one fact the gate rests on;
- a **ledger** in each process, naming its running work, so that a refused change can say *which* work stopped it.

With these, a `Recompose` installation verb (`l2-invocable-registry` §4.8.1) is refused for a stated, checkable reason — "an agent turn is running" — rather than on an assumption that the office is idle, which `l2-tui` §4.7 forbids.

## Related Specifications

- [l1-background-activation.md](l1-background-activation.md) — BA-11 is this spec's subject; BA-3 (at most one activation registration) is the same protection one level up.
- [l2-service-activation.md](l2-service-activation.md) — §4.7 states the attach rule in outline; this spec is its realization, and supersedes its sketch of the lock record (§4.2 below).
- [l1-architecture.md](l1-architecture.md) — INV-4 hub-and-spoke (the spoke here is at distance zero) and INV-8, whose sanctioned process boundary this is.
- [l1-work-liveness.md](l1-work-liveness.md) — WL-1 exclusive ownership and WL-4 single active run, which one seat per state root protects; WL-5 stranded-work reconciliation, the owner of recovery after a crash.
- [l2-invocable-registry.md](l2-invocable-registry.md) — The dispatch contract carried over the wire unchanged; the live-effect classes the gate serves (§4.8.1).
- [l2-tui.md](l2-tui.md) — The first resident frontend; it holds a role from start and asks this spec's gate before it asks the person to agree to a `Recompose` (§4.7 there).
- [l2-cli.md](l2-cli.md) — The short-lived frontend, which attaches when an engine exists and otherwise stands alone (§4.1).
- [l2-office-control.md](l2-office-control.md) — The drain-and-checkpoint path a refused `Recompose` offers; the ledger is what tells it there is something to drain.
- [l1-security.md](l1-security.md) — The endpoint is local, same-user and authenticated; it opens no egress.

## 1. Motivation

**BA-11 is mandated and unimplemented.** The state-root lock exists as a module that nothing calls, with a liveness check that always answers "alive" — a placeholder that can never reclaim a dead engine's claim and so would, once wired, lock every user out after the first crash. The attach half has no endpoint behind it at all.

**Today every process is its own engine.** The terminal UI, each command-line invocation and the desktop shell compose the same core over the same files. That works while nothing runs for long. It stops working the moment a turn, a workflow or a scheduled job can run in one process while another process changes what that work is built from — and a `Recompose` verb changes exactly that.

**A gate that cannot see all the work is worse than no gate.** A terminal UI that knows only its own chat turn, asked "is anything running?", would answer *no* while a workflow runs in another process — and would then be trusted. The honest gate has to be fed by something every process takes part in, whether or not it is attached to anything.

**The facts that need an owner are two, and they have different owners.** *Who runs the autonomous workload over this state root* needs one answer, and a process that holds it (the seat). *Is any work running at all* needs to be true for work in a process that never attached — a command that lasts a second and should not elect itself an engine — and is therefore the operating system's, not the engine's. Putting the second fact in the engine's memory, as a first design did, made the gate blind to exactly the processes that choose not to attach.

## 2. Constraints & Assumptions

- **The domain tier does no I/O.** The seat, the endpoint and the locks live in the facade (`cronus-core`), as the lock module already did. The wire types are the facade's, built on the contract types without changing them.
- **The contract types stay as they are.** They deliberately derive no deserialization (their names and binders are `'static`). The wire is therefore written out explicitly in both directions, and a rejection's binder is restored from the invocable's own declared binders.
- **Standard library first.** The seat and the work lock use the operating system's advisory file locks through `std`; the channel is `std::net` over loopback; the framing is line-delimited JSON through the `serde_json` the facade already carries; the token is drawn with the `getrandom` it already carries. No dependency is added.
- **Exact catalog match.** An engine and the processes that attach to it compose one catalog (§4.4). A mismatch fails visibly; it never degrades to a second engine and never to a guess.
- **Same user, same machine.** The endpoint binds the loopback interface only and answers only a caller that presents the engine's token, which is readable only where the state root is.
- **A claim dies with its holder.** The operating system releases a lock when the process ends, however it ends. No process-liveness probe is needed or kept.
- **A handler never reads the process's own directory.** An invocation made for a caller standing elsewhere means the caller's directory by "here" and by every relative path (§4.4); a structural test fails any handler that reads the process's own.

## 3. Invariant Compliance (Layer 2 only)

| L1 Invariant | Implementation |
| --- | --- |
| BA-11 One engine per state root | §4.2: the seat is an exclusive operating-system lock keyed to the state root. A resident process that cannot take it attaches (§4.4); one that can becomes the engine and serves (§4.3). A short-lived process attaches when an engine exists and otherwise runs on its own without claiming the seat (§4.1). Two engines over one root cannot exist; two over two roots coexist. |
| WL-1 Exclusive ownership / WL-4 Single active run | One seat per state root; every start of long-running work passes the work lock and the ledger (§4.6). |
| INV-3 Command parity | The wire carries the shared dispatch contract unchanged: the same invocation in, the same outcome out. A spoke does not re-derive anything; it re-points the handlers of the loci that run in the engine (§4.4). |
| INV-8 Single deployable | One binary plays every role. The attach channel is the sanctioned frontend↔core process boundary, loopback only; it is not a service and adds no orchestration dependency. |
| SP-13 Unrecognized ≠ unavailable | The wire distinguishes an unknown identity from a resolved one that could not answer, and an unreachable engine from both (§4.3). Neither is folded into the other. |
| SEC-6/SEC-9 No untrusted local caller | The endpoint demands the token on every request, compares it without early exit, and caps every frame (§4.3, §4.9). |
| INV-4 Hub-and-spoke | A resident frontend that wins the seat is a hub at distance zero; one that loses it is its spoke. A background-activated engine (BA-2) wins the seat first and every frontend is then a spoke, with no code path that differs. |

## 4. Detailed Design

### 4.1 Roles

A process over a state root is exactly one of:

| Role | Meaning |
| --- | --- |
| **Engine** | Holds the seat. Composes the core as it always did, serves the endpoint, names its running work. |
| **Spoke** | Did not get the seat. Composes the same catalog, but the handlers of every locus that runs in the engine forward to it (§4.4). |
| **Stand-alone** | Short-lived, and found no engine. Composes and runs its own handlers; claims nothing and serves nothing. |

Which a process is follows from how long it lives. A **resident frontend** (terminal UI, desktop shell) always takes the seat or attaches: it is where a person stays, and an engine that other processes must reach cannot be a process nobody expects to stay. A **short-lived** command-line invocation that dispatches a semantic verb attaches if an engine owns the root and otherwise stands alone, because a command that lasts milliseconds is not somewhere a terminal UI that starts meanwhile should find itself a spoke of, and because electing it engine would make every pair of concurrent invocations a race between a server that is exiting and a client that has just found it. A command-line invocation that only inspects the installation neither attaches nor stands in anyone's way: it reads, it runs no work, and it must stay answerable when the composition itself is what failed (`l2-cli`).

Work run by a stand-alone process is still seen by every process that must know, because the work lock does not depend on attaching (§4.6).

### 4.2 The seat

Two files in the state root, because an operating-system lock held on a file makes the file unreadable to everyone else on some platforms:

- `engine.lock` — empty. The engine holds an exclusive advisory lock on it for as long as it runs. The lock is the claim.
- `engine.endpoint` — the record: the engine's process id, its loopback address, the token, its catalog release (§4.4), and a start time. Written by renaming a complete file into place, so a reader never sees half of it.

```text
[REFERENCE] join(state_root)
  lock := open(state_root/engine.lock)
  if lock.try_exclusive():                      // nobody holds it: this process is the engine
      delete state_root/engine.endpoint         // whatever is there belongs to a dead engine
      return Engine(seat = lock)                // the endpoint is published when serving starts
  else:                                         // a live engine holds it
      loop until deadline:
          record := read(engine.endpoint)
          if record exists and release equal:  return Spoke(connect(record))
          if record exists and release differs: fail "an engine of another release owns this state root"
          sleep briefly                          // the engine is between claiming and serving
      fail "the engine did not publish an endpoint"
```

A record whose engine has died is never trusted on its own: a lock that can be taken means there is no engine, whatever the record says; a record that exists while the lock is held was written by the holder, because a new holder deletes the old record before anything else. A joiner whose connection is refused retries the whole join until its patience runs out, so an engine that died between the lock check and the connect is replaced rather than reported.

Stale-claim recovery therefore needs no probing and no timeout: it is the operating system's. Work stranded by a crash is the business of the crash-recovery ladder (WL-5), which the seat neither replaces nor weakens.

### 4.3 The endpoint

A listener on the loopback interface, port assigned by the system, published only in the record. Every request is **one connection, one request**:

```text
[REFERENCE] request  (one JSON object, one line)
  { "v": 1, "token": "<hex>", "op": "invoke" | "work" | "ping", ... }

invoke: { "id": "core:board.list", "caller": "cli", "cwd": "<caller's directory>",
          "args": { "<binder>": <arg>, ... } }
  reply, one line each:
    { "t": "unknown" }                                   // the identity resolved to nothing
    { "t": "outcome", "outcome": <outcome> }             // a finished result
    { "t": "chunk", "value": <value> } …                 // a streamed result: pieces,
    { "t": "end", "outcome": <outcome> }                 //   then the settled result
work:   reply { "t": "work", "work": [ { "kind", "label", "since" } … ] }   // by name
ping:   reply { "t": "pong", "version": "<release>" }
bad token, bad version, oversize or malformed frame: { "t": "refused", "reason": "<why>" } and close
```

- **Values are tagged and ordered.** An argument or result value is written as a one-key object naming its kind (`text`, `int`, `float`, `bool`, `flag`, `list`, `empty`, `record`); a record is a list of name/value pairs so field order survives. A float that is not finite is refused at the sender rather than written as something a reader would misread.
- **An outcome is** `value`, `unavailable` (with its reason), or `rejected` (binder, mode, detail). A stream handle never crosses the wire: the engine reads its own stream and relays it as `chunk`/`end` (§4.5).
- **Bounded.** A request line is capped (8 MiB); a connection that sends nothing for a bounded time is dropped; the number of connections served at once is capped. A stream relay stops at the first failed write.
- **Authenticated.** The token is 128 bits from the operating system's entropy source, in the record only. It is compared over its whole length. A request without it receives `refused` and nothing else; there is no unauthenticated operation, not even `ping`.
- **Masked once.** The engine's dispatcher masks secrets in every outcome and in every chunk (including across chunk boundaries) before it is written. A spoke masks again with its own list, which is the same list; neither trusts the other.
- **Stopping closes what is open.** When the server stops, the connections still being served are closed, so a spoke learns at once that its engine is going rather than at its next failed write. The work behind a stream is the work's to finish; the relay fails at its next write.

### 4.4 Spokes

A spoke composes the shared catalog exactly as an engine does (`invocable_bootstrap`), so its descriptors, binders and help are the engine's. That is only safe if the two really are the same catalog, so the **release** a process presents is the crate's version together with a fingerprint of every engine-run identity and its binders and locus: two builds that differ in what an invocation means differ here, and do not attach. It then **re-points the handlers** of the loci that run in the engine. Nothing else about the surface changes.

| Locus | In a spoke |
| --- | --- |
| `Semantic` | Forwarded. The work runs in the engine, enters its ledger and holds the work lock there, and uses its dispatcher, journal and secrets. |
| `Installation` | Forwarded, with the exception below. An `Inspect` verb reads the engine's state root; an `Install` verb acts on it. |
| `ClientLocal` | Stays local. It changes what this client shows and touches nothing the engine owns. |
| `HostOnly` | Stays local. |

**An invocation means the caller's directory.** The request carries the directory the spoke stands in, and the engine runs the handler as an invocation made *from there*: "the current directory" defaults and relative paths in its arguments resolve against the caller's, not the engine's. Handlers read it through one function (`paths::caller_dir` / `paths::from_caller`) and never the process's own, and a structural test fails any handler that does. The engine's **environment**, by contrast, is the engine's: what an environment variable configures (a model endpoint, a portable base) is read where the work runs.

An unreachable engine is an `Unavailable` naming that fact and its address, never an empty result and never a silent local fallback — a spoke that quietly ran the verb itself would be a second engine.

The command line's installation verbs are answered without a composition (so they work when composition fails) and so are not forwarded; they act on the state root directly. They are made safe against a running engine by the rule in §4.7, not by forwarding.

### 4.5 Streams

A handler that answers with a stream is read by the server through the dispatcher's own subscription, so masking and the single-subscriber rule apply, and relayed as `chunk` frames and one `end`. The spoke's forwarding handler reads the first reply frame: for `outcome`, `unknown` or `refused` it answers at once; for `chunk` it opens a stream on its **own** dispatcher, answers with that handle, and pumps the remaining frames into it on a thread of its own. A surface that cannot show chunks collects the end, exactly as for a local stream. A connection that drops mid-stream ends the local stream with an `Unavailable` saying so; the work in the engine runs to its end and is recorded there, because abandoning a half-written reply would leave the engine's journal inconsistent with what the person saw. Pieces may be cut differently from how they were written, because masking holds a secret-length tail back; their order and their text are what is guaranteed.

### 4.6 The work lock and the ledger

**The work lock.** `work.lock` in the state root. Every producer of long-running work takes it **shared** before the work starts and holds it until the work ends; a change that recomposes takes it **exclusive** for as long as the change takes. The operating system admits one or the other, so:

- a change cannot begin while any work, in any process, holds the lock;
- no work can begin while the change holds it, and work that cannot begin says so (`Unavailable`: an installation change is being applied);
- there is no instant between "nothing is running" and "the change holds the lock" in which work can slip in — the system decides both under one lock;
- a process that ends, however it ends, releases what it held. There are no marker files to go stale and nothing to probe.

A producer that cannot take the lock for a reason other than a change in progress (the state root is unwritable) does not start: work nobody can see is work a change could be applied over.

**The ledger.** In each process, a list of its running work by name. Kinds are a closed set — an agent turn, a workflow or automation run, a scheduled job, a long-running index build — so a new producer adds its kind there and the gate's idea of "work" is read in one place. A producer enters the ledger and takes the lock in one call (`begin_work`) and holds one guard; its drop ends the work in both, however the work ends, including a panic that unwinds. The ledger is what lets a refusal say *which* work, and it is answered over the endpoint (`work`) for a process that is not the engine.

**Short calls are not work.** Reading a board or a listing does not register. What registers is what a `Recompose` verb could alter if it ran underneath: something with a lifetime.

### 4.7 The Recompose gate

```text
[REFERENCE] before applying a Recompose verb
  lock := state_root/work.lock exclusive, try
  if not taken:
      names := this process's ledger ∪ the engine's ledger (asked over the endpoint, if one answers)
      refuse with names, or with "work is running in another process" when none can be named
      never queue, never apply anyway; offer the pause (l2-office-control)
  apply the change
  release the lock
```

The gate does not attach, claim the seat or depend on there being an engine: the lock is the operating system's, so work in a stand-alone process is as visible as work in the engine. The refusal's *reason* is as precise as the ledgers that can be reached; its *decision* never is less than the lock's. The terminal UI's and the command line's `Recompose` verbs share this gate; neither holds a copy of it. A verb is gated by the class it resolves to for these arguments (the repair flag; deleting the running workspace), not by its name.

`[ADDED v1.1.0]` **Three places ask it, each for its own reason.** The handler registered for every recomposing verb takes the gate itself before it changes anything and holds it until the change is done — the floor under every surface, since a surface that dispatches a verb without asking meets it anyway. A surface asks *whether the change could be applied now* (`check_for`: the same attempt, let go at once) before it asks the person, so nobody is invited to agree to a refusal; that answer is a moment's, and the handler's is the one that counts. A verb's **scope** says whose work it waits for: the workspace's own state (the archetype, a restore, diagnostics with repair) waits for work over that state root; the machine's (agent definitions, extensions, the developer office) waits for work over the machine's, because work in every workspace is built from them. Every producer of work registers over both (`begin_work` takes the state root and the machine's).

`[ADDED v1.1.0]` **A relaunch is the engine's.** A change applied by relaunch — a restore, the developer office admitted or revoked, diagnostics with repair — leaves the composition the engine runs out of date, and only the engine's process can rebuild it. A resident frontend that is the engine lets go of its role (the seat, the endpoint, the connections) and takes it again when it composes afresh; one that is a spoke does not offer those verbs, and says that the session that owns the engine is where to run them. Applying one from a short-lived command line while an engine runs is the same change to the same files, and leaves the engine's composition as stale as the command line's own was when it began; it is not refused, because stale is not running.

### 4.8 Failure modes

| Situation | Behavior |
| --- | --- |
| The engine exits while a spoke is connected | The spoke's next forward fails with `Unavailable` naming the loss. The next join elects a new engine. |
| The engine relaunches after a change applied by relaunch `[ADDED v1.1.0]` | Its role is let go and taken again; the spokes it had are cut off exactly as when it exits, and are restarted by their person. A spoke that finds the seat held by a new engine attaches to that one on its next start. |
| The engine exits mid-stream | The local stream ends `Unavailable`; the person is told the engine went away. |
| A different release holds the seat | Join fails, naming both releases and the engine's process. |
| The record is unreadable or corrupt while the lock is held | Join fails; it never guesses and never deletes a record it does not own. |
| The state root cannot be created or a lock cannot be opened | The operating system's reason, in words. A resident frontend that needs a role says so and exits; it does not run unowned. A producer of work does not start. A gated change is refused as *unknown*, never as idle. |
| A connection presents the wrong token | `refused`, closed, logged without the token. |
| A process is killed holding the work lock | The system releases it. |

### 4.9 Security

- Loopback bind only; never `0.0.0.0`. The record is created with owner-only permission where the platform has the notion, and otherwise inherits the state root's.
- The token never appears in a log, an error, a journal entry or an outcome, and is not an argument of any invocable.
- The endpoint exposes the dispatch contract and nothing else: no file access, no process control, no raw secret read. What a spoke can do is what the same verb does at the engine, under the same gating (`l2-invocable-registry`).
- A local process of **another** user cannot read the record and so cannot authenticate; one of the **same** user can, and is trusted exactly as that user's own command line is.

## 5. Implementation Notes

1. The seat replaces the placeholder lock module: the process-liveness parameter and its always-true default go; the file is the claim.
2. Producers today: the chat turn (`session say`), a workflow run, a loop run and a codegraph index build, each through `begin_work` before it starts; a test fails if any of them starts while a change is being applied. A scheduled job registers when its runner is composed into the engine; until then it is the stated gap in the gate's completeness, and §4.6's closed kind set is where a new producer is added. The cancellation registry that `session stop` reads is a separate concern from the ledger: it holds the means to cut a turn short, not the fact that one exists.
3. The terminal UI takes a role at start (`join_composition`) and holds it until it exits; the command line attaches for a semantic verb (`attach_or_stand_alone`) and gates a recomposing installation verb (§4.7) before applying it.
4. The desktop shell joins when its composition does; it embeds the core today and takes no seat. Until it does, a desktop running beside a terminal UI is two engines over one root — the pre-existing condition, now detected on the terminal side only. Its work already holds the work lock when it begins through `begin_work`, so the gate sees it.
5. A background-activated engine (BA-2) is this same code started without a frontend; making it so is a launch mode, not a second implementation.
6. **Scenario debt (QLY-8, recorded 2026-10-22).** The seat, the endpoint, the spoke attach and the `Recompose` gate are user-observable — a second terminal that joins the running office, a change refused with the name of the work that stopped it — and no usage-simulation scenario covers them yet (`l1-usage-simulation` USM-10, RULES C30). The chat turn and the terminal UI's conversation, sessions and board screens, built over the same period, now have scenarios (`talk-to-a-local-model-and-find-the-conversation-again`, `have-a-conversation-from-inside-the-terminal-ui`). This item is discharged when a scenario for the seat and the gate is authored; the difficulty is real — a refused change needs a turn that is genuinely running, which the harness's worlds cannot yet provide on demand — and is a reason to record the debt, not to omit it.

## 6. Drawbacks & Alternatives

**Cost.** Every semantic dispatch from a spoke is a loopback round trip and a JSON encode in each direction. The dispatch contract is small and local; the cost is a fraction of a millisecond against operations that already touch disk. A frontend that needs a tighter loop (streaming tokens) pays per chunk, which is the same cost as any pipe.

**Cost — an engine's exit ends its spokes.** A resident frontend that is the engine takes its spokes down with it. A spoke's next call fails visibly and the next launch elects a successor. Handing the engine to a survivor needs a state transfer protocol and is deferred; a background-activated engine (BA-2) removes the case.

**Alternative — the engine holds the gate in memory (a hold on its ledger, taken over the wire).** The first design. Rejected: it is blind to every process that did not attach, which includes every short-lived command, and "the gate sees the work in the processes that chose to be seen" is not a gate.

**Alternative — presence marker files, one per running work, scanned by the gate.** Rejected: a parallel register of what the system's lock already says, with its own staleness rules and a read race. The work lock carries the same fact with none of them; the ledger carries the names.

**Alternative — every short-lived command takes the seat and serves.** Rejected: concurrent invocations become a race between an engine that is exiting and a client that has just found it, and a terminal UI that starts meanwhile attaches to something that is about to go. A command that lasts a second is a guest, not a host.

**Alternative — a Unix socket / named pipe.** The same shape, but two platform APIs and permission models where one loopback listener with a token serves both, using only the standard library. Revisit if a platform's loopback policy forbids it.

**Alternative — keep the pid-in-lock-record and probe liveness.** Rejected: it needs a per-platform process probe, is racy against pid reuse, and the placeholder that stands in for it today can never reclaim anything. The operating system already knows when a holder is gone.

## Canonical References

| Tag | Path | Why |
| --- | --- | --- |
| `[BA]` | `.design/main/specifications/l1-background-activation.md` | BA-11, the rule this realizes |
| `[ACT]` | `.design/main/specifications/l2-service-activation.md` | §4.7 outline of the lock and attach |
| `[REG]` | `.design/main/specifications/l2-invocable-registry.md` | The dispatch contract carried unchanged, and the live-effect classes the gate serves |
| `[TUI]` | `.design/main/specifications/l2-tui.md` | The first resident frontend, its role at start and the gate it asks before a `Recompose` |

## Document History

| Version | Date | Notes |
| --- | --- | --- |
| 1.1.1 | 2026-10-22 | §5 note 6 records the scenario debt (QLY-8; RULES C30, USM-10) for the seat, the endpoint, the spoke attach and the `Recompose` gate: no usage-simulation scenario covers them yet. The chat turn and the terminal UI's conversation, sessions and board screens, built over the same period, now have scenarios (design-layer audit, 2026-10-22). |
| 1.1.0 | 2026-10-21 | §4.7: the gate is asked in three places — by the handler of every recomposing verb (the floor), by a surface before it asks the person, and by the producers of work, over the workspace's state root and the machine's according to the verb's scope. A relaunch is the engine's to make: the engine lets go of its role and takes it again; a spoke leaves those verbs to the session that owns the engine. §4.8: the relaunching engine's spokes are cut off as on exit. |
| 1.0.0 | 2026-10-21 | Initial spec. Realizes BA-11: the seat (an operating-system advisory lock plus a separately published record — no process-liveness probe), the loopback token-guarded endpoint with its one-request-per-connection line protocol, stream relay and the caller's directory carried with every invocation, spokes that re-point the engine-run loci's handlers under an exact catalog release, short-lived processes that attach when an engine exists and otherwise stand alone; the work lock (shared by work, exclusive for a recomposing change — the operating system's, so it sees processes that never attached) and the per-process ledger that names the work; and the `Recompose` gate built on them. States the producers that exist, the gap for those not yet wired, and the desktop shell's pending adoption. |
