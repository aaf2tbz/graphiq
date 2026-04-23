# Supported Harnesses

GraphIQ integrates with AI coding assistants ("harnesses") through the Signet MCP server. Any harness that supports MCP over stdio can use GraphIQ's code retrieval tools.

## How it works

Signet spawns an MCP stdio server (`signet-mcp`) that exposes GraphIQ tools to connected harnesses. The harness calls tool names like `signet_code_search`, `signet_code_context`, etc., and Signet routes them to the `graphiq` binary installed via Homebrew.

## Supported harnesses

| Harness | Integration | Notes |
|---|---|---|
| **Claude Code** | MCP stdio via `claude_desktop_config.json` or `settings.json` | Full support |
| **OpenCode** | MCP stdio via config | Full support |
| **Codex CLI** | MCP stdio via config | Full support |
| **Gemini CLI** | MCP stdio via config | Full support |
| **OpenClaw** | MCP stdio via config | Full support |
| **Forge** | MCP stdio via config | Full support |
| **Hermes Agent** | MCP stdio via config | Full support |
| **Oh My Pi** | MCP stdio via config | Full support |
| **Pi** | MCP stdio via config | Full support |

## Adding a new harness

1. Ensure the harness supports MCP over stdio.
2. Add a `connectorCapability` entry to `signet-plugin/manifest.json` with the harness ID, title, and summary.
3. Add the corresponding connector package in Signet (`packages/connector-<name>/`) if install-time setup is needed.

## Tool names

All GraphIQ MCP tools use the `signet_code_*` prefix:

- `signet_code_search` — Search indexed code
- `signet_code_context` — Read symbol context
- `signet_code_blast` — Analyze impact radius
- `signet_code_status` — Check index status
- `signet_code_doctor` — Diagnose index health
- `signet_code_constants` — Find shared constants

Backward-compat aliases (`code_search`, `code_context`, etc.) are registered as deprecated names that delegate to the canonical tools.
