# GraphIQ v4 Roadmap — Structural Intelligence Expansion

> Status: `approved`
> Last updated: 2026-04-24
> Source research: competitive gap analysis against codebase-memory-mcp, axon, dora, sourcegraph

## Problem Statement

GraphIQ's search pipeline is best-in-class for zero-embedding code retrieval (+48% NDCG@10, +128% MRR@10 vs grep). But agents need more than search. They need to understand architecture, find dead weight, trace execution paths, and keep their index fresh without manual intervention. The competitive landscape exposes four capability gaps where GraphIQ has the data but doesn't surface it.

## Guiding Principles

1. **Don't touch the search pipeline.** The 8-family router, seed generation, graph walk, and scoring are the product. v4 features are additive surface area, not search changes.
2. **Graph-first.** Every feature starts from the existing graph. No new parsing passes, no new external dependencies, no embeddings.
3. **MCP tool surface.** Every feature is exposed as an MCP tool. GraphIQ is an agent tool, not a developer dashboard.
4. **Incremental indexing.** New edges and metadata must integrate into the existing incremental reindex path (content-hash-gated).

## Features (Priority Order)

---

### F1: Dead Code Detection

**Priority:** P0
**Effort:** Days
**Source:** codebase-memory-mcp (14 MCP tools, zero-caller detection)
**New MCP tool:** `dead_code`

#### What

Identify symbols with zero incoming calls (no `Calls`, `References`, `Overrides` edges pointing at them), minus exemptions. Return grouped by file with dead symbol count and total dead LOC estimate.

#### Exemption rules (from axon's multi-pass approach)

A symbol is NOT dead if any of:

1. It has `RoleTag::EntryPoint` or `RoleTag::Handler` or `RoleTag::Router`
2. It has an `Extends` or `Implements` edge pointing at it (subclass target)
3. It has `Tests` edges pointing at it (test subject)
4. It is a constructor (`new`, `init`, or same name as parent struct/class)
5. It is exported/public and in a non-test file (API surface)
6. It overrides a method that is NOT dead (inheritance chain alive)
7. It is a trait/interface definition (contract, not implementation)
8. It has an incoming `Contains` edge (it's a child — parent may be called)

#### Data already present

- `cruncher.rs`: `incoming` adjacency list — zero-length incoming = no callers
- `roles.rs`: `EntryPoint`, `Handler`, `Router` role tags
- `edge.rs`: `Tests`, `Extends`, `Implements`, `Contains` edge kinds
- `symbol.rs`: `Visibility::Public`, `SymbolKind::Constructor`

#### Algorithm

```
dead = all_symbols
  .filter(|s| incoming_edges(s).is_empty())
  .filter(|s| !is_exempt(s))
  .filter(|s| !is_test_only(s))
```

Group by file, estimate LOC from `end_line - start_line`, return sorted by dead LOC descending.

#### Output format

```json
{
  "total_dead_symbols": 23,
  "estimated_dead_loc": 847,
  "files": [
    {
      "path": "src/legacy/payments.ts",
      "dead_symbols": ["oldCharge", "legacyRefund", "formatReceipt"],
      "dead_loc": 312
    }
  ]
}
```

#### Acceptance criteria

- [ ] Returns correct dead symbols on signetai codebase (manual verification against known dead code)
- [ ] Does not flag entry points, exported functions, or trait definitions
- [ ] MCP tool returns results in <500ms on a 20K symbol codebase
- [ ] CLI: `graphiq dead-code` prints grouped results

---

### F2: File Watcher / Auto-Reindex

**Priority:** P0
**Effort:** Days
**Source:** codebase-memory-mcp (auto_index config), axon (--watch), dora (watch mode)
**New CLI flag:** `--watch` on `graphiq-mcp`
**New MCP tool:** none (infrastructure)

#### What

When `graphiq-mcp` runs with `--watch`, use the `notify` crate to watch the project directory for file changes. On change, incrementally reindex only modified files (using existing content-hash gating in `files` table). Rebuild CruncherIndex in background. Agent sees fresh results without manual `graphiq index`.

#### Architecture

```
main thread: MCP JSON-RPC loop
  |
  +-- background thread: notify::Watcher
        |
        on file change event:
          1. debounce 2s (coalesce rapid saves)
          2. diff changed files against content hashes in `files` table
          3. re-parse changed files (existing incremental index path)
          4. rebuild CruncherIndex (background, non-blocking)
          5. swap ServerState.cruncher_index (Arc<Mutex<>>)
```

#### Data already present

- `files` table: content hashes for incremental detection
- `index.rs`: file-level incremental reindex
- `cruncher.rs`: CruncherIndex rebuild from DB
- `manifest.rs`: artifact freshness tracking

#### Dependencies

- `notify` crate (Rust file system watcher, cross-platform, MIT)

#### Acceptance criteria

- [ ] `graphiq-mcp /path --watch` starts watcher alongside MCP server
- [ ] File save triggers reindex within 5 seconds
- [ ] In-flight search queries are not interrupted during reindex
- [ ] Watcher ignores `.git`, `node_modules`, `target/`, `build/`
- [ ] Watcher respects `.gitignore` and `.graphiqignore`

---

### F3: Multi-Agent Auto-Setup

**Priority:** P1
**Effort:** Day
**Source:** codebase-memory-mcp (11-agent auto-detect)
**Modified CLI command:** `graphiq setup`

#### What

Extend `graphiq setup` to detect all installed agent harnesses and write MCP config entries for each. Current setup only handles one at a time. New behavior: scan for all known harnesses, configure each.

#### Target harnesses

| Harness | Config path | Format |
|---|---|---|
| Claude Code | `.claude/.mcp.json` | JSON |
| Claude Desktop | `~/Library/Application Support/Claude/claude_desktop_config.json` | JSON |
| Codex CLI | `~/.codex/config.toml` | TOML |
| OpenCode | `~/.config/opencode/opencode.json` | JSON |
| Cursor | `.cursor/mcp.json` | JSON |
| Windsurf | `.windsurf/mcp.json` | JSON |
| Zed | `settings.json` | JSONC |
| Gemini CLI | `~/.gemini/settings.json` | JSON |
| Aider | `.aider.conf.yml` | YAML (instructions only) |

#### Algorithm

1. Detect: check if config file exists and binary is on PATH
2. Read existing config (if any)
3. Add/update `graphiq` MCP entry pointing to installed binary
4. Write back (preserve existing entries)
5. Print summary: "Configured graphiq for: Claude Code, OpenCode"

#### Acceptance criteria

- [ ] `graphiq setup` detects all installed harnesses and configures each
- [ ] Does not overwrite existing MCP entries for other servers
- [ ] Prints which harnesses were configured and which were skipped
- [ ] `graphiq setup --harness opencode` limits to one specific harness

---

### F4: Route Node Extraction

**Priority:** P1
**Effort:** Days
**Source:** codebase-memory-mcp (Route nodes with HANDLES edges)
**New edge kind:** `Handles` (route → handler function)
**New symbol kind:** `Route`

#### What

Extract HTTP route declarations as first-class graph nodes with method, path, and handler function. Connect routes to their handler functions with `Handles` edges. Enables "what handles POST /api/users?" queries and "what routes does this function serve?" context.

#### Target patterns (per language)

**TypeScript/JavaScript:**
- Express: `app.get("/path", handler)`, `router.post("/path", ...)`
- Hono: `app.get("/path", ...)`, `app.post("/path", ...)`
- Fastify: `fastify.get("/path", ...)`
- Koa: `router.get("/path", ...)`

**Rust:**
- Axum: `.route("/path", get(handler))`, `.route("/path", post(handler))`
- Actix: `web::resource("/path").route(web::get().to(handler))`

**Go:**
- net/http: `http.HandleFunc("/path", handler)`, `mux.HandleFunc(...)`
- Gin: `r.GET("/path", handler)`, `r.POST("/path", handler)`
- Chi: `r.Get("/path", handler)`, `r.Post("/path", handler)`

**Python:**
- Flask: `@app.route("/path", methods=["GET"])`
- FastAPI: `@app.get("/path")`, `@app.post("/path")`

#### Storage

New symbol kind `Route` with:
- `name`: HTTP method + path (e.g., `GET /api/users/:id`)
- `signature`: full route declaration source
- `file_path`: where the route is declared

New edge kind `Handles` from Route symbol → handler function symbol.

#### Impact on existing systems

- `hints` column: route paths indexed for BM25 discovery
- `briefing`: include route count in architecture overview
- `blast`: routes connected to handlers propagate blast correctly
- `search`: "GET /api/users" finds the route, graph walk finds handler

#### Acceptance criteria

- [ ] Detects Express, Hono, Axum, and Flask route patterns
- [ ] Route symbols appear in search results with correct handler connections
- [ ] `briefing` output includes route count
- [ ] Blast analysis traces through route → handler edges

---

### F5: Community Detection

**Priority:** P2
**Effort:** Week
**Source:** axon (Leiden algorithm), codebase-memory-mcp (Louvain)
**New module:** `graphiq-core/src/community.rs`
**New MCP tool:** `communities`

#### What

Run label-propagation clustering on the existing adjacency graph at CruncherIndex build time. Tag each symbol with a community ID. Communities represent functional clusters (auth, payments, pipeline, etc.) that emerge from code structure, not directory layout.

#### Why label-propagation, not Leiden

- Zero external dependencies (Leiden needs igraph + leidenalg Python bindings)
- O(n) time on the existing adjacency lists
- Good enough for the purpose (agents need rough clusters, not optimal modularity)
- Runs during CruncherIndex build, not as a separate phase

#### Algorithm

```
1. Build undirected adjacency from CruncherIndex outgoing/incoming
2. Weight edges: Calls=1.0, References=0.8, SharesType=0.6, SharesErrorType=0.7, SharesDataShape=0.5
3. Run label propagation (max 20 iterations):
   - Each node adopts the label most common among its weighted neighbors
   - Ties broken by current label (inertia)
4. Post-process:
   - Merge communities with <3 symbols into nearest neighbor's community
   - Auto-label: take top-3 non-stopword terms from community's symbol names
```

#### Storage

- CruncherIndex: `community_ids: Vec<u32>` parallel to symbol list
- Community metadata: `communities: HashMap<u32, CommunityMeta>` (label, symbol count, top terms)
- DB: `symbols` table gets optional `community_id` column

#### MCP tool output

```json
{
  "communities": [
    {
      "id": 0,
      "label": "auth token user session",
      "symbol_count": 47,
      "files": ["src/auth/token.ts", "src/auth/session.ts", "..."],
      "top_symbols": ["authenticateUser", "validateToken", "refreshSession"]
    }
  ]
}
```

#### Integration points

- `briefing`: include community breakdown in overview
- `search`: optional `community` filter parameter
- `interrogate`: include community membership in symbol context

#### Acceptance criteria

- [ ] Label propagation completes in <1s on 20K symbol codebase
- [ ] Communities map to intuitive code groupings (spot-check on signetai)
- [ ] `communities` MCP tool returns valid JSON
- [ ] `briefing` includes community summary

---

### F6: Execution Flow Tracing

**Priority:** P2
**Effort:** Week
**Source:** axon (framework-aware entry point detection + BFS flow tracing)
**New module:** `graphiq-core/src/flows.rs`
**New MCP tool:** `flows`

#### What

Detect entry points (HTTP handlers, CLI commands, test functions, main) and trace execution flows through the call graph. An execution flow is a named path from entry point to terminal nodes, capturing the runtime journey through the code.

#### Entry point detection (framework-aware)

Leverage existing `RoleTag::EntryPoint` and `RoleTag::Handler` plus new patterns:

| Framework | Pattern | Entry type |
|---|---|---|
| Express/Hono | Route handler function | `http_handler` |
| CLI (clap, commander) | Command handler | `cli_command` |
| Test | `test_*`, `it(`, `describe(` | `test_entry` |
| Rust | `fn main()` | `main` |
| Background | Cron/scheduled patterns | `scheduled` |

#### Algorithm

```
1. Identify entry points from RoleTag::EntryPoint + Route handlers + test functions + main
2. For each entry point:
   a. BFS through Calls edges (max depth 10)
   b. Track path: entry → intermediate → terminal
   c. Classify: intra-community (stays in cluster) vs cross-community
3. Store flow metadata: entry symbol, depth, terminal count, community crossings
```

#### MCP tool output

```json
{
  "flows": [
    {
      "entry": "handleCreateUser",
      "entry_type": "http_handler",
      "route": "POST /api/users",
      "depth": 6,
      "terminals": ["insertUserDB", "sendWelcomeEmail", "createAuditLog"],
      "crosses_communities": true,
      "path_preview": "handleCreateUser → validateInput → hashPassword → createUser → insertUserDB"
    }
  ],
  "total_flows": 34
}
```

#### Integration points

- `briefing`: include flow count and entry point types
- `interrogate`: for entry points, show the full flow
- `blast`: flows provide natural scope for impact ("this change affects 3 execution flows")

#### Acceptance criteria

- [ ] Detects HTTP handlers, test entries, and main() on signetai codebase
- [ ] Flow traces match expected call chains (manual spot-check)
- [ ] `flows` MCP tool returns results in <200ms
- [ ] `briefing` includes flow summary

---

### F7: Git Change Coupling

**Priority:** P3
**Effort:** Week
**Source:** axon (6-month co-change analysis with coupling strength)
**New edge kind:** `CoupledWith`
**New module:** `graphiq-core/src/coupling.rs`
**New CLI flag:** `graphiq index --with-coupling`

#### What

Analyze git history to find files that frequently change together. Adds `CoupledWith` edges between files (not symbols) with coupling strength. Catches behavioral dependencies that static analysis misses: "whenever someone touches user.ts, they also touch auth_middleware.ts."

#### Algorithm

```
1. git log --numstat --since="6 months ago" (or configurable)
2. Group commits by hash → set of changed files per commit
3. For each file pair (A, B):
   coupling(A, B) = co_changes(A, B) / max(changes(A), changes(B))
4. Keep pairs with coupling >= 0.3 AND co_changes >= 3
5. Add CoupledWith edge between symbols in coupled files
```

#### Storage

- `CoupledWith` edge kind with `strength` and `co_changes` metadata
- Stored at file level, propagated to top-level symbols in each file
- Optional: only computed when `--with-coupling` flag is used

#### Integration points

- `blast`: coupling edges expand impact radius beyond call graph
- `interrogate`: "files that change with this one"
- `search`: coupling strength as secondary ranking signal for relationship queries

#### Why P3

Requires git history access (not available for all projects). Adds indexing time. Marginal value on top of the existing `SharesConstant` and `SharesDataShape` edges which partially capture behavioral coupling through shared literals and field access.

#### Acceptance criteria

- [ ] `graphiq index --with-coupling` computes coupling edges
- [ ] Coupling strength is accurate against manual commit history verification
- [ ] Blast analysis includes coupled files in impact radius
- [ ] Index time increases by <30% with coupling enabled

---

## Dependency Graph

```
F1 (dead code)     — independent, ships immediately
F2 (watcher)       — independent, ships immediately
F3 (multi-setup)   — independent, ships immediately
F4 (routes)        — independent, can ship after F1/F2/F3
F5 (communities)   — independent, but F4 routes enrich community labels
F6 (flows)         — depends on F4 (routes as entry points) and F5 (community crossings)
F7 (coupling)      — independent, lowest priority
```

Recommended shipping order: **F1 + F2 + F3** → **F4** → **F5** → **F6** → **F7**

## What We're NOT Doing

| Idea | Source | Why not |
|---|---|---|
| Embeddings / semantic search | axon (BAAI/bge-small) | GraphIQ's identity is "no embeddings." Marginal NL gains. |
| Cypher query engine | codebase-memory-mcp, axon | 8-family router is better UX than making agents write Cypher. |
| 3D graph visualization | codebase-memory-mcp | Cool demo, doesn't help agents. MCP tools are the product. |
| Neo4j / external DB backend | axon (optional) | SQLite is the competitive advantage. |
| K8s / Dockerfile indexing | codebase-memory-mcp | Niche. GraphIQ already tracks YAML/TOML/JSON. |
| LSP integration | dora (SCIP layer) | Tree-sitter is sufficient. LSP requires language servers. |

## Index Schema Changes Summary

| Feature | New tables | New columns | New edge kinds | New symbol kinds |
|---|---|---|---|---|
| F1 | none | none | none | none |
| F2 | none | none | none | none |
| F3 | none | none | none | none |
| F4 | none | none | `Handles` | `Route` |
| F5 | none | `symbols.community_id` | none | none |
| F6 | none | none | none | none |
| F7 | none | none | `CoupledWith` | none |

All changes are additive. No breaking schema migrations.

## Success Metrics

| Metric | Current | Target |
|---|---|---|
| MCP tools | 13 | 17 (+dead_code, communities, flows) |
| Language coverage (full parsing) | 16 | 16 (unchanged) |
| Index freshness | Manual only | Auto (watch mode) |
| Agent harness support | 4 | 9+ |
| Architectural understanding | Flat (briefing) | Clustered (communities + flows) |
