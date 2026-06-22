# Reliability & Resource Control

GraphIQ is designed to run unattended in the background without pegging your machine or corrupting state.

## CPU / load control

The long-running MCP daemon (`graphiq-mcp`) honors thread caps so background sessions stay light:

- `GRAPHIQ_MAX_THREADS=<n>` — max CPU threads for indexing/search (highest priority)
- `RAYON_NUM_THREADS=<n>` — same effect, the standard rayon variable
- In **session/background mode** (`--ephemeral` or `--session-id`) the default caps to 4 threads, so a harness-spawned daemon won't peg every core. Foreground indexing uses all cores.

```bash
GRAPHIQ_MAX_THREADS=2 graphiq-mcp /path/to/project
```

## Background indexing profile

Foreground CLI indexing keeps the full source-token window for maximum recall. Background indexing can use a lower-memory profile:

```bash
GRAPHIQ_INDEX_MODE=background graphiq index /path/to/project   # lower-memory mode
GRAPHIQ_SOURCE_TERM_LIMIT=1200 graphiq index ...               # tune the source window
```

## Dormancy

Background indexers stop when the session/harness closes — graphiq does no work while no harness/project is active.

- The Signet session hook kills any still-running indexer (PID-locked) on `session:end`.
- The pi extension tracks its background-indexer child and `SIGTERM`s it on `session_shutdown`.

## Incremental & crash-safe

- Indexing is **content-hash incremental** — only changed files reparse. Re-indexing after a commit syncs the index to the new tree (verified: a v2 commit reindex shows new symbols and drops stale ones).
- A **0-symbol index degrades gracefully** to "No results" instead of crashing (regression-tested).

## SIGPIPE behavior

Graphiq behaves like classic Unix tools: piping output to `head`/`grep -q` (a reader that closes early) terminates it via SIGPIPE (exit 141), never a panic. This is the canonical fix used by ripgrep/fd/bat.

## What gets excluded

Dependency lockfiles and generated data files are **file-tracked** for freshness but **never symbol-extracted**, so they can't silently dominate the graph with thousands of low-value keys:

- `package-lock.json`, `Cargo.lock`, `pnpm-lock.yaml`, `yarn.lock`, `go.sum`, `composer.lock`, `Gemfile.lock`, `poetry.lock`, `uv.lock`, `flake.lock`, …
- Oversized JSON/YAML/TOML blobs (>256 KB)

Wipe an index any time with `graphiq clear` (or the MCP `clear` tool).
