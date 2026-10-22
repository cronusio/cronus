---
id       = "talk-to-a-local-model-and-find-the-conversation-again"
tier     = "short"
surfaces = ["cli"]
covers   = ["session.say", "session.list", "session.show", "session.stop"]
roles    = ["owner"]
vantage  = "builtin-help"
world    = "fresh"
bound    = { steps = 30, wall_secs = 600, spend_usd = 0.50 }

[[perturbation]]
class = "interruption"
at    = "believing a reply is still being written, asks from a second shell for it to be cut short"

[[perturbation]]
class = "repetition"
at    = "asks a second question in the same conversation and expects the model to know what was said a moment ago"

[[obligation]]
id         = "a-reply-that-cannot-be-had-says-where-it-went"
statement  = "asking for a reply while the model server is switched off tells the person the server was unreachable and names the conversation the attempt was recorded in"
decided_by = "observation-of-output"
evidence   = "the failure text names the address that was tried and a conversation id, and names the command that reads that conversation back"

[[obligation]]
id         = "a-conversation-whose-reply-failed-is-still-kept"
statement  = "the person's message is still held in a conversation after its reply failed, and the conversation is listed"
decided_by = "observation-of-state"
evidence   = "the conversation listing names the conversation from the failed attempt; showing it prints the person's message followed by an entry that records the failure"

[[obligation]]
id         = "stopping-from-a-second-shell-says-what-that-shell-can-reach"
statement  = "asking from a second shell to stop a conversation's turn answers in plain words that the turn is outside what that shell can reach, instead of reporting it stopped"
decided_by = "observation-of-output"
evidence   = "the stop request answers that no turn is running in that conversation in this process and exits unsuccessfully; the conversation read back afterwards is unchanged by the request"

[[obligation]]
id         = "a-help-listing-is-enough-to-find-the-conversation-commands"
statement  = "the shell's own help for the session commands names how to talk to a model, list conversations and read one back"
decided_by = "judgement"
evidence   = "reading only the help for the session group, a first-time reader can name the command that asks a model a question, the one that lists conversations and the one that shows one"
---

# Talk to a local model and find the conversation again

## Persona

Someone who has a model running on their own machine and wants to ask it a question from
the shell, the way they would ask a colleague — then come back an hour later and see what
was said. They have not read any documentation; they know only what the product's own help
tells them.

## Goal

Ask a question, get an answer or a clear reason there was none, and be able to find that
exchange again afterwards — including when the model was not running at the moment they asked.

## Notes

A discovery here most likely means one of: a failed reply leaves no trace, so the person
believes nothing was said; the failure text does not say where to look, so the exchange is
effectively lost; or asking to stop a reply from a second shell appears to succeed when the
reply is in fact running somewhere the request cannot reach — the person then thinks the
model has gone quiet when it is still working.
