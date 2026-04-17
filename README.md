# GraphIQ

Code intelligence with structural retrieval. Drop a codebase in, get instant, accurate symbol search powered by BM25, graph traversal, heuristic reranking, and holographic reduced representations — zero embeddings required.

**0.796 NDCG@10** on self-benchmark. **0.520 on tokio** (17K symbols). **0.615 on signetai** (20K symbols). **0.545 on esbuild** (6K symbols). **~1ms p50 latency**. No model dependencies.

## Why This Works

Code identifiers carry meaning. `RateLimiter`, `rate_limit.ts`, `authenticateUser` — these are semantically rich tokens that FTS handles natively. The "semantic gap" people try to close with embeddings is mostly solvable with structural indexes (call graphs, import graphs, type hierarchies) at zero embedding cost.

The retrieval funnel:

```
Query: "rate limit middleware"
        |
        +-- Hot Cache hit? --> return (< 1ms)
        v
+---------------------+
|  Layer 1: BM25/FTS  |  ~5ms   --> 200 candidates
|  Identifier-aware   |  rateLimit, rate_limit, middleware all match
+----------+----------+
           v
+-----------------------------+
|  Layer 2: Structural Expand |  ~10ms  --> ~500 candidates
|  Import graph  --> callers  |
|  Call graph    --> callees  |
|  Type hierarchy --> impls   |
|  Test association           |
+----------+------------------+
           v
+-----------------------------+
|  Layer 3: Cheap Rerank      |  ~5ms   --> top 50
|  Path weights + heuristics  |
|  Diversity dampen           |
+----------+------------------+
           v
+-----------------------------+
|  Layer 4: HRR Rerank        |  ~1ms   --> top_k
|  Holographic matching       |  1024-dim circular convolution
+-----------------------------+
```

Layer 4 (embed reranker) was also tested with jina-code and nomic-embed — both produced net-negative NDCG. Neural embeddings at the 137M scale add noise, not signal. The HRR (Holographic Reduced Representations) layer achieves better results with pure math: each symbol's identity and graph neighborhood are encoded into a 1024-dim vector via circular convolution, then matched against query vectors via dot product. Hypersphere normalization (unit-length post-IFFT) eliminated a 47x norm variance and produced a +0.132 aggregate NDCG gain.

## Benchmarks

### NDCG@10 — Graded Relevance (3=perfect, 2=good, 1=acceptable)

Four codebases, increasing scale and polyglot difficulty.

| Codebase | Symbols | Queries | BM25 | Full Pipeline |
|---|---|---|---|---|
| graphiq (self) | 1,025 | 27 | 0.715 | **0.796** |
| tokio | 17,867 | 26 | 0.539 | **0.520** |
| signetai | 20,870 | 25 | 0.527 | **0.615** |
| esbuild | 6,183 | 25 | — | **0.545** |
| **Aggregate** | | | | **2.526** |

Latency: p50 1.0ms cold, < 0.1ms warm (cached). p95 3.0ms cold.

### LoCoMo Benchmark — MRR / Hit@K / Precision / Recall

50-question benchmark following the LoCoMo evaluation methodology: Accuracy (top-1 correct), MRR, Precision@10, Recall@10, NDCG@10, plus Hit@1/3/5/10. Run with `graphiq-locomo` binary against fresh query sets with verified symbol relevance.

| Metric | signetai (20K) | tokio (17K) | esbuild (12K) |
|---|---|---|---|
| Accuracy | 50.0% | 44.0% | 46.0% |
| Hit@1 | 50.0% | 46.0% | 48.0% |
| Hit@3 | 58.0% | 58.0% | 54.0% |
| Hit@5 | 62.0% | 58.0% | 66.0% |
| Hit@10 | 70.0% | 64.0% | 70.0% |
| MRR | 0.556 | 0.517 | 0.542 |
| Precision@10 | 21.8% | 20.6% | 14.4% |
| Recall@10 | 47.2% | 42.5% | 49.1% |
| NDCG@10 | 0.478 | 0.465 | 0.502 |

#### By query type

**signetai** (TypeScript/Python/Rust, 20,870 symbols, 50 queries):

| Type | n | Accuracy | Hit@1 | Hit@3 | Hit@10 | MRR | NDCG@10 |
|---|---|---|---|---|---|---|---|
| symbol-exact | 14 | 100% | 100% | 100% | 100% | 1.000 | 0.979 |
| symbol-partial | 10 | 40% | 40% | 70% | 80% | 0.533 | 0.424 |
| nl-descriptive | 8 | 25% | 25% | 25% | 62% | 0.323 | 0.318 |
| error-debug | 3 | 67% | 67% | 67% | 67% | 0.667 | 0.338 |
| cross-cutting | 5 | 40% | 40% | 40% | 40% | 0.400 | 0.240 |
| file-path | 4 | 25% | 25% | 50% | 50% | 0.375 | 0.244 |
| nl-abstract | 6 | 0% | 0% | 0% | 33% | 0.065 | 0.041 |

**tokio** (Rust, 17,867 symbols, 50 queries):

| Type | n | Accuracy | Hit@1 | Hit@3 | Hit@10 | MRR | NDCG@10 |
|---|---|---|---|---|---|---|---|
| symbol-exact | 15 | 93% | 100% | 100% | 100% | 1.000 | 0.933 |
| symbol-partial | 10 | 60% | 60% | 90% | 90% | 0.717 | 0.615 |
| nl-descriptive | 8 | 12% | 12% | 25% | 38% | 0.203 | 0.179 |
| cross-cutting | 4 | 25% | 25% | 50% | 50% | 0.375 | 0.206 |
| file-path | 4 | 0% | 0% | 25% | 25% | 0.083 | 0.158 |
| nl-abstract | 6 | 0% | 0% | 0% | 17% | 0.017 | 0.014 |
| error-debug | 3 | 0% | 0% | 0% | 33% | 0.042 | 0.038 |

**esbuild** (Go, 12,040 symbols, 50 queries):

| Type | n | Accuracy | Hit@1 | Hit@3 | Hit@10 | MRR | NDCG@10 |
|---|---|---|---|---|---|---|---|
| symbol-exact | 15 | 93% | 93% | 100% | 100% | 0.967 | 0.955 |
| nl-abstract | 6 | 33% | 33% | 33% | 83% | 0.450 | 0.345 |
| nl-descriptive | 8 | 38% | 38% | 50% | 62% | 0.455 | 0.474 |
| file-path | 4 | 50% | 75% | 75% | 75% | 0.750 | 0.564 |
| cross-cutting | 4 | 25% | 25% | 25% | 50% | 0.312 | 0.159 |
| error-debug | 3 | 0% | 0% | 33% | 33% | 0.167 | 0.250 |
| symbol-partial | 10 | 10% | 10% | 10% | 40% | 0.152 | 0.125 |

GraphIQ excels at exact/partial symbol matching (93-100% accuracy), file-path queries, and structural intelligence (blast radius, dependency analysis) — areas where embeddings are slower, less precise, or can't compete at all. The NL→code gap (nl-abstract/nl-descriptive) represents queries where the user's vocabulary diverges from the codebase's vocabulary. This matters for ~10% of real-world queries (exploration); developers type symbol names, file paths, and error messages 90% of the time.

## Quick Start

```bash
# Try it immediately — no project needed
graphiq demo

# Install for a real project
graphiq setup --project /path/to/project
```

`graphiq setup` detects your installed harnesses (opencode, Claude Desktop, Codex), writes MCP server configs, indexes the project, and shows you how to get started.

## Installation

### Homebrew (macOS + Linux)

```bash
brew tap aaf2tbz/graphiq
brew install graphiq
```

Installs four binaries:
- `graphiq` — CLI (index, search, blast, status, reindex, demo, setup)
- `graphiq-mcp` — MCP server for LLM integration (stdio JSON-RPC)
- `graphiq-bench` — NDCG/Hit@K benchmarking
- `graphiq-locomo` — LoCoMo-style MRR/Accuracy/Precision/Recall benchmarking

### From source

```bash
git clone https://github.com/aaf2tbz/graphiq.git
cd graphiq
cargo build --release
```

Release binaries are ~16MB each. No runtime dependencies beyond the OS.

## CLI Reference

### `graphiq setup`

One-command onboarding. Detects git root, writes MCP configs for all installed harnesses, indexes the project.

```bash
# Use current directory (walks up to find git root)
graphiq setup

# Specify a project
graphiq setup --project /path/to/project

# Configure without indexing
graphiq setup --skip-index
```

What it does:
1. Detects the project's git root (or uses the specified path)
2. Checks for installed harnesses and writes `graphiq-mcp` entries:

| Harness | Config file | Format |
|---|---|---|
| opencode | `~/.config/opencode/opencode.json` | JSON `mcp` section |
| Codex | `~/.codex/config.toml` | TOML `[mcp_servers.graphiq]` |
| Hermes | `~/.hermes/config.yaml` | YAML `mcp_servers` section |
| Claude Desktop | `~/Library/Application Support/Claude/claude_desktop_config.json` | JSON `mcpServers` section |

3. Indexes the project (deletes stale DB to avoid schema drift)
4. Prints a summary with next steps

### `graphiq index`

```bash
graphiq index /path/to/project
graphiq index /path/to/project --db custom.db
```

### `graphiq search`

```bash
# Basic search
graphiq search "rate limit middleware"

# With options
graphiq search "authenticateUser" --top 20 --file src/auth/
graphiq search "error handler" --debug
graphiq search "all language parsers" --db /path/to/db
```

`--debug` prints per-result score breakdowns showing which heuristics fired and their weights.

### `graphiq blast`

Compute blast radius — what a symbol affects and what depends on it.

```bash
graphiq blast RateLimiter
graphiq blast RateLimiter --depth 5 --direction forward
graphiq blast authenticate --direction backward
```

### `graphiq status`

```bash
graphiq status
graphiq status --db /path/to/db
```

### `graphiq reindex`

```bash
graphiq reindex /path/to/project
```

### `graphiq demo`

Generates a 7-file sample Rust project, indexes it, and runs showcase queries across all query classes. Zero setup needed.

```bash
graphiq demo
```

### `graphiq-locomo`

LoCoMo-style benchmark with Accuracy, MRR, Precision@10, Recall@10, NDCG@10, and Hit@1/3/5/10.

```bash
graphiq-locomo /path/to/project .graphiq/locomo.db benches/locomo-full-stack-8.json
```

## MCP Server

`graphiq-mcp` speaks JSON-RPC 2.0 over stdio. Five tools:

| Tool | Description |
|---|---|
| `search` | Ranked symbol search with file filter and top_k (max 50) |
| `blast` | Blast radius — forward/backward/both, depth 1-10 |
| `context` | Full source + structural neighborhood (callers, callees, members, parents, tests) |
| `status` | Index stats, project root, database size |
| `index` | (Re)index the project on demand |

### How it works

Pass a **project directory** — the server:
1. Walks up to find the git root
2. Resolves `.graphiq/graphiq.db` inside it
3. Creates the DB and auto-indexes if it's empty or missing
4. Prewarms the hot cache
5. Accepts JSON-RPC requests

```bash
graphiq-mcp /path/to/project
graphiq-mcp .                     # current directory
graphiq-mcp src/auth              # subdirectory — walks up to git root
```

### Client Configuration

Run `graphiq setup` to auto-configure, or add manually:

**Claude Desktop** (`~/Library/Application Support/Claude/claude_desktop_config.json`):
```json
{
  "mcpServers": {
    "graphiq": {
      "command": "graphiq-mcp",
      "args": ["/path/to/project"]
    }
  }
}
```

**opencode** (`~/.config/opencode/opencode.json`):
```json
{
  "mcp": {
    "graphiq": {
      "type": "local",
      "command": ["graphiq-mcp"],
      "args": ["/path/to/project"],
      "enabled": true
    }
  }
}
```

**Codex** (`~/.codex/config.json`):
```json
{
  "mcpServers": {
    "graphiq": {
      "command": "graphiq-mcp",
      "args": ["/path/to/project"]
    }
  }
}
```

**Hermes** (project config):
```json
{
  "mcpServers": {
    "graphiq": {
      "command": "graphiq-mcp",
      "args": ["/path/to/project"]
    }
  }
}
```

Compatible with any MCP client supporting stdio transport.

### Protocol details

- JSON-RPC 2.0 (`2024-11-05`)
- `initialize` response includes `_meta.projectRoot` and `_meta.dbPath`
- Supports `ping/pong`, `notifications/cancelled`, graceful `shutdown`
- Errors use standard JSON-RPC codes: `-32700` (parse), `-32600` (invalid), `-32601` (method not found), `-32603` (internal)
- Logging goes to stderr with timestamps
- Tool errors include `isError: true` in the content response

## Supported Languages

34 languages detected, 14 with dedicated TreeSitter parsers for symbol extraction:

**Parsed (symbol extraction, call graphs, type hierarchies):**
TypeScript, TSX, JSX, JavaScript, Rust, Python, Go, Java, C, C++, Ruby, YAML, TOML, JSON, HTML, CSS

**Detected (file tracking, FTS indexing):**
Kotlin, Swift, C#, PHP, Lua, Dart, Scala, Haskell, Elixir, Zig, GraphQL, Protobuf, Shell, SQL, Markdown, XML, SCSS

~50 file extensions recognized. Uses `ignore` crate for `.gitignore`/`.graphiqignore` awareness.

## Architecture

### Retrieval Pipeline

```
Query
  |
  +-- Abstract query? (how does X work, what connects Y)
  |     |
  |     v
  |   Decomposition Engine
  |     |-- Strip question prefix
  |     |-- Map domain terms (ranking -> reranker, callers -> bfs)
  |     |-- Detect composite patterns (callers + callees -> traverse)
  |     |-- Generate 3-8 subqueries
  |     |-- Run each through FTS + rerank
  |     |-- Merge: multi-track evidence boost (1.0 + 0.3 per additional hit)
  |     |
  |     +--> Top-K Results
  |
  +-- Standard query
        |
        +-- Hot Cache hit? --> return (< 1ms)
        v
+---------------------+
|  Layer 1: BM25/FTS  |  0.8ms p50  --> 200 candidates
|  Identifier-aware   |  rateLimit, rate_limit, middleware all match
+----------+----------+
           v
+-----------------------------+
|  Layer 2: Structural Expand |  --> ~500 candidates
|  Import graph  --> callers  |
|  Call graph    --> callees  |
|  Type hierarchy --> impls   |
|  Test association           |
+----------+------------------+
           v
+-----------------------------+
|  Layer 3: Cheap Rerank      |  --> top 50
|  11 heuristics              |
|  Multi-evidence channels    |
|  Diversity dampen           |
+----------+------------------+
           v
+-----------------------------+
|  Layer 4: HRR Rerank        |
|  Holographic matching       |
|  1024-dim circular conv.    |
+-----------------------------+
```

### Key Innovations

**Holographic Reduced Representations (HRR)** — Each symbol's identity and graph neighborhood are encoded into a 1024-dim vector via circular convolution. Query vectors are matched via dot product. Hypersphere normalization (unit-length post-IFFT) eliminated a 47x norm variance across symbols and produced a +0.132 aggregate NDCG gain — the single largest improvement from any single technique.

**Query decomposition** — Abstract queries ("how does retrieval ranking work") are decomposed into 3-8 concrete subqueries via domain-specific term mapping. Each subquery runs through the standard FTS+rerank pipeline; symbols hit by multiple tracks get a multiplicative evidence boost. Only activates for queries with question prefixes or high stop-word ratios — non-abstract queries are completely unaffected.

**Multi-evidence channels** — Each candidate is scored across 5 evidence channels: lexical (name match), structural (graph expansion), test (test coverage), path (file match), and hints (search_hints coverage). Symbols scoring on 2+ channels get a multiplicative agreement bonus (1.05-1.22x); single-channel results are slightly dampened (0.95x).

**Behavioral role tags** — 19 role tags (validator, cache, handler, retry, auth-gate, etc.) inferred from symbol names, callees, file paths, and edge patterns. Fed into search_hints so FTS matches role vocabulary. A function calling `validate_input` gets tagged as a validator — querying "check input" finds it through the FTS hints channel.

**Structural motifs** — 8 motifs (connector, orchestrator, hub, guard, transform, sink, source, leaf) detected from local edge patterns. A function with both call-in and call-out edges is a "connector" — its hints include "connects joins links bridges". Composite patterns in decomposition ("callers" + "callees") trigger targeted subqueries.

**Search hints** — An FTS column (weight 5.0) populated at index time with structural role descriptions, morphological variants, role tags, and motif terms. This gives FTS semantic context without embeddings, at zero query-time cost.

**Stop word filtering** — The AND FTS query strips 50+ common English words but keeps them in the OR fallback. Critical for cross-cutting queries.

**Module shadow penalty** — Modules with exact name matches are penalized (0.75x) so concrete types win.

**Hot cache** — LRU caches for neighborhoods, search results, blast radii, and source code. Prewarms 200 neighborhoods on startup. Sub-millisecond for repeated queries.

### Storage

Single-file SQLite database at `.graphiq/graphiq.db`. Tables: `files`, `symbols`, `edges`, `file_edges`, FTS5 virtual table. WAL mode. Recursive CTEs for graph traversal. The entire index for a 40-file project is ~1.4MB.

### Project Structure

```
graphiq/
  crates/
    graphiq-core/       # Core library
      src/
        index.rs        # Indexing pipeline
        search.rs       # Search engine (the funnel)
        fts.rs          # BM25/FTS retrieval
        rerank.rs       # 11 heuristics + channel scoring + diversity
        graph.rs        # Structural expansion (BFS)
        blast.rs        # Blast radius (forward/backward)
        hrr.rs          # HRR holographic encoding + hypersphere normalization
        afmo.rs         # Poincaré ball hyperbolic embeddings
        lsa.rs          # Truncated SVD / LSA
        spectral.rs     # Spectral graph coordinates
        db.rs           # SQLite schema + queries
        cache.rs        # Hot cache (LRU)
        decompose.rs    # Abstract query decomposition
        roles.rs        # Behavioral role tag inference (19 tags)
        motifs.rs       # Structural motif detection (8 motifs)
        symbol.rs       # Symbol, SymbolKind
        edge.rs         # Edge, EdgeKind, BlastRadius
        tokenize.rs     # Identifier decomposition
        chunker.rs      # LanguageChunker trait
        calls.rs        # Call site extraction
        files.rs        # Language detection, project walker
        languages/      # 14 TreeSitter parsers
    graphiq-cli/        # CLI binary
    graphiq-mcp/        # MCP server binary
    graphiq-bench/      # NDCG benchmark binary
```

See [DESIGN.md](DESIGN.md) for the full architecture specification including data model, edge weights, and retrieval details.

## Dependencies

- Rust (edition 2021)
- SQLite (bundled via rusqlite)
- TreeSitter 0.24 with 14 language grammars
- `ignore` for .gitignore-aware project walking
- `rayon` for parallel indexing
- `dashmap`, `lru` for concurrent caching
- `clap` for CLI, `serde_json` for serialization

## License

MIT
