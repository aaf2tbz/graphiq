# Supported Harnesses

GraphIQ integrates with AI coding assistants through two independent surfaces: its own MCP server and the Signet MCP proxy. Each supports a different set of harnesses.

## GraphIQ (native)

GraphIQ ships a standalone MCP server (`graphiq-mcp`) that can be configured directly in harness configs without Signet.

| Harness | Status | Notes |
|---|---|---|
| **Claude Code** | Supported | Configure in `claude_desktop_config.json` |
| **Codex CLI** | Supported | MCP stdio via config |
| **OpenClaw** | Supported | MCP stdio via config |
| **Hermes Agent** | Supported | MCP stdio via config |

## Signet (proxy)

Signet wraps GraphIQ through its own MCP stdio server (`signet-mcp`), exposing GraphIQ tools with the `signet_code_*` prefix alongside its memory and knowledge tools. Any harness connected to Signet gets GraphIQ access automatically.

| Harness | Status | Notes |
|---|---|---|
| **Claude Code** | Supported | MCP stdio via Signet connector |
| **OpenCode** | Supported | MCP stdio via Signet connector |
| **Codex CLI** | Supported | MCP stdio via Signet connector |
| **Gemini CLI** | Supported | MCP stdio via Signet connector |
| **OpenClaw** | Supported | MCP stdio via Signet connector |
| **Forge** | Supported | MCP stdio via Signet connector |
| **Hermes Agent** | Supported | MCP stdio via Signet connector |
| **Oh My Pi** | Supported | MCP stdio via Signet connector |
| **Pi** | Supported | MCP stdio via Signet connector |

## Tool names

When using GraphIQ natively, tool names are unprefixed (`code_search`, `code_context`, etc.).

When using GraphIQ through Signet, tool names use the `signet_code_*` prefix (`signet_code_search`, `signet_code_context`, etc.). Signet also registers backward-compat aliases for the unprefixed names.
