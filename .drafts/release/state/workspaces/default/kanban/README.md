# Kanban Board (one per office)

Canonical fixed pipeline: triage -> todo -> ready -> running -> blocked -> done.
Cards are moved by the office (manager/agents), not the client.
`done` cards auto-archive into `archive/` by a configurable condition; nothing is deleted.

- `board.json` — board meta + state set + card index.
- `cards/<id>.json` — one active card per file (state, task ref, reason?, history).
- `archive/<id>.json` — auto-archived done cards (history preserved).
