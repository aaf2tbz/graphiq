# Supported Harnesses

GraphIQ integrates with AI coding assistants through two independent surfaces: its own MCP server (native) and the Signet MCP proxy. `graphiq setup` configures **both** as appropriate for each harness.

## Native (graphiq-mcp)

GraphIQ ships a standalone MCP server (`graphiq-mcp`) that harnesses talk to directly over stdio. `graphiq setup` writes temp-backed MCP configs by default — the server auto-binds supported provider workspace environment variables at launch, rejects GraphIQ hook-marker workspaces, and warms the temp index in the background. Use `graphiq setup --persistent` only when you want a project-local `.graphiq` database.

| Harness | How it's wired | Notes |
|---|---|---|
| **Claude Code** | `.claude/.mcp.json` (project) | `mcpServers.graphiq` |
| **Claude Desktop** | `~/Library/Application Support/Claude/claude_desktop_config.json` (macOS) or `~/.config/Claude/...` | `mcpServers.graphiq` |
| **Codex CLI** | `~/.codex/config.toml` | `[mcp_servers.graphiq]` (TOML) |
| **OpenCode** | `~/.config/opencode/opencode.jsonc` | `mcp.graphiq` (JSONC — comments preserved) |
| **Cursor** | `.cursor/mcp.json` (project) | `mcpServers.graphiq` |
| **Windsurf** | `.windsurf/mcp.json` (project) | `mcpServers.graphiq` |
| **Gemini CLI** | project config | MCP stdio |
| **Hermes Agent** | `~/.hermes/config.yaml` | MCP stdio |
| **Aider** | `.aider.conf.yml` (project) | MCP config pointer |

`graphiq sync` verifies each of these is actually attached (parsing JSONC/TOML correctly) and `graphiq sync --apply` re-runs setup for any that are missing.

## Pi (extension — no MCP)

[Pi](https://pi.dev) intentionally does **not** support MCP. GraphIQ integrates with it the same way Signet does — via a pi extension that lives in `~/.pi/agent/extensions/graphiq-pi.ts` (auto-discovered). `graphiq setup --harness pi` installs it.

The extension registers `graphiq_search`, `graphiq_context`, and `graphiq_status` tools the LLM calls, a `/graphiq` command (index | status | search), and auto-indexes the active project on session start. It shells out to the `graphiq` CLI directly — the same tested binary used everywhere else.

## Signet (proxy)

Signet wraps GraphIQ through its own MCP stdio server (`signet-mcp`), exposing GraphIQ tools with the `signet_code_*` prefix alongside Signet's memory and knowledge tools. Any harness connected to Signet gets GraphIQ access automatically.

The Signet lifecycle hook warms `$TMPDIR/graphiq-session-<session>/graphiq.db` for the resolved provider workspace and removes that temp directory (and stops any in-flight indexer) on session end.

| Harness | Notes |
|---|---|
| Claude Code, OpenCode, Codex CLI, Gemini CLI, OpenClaw, Forge, Hermes Agent, Pi, Oh My Pi | MCP stdio via the Signet connector |

## Tool names

| Surface | Tool names |
|---|---|
| **Native** (`graphiq-mcp`) | unprefixed — `search`, `context`, `blast`, `briefing`, `status`, `clear`, … |
| **Signet** | `signet_code_` prefixed — `signet_code_search`, `signet_code_context`, … (with unprefixed aliases) |
| **Pi extension** | `graphiq_` prefixed — `graphiq_search`, `graphiq_context`, `graphiq_status` |

## Discovering what's installed

Not sure which harnesses are on your machine? `graphiq discover` scans for them:

```bash
graphiq discover          # table: which harnesses are installed + how detected
graphiq discover --json   # machine-readable
```

It checks three signals per harness (config directory, binary on PATH, macOS `.app` bundle) so it's robust to any install method.
