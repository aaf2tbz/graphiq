# Quickstart

Get from zero to search in under a minute.

## 1. Index a project

```bash
graphiq index /path/to/project
```

This walks the project (respecting `.gitignore`), parses source files into symbols and structural edges, and writes a single SQLite index at `.graphiq/graphiq.db`. It's incremental — only changed files reparse on subsequent runs.

## 2. Search it

```bash
graphiq search "rate limit middleware"
graphiq search "rateLimitMiddleware"      # exact symbol name
graphiq context rateLimitMiddleware        # read source + structural neighborhood
graphiq blast rateLimitMiddleware          # what depends on it?
```

GraphIQ ranks results by structural evidence (calls, type flow, shared constants), not just text match — see [How it works](how-graphiq-works.md).

## 3. Wire it into your agent harness

```bash
graphiq setup --project /path/to/project
graphiq sync                               # verify the harnesses are attached
```

`setup` configures MCP servers with **temp-backed indexes** by default (so large indexes don't land in your checkout). Use `setup --persistent` when you want a project-local `.graphiq` database.

Supported harnesses: Claude Code, Claude Desktop, OpenCode, Codex CLI, Cursor, Windsurf, Gemini CLI, Hermes Agent, Aider, and Pi. See [Supported harnesses](supported-harnesses.md).

## 4. Keep it in sync

As you change code, re-index (incremental, fast). For the current project's git scope at a glance:

```bash
graphiq git --project .                    # branch, working-tree state, recent commits
graphiq projects                           # list every indexed project + health
```
