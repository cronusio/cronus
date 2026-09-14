# Extensions (custom + generated)

Unified registry for three kinds: skill | mcp-server | plugin.
- `skills/` — custom and generated skills (generated ones await review).
- `mcp/` — connected MCP server configs (Cronus is the client).
- `plugins/` — installed code plugins.

Read-only preset extensions live in the program tier (`program/extensions/`).
All extensions are default-deny + sandboxed: scoped fs/network/secret grants, egress gate.
Each declares an `extension.json` manifest (kind, capabilities, permissions, source).
