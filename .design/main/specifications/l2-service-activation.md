# Service Activation (OS Realization)

**Version:** 1.0.2
**Status:** Stable
**Layer:** implementation
**Implements:** l1-background-activation.md

## Overview

The concrete realization of background activation on Windows, macOS, and Linux: which OS facility backs each activation mode, how registration is authorized, how the actual state is read back, and how the agent is structurally prevented from activating itself.

Two constraints drive every choice below and are worth stating before the mechanisms:

**The background engine runs as the user, never as a superuser.** A system-scoped engine must reach the same durable state root as a foreground one — the per-user application-data directory of `l2-filesystem-layout.md`. A service running as `LocalSystem`, or a `LaunchDaemon` running as `root`, has a different profile and would silently operate on a different (empty) state root. This single requirement eliminates the textbook Windows Service and the textbook root daemon, and it is what makes BA-11 (one engine per state root) hold across a mode switch.

**Elevation authorizes registration, never execution.** Every system-scoped path below is registered under an OS-mediated human authorization and then runs unelevated (BA-6).

## Related Specifications

- [l1-background-activation.md](l1-background-activation.md) - The concept this realizes; BA-1…BA-11.
- [l1-security.md](l1-security.md) - SEC-10 (the agent has no write path to activation); SEC-6 sandboxed execution, which enforces it.
- [l2-sandbox-policy.md](l2-sandbox-policy.md) - The deny-by-default filesystem scope that makes BA-4 structural (§4.5).
- [l2-tool-security.md](l2-tool-security.md) - Tool allowlisting; no activation tool is ever exposed to the agent (§4.5).
- [l2-filesystem-layout.md](l2-filesystem-layout.md) - The per-user state root every mode must share (STO-1, STO-2).
- [l2-crate-topology.md](l2-crate-topology.md) - Why OS registration lives in a platform adapter crate behind a seam, not in the domain tier.
- [l2-cli.md](l2-cli.md) - The verb-first command grammar the activation surface conforms to (§4.8).
- [l2-app-ui.md](l2-app-ui.md) - The settings surface that hosts the consent moment (BA-5).
- [l2-config-hotreload.md](l2-config-hotreload.md) - The agent-reachable configuration file that activation state is deliberately **not** stored in.
- [l2-technology-stack.md](l2-technology-stack.md) - Names the OS-service mechanisms this spec fixes; Tauri v2 as the desktop shell.

## 1. Motivation

The concept spec fixes *what* activation means and *who* may grant it. What remains is genuinely per-platform: three operating systems expose four different supervisors, three different authorization ceremonies, and three different ways to lie about whether a registration is actually in effect. Choosing wrong produces one of two failures — a background engine that cannot see the user's data, or an activation toggle whose displayed state is fiction.

## 2. Constraints & Assumptions

- The engine binary is identical in all modes (INV-8); activation supplies no special build, flag-set, or capability.
- The durable state root is the per-user application-data directory in every mode. A mode that cannot reach it is not a valid realization of that mode.
- Registration is performed by the human-interactive frontend (desktop app, or a CLI invocation the human types). It is never performed by agent-run code (§4.5).
- Only systemd is treated as a first-class Linux supervisor. On a host without systemd (OpenRC / runit / s6), **system-scoped activation reports `Unsupported` (BA-10)** — there is no init-agnostic user-supervisor that survives logout — while **login-scoped falls back to the init-independent XDG autostart entry** (`~/.config/autostart/*.desktop`, honored by every freedesktop-compliant desktop environment); see §4.4. A headless non-systemd host with no desktop session is a spoke for both modes.
- The consent and elevation prompts are OS-native. The product renders no imitation of a privilege dialog.

> [!NOTE]
> **Divergence from the stack spec — reconciled at promotion.**
> [l2-technology-stack.md](l2-technology-stack.md) §3 originally named the always-on hub's Windows mechanism as a "Windows service". This spec **rejects the Windows Service** on both available account models (§4.2 and §6): a machine account cannot reach the user's state root, and a named user account requires a stored credential that BA-6 forbids. It selects an S4U scheduled task instead, which yields the same lifecycle with neither defect.
> The divergence was confined to Windows; `systemd` and `launchd` are adopted as the stack spec names them. On this spec's RFC→Stable promotion, `l2-technology-stack` was amended (1.2.0 → 1.2.1) to replace "Windows service" with "Windows S4U scheduled task", so the two specs now agree. The remaining empirical question — whether an S4U task with an At-startup trigger launches before interactive logon — is an implementation-time validation gate (§4.2, §5 note 7), not a design blocker.

## 3. Invariant Compliance (Layer 2 only)

| L1 Invariant | Implementation |
| --- | --- |
| BA-1 Manual default | The installer registers nothing. No Run value, no login item, no unit file, no scheduled task is written at install time. The first background registration is always the consequence of a human act in the settings surface. |
| BA-2 Two modes, one engine | Login-scoped maps to the session-owned facility of each OS (Run value / `SMAppService.loginItem` / systemd **user** unit). System-scoped maps to the boot-time supervised facility (scheduled task with a startup trigger / `LaunchDaemon` / systemd **system** unit or lingering user unit). The same binary and the same state root serve both; only the supervisor differs. |
| BA-3 One registration | Every enable path executes remove-then-register: the adapter first removes the *other* mode's registration and verifies its absence (§4.6), and only then writes the new one. A verification failure aborts before writing, leaving the host in the `Inactive` state rather than doubly registered. |
| BA-4 Human-authored only | Enforced structurally by three independent facts, none of which relies on the agent's cooperation: (1) no activation tool exists in the agent's tool surface, and tool exposure is allowlist-inverted so a new one cannot appear by omission (`l2-tool-security`); (2) the OS registration locations — files, registry values, and the OS registration interfaces alike — are denied to the sandbox, so agent-run code cannot reach them even with a shell, and any location a platform's sandbox cannot deny is reported as a stated gap rather than assumed covered (§4.5); (3) system-scoped registration additionally requires an OS-mediated elevation the agent cannot supply. |
| BA-5 Disclosed autonomy grant | The consent moment lives in the settings surface (`l2-app-ui`), not in the OS dialog, and is rendered before the OS authorization is requested. It names the autonomy level and spend ceiling in force. The CLI equivalent refuses to run non-interactively without an explicit acknowledgement flag (§4.8). |
| BA-6 Least privilege | Login-scoped registration touches only per-user locations and never elevates. System-scoped registration elevates once, through the OS's own ceremony (UAC / `SMAppService` authorization / polkit), and the registered process runs **as the user, unelevated** on all three platforms (§4.2–§4.4). No resident privileged helper is installed. A refused authorization returns an error and mutates nothing. |
| BA-7 Reversible and complete | Disable removes the registration and then re-reads the OS to confirm absence (§4.6); a call that succeeded but left the entry is reported as failure. Uninstall enumerates all four possible registration locations — not only the currently-believed one — and removes those the product created, identified by a product-owned label. Foreign registrations are never touched. |
| BA-8 Observed state | Every read queries the OS. Where the OS distinguishes *registered* from *effective*, both are read and the weaker one wins: on Windows the `StartupApproved` veto is read alongside the `Run` value; on macOS `SMAppService.status` distinguishes `requiresApproval` from `enabled`; on Linux `is-enabled` is read together with the linger property. An unreadable facility yields `Unknown`, never `Enabled`. |
| BA-9 Supervision is the mode's | Restart policy is delegated to the supervisor and configured declaratively — `Restart=on-failure` (systemd), `KeepAlive` (launchd), the task's restart-on-failure settings (Windows). The engine's own crash-recovery ladder and liveness reconciliation are compiled and executed identically in all modes; nothing in the engine reads which supervisor started it. |
| BA-10 Spokes refuse visibly | The adapter's `capabilities()` probe returns `Unsupported { reason }` on a host with no usable supervisor: mobile targets (both modes), and a Linux host without systemd for **system-scoped** — where login-scoped is still offered via the XDG autostart entry (§2, §4.4), so the refusal is per-mode, not whole-host. The settings surface renders the reason and offers "connect to a hub" in place of the unavailable toggle. The setting is never rendered as an inert control. |
| BA-11 One engine per state root | Enforced by an exclusive lock on the state root taken at engine start, independent of activation mode. A frontend that fails to take the lock discovers the running engine's endpoint from the lock record and attaches to it (§4.7). A frontend that can neither lock nor attach fails visibly rather than starting a second engine. |

## 4. Detailed Design

### 4.1 The registration seam

OS registration opens registries, writes plists, and shells out to service managers — infrastructure by every definition, and therefore not domain code. Per the crate-minting rule of [l2-crate-topology.md](l2-crate-topology.md) §4.4 (a module earns a crate when it requires an infrastructure dependency the domain tier may not hold), registration lives behind a seam:

```rust
// [REFERENCE] the seam, in the contract crate — no OS types cross it
pub enum ActivationMode { Login, System }

pub enum ActivationState {
    Inactive,
    Active(ActivationMode),
    RequiresApproval(ActivationMode),   // registered, vetoed by the user or OS (BA-8)
    Unknown { reason: String },         // the facility could not be queried (BA-8)
}

pub trait ActivationRegistry: Send + Sync {
    /// What this host can offer. `Unsupported` on a spoke (BA-10).
    fn capabilities(&self) -> ActivationCapabilities;

    /// Read the OS. Never a cached value (BA-8).
    fn observe(&self) -> ActivationState;

    /// Remove the other mode, verify, then register. May prompt for elevation (BA-6).
    fn enable(&self, mode: ActivationMode) -> Result<()>;

    /// Remove and verify absence (BA-7).
    fn disable(&self) -> Result<()>;
}
```

The domain tier holds the policy — mutual exclusion, consent bookkeeping, the transition state machine — and calls the seam. The adapter (`cronus-activation-os`, or an equivalent platform crate) holds the OS calls. The domain never names a registry key.

There is no `set_state` and no persisted mirror. The only way to learn the activation state is `observe()`, and the only way to change it is `enable`/`disable` from an interactive frontend.

### 4.2 Windows

| Mode | Facility | Elevation to register | Runs as | Supervised |
| --- | --- | --- | --- | --- |
| Login | `HKCU\…\CurrentVersion\Run` value | none | the user | no |
| System | Scheduled Task, `At startup` trigger, `S4U` logon type, `RunLevel = Limited` | UAC, once | the user, unelevated | Task Scheduler restart-on-failure |

**Why not a Windows Service.** The Service Control Manager offers exactly two accounts for an unattended process: a machine account (`LocalSystem`, `LocalService`) or a named user account whose password must be stored in the LSA secret store. The first cannot reach the user's `%APPDATA%` state root; the second is a cached credential, which BA-6 rejects. Both are worse than the scheduled-task path, which obtains a user token *without* a password.

**S4U.** A task registered with the `S4U` (Service-For-User) logon type runs under the user's identity without a stored password, is not tied to an interactive session, and therefore survives logout — the semantics BA-2 asks of system-scoped activation. Registering a task with a startup trigger requires administrator rights, satisfying BA-6's "elevation for registration only"; `RunLevel = Limited` keeps the running engine unelevated. An `S4U` token carries no network credentials, which is immaterial here: the engine's outbound HTTPS to model providers requires none, and the product performs no authenticated SMB or Kerberos access. S4U produces a **local** user token, so the target user's profile must exist for `%APPDATA%` to resolve — guaranteed for the installing user, who has logged in at least once. The one empirical property to confirm at build time is timing: that the At-startup task actually launches before any interactive logon (validation gate, §5 note 7). Its documented fallback, if a given Windows build defers S4U-at-startup until first logon, is an `At log on` trigger — still session-independent supervision, one logon later — recorded so the mode degrades predictably rather than failing silently.

**Reading the truth (BA-8).** The `Run` value alone is not the state. Windows records a user's disablement of a startup entry — made through Task Manager's *Startup apps* tab — in a separate `StartupApproved\Run` binary value, leaving the original `Run` value intact. A product that reads only `Run` reports "enabled" for an entry Windows will never launch. The adapter reads both and reports `RequiresApproval` when the veto is present.

### 4.3 macOS

| Mode | Facility | Elevation to register | Runs as | Supervised |
| --- | --- | --- | --- | --- |
| Login | `SMAppService.loginItem` (or `.agent`) | none | the user | no |
| System | `SMAppService.daemon` → `LaunchDaemon` with `UserName = <user>` | authorization prompt, once | the user, unelevated | `launchd`, via `KeepAlive` |

A `LaunchAgent` runs only inside a user session and cannot satisfy "survives logout"; a `LaunchDaemon` runs outside every session but defaults to `root`. Setting the daemon's `UserName` to the installing user reconciles the two: session-independent supervision, user identity, the correct state root, no superuser at run time.

**Reading the truth (BA-8).** `SMAppService.status` is the reason BA-8 is written the way it is. It reports `notRegistered`, `enabled`, `requiresApproval` (the user has not yet approved the item in System Settings, or has disabled it there), and `notFound`. A mirrored boolean cannot express `requiresApproval`, and a product that stored one would show a green toggle for an item macOS is refusing to launch. The adapter maps `requiresApproval` straight to `ActivationState::RequiresApproval` and the settings surface tells the user where to approve it.

**Caveat.** A daemon running as the user outside a session is still subject to TCC (privacy) policy for protected locations. The state root under `~/Library/Application Support` is not TCC-protected, so the default path is unaffected; a workspace the user places under `~/Documents` or `~/Desktop` is, and the engine surfaces the resulting permission error rather than failing silently.

### 4.4 Linux

| Mode | Facility | Elevation to register | Runs as | Supervised |
| --- | --- | --- | --- | --- |
| Login | systemd **user** unit in `~/.config/systemd/user/`, `systemctl --user enable` | none | the user | `systemd --user` |
| System | systemd **system** unit with `User=<user>`, **or** the same user unit plus `loginctl enable-linger` | polkit, once | the user, unelevated | systemd |

Linux is the one platform where a system supervisor can run a process as an arbitrary user with no stored credential, so the system unit is straightforward: `User=<user>`, `Restart=on-failure`, and the state root resolves exactly as it does for a foreground launch.

**Two realizations, one mode.** Enabling *lingering* for a user causes systemd to start that user's manager at boot and run its enabled units without any login — session-independent, boot-started, systemd-supervised. That is the definition of system-scoped in BA-2, achieved without a system unit and without root, which matters for a user on a machine where they cannot obtain polkit authorization for `/etc/systemd/system`. The adapter therefore treats both as **system-scoped**, preferring the system unit when the user can authorize it and falling back to the lingering user unit when they cannot. Both survive logout; both start at boot; both run as the user. Presenting them as two modes would expose an implementation detail BA-2 explicitly denies.

**Reading the truth (BA-8).** `systemctl --user is-enabled` reports the unit's enablement, and `loginctl show-user --property=Linger` reports whether it will start without a login. Neither alone is the state: an enabled user unit without linger is login-scoped; the same unit with linger is system-scoped. The adapter reads both and derives the mode, which is precisely why the seam exposes `observe()` rather than a stored flag.

**Non-systemd hosts (OpenRC / runit / s6).** There is no init-agnostic user-supervisor that starts a unit at boot and restarts it, so `capabilities()` reports **system-scoped `Unsupported`** on these hosts (BA-10) rather than pretending. **Login-scoped remains available** through the init-independent XDG autostart entry — a `~/.config/autostart/cronus.desktop` file honored by every freedesktop-compliant desktop environment (GNOME, KDE, XFCE, …) regardless of init system. Its state is read back by the presence of that file plus its `Hidden`/`X-GNOME-Autostart-enabled` keys (the DE-level disable equivalent of the Windows `StartupApproved` veto), keeping BA-8 honest. A headless non-systemd host with no desktop session offers neither mode and is a spoke.

### 4.5 How BA-4 is enforced structurally

BA-4 claims the agent has no write path to the activation plane. On a host where the agent can run shell commands, that claim is only as good as its enforcement. Three independent barriers make it structural:

**No tool.** The activation seam is never exposed as an agent-callable tool. `l2-tool-security` builds the tool policy by inverting an allowlist, so a tool that is not explicitly permitted is disabled with a reason — a future activation tool cannot slip in by omission.

**No write path to a persistence location.** The registration locations are absent from every `read_write` entry of the sandbox filesystem scope (`l2-sandbox-policy` §4), which is deny-by-default. Agent-run code cannot write `~/Library/LaunchAgents`, `~/.config/systemd/user/`, `/etc/systemd/system/`, or the XDG autostart directory `~/.config/autostart/`, regardless of what it is persuaded to attempt — and the same deny list covers the other places a process can make itself start again without elevation: shell startup files and the user crontab spool.

Not every such location is a file. `HKCU\…\Run` is a registry value; a per-user scheduled task and a macOS login item are created through an OS API that a standard user may call without elevation. A filesystem scope alone does not cover those, so the sandbox baseline must also deny them at the level they live at: registry writes under the Run keys and access to the Task Scheduler interface (Windows), and the login-item and `launchctl` interfaces (macOS). Where a platform's sandbox backend cannot deny one of these, that path is a **stated gap** (SEC-12): the settings surface reports that background persistence is not structurally blocked on this host, rather than claiming a barrier that is not there. Writing to any of these locations is also a privileged sink in the AG-10 sense — a write that decides what will run later — so it is approval-tier even where the sandbox would have allowed it.

**No elevation.** System-scoped registration additionally requires an OS-mediated authorization the agent cannot produce — there is no API by which agent-run code satisfies a UAC consent, an `SMAppService` authorization, or a polkit prompt.

The first two are what matter, and specifically the second: login-scoped registration needs no elevation on any platform, so the sandbox's denial of every persistence location — file, registry, or API — is the only thing standing between a subverted agent and a persistent, unattended copy of itself. Those deny entries are a security control, not a hygiene measure, and they belong in the sandbox policy's baseline rather than in a preset.

The complementary channel stays open: the agent may call `observe()` — a read — and may raise a request the human resolves. Both are audited (SEC-7).

### 4.6 Verification: remove-then-register, and read-back

Every mutation is a three-step sequence, and the ordering is what makes a partial failure safe.

```text
[REFERENCE] enable(target_mode)
  1. remove(other_mode)            // no-op if absent
  2. observe() == Inactive ?       // read the OS back (BA-7)
       no  -> abort, report failure; host remains as it was
  3. register(target_mode)         // may prompt for elevation (BA-6)
  4. observe() == Active(target)?  // read the OS back
       no  -> report failure (incl. RequiresApproval, which is NOT success)
```

A failure at step 3 or 4 leaves the host in `Inactive` — BA-1's default — never in a doubly-registered state. `RequiresApproval` is reported as a distinct, actionable outcome rather than folded into success, because the engine genuinely will not start until the user approves it.

`disable()` is the same shape without step 3, and uninstall enumerates **all four** registration locations rather than the one currently believed active, so an orphan left by a previously-failed transition is still collected. Registrations lacking the product's own label are foreign and are reported, never removed (BA-7).

### 4.7 Attach, never duplicate

BA-11 is enforced below activation, so it holds identically for a manually-launched engine and a supervised one.

```text
[REFERENCE] engine start
  lock = try_exclusive_lock(state_root / "engine.lock")
  if acquired:
      write { pid, endpoint, version } into the lock record
      run as the engine
  else:
      read the lock record
      if version compatible -> attach to endpoint, run as a frontend/spoke
      else                  -> fail visibly (never start a second engine)
```

The lock record — not a fixed port, not a well-known socket name — is what a second launch reads to find the endpoint. Keying it to the state root rather than the machine means two engines over two distinct state roots coexist normally, while two engines over one state root cannot exist at all.

A stale lock (holder dead) is reclaimed through the existing liveness/crash-recovery path (`l1-work-liveness` WL-5 stranded-work reconciliation, `l1-crash-recovery` CR-1 unclean-shutdown detection); activation introduces no second reclamation mechanism.

### 4.8 Command surface

Conforming to the verb-first grammar of `l2-cli.md` §4, with the TUI mirroring each as a slash command and the desktop settings pane binding to the same core calls (INV-3 parity):

```text
cronus activation status                       # observe(); prints mode + effective state
cronus activation enable --mode login          # no elevation
cronus activation enable --mode system         # triggers the OS elevation ceremony
cronus activation disable
```

`enable` is interactive by default: it renders the BA-5 disclosure and requires confirmation. In a non-interactive context (a script, a CI runner) it **refuses** unless given an explicit acknowledgement flag, because an unattended `enable` would silently satisfy the consent moment BA-5 requires a human to see. That refusal is deliberate friction on the one command that grants the engine the right to run unattended.

The command is human-invoked by construction; it is not, and must never become, an agent tool (§4.5).

## 5. Implementation Notes

1. **Seam and probe first.** Land `ActivationRegistry` with `capabilities()` and `observe()` only, plus a no-op adapter. This makes BA-8 and BA-10 testable before any registration code exists, and gives the settings surface something honest to render.
2. **Add the sandbox deny entries before the first `enable()` ships.** The BA-4 barrier must precede the capability it guards; shipping registration first would open the window §4.5 exists to close.
3. **Login-scoped per platform**, cheapest and unelevated, exercising remove-then-register with only one mode implemented.
4. **System-scoped per platform**, each behind its own elevation ceremony. Windows last, gated by the S4U validation (§5 note 7): the elevation ceremony and `RunLevel = Limited` are settled by design, so the gate is purely the empirical launch-timing check, with the `At log on` fallback already specified if it fails.
5. **The state-root lock (§4.7)** is independent of the rest and can land at any point; it is a prerequisite for shipping *any* background mode, since a supervised engine plus a manually-launched one is the exact collision BA-11 forbids.
6. **Uninstall enumeration** is written against all four locations from the start, not grown incrementally, so that an orphan from a partially-implemented mode is still collected.

7. **S4U launch-timing validation (Windows system-scoped gate).** Before shipping Windows system-scoped activation, confirm on a real Windows build that an S4U task with an `At startup` trigger launches before interactive logon and resolves `%APPDATA%` for the installing user. Pass → ship as designed. Fail → switch that trigger to `At log on` (documented fallback, §4.2) — session-independent supervision one logon later, never a silent no-op. This is the sole empirical gate; every other Windows design choice (S4U over Service, `RunLevel = Limited`, `StartupApproved` read) is settled.

**Dependency decision (resolved): per-OS registration is implemented directly in the adapter crate; `tauri-plugin-autostart` is not taken.** The plugin covers login-scoped on all three platforms but nothing of system-scoped — the harder half — and it would place a Tauri/shell dependency inside an adapter crate the CLI also links, inverting the dependency direction and pulling the desktop shell into a headless path. The project dependency policy (prefer std; justify every third-party crate) rejects a plugin that solves half the problem at that cost; the underlying OS registration calls (an `HKCU\…\Run` value, an `SMAppService`/plist registration, a `systemctl --user` invocation or a `.desktop` file) are modest and platform-specific whether or not a plugin wraps them.

## 6. Drawbacks & Alternatives

**Drawback — four registration locations to enumerate on uninstall.** Two modes × the possibility of an orphan from a failed transition. Mitigated by labelling every registration the product creates and enumerating unconditionally (§4.6).

**Drawback — `observe()` on every settings render.** A registry read, an `SMAppService.status` call, or two `systemctl` invocations. Measured in milliseconds, on a screen the user opened deliberately, in exchange for never displaying a state the OS disagrees with.

**Alternative — a Windows Service under `LocalSystem`.** Rejected: it cannot reach the user's state root, and it would run the engine with far more privilege than any of its work requires (BA-6).

**Alternative — a Windows Service under a named account with a stored password.** Rejected by BA-6: a cached credential in the LSA secret store is a durable escalation path, and the S4U task achieves the same lifecycle without one.

**Alternative — macOS `LaunchAgent` for system-scoped.** Rejected: a `LaunchAgent` is session-scoped by definition and cannot survive logout, so it would silently implement login-scoped semantics under the system-scoped label.

**Alternative — expose linger as a third, user-visible mode.** Rejected by BA-2, which fixes the mode count at two. Linger and the system unit differ in mechanism, not in any property the user can observe: both start at boot, both survive logout, both run as the user. Surfacing the distinction would export an implementation detail as a decision the user is unequipped to make.

**Alternative — store activation state in `config.json` and reconcile with the OS periodically.** Rejected by BA-4/BA-8. It reintroduces the forgeable mirror the concept spec exists to prevent, and reconciliation cannot fix a window during which the mirror is authoritative and wrong.

**Risk — a supervisor restarts a crash-looping engine indefinitely.** Both `systemd` and `launchd` apply their own rate limiting (`StartLimitBurst`, launchd's throttle interval); the Windows task's restart count is bounded explicitly. The engine's bounded recovery escalation (`l1-work-liveness` WL-6) is unchanged and remains the primary defense — BA-9 forbids weakening it because a supervisor exists.

**Risk — an administrator's policy blocks the registration the user just authorized.** Surfaced, not fought: `observe()` reports the effective state, the user sees `RequiresApproval` or `Inactive`, and BA-7's rule against touching foreign registrations keeps the product from escalating a fight it should not be in.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[CONCEPT]` | `.design/main/specifications/l1-background-activation.md` | BA-1…BA-11, the invariants this realizes |
| `[SANDBOX]` | `.design/main/specifications/l2-sandbox-policy.md` | The deny-by-default filesystem scope that makes BA-4 structural (§4.5) |
| `[TOOLSEC]` | `.design/main/specifications/l2-tool-security.md` | Allowlist-inverted tool policy; no activation tool is exposed |
| `[LAYOUT]` | `.design/main/specifications/l2-filesystem-layout.md` | The per-user state root every activation mode must share |
| `[TOPOLOGY]` | `.design/main/specifications/l2-crate-topology.md` | Why the OS registration adapter is its own crate behind a seam |
| `[CLI]` | `.design/main/specifications/l2-cli.md` | The verb-first grammar `cronus activation …` conforms to |

## Document History

| Version | Date | Notes |
| --- | --- | --- |
| 1.0.2 | 2026-09-23 | Consistency pass (2026-09-23): BA-4's structural barrier was stated as a filesystem deny list, but `HKCU\\…\\Run` is a registry value and per-user scheduled tasks, macOS login items, XDG autostart, shell startup files and user crontabs are persistence paths a filesystem scope does not cover — the baseline denies them at the level they live at, a platform gap is reported as a stated gap (SEC-12), and a write to any of them is an AG-10 privileged sink. The BA-3 row named a `None` state the seam does not have — it is `Inactive`. |
| 1.0.1 | 2026-07-17 | Promoted RFC→Stable via Post-Update Review (`@role:spec-critic` + `@role:prompt-engineer`). Resolved all three open questions with design decisions (not deferrals): (1) **non-systemd Linux** — system-scoped reports `Unsupported` (BA-10), login-scoped falls back to the init-agnostic XDG autostart entry (`~/.config/autostart/*.desktop`), so the refusal is per-mode not whole-host (§2, §3 BA-10, §4.4); (2) **`tauri-plugin-autostart`** — rejected per the std-first dependency policy (covers only login-scoped, inverts the dependency direction by pulling Tauri/shell into a CLI-linked adapter); per-OS registration is implemented directly (§5); (3) **Windows S4U** — the design (S4U over Service, `RunLevel = Limited`) is settled; only the empirical At-startup launch-timing remains, reframed as a build-time validation gate with a documented `At log on` fallback (§4.2, §5 note 7), never a design blocker. Reconciled the §2 divergence: `l2-technology-stack` amended 1.2.0→1.2.1 ("Windows service" → "Windows S4U scheduled task"), so the two specs now agree; the callout is updated to record the resolution. Stable = design agreed; empirical S4U timing validated during implementation. |
| 1.0.0 | 2026-07-10 | Initial spec. Fixes the per-OS realization of BA-1…BA-11 under one governing constraint — the background engine runs as the user, never as a superuser — which rejects the `LocalSystem` Windows Service and the root `LaunchDaemon` and selects: Windows `Run` value / S4U scheduled task, macOS `SMAppService` login item / `LaunchDaemon` with `UserName`, Linux systemd user unit / system unit or lingering user unit. Specifies the `ActivationRegistry` seam with `observe()` and no persisted mirror (BA-8), the three structural barriers enforcing BA-4 (no tool, no sandbox write path, no elevation), remove-then-register with read-back verification (BA-3/BA-7), and the state-root lock realizing attach-never-duplicate (BA-11). Open: Windows S4U spike, non-systemd Linux hosts, `tauri-plugin-autostart` dependency decision. |
