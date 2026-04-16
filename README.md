# GraphIQ

Code intelligence with structural retrieval. Drop a codebase in, get instant, accurate symbol search powered by BM25, graph traversal, and heuristic reranking — not embeddings.

## Why This Works

Code identifiers carry meaning. `RateLimiter`, `rate_limit.ts`, `authenticateUser` — these are semantically rich tokens that FTS handles natively. The "semantic gap" people try to close with embeddings is mostly solvable with structural indexes (call graphs, import graphs, type hierarchies) at zero embedding cost.

The funnel:

```
Query: "rate limit middleware"
        │
        ├─ Hot Context Cache hit? → return (< 1ms)
        ▼
┌─────────────────────┐
│  Layer 1: BM25/FTS  │  ~5ms   → 200 candidates
└────────┬────────────┘
         ▼
┌─────────────────────────────┐
│  Layer 2: Structural Expand │  ~10ms  → ~500 candidates
│  Call graph, imports, types │
└────────┬────────────┘
         ▼
┌──────────────────────────┐
│  Layer 3: Cheap Rerank   │  ~5ms   → top 50
│  Heuristics + diversity   │
└────────┬─────────────────┘
         ▼
┌──────────────────────────┐
│  Layer 4: Embed Rerank   │  ~30ms  → top_k (optional)
│  Only top 50 candidates   │
└──────────────────────────┘
```

Embeddings only touch 50 candidates max — never the full corpus. And only when heuristic confidence is low.

## Benchmark Results

Self-benchmarked against the graphiq codebase (40 files, 752 symbols):

| Query Class | MRR | Hit@1 | Hit@3 | Hit@5 | Hit@10 |
|---|---|---|---|---|---|
| `symbol-exact` | 1.000 | 100% | 100% | 100% | 100% |
| `error-debug` | 1.000 | 100% | 100% | 100% | 100% |
| `nl-descriptive` | 0.867 | 80% | 100% | 100% | 100% |
| `symbol-partial` | 0.806 | 67% | 100% | 100% | 100% |
| `file-path` | 0.833 | 67% | 100% | 100% | 100% |
| `cross-cutting` | 1.000 | 100% | 100% | 100% | 100% |
| `nl-abstract` | 0.042 | 0% | 0% | 0% | 33% |
| **Overall** | **0.807** | **67%** | **85%** | **89%** | **93%** |

Latency: p50 1.9ms cold, < 0.1ms warm (cached).

## Installation

```bash
cargo build --release
```

Four binaries:
- `graphiq` — CLI (index, search, blast, status, reindex)
- `graphiq-bench` — MRR/Hit@K benchmarking
- `graphiq-mcp` — MCP server for LLM integration (stdio JSON-RPC)

## Usage

```bash
# Index a project
graphiq index /path/to/project

# Search
graphiq search "rate limit middleware"
graphiq search "authenticateUser" --debug
graphiq search "error handler" --file src/middleware/

# Blast radius
graphiq blast RateLimiter --depth 3

# Status
graphiq status

# Benchmark
graphiq-bench /path/to/project [db-path] [queries.json]
```

## MCP Server

The `graphiq-mcp` binary speaks JSON-RPC over stdio. Four tools:

- **`search`** — ranked symbol search with optional file filter
- **`blast`** — blast radius analysis (forward/backward/both)
- **`context`** — full symbol source + structural neighborhood (callers, callees, members, tests)
- **`status`** — indexing stats

```bash
graphiq-mcp /path/to/.graphiq/graphiq.db
```

Compatible with any MCP client (Claude Desktop, opencode, etc).

## Supported Languages

TypeScript, TSX, JSX, JavaScript, Rust, Python, Go, Java, C, C++, Ruby, YAML, TOML, JSON, HTML, CSS, SCSS (38 file extensions, 14 dedicated parsers).

## Architecture

- **FTS5 with weighted columns** — name (10.0), name_decomposed (8.0), qualified_name (6.0), search_hints (5.0), signature (4.0), doc_comment (3.0), file_path (2.0), source (1.0)
- **Structural graph** — Calls, Contains, Extends, Implements, Overrides, Imports, References, Tests edges with path-weight scoring
- **Search hints** — indexing-time structural role hints derived from graph relationships, decomposed identifiers, and source terms. Gives FTS semantic context without embeddings.
- **Heuristic reranker** — 7 toggleable heuristics (density, entry-point, export, test-proximity, importance, recency, name-exact) with debug score breakdowns
- **Hot cache** — neighborhood prewarming, LRU result cache, blast radius cache, source cache
- **SQLite everything** — single-file database, FTS5, recursive CTEs for graph traversal

See [DESIGN.md](DESIGN.md) for the full architecture specification.

## Dependencies

- Rust (edition 2021)
- SQLite (bundled via rusqlite)
- Tree-sitter 0.24 with 14 language grammars
- rayon, dashmap, lru, clap, serde

## License

MIT
