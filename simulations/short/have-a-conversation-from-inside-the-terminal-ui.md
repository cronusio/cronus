---
id       = "have-a-conversation-from-inside-the-terminal-ui"
tier     = "short"
surfaces = ["tui", "cli"]
covers   = ["tui.chat", "tui.sessions", "tui.board", "session.say", "session.list", "session.show", "board.list"]
roles    = ["owner"]
vantage  = "builtin-help"
world    = "seeded-small"
bound    = { steps = 40, wall_secs = 600, spend_usd = 0.50 }

[[perturbation]]
class = "surface-crossing"
at    = "after talking to the model from inside the terminal UI, wants to be sure the same conversation can be read from a shell"

[[perturbation]]
class = "reversal"
at    = "leaves the conversation screen in the middle of waiting for a reply and then returns to it"

[[obligation]]
id         = "a-reply-that-cannot-be-had-is-explained-on-the-conversation-screen"
statement  = "when the model server is switched off, the conversation screen shows the message that was sent and a line saying the server was unreachable"
decided_by = "observation-of-output"
evidence   = "after sending one message with the server off, the conversation shows the person's message and a line naming the address that was tried; the keys keep responding and the ticker keeps moving"

[[obligation]]
id         = "the-sessions-screen-lists-what-the-shell-lists"
statement  = "the conversations shown on the sessions screen are the conversations the shell's session listing prints for the same workspace"
decided_by = "observation-of-output"
evidence   = "the conversation ids on the sessions screen match, entry for entry, the ids the shell's session listing prints for the same workspace"

[[obligation]]
id         = "the-board-screen-shows-what-the-shell-lists"
statement  = "the board screen shows, column by column, the cards the shell's board listing prints"
decided_by = "observation-of-output"
evidence   = "every card in the shell's board listing appears on the board screen under the column for its state, and each column's count matches the number of cards under it"

[[obligation]]
id         = "one-key-leaves-a-full-screen-view-and-the-conversation-survives"
statement  = "pressing the escape key on a full-width screen returns to the overview, and the conversation read back afterwards is unchanged"
decided_by = "observation-of-state"
evidence   = "after the key, the overview is shown and the application is still running; showing the conversation from the shell prints the same entries as before the key"
---

# Have a conversation from inside the terminal UI

## Persona

Someone who spends the day in the terminal UI and would rather not leave it to talk to a
model, look at past conversations or see the board. They know only what the terminal UI's own
command listing and function-key bar show them.

## Goal

Ask the model a question from inside the screen they are already in, look back over the
conversations they have had, glance at the board — and be able to trust that what the screen
shows is what the shell would show for the same workspace.

## Notes

A discovery here most likely means one of: the conversation screen freezes while a reply is
awaited, so the person thinks the application has hung; a screen shows a copy of the data
that has drifted from what the shell reports, so neither can be trusted; or leaving a
full-width view loses or reorders what was said.
