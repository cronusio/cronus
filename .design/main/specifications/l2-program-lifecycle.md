# Program Lifecycle (Installation, Command Registration, Update)

**Version:** 1.2.0
**Status:** Stable
**Layer:** implementation
**Implements:** l1-program-lifecycle.md

## Overview

The concrete realization of the program lifecycle on Windows, macOS and Linux: where each installation route puts the program, how `cronus` becomes a command in every new terminal, the signed release manifest, the `update` and `shell` verb groups, the staged switch with an automatic way back, and uninstallation. Desktop bundles update through the desktop shell's updater; archive installations update themselves; package-manager and store installations are left to their owner.

## Related Specifications

- [l1-program-lifecycle.md](l1-program-lifecycle.md) - The contract this realizes (PLC-1…PLC-12).
- [l2-filesystem-layout.md](l2-filesystem-layout.md) - The program and state tier locations per OS; §4.1 there names the per-user default this spec installs into.
- [l2-invocable-registry.md](l2-invocable-registry.md) - §4.7.1 / §4.8.1: the installation declaration the new verbs join, each with its live-effect class.
- [l2-engine-endpoint.md](l2-engine-endpoint.md) - The work gate an `apply` takes and the engine-owned relaunch it is carried out by.
- [l2-service-activation.md](l2-service-activation.md) - The operating-system registrations of the always-on engine that a switch re-points and an uninstall removes.
- [l2-backup.md](l2-backup.md) - The backup a forward migration takes first, and the one a return restores.
- [l2-doctor.md](l2-doctor.md) - Probes for command registration, shadowing, a kept previous release and a half-applied switch.
- [l2-cli.md](l2-cli.md) - The command line is the guaranteed path for every verb here.
- [l2-technology-stack.md](l2-technology-stack.md) - The desktop shell's bundler and updater.

## 1. Motivation

The layout says where the program lives but no specification says how it gets there, how the person reaches it, or how it is replaced. This one fixes those mechanics so packaging, the command surface and the doctor agree on one account.

## 2. Constraints & Assumptions

- One executable, `cronus` (`cronus.exe` on Windows), carries the command line, the terminal interface and the engine; the desktop shell ships beside it in desktop bundles.
- Releases are published on the project's release host with one manifest per channel.
- The release signing key's public half is compiled into the program; the private half never sits in a tracked file or a build log. <!-- TBD: key rotation — a manifest signed by the current key announcing its successor, and how an installation that skipped that release learns the new key -->
- Windows is realized first; macOS and Linux follow the same verbs and differ only in §4.3.

## 3. Invariant Compliance (Layer 2 only)

| L1 Invariant | Implementation |
| --- | --- |
| PLC-1 Program tier only | Installers and `update apply` write `<program>/`, the command registration of §4.3 and the activation registration; they never open `<state>/` or a project folder. The forward migration runs in the new program's first open (STO-9), not in the updater. |
| PLC-2 Command in a new terminal | `cronus shell register` (run by every installer) adds `<program>/bin` to the per-user search path the OS's own way (§4.3); `cronus shell status` resolves `cronus` the way a *new* shell would — from the persisted user and machine path values, not the current process environment — and reports the resolved file, whether it is this installation, any shadowing installation, and that already-open terminals keep their old path. Profile edits live only inside a `# >>> cronus >>>` … `# <<< cronus <<<` block. |
| PLC-3 Verified before it runs | §4.2 manifest signed with an Ed25519 key (minisign format, the same format the desktop updater verifies); each artifact's SHA-256 is checked after download and before staging completes. A failure deletes the staged files and reports `refused: signature` or `refused: digest`. No flag disables verification. |
| PLC-4 Never interrupts work | `update apply` is an installation verb of class `Recompose` applied by relaunch: it takes the work gate (`l2-engine-endpoint` §4.7) and is refused while work it would alter is in flight, naming that work; `--when idle` and `--when next-start` record a pending switch instead, carried out by the engine at the next idle point or by the next start before composition. |
| PLC-5 Atomic, automatic way back | §4.4: the staged release is switched in by rename-aside, the previous release kept under `<program>/previous/`; the relaunched program runs the post-switch health check before opening state; a failure or a crash before the check passes switches back on the next start and reports it. `update rollback` returns explicitly while `previous/` exists. Activation and command registrations point at stable paths, so the switch leaves them valid. |
| PLC-6 Forward once, never backwards | The new program's first open of each store takes the STO-9 safety copy and migrates; `install.json` records the release and the backup that preceded the migration. `update rollback` after a migration restores that backup through `l2-backup` and states first, in plain words, what is lost (work since the migration). Every store already refuses a newer version (`l2-filesystem-layout` STO-9 row). |
| PLC-7 Announced, never pressed | `[MODIFIED v1.2.0]` Only when `update.check` is on (§4.1.1), the engine checks at most once per day and announces a newer release once, as an inbox notice and a status-line marker: version, plain-language notes in the active locale (`notes.<locale>`, English fallback), and a security marker. Dismiss, postpone (7 days) and "turn off checks" are offered; no countdown, no modal. A person who never turned checks on sees no announcement and no `update check` traffic; `update status` reports the installed release and the time of the last check, or that none has run. `update.auto = true` (off by default) stages and applies at the next idle point under PLC-4. |
| PLC-8 Private, quiet | The check is one HTTPS GET of the manifest with a fixed user-agent naming only the product; no query string, no identifier, no cookie. Offline or an unreachable host yields `could-not-check` with the time of the last successful check; the next attempt waits for the next daily slot. `[MODIFIED v1.2.0]` `update.check` is `false` until the person answers the one-time question (§4.1.1); setting it back to `false` turns checks off again. |
| PLC-9 Channels and skew | `update channel <stable\|pre-release>` (default `stable`) is an `Install` verb; the program never changes it. Versions are SemVer starting at 0.1.0. After a switch, the engine's peer-compatibility verdict (PCO-14) is what an older spoke or extension sees on connect. |
| PLC-10 Owner updates | `install.json` records `origin` (§4.1). For `package-manager:<name>`, `store` and `source`, `update check` still reports a newer release but `apply` is refused with the owner's command (`winget upgrade Cronus`, `scoop update cronus`, `brew upgrade cronus`, the distribution's package command) or the store page; a source build reports `source build — not self-updating`. |
| PLC-11 Uninstall keeps the state | §4.5: the Windows uninstaller and `cronus uninstall` (archive installs) disable activation, run `shell unregister`, and remove `<program>/`; `<state>/` stays unless `--remove-state` was chosen, which offers `backup create` first. Every registration removed is listed in the outcome. |
| PLC-12 One verb set | `update status\|check\|apply\|rollback\|channel`, `shell status\|register\|unregister` and `uninstall` join the installation declaration (`l2-invocable-registry` §4.7.1) with classes `Inspect` (status, check, shell status), `Install` (channel, shell register/unregister) and `Recompose` (apply, rollback, uninstall). The terminal UI and desktop render them under the same names; the command line answers all of them without composing the office. |
| PLC-13 Stable entry in every registration | **Pending — to verify.** The search-path entry already names `<program>/bin`, which the switch keeps stable (§4.4); the engine's login/service registration and any completion or helper registration must name the same stable entry, never a staged release directory, and `doctor` must check each against it. |
| PLC-14 Announcement timing | **Partial.** The engine already checks at most once per day (PLC-7 row). Owed: a command-line check only when standard error is a terminal and never under CI, abandoned at exit if unfinished; package-manager origins announced only after the channel carries the release or a declared grace period; per-extension throttling. |

## 4. Detailed Design

### 4.1 Installation routes and `install.json`

| Route | Windows | macOS | Linux | `origin` |
| --- | --- | --- | --- | --- |
| Product installer, per user (default) | NSIS bundle into `%LOCALAPPDATA%\Programs\Cronus\` | `.dmg` → `~/Applications/Cronus.app` | AppImage under `~/.local/lib/cronus/` | `installer` |
| Product installer, all users | MSI into `%ProgramFiles%\Cronus\` (administrator) | `/Applications/Cronus.app` | `.deb`/`.rpm` → `/opt/cronus/` | `installer` |
| Archive (command line only) | `.zip` unpacked anywhere, then `cronus shell register` | `.tar.gz` | `.tar.gz` | `archive` |
| Package manager / store | winget, scoop | Homebrew | distribution repositories, Flathub | `package-manager:<name>` / `store` |
| Source build | `cargo` build tree | same | same | `source` |

```text
[REFERENCE]
<program>/install.json
{
  "origin":   "installer" | "archive" | "package-manager:winget" | "store" | "source",
  "scope":    "user" | "machine",
  "channel":  "stable" | "pre-release",
  "release":  "0.1.3",
  "previous": "0.1.2" | null,          // present while <program>/previous/ is kept
  "pending":  { "release": "0.1.4", "when": "idle" | "next-start" } | null,
  "migration_backup": "<backup id>" | null,  // the STO-9 copy taken by this release's first open
  "update_check": true | false | null        // the installer's answer to the §4.1.1 question; null = never asked
}
```

A package or store installation writes `origin` at packaging time; a source build has no `install.json` and is treated as `source`.

#### 4.1.1 The update-check question `[ADDED v1.2.0]`

An update check is off until the person says otherwise (`l1-program-lifecycle` §2, decided 2026-10-22; DN-1, DN-4, HSO-4). The question is asked exactly once, in plain words, with the box **not** ticked:

```text
[REFERENCE]
question  : "Tell me when a new version is available?"
detail    : "Once a day Cronus asks the release server for the latest version number.
             It sends nothing about you, your projects or your work. If you say no,
             Cronus never contacts the server, and you will only learn about a
             new version — security fixes included — by checking yourself."
choices   : [ ] Yes, check once a day        (not pre-selected)
            [x] No, never                    (the answer when nothing is chosen)
```

Where it is asked depends on the route. The product installer (NSIS, `.dmg`, AppImage, MSI) shows it on its options page and writes the answer into `install.json` as `update_check` (`true`, `false`, or absent when it was never asked). Every other route — archive, package manager, store, source build — has no installer page, so the engine asks at the first **interactive** start, when `update_check` is absent and the `update.check` setting has never been written. The answer is stored as the `update.check` setting (state tier) and the question is never asked again; `update status` and the settings surface let the person change it at any time. A non-interactive start — a pipe, automation, continuous integration, a service start with no terminal — never asks and never checks, which is also the state of a person who has never been asked.

### 4.2 Release manifest

```text
[REFERENCE]
<release host>/manifest-<channel>.json   +   manifest-<channel>.json.minisig
{
  "version":   "0.1.4",
  "channel":   "stable",
  "published": "2026-11-02T10:00:00Z",
  "security":  false,
  "notes":     { "en": "…", "ru": "…" },           // plain words for the person, not a commit log
  "artifacts": {
    "x86_64-pc-windows-msvc": { "installer": { "url": "…", "sha256": "…" }, "archive": { "url": "…", "sha256": "…" } },
    "aarch64-apple-darwin":   { … },
    "x86_64-unknown-linux-gnu": { … }
  }
}
```

The signature covers the manifest bytes; each artifact's digest is in the signed manifest, so one verified manifest vouches for every file it names. The desktop shell's updater verifies its own bundle against the same key.

### 4.3 Command registration per OS

| OS | `shell register` does | What a new shell sees |
| --- | --- | --- |
| Windows | Appends `<program>\bin` to `HKCU\Environment\Path` (keeping it `REG_EXPAND_SZ`, not duplicating an existing entry) and broadcasts `WM_SETTINGCHANGE` with `"Environment"`. An all-users install uses the machine path instead. | New terminals, and terminals restarted by Explorer, resolve `cronus`; terminals already open do not, and `shell status` says so. |
| macOS | Symlinks `~/.local/bin/cronus` → the bundle's binary; if `~/.local/bin` is not on the login shell's path, adds the marked block to `~/.zprofile` (and `~/.bash_profile` when it exists). | New login shells. |
| Linux | Symlinks `~/.local/bin/cronus` → `<program>/bin/cronus`; adds the marked block to `~/.profile` only when `~/.local/bin` is not already on the path. Packages install `/usr/bin/cronus` themselves. | New login shells. |

`shell register --completion` also installs the completion script the existing `completion` verb generates, inside the same marked block. `shell unregister` removes exactly what `register` added: the path entry it wrote (never an entry it did not write), the symlink if it still points into this installation, and the marked block.

`shell status` reports one of `registered`, `registered (open terminals need a restart)`, `shadowed by <path>`, `not registered`, each with the exact command that changes it. The doctor runs the same observation as a probe.

### 4.4 Staging, switch, health check, return

```text
[REFERENCE]
apply(release):
  manifest := fetch + verify signature                    // PLC-3
  artifact := download to <program>.staging/ ; verify sha256
  take work gate (refuse if work in flight, or record pending)   // PLC-4
  copy <program>/{bin,app,…} -> <program>/previous/       // kept for return (PLC-5)
  for each file in staging:                               // rename-aside: a running image is renamed, never overwritten
      rename <program>/<f> -> <program>/<f>.old ; move staging/<f> -> <program>/<f>
  write install.json { release, previous, migration_backup: null }
  relaunch through the engine (l2-engine-endpoint §4.8)

first start of the new release, before composition:
  health := binary starts, configuration parses, every store's version is ≤ supported, quick doctor probes pass
  if !health or a crash marker from a previous attempt exists:
      switch back from <program>/previous/ ; report "update to X failed: <reason>; returned to Y"
  else:
      delete <program>/*.old ; continue — each store migrates on first open after its STO-9 copy

rollback():
  require <program>/previous/
  if install.json.migration_backup: state what is lost since that backup, then restore it (l2-backup)
  switch back exactly as above
```

`previous/` is released 14 days after a successful switch or by the next successful update, whichever comes first. A desktop bundle performs the same sequence through its updater and then restarts the shell; the engine it hosts is relaunched by the same path.

### 4.5 Uninstallation

| Route | How | Order |
| --- | --- | --- |
| Product installer | the operating system's "remove program" entry runs the bundle's uninstaller | `activation disable` → `shell unregister` → remove `<program>/` |
| Archive | `cronus uninstall [--remove-state]` | same order; the running executable's own file is removed after the process exits (on Windows by a deferred delete) |
| Package manager / store | the owner's command | the package's own scripts call `shell unregister` and `activation disable` when present |

`--remove-state` asks first whether to take a backup and where to put it; without it `<state>/` and every project's `.cronus/` remain, and the outcome says where they are.

### 4.6 Command surface

| Action | CLI | TUI | Class |
| --- | --- | --- | --- |
| Installed release, channel, origin, pending switch, kept previous | `cronus update status` | `/update status` | Inspect |
| Check for a newer release | `cronus update check` | `/update check` | Inspect |
| Apply | `cronus update apply [--when now\|idle\|next-start]` | `/update apply` | Recompose (relaunch) |
| Return to the previous release | `cronus update rollback` | `/update rollback` | Recompose (relaunch) |
| Choose channel | `cronus update channel <stable\|pre-release>` | `/update channel …` | Install |
| Command registration | `cronus shell status \| register [--completion] \| unregister` | `/shell …` | Inspect / Install |
| Uninstall (archive installs) | `cronus uninstall [--remove-state]` | — (answered with the command) | Recompose |

Settings: `update.check` (default `false` until the §4.1.1 question is answered — `[MODIFIED v1.2.0]`, was `true`), `update.auto` (default `false`), `update.channel` (default `stable`).

## 5. Drawbacks & Alternatives

- **Rename-aside leaves `.old` files until the next start.** Accepted: it is what lets a running executable be replaced on Windows without a reboot; the next start removes them.
- **Two updaters (desktop bundle, archive) share one manifest and key but not one code path.** Accepted: the desktop shell's updater already handles its installer formats; the archive path is small. Both are held to the same verification and the same health check.
- **Alternative — a separate launcher executable that selects a version directory.** Rejected: it would make two executables where the product promises one, and the launcher itself would then need updating.
- **Alternative — machine-wide installation by default.** Rejected: it needs administrator rights for the first step a non-specialist takes and for every update after it.

## Canonical References

| Alias | Path | Purpose |
| --- | --- | --- |
| `[CONTRACT]` | `.design/main/specifications/l1-program-lifecycle.md` | PLC-1…PLC-12 |
| `[LAYOUT]` | `.design/main/specifications/l2-filesystem-layout.md` | Tier locations per OS |
| `[INSTALL-VERBS]` | `crates/core/src/installation.rs` | The installation declaration the new verbs join |
| `[ENDPOINT]` | `.design/main/specifications/l2-engine-endpoint.md` | Work gate and engine-owned relaunch |

## Document History

| Version | Date | Notes |
| --- | --- | --- |
| 1.0.0 | 2026-10-22 | Initial realization of `l1-program-lifecycle`: installation routes and `install.json` (origin, scope, channel, pending switch, kept previous, migration backup); a signed per-channel manifest with per-artifact digests and plain-language notes in English and Russian; command registration per OS (Windows per-user path plus the settings broadcast, a symlink into `~/.local/bin` elsewhere, profile edits only inside a marked block) with a status that resolves `cronus` as a new shell would; staged switch by rename-aside, previous release kept 14 days, post-switch health check with automatic return, explicit rollback restoring the migration backup; uninstall keeping the state tier; the `update`, `shell` and `uninstall` installation verbs with their effect classes. All Pending in code. Open: release-key rotation. |
| 1.1.0 | 2026-10-22 | Invariant Compliance rows for PLC-13 (registrations name the stable entry) and PLC-14 (announcement timing: terminal only, never delays the command, channel lag, per-extension throttle). |
| 1.2.0 | 2026-10-22 | `update.check` defaults to `false` (was `true`) and the question that turns it on is specified: new §4.1.1 (asked once, unticked, plain words naming what is sent and what declining costs; the installer asks on its options page and records `update_check` in `install.json`, every other route asks at the first interactive start; a non-interactive start never asks and never checks); `install.json` gains `update_check`; the PLC-7 and PLC-8 rows follow; `update status` reports the time of the last check, or that none has run. Follows the L1 decision of the same date. |
