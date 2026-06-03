# Signet Plugin

GraphIQ ships as a verified managed plugin for [Signet](https://github.com/Signet-AI/signetai). The plugin manifest is at `signet-plugin/manifest.json`.

## Installation

GraphIQ is installed and managed through the Signet dashboard or CLI:

```bash
# Via dashboard: Skills > Plugins > GraphIQ > Install
# Via CLI:
signet index /path/to/project
```

The Signet daemon handles `brew tap` and `brew install` automatically.

## What the plugin provides

### MCP Tools

All tools use the `signet_code_*` prefix:

| Tool | Description |
|---|---|
| `signet_code_search` | Search indexed symbols and implementation context |
| `signet_code_context` | Read source and structural neighborhood for a symbol |
| `signet_code_blast` | Analyze forward/backward impact radius for a symbol |
| `signet_code_status` | Show index status for the active project |
| `signet_code_doctor` | Diagnose index artifact health |
| `signet_code_constants` | Find shared numeric and string constants |

### CLI Commands

| Command | Description |
|---|---|
| `signet index <path>` | Index a project |
| `signet graphiq status` | Show active project status |
| `signet graphiq doctor` | Diagnose index health |
| `signet graphiq upgrade-index` | Rebuild stale artifacts |

### Dashboard

The plugin adds a GraphIQ management panel in the Signet dashboard under Skills > Plugins. From there you can:

- View installation status, active project, and indexed projects
- Install or uninstall GraphIQ
- Update to the latest version
- Index new projects
- View per-project file/symbol/edge counts

### Prompt Contributions

The plugin contributes bounded guidance to agent prompt context, advising agents to use GraphIQ tools when working in an indexed codebase.

## Manifest

The manifest (`signet-plugin/manifest.json`) declares:

- **Capabilities** — What the plugin can do (index, search, context, etc.)
- **Surfaces** — MCP tools, CLI commands, connector capabilities, prompt contributions
- **Connector capabilities** — Which harnesses can use the plugin (Claude Code, OpenCode, Codex, etc.)
- **Docs** — Human-readable capability descriptions
- **Marketplace metadata** — Categories, license, repository links

## State

GraphIQ state is stored in the Signet workspace:

```
$SIGNET_WORKSPACE/.daemon/graphiq/state.json
```

The session lifecycle hook updates this file when a Signet session starts:

```json
{
  "pluginId": "graphiq",
  "enabled": true,
  "activeProject": "/path/to/current/project",
  "activeSessionId": "session-id",
  "indexedProjects": ["/path/to/current/project"],
  "updatedAt": "2026-06-03T00:00:00Z"
}
```

GraphIQ desktop reads the same state file and prioritizes the active project in
the project selector, so switching Signet sessions also switches the workspace
shown by the app.

Indexed project databases live in each project:

```
<project>/.graphiq/graphiq.db
```

When the session hook creates a missing index, it runs GraphIQ in background
mode:

```bash
GRAPHIQ_INDEX_MODE=background graphiq index "$SIGNET_PROJECT_ROOT"
```

Background mode keeps the GPU acceleration path available when GraphIQ is built
with GPU support, while reducing CPU-side source token retention for quieter
background indexing.

## Updating the plugin

After a new version of GraphIQ is released to the Homebrew tap:

1. Open the Signet dashboard > Skills > Plugins > GraphIQ.
2. Click **Update** to pull the latest version via Homebrew.
3. Or restart the Signet daemon (`signet dev`) — it picks up the updated binary automatically.

If the tool list or capabilities changed, click **Refresh** in the plugins panel to re-sync the manifest.
