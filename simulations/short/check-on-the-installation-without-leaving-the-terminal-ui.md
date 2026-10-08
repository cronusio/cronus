---
id       = "check-on-the-installation-without-leaving-the-terminal-ui"
tier     = "short"
surfaces = ["tui", "cli"]
covers   = ["status", "doctor", "backup.list", "backup.create", "workspace.list", "ext.list"]
roles    = ["owner"]
vantage  = "builtin-help"
world    = "seeded-small"
bound    = { steps = 40, wall_secs = 600, spend_usd = 1.00 }

[[perturbation]]
class = "repetition"
at    = "asks for the health check a second time, now with the repair option, expecting it to behave like the first"

[[perturbation]]
class = "surface-crossing"
at    = "having looked around from inside the terminal UI, wants a backup taken before trying something risky and has to decide whether to do that here or in a shell"

[[obligation]]
id         = "the-installation-commands-are-found-from-the-listing-alone"
statement  = "the terminal UI's own command listing shows the commands for the installation apart from the commands on the person's work"
decided_by = "observation-of-output"
evidence   = "after the person asks the terminal UI for its command listing, the installation commands appear under a heading of their own, and the commands on their work appear above that heading"

[[obligation]]
id         = "a-health-check-inside-matches-the-one-outside"
statement  = "asking the terminal UI for a health check and asking the shell for one report the same findings"
decided_by = "observation-of-output"
evidence   = "the finding lines shown in the terminal UI's result block are the same lines the shell's diagnostics prints for the same workspace, or both report that all checks passed"

[[obligation]]
id         = "a-change-asked-for-inside-is-left-undone-and-points-to-the-shell"
statement  = "asking the terminal UI to take a backup makes no backup and names the shell command that does"
decided_by = "observation-of-state"
evidence   = "listing backups after the request shows no new backup, and the result block names a shell command the person could run to take it"
positive_control = "replays/backup-create-from-the-shell-succeeds.toml"

[[obligation]]
id         = "a-refusal-tells-a-first-time-reader-what-to-do-next"
statement  = "the result of a refused request tells a first-time reader that the request was left undone and which command to run instead"
decided_by = "judgement"
evidence   = "reading only the result block, a first-time reader can say that nothing changed and can type the command it names"
---

# Check on the installation, without leaving the terminal UI

## Persona

Someone who lives in the terminal UI all day and has just noticed that something feels off
— an agent that stalled, an extension that stopped answering. They do not want to leave the
screen they are working in to find out whether the installation itself is healthy, and they
have never needed to learn the shell commands for it. They know only what the terminal UI's
own listing shows them.

## Goal

Find out, from inside the terminal UI, whether the installation is healthy and what is in it;
then, before trying something risky, get a backup taken — wherever that turns out to be
possible — and be sure afterwards whether it actually happened.

## Notes

A discovery here most likely means one of: the installation commands are present but hidden
among the commands on the person's work, so they are never found; the terminal UI and the
shell answer the same question differently, so neither can be trusted; or a request that
changes the installation is refused so tersely that the person believes it was done — the
worst outcome for someone about to take a risk on the strength of a backup that does not
exist.
