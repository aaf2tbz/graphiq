# GraphIQ Hardening Roadmap

**Date:** 2026-05-24
**Branch:** `codex/hardening`
**Source:** Codebase audit of `graphiq` v3.3.0 (crates: graphiq-core, graphiq-cli, graphiq-mcp, graphiq-bench)
**Goal:** Zero-friction, always-ready indexing layer that agents plug into without manual index management, stale DB conflicts, or setup confusion. Multiple concurrent sessions with isolated state. Search results surfaced in the most agent-consumable format possible.

---

## Overview

GraphIQ has proven search quality (+48% NDCG, +128% MRR). The next inflection is **reliability and usability**: making the tool disappear into the agent workflow so the user never thinks about indexing, stale databases, or which tool to call. This roadmap covers 6 phases, each independently shippable with tests.

---

## Phase 1: Auto-Index Lifecycle & Stale Resolution

**Problem:** Users (and agents) encounter stale or wrong indexes. Opening a project from a different path, switching branches, or having old `.graphiq/graphiq.db` data causes search to silently degrade or fail. The user has to manually run `doctor`, figure out what's wrong, and reindex.

**Files touched:** `crates/graphiq-core/src/manifest.rs`, `crates/graphiq-mcp/src/main.rs`, `crates/graphiq-core/src/index.rs`

### 1A. Proactive staleness guard on every search

Currently `check_artifact_freshness` is only called in `cmd_search` (CLI) and `tool_doctor` (MCP). The MCP server's `tool_search` and `background_warm` don't check staleness before serving results.

**Changes:**
- In `tool_search` (MCP `main.rs:1105`): before running search, compare manifest freshness hash against current DB state. If cruncher is stale, trigger a silent rebuild of the cruncher index (not a full reindex — just `build_cruncher_index`). Surface a note in the response: `"mode: GraphWalk (cruncher rebuilt)"`.
- Add `SearchResult::staleness_note: Option<String>` to communicate this transparently.
- In `background_warm` (MCP `main.rs:556`): after prewarm, check manifest. If stale, rebuild cruncher before marking ready.

**Test:** Unit test that inserts symbols, builds manifest, adds more symbols, then calls search and verifies the cruncher was silently rebuilt and the note is present.

### 1B. Content-hash-based incremental reindex detection

The indexer already skips files whose content hash hasn't changed (`index.rs:144-153`). But the manifest freshness check (`manifest.rs:82-86`) only compares symbol/edge/file *counts*, not actual content hashes. Adding/removing one symbol flips counts but so does adding a file with 0 symbols — both look the same.

**Changes:**
- Add `content_hash: String` (SHA-256 of all file content hashes concatenated) to `FreshnessHash`.
- Compute it during indexing: hash all `files.content_hash` values in sorted order.
- `is_stale_vs` compares this hash instead of counts. This detects branch switches, file edits that don't change symbol count, etc.
- Fall back gracefully: if manifest has no `content_hash` field (old manifests), use count-based comparison.

**Test:** Index a project, change one file's contents without changing symbol count, verify `is_stale_vs` returns true with content hash but false with count-only.

### 1C. Automatic reindex on project root mismatch

The MCP server already detects stored `project_root` mismatches (`main.rs:273-293`). But it only logs and uses the stored root — it doesn't reindex. If the stored root no longer matches the filesystem (user moved the project, or is working in a different checkout), search returns stale data.

**Changes:**
- In `resolve_project_root`: if the stored `project_root` doesn't match the candidate and both exist, compare their content hash against the manifest. If they differ, nuke the DB and reindex from the new root.
- Add a `--force-reindex` CLI flag that explicitly clears the DB before indexing.
- In MCP: on `initialize`, if the DB has data but the project root differs, automatically clear and reindex. Surface this in the `_meta.ready` field.

**Test:** Index project at path A, move to path B, start MCP server pointing at B, verify it auto-reindexes.

---

## Phase 2: Multi-Session Isolation & Concurrency

**Problem:** Multiple agents sharing one `.graphiq/graphiq.db` creates race conditions. The watcher's `indexing` bool (`main.rs:517`) is not a real lock — two concurrent `do_index` calls could interleave writes. The `--ephemeral` flag exists but isn't wired to any auto-session hook.

**Files touched:** `crates/graphiq-mcp/src/main.rs`, `crates/graphiq-core/src/db.rs`

### 2A. Session-scoped ephemeral databases

**Changes:**
- Add `--session-id <ID>` flag to `graphiq-mcp`. When set, creates an ephemeral temp DB at `$TMPDIR/graphiq-<session-id>/graphiq.db` instead of `.graphiq/graphiq.db`.
- The Signet plugin manifest already has `prompt:contribute:user-prompt-submit`. Add a Signet hook that detects when an agent session starts, auto-launches `graphiq-mcp --session-id <session-id> --project <path>`, and the agent's MCP config points to this ephemeral instance.
- On session end (SIGTERM/SIGINT), clean up the temp directory (already done for `--ephemeral`, extend to `--session-id`).
- Shared (non-session) mode still uses `.graphiq/graphiq.db` and is the default.

**Test:** Launch two MCP servers with different `--session-id` values pointing at the same project. Verify each has its own DB. Verify search in one doesn't affect the other. Verify cleanup on exit.

### 2B. Reader-writer lock for concurrent access

**Changes:**
- Replace the `indexing: bool` guard in `ServerState` with a proper `RwLock`-style concurrency model:
  - Searches read from `CruncherIndex` (which is `Clone`-cheap after initial build).
  - Indexing holds an exclusive write lock, during which searches return a "reindexing in progress" response.
- In practice: wrap `cruncher_index` in a `RwLock<Option<CruncherIndex>>` separate from the `Mutex<ServerState>`. Search acquires read lock, indexing acquires write lock.
- This means searches can run concurrently with each other, and only block during cruncher rebuilds.

**Test:** Spawn 10 concurrent search goroutines while triggering a reindex. Verify no panics, no corrupted results, and that searches during reindex return a retry-able response.

### 2C. Watcher debouncing and merge

**Changes:**
- The current watcher (`main.rs:418-540`) debounces by 2 seconds but doesn't batch multiple file changes. If 20 files change in a git checkout, it may trigger 5-10 reindexes.
- Add a merge window: accumulate file change events for a configurable period (default 5s), then do a single reindex pass for all changed files.
- Use the existing incremental reindex logic (content-hash-based skip) so unchanged files aren't re-parsed.

**Test:** Touch 10 files simultaneously, verify only one reindex is triggered within the merge window.

---

## Phase 3: Search Result Surfacing for Agents

**Problem:** Agents using GraphIQ often need to follow up a `search` with a `context` call to understand what a symbol does. The search result could include more structural context inline, reducing round trips and giving agents better first-pass understanding.

**Files touched:** `crates/graphiq-mcp/src/main.rs` (tool output formatting), `crates/graphiq-core/src/search.rs`

### 3A. Enriched search result format

The default MCP search response is now a compact Markdown list with rank,
score, file:line, kind::name, signature/doc summary, and structural tags.
`format: "detailed"` retains the richer source preview and calls/callers view.
The compact view should keep the common agent path bounded while still
surfacing:

- **Subsystem membership** (which module/subsystem does this symbol belong to?)
- **Structural role** (hub, bridge, leaf, entry point?)
- **Evidence edges** (what kinds of edges connect to it — calls, imports, implements?)

**Changes:**
- In `tool_search` output: add a `role` line showing structural role tags from the `roles` module.
- Add a `subsystem` line if subsystems have been computed (from `subsystems` table).
- Add `edges: N incoming, M outgoing` summary.
- Keep the format compact — one or two extra lines per result, not a wall of text.

**Example output:**
```
#1 [8.52] src/auth.rs:142  function::authenticateUser (24L) [hub]
  fn authenticateUser(token: string): Promise<User>
  edges: 12 incoming, 8 outgoing  subsystem: auth
  calls:
    -> verifyToken
    <- handleLogin
```

**Test:** Index a project with subsystems, search, verify the enriched format includes role and subsystem info.

### 3B. Smarter tool routing guidance

The MCP tool descriptions are already good, but agents sometimes use `search` when they should use `context`, or `blast` when they should use `search`.

**Changes:**
- Add a `guidance` field to the `search` tool response when results are ambiguous (multiple symbols match): `"2 symbols named 'handle'. Use context with qualified name for specific result."`
- Add tool usage hints to `briefing` output: when to use each tool, with examples.
- In `tool_blast`: if the symbol has a very large blast radius (>50 dependents), suggest narrowing with `topology` instead.

**Test:** Search for an ambiguous name, verify guidance is included. Search for an unambiguous name, verify no guidance noise.

### 3C. Structural search mode

Add a new tool or mode that lets agents ask structural questions directly, without needing to translate them into symbol-name queries.

**Changes:**
- Enhance `interrogate` tool (already exists in MCP `tools_list`) to accept structured queries like `{"pattern": "hub_symbols", "subsystem": "auth"}` or `{"pattern": "error_handlers"}`.
- Wire `interrogate` to the existing `roles` and `motifs` modules: hub symbols, bridge symbols, entry points, error handlers, validators.
- Return results in the same format as `search` for consistency.

**Test:** Interrogate for "entry points", verify the response lists symbols with entry_point role.

---

## Phase 4: Install & Setup Hardening

**Problem:** `graphiq setup` writes configs for 9 harnesses but has several reliability issues: no PATH verification, partial failure handling, and a SQL injection risk in `cmd_roles`.

**Files touched:** `crates/graphiq-cli/src/main.rs`, `install.sh`

### 4A. Binary PATH verification

**Changes:**
- In `cmd_setup`: before writing any config, verify `graphiq-mcp` is on PATH by running `which graphiq-mcp`. If not found, print a clear error with install instructions and abort.
- Add a `--check` flag that validates the current setup without modifying anything.

**Test:** Run setup with graphiq-mcp not on PATH, verify clear error. Run with it on PATH, verify success.

### 4B. Partial failure recovery

Currently, if writing the Claude Desktop config fails, the function returns early and doesn't configure the remaining harnesses.

**Changes:**
- Each harness config write should be independent — a failure in one should not prevent the others from being attempted.
- Collect all successes and failures, print a summary at the end.
- Add a `--dry-run` flag that shows what would be written without writing.

**Test:** Mock a write failure for one harness, verify others are still configured.

### 4C. Fix SQL injection in `cmd_roles`

**Current code** (`main.rs:572-576`):
```rust
let query = format!("... WHERE subsystem_id = {} ... LIMIT {}", sub_id, top);
```

`sub_id` is a `usize` (safe), but the raw `format!()` pattern is a ticking time bomb if the API ever widens.

**Changes:**
- Use parameterized queries with `params![sub_id as i64, top as i64]`.
- Audit all other raw SQL format strings in the CLI for the same pattern.

**Test:** Existing tests pass. Add a test that verifies parameterized query behavior.

---

## Phase 5: Signet Plugin Integration Depth

**Problem:** The Signet plugin manifest (`signet-plugin/manifest.json`) declares capabilities but the integration is shallow — it doesn't leverage Signet's session hooks for auto-indexing, doesn't report index health to the Signet dashboard, and doesn't participate in Signet's memory system.

**Files touched:** `signet-plugin/manifest.json`, `signet-plugin/integration.json`, new `signet-plugin/hooks/`

### 5A. Session lifecycle hooks

**Changes:**
- Add `session:start` and `session:end` hook handlers to the Signet plugin.
- On `session:start`: detect the project root from the session context, check if `.graphiq/graphiq.db` exists and is fresh, trigger a background index if needed, and register the MCP tools for that session.
- On `session:end`: clean up ephemeral databases if session-scoped.
- This means agents get GraphIQ context automatically without any manual setup step.

**Test:** Start a Signet session in a project directory, verify GraphIQ auto-indexes and tools are available.

### 5B. Index health reporting

**Changes:**
- Add a `dashboardPanels` entry to the manifest that shows index status (file count, symbol count, freshness, search mode) in the Signet dashboard.
- Report staleness events as Signet notifications: "GraphIQ index stale — 3 files changed since last index".

**Test:** Verify dashboard panel appears in Signet UI with correct data.

### 5C. Memory cross-pollination

**Changes:**
- When GraphIQ computes structural roles, motifs, or subsystems, emit them as Signet knowledge graph entities. Example: `entity:graphiq_subsystem_auth { symbols: 42, cohesion: 0.87 }`.
- This lets other Signet plugins and agents query the codebase structure without calling GraphIQ directly.
- Keep this opt-in via a `--emit-knowledge` flag or manifest config.

**Test:** Index a project with knowledge emission enabled, verify entities appear in Signet's knowledge graph.

---

## Phase 6: Search Methodology Refinement

**Problem:** Each search tool is better at different tasks, but the distinctions aren't surfaced to agents effectively. The query family classification is internal and invisible to the agent.

**Files touched:** `crates/graphiq-core/src/query_family.rs`, `crates/graphiq-mcp/src/main.rs`

### 6A. Query family transparency

**Changes:**
- Include the detected `query_family` in every search response so agents learn which queries work best.
- Add a `query_family` summary to the `briefing` tool output explaining the 8 families and what they're good for.
- Example: `"query family: NaturalDescriptive — works well for 'how does authentication work', 'error handling middleware'"`.

**Test:** Search with different query styles, verify correct family classification in response.

### 6B. Search result clustering

Currently results are a flat ranked list. For exploratory queries, this means the top 10 results might all be from the same file.

**Changes:**
- After ranking, cluster results by file or subsystem. Show at most 3 results per cluster in the top-K, then backfill from remaining clusters.
- This gives agents broader coverage of the codebase per query.
- Add `--cluster` flag (default on for MCP, off for CLI) to enable/disable.

**Test:** Search in a project where one file has 10 matching symbols, verify results are spread across files.

### 6C. Context window optimization

**Changes:**
- In `tool_context`: currently returns the full source. For large symbols (>50 lines), return a smart excerpt: signature + first 10 lines + last 5 lines, with a `"showing 15 of 150 lines"` note.
- Add `context_lines: usize` parameter to include N lines of surrounding context from the file (lines before/after the symbol).
- This reduces token waste for agents working within context windows.

**Test:** Query context for a 200-line function, verify smart excerpt. Query with `context_lines: 5`, verify surrounding code.

---

## Execution Order & Dependencies

```
Phase 1 (auto-index + staleness)     ← start here, unblocks everything
  ├── 1A: proactive staleness guard
  ├── 1B: content-hash freshness
  └── 1C: project root mismatch fix

Phase 2 (isolation & concurrency)    ← enables multi-agent
  ├── 2A: session-scoped ephemeral DB
  ├── 2B: reader-writer lock
  └── 2C: watcher debouncing

Phase 3 (search surfacing)           ← improves agent effectiveness
  ├── 3A: enriched result format
  ├── 3B: tool routing guidance
  └── 3C: structural search mode

Phase 4 (install hardening)          ← independent, ship anytime
  ├── 4A: PATH verification
  ├── 4B: partial failure recovery
  └── 4C: SQL injection fix

Phase 5 (Signet integration)         ← depends on Phase 1 + 2
  ├── 5A: session lifecycle hooks
  ├── 5B: index health reporting
  └── 5C: memory cross-pollination

Phase 6 (search methodology)         ← independent, ship anytime
  ├── 6A: query family transparency
  ├── 6B: search result clustering
  └── 6C: context window optimization
```

## Verification Strategy

Every phase ships with:
1. `cargo test` — all existing + new tests pass
2. `cargo fmt --all -- --check` — no formatting regressions
3. Live smoke test — index `graphiq` itself, search, verify results
4. MCP smoke test — start `graphiq-mcp`, call each tool via JSON-RPC, verify responses
5. No performance regression — `cargo bench` runs within 5% of baseline

## Key File Map

| File | Role | Phases |
|---|---|---|
| `crates/graphiq-core/src/manifest.rs` | Artifact freshness tracking | 1A, 1B |
| `crates/graphiq-core/src/index.rs` | Indexing pipeline | 1B, 1C, 2C |
| `crates/graphiq-core/src/db.rs` | SQLite storage layer | 1C, 2B |
| `crates/graphiq-core/src/search.rs` | Search engine orchestration | 3A, 6A |
| `crates/graphiq-core/src/cache.rs` | In-memory LRU cache | 2B |
| `crates/graphiq-core/src/cruncher.rs` | CruncherIndex | 1A, 2B |
| `crates/graphiq-core/src/query_family.rs` | Query classification | 6A |
| `crates/graphiq-mcp/src/main.rs` | MCP server (14 tools) | 1A, 2A, 2B, 2C, 3A, 3B, 3C, 6B, 6C |
| `crates/graphiq-cli/src/main.rs` | CLI commands | 1C, 4A, 4B, 4C |
| `signet-plugin/manifest.json` | Signet plugin config | 5A, 5B, 5C |
| `signet-plugin/integration.json` | Signet version compat | 5A |
