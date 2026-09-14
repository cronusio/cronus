# Workspaces (offices)

Two kinds:

- **home** — one permanent, non-deletable organizer office (pinned tab, home/star icon).
  Cross-workspace oversight; default purpose: life management, reminders, scheduling.
- **project** — zero or more project offices, added via the tab bar "+".

A new project is created from `program/templates/workspace/` into
`state/workspaces/<id>/`, where `<id>` is the normalized name:
lowercase, only `-` as separator (e.g. "My Game Dev!" -> "my-game-dev").

Creation form fields (all editable later): name, description, local path.
On creation a default manager appears immediately and then hires/releases
specialists as the project's requirements evolve.

The example `default/` directory here stands in for the home workspace.
