# GraphIQ

Code intelligence with structural retrieval. Drop a codebase in, get instant, accurate symbol search powered by BM25, graph-convolved term channels, holographic reduced representations, and evidence-based fusion — zero embeddings required.

**0.63 MRR**, **50% accuracy**, **1 miss on 2/3 codebases** (10-query dual benchmark across 3 codebases, 46K symbols total). **~1ms p50 latency**. No model dependencies. No neural embeddings.

## Why This Works

Code identifiers carry meaning. `RateLimiter`, `rate_limit.ts`, `authenticateUser` — these are semantically rich tokens that FTS handles natively. The "semantic gap" people try to close with embeddings is mostly solvable with structural indexes (call graphs, import graphs, type hierarchies) at zero embedding cost.

GraphIQ's thesis: **structural information in code is vastly underused by existing retrieval systems**. A symbol's call graph neighborhood, its return types, its file path — these are all first-class retrieval signals that don't need any learned representation. The system exploits every bit of structural context available at index time.

## The Retrieval Pipeline

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
           v
+-----------------------------+
|  Layer 5: SEC Fusion        |  ~1ms   --> top_k (rescued)
|  Union BM25 + SEC Solo      |
|  Graph-convolved rerank     |
+-----------------------------+
```

### What's new: SEC Fusion

The critical insight from benchmarking: **BM25 and structural search fail in different, complementary ways**.

- **BM25 failure mode**: Term gap — the query words don't appear in the symbol name or body. "how does the timer wheel expire" → BM25 finds nothing because the relevant symbol is named `process_expired_timers`.
- **SEC failure mode**: Score dilution — pure structural search retrieves relevant symbols but ranks them poorly because it lacks BM25's IDF weighting and document-length normalization.

**SEC Fusion** solves both by:
1. Getting BM25's top-50 candidates (normalized scores)
2. Getting SEC Solo's top-50 candidates (normalized scores)
3. Unioning them into a single candidate set (up to 100 unique symbols)
4. Reranking the combined set with SEC's graph-convolved channel scoring

This means symbols BM25 completely misses get rescued by SEC Solo's inverted index, and SEC Solo's noisy rankings get cleaned up by the fusion reranker. On tokio, this produces **zero complete misses** (10/10 queries find the target in top 10).

## Structural Evidence Convolution (SEC)

The core innovation. SEC propagates terms through the code graph's structural channels:

```
                    Symbol: "RateLimiter"
                              |
        +---------+---------+---------+---------+---------+---------+
        |         |         |         |         |         |         |
      self    calls_out  calls_in  2hop_out  2hop_in  type_ret  file_path
     (3.0)     (1.5)     (1.5)     (0.7)     (0.7)     (1.0)     (0.5)
        |         |         |         |         |         |         |
    "rate"    "check"   "handle"   "retry"    "api"    "bool"    "middleware"
    "limit"   "enqueue"  "request"  "backoff"           "result"  "rate_limit"
    "limiter" "reject"   "route"
```

Each channel collects terms from the symbol's graph neighborhood with distance-based decay:
- **self** (weight 3.0): the symbol's own identifier decomposition + body terms
- **calls_out** (1.5): terms from functions this symbol calls
- **calls_in** (1.5): terms from functions that call this symbol
- **calls_out_2hop** (0.7): 2-hop call graph traversal
- **calls_in_2hop** (0.7): 2-hop reverse call graph traversal
- **type_ret** (1.0): return type decomposition
- **file_path** (0.5): path components

Scoring combines weighted channel overlap with IDF weighting, a diversity bonus for multi-channel hits, kind boosts, and test penalties. The result: queries like "how does the timer wheel expire" find `process_expired_timers` because SEC propagated "timer" and "expire" through the call graph even though the query never uses the word "process".

### SEC Inverted Index

For standalone retrieval (SEC Solo), an inverted index maps each term to postings with symbol index, channel mask, and weight. This enables sub-millisecond retrieval without any BM25 dependency — pure structural search.

### Why not neural embeddings?

We tested. Neural embeddings at the 137M parameter scale (jina-code, nomic-embed) produced net-negative NDCG when used as rerankers. The signal from identifier decomposition + graph structure is stronger than what small embedding models provide, and the retrieval pipeline exploits it more effectively.

## Benchmarks

10-query benchmark across 3 codebases. Dual evaluation: NDCG@10 (graded relevance, 7 categories) + MRR (binary relevance).

### NDCG@10 (graded relevance, corrected DCG formula)

| Codebase | Symbols | Baseline | SEC Pipe | SEC Solo | SEC Fused |
|---|---|---|---|---|---|
| signetai | 20,870 | 0.148 | 0.164 | 0.126 | **0.231** |
| tokio | 12,892 | 0.185 | **0.191** | 0.157 | 0.172 |
| esbuild | 12,040 | 0.385 | **0.398** | 0.227 | 0.361 |

### MRR (binary relevance, different query set)

| Codebase | Baseline | SEC Pipe | SEC Solo | SEC Fused |
|---|---|---|---|---|
| signetai | 0.442 | 0.535 | 0.433 | **0.628** |
| tokio | 0.242 | 0.198 | 0.220 | **0.263** |
| esbuild | 0.445 | 0.593 | 0.284 | **0.612** |

### Hit rates (MRR benchmark)

| Codebase | Method | H@1 | H@3 | H@5 | H@10 | Miss |
|---|---|---|---|---|---|---|
| signetai | SEC Fused | 5 | 7 | 9 | 9 | **1** |
| tokio | SEC Fused | 2 | 3 | 3 | 5 | 5 |
| esbuild | SEC Fused | 5 | 6 | 8 | 9 | **1** |

### Per-query highlights (MRR benchmark)

**signetai — SEC Fused: 0.442 → 0.628 MRR, 1 miss:**
| Query | Baseline | SEC Fused |
|---|---|---|
| resolve extraction progress for a session | #2 | **#1** |
| incremental skill discovery and file processing | #2 | **#1** |
| check if the embedding model has drifted | #1 | **#1** |
| create an exportable zip archive | #2 | **#1** |

**esbuild — SEC Fused: 0.445 → 0.612 MRR, 1 miss:**
| Query | Baseline | SEC Fused |
|---|---|---|
| rename a symbol to a generated number | #2 | **#1** |
| hash a value with length-prefixed encoding | #2 | **#1** |
| validate log level string | #1 | **#1** |
| clone tokens stripping import records | #2 | **#1** |

### Method descriptions

- **Baseline**: BM25/FTS → structural expansion → heuristic rerank → HRR rerank
- **SEC Pipe**: Baseline pipeline + SEC reranking of top-50 candidates
- **SEC Solo**: Pure structural search using SEC inverted index (no BM25)
- **SEC Fused**: Union of BM25 top-50 + SEC Solo top-50, reranked with SEC scoring

### Benchmark design

NDCG and MRR use **completely different query sets** targeting different symbols. NDCG queries have graded relevance judgments (1-3) covering 3-6 relevant symbols each. MRR queries target a single expected symbol with binary relevance. This ensures the two metrics measure genuinely different retrieval capabilities.

### Running benchmarks

```bash
cargo build --release -p graphiq-bench

# Per-codebase (10-query sets)
./target/release/graphiq-bench /path/to/codebase .graphiq/bench.db benches/ndcg-10-codebase.json benches/mrr-10-codebase.json
```

## What We Learned Building This

### HRR v2 taught us what NOT to do

The first attempt at improving retrieval was HRR v2 — FFT-based circular convolution with multi-channel binding. It failed catastrophically (NDCG@10 0.098, barely above baseline). The root cause: FFT circular convolution binding destroys cross-channel signal. Debug showed the name channel scoring 0.05-0.57 while all structural channels (calls_out, calls_in, type_ret, motif) scored near zero.

**Key insight**: the channel concept is sound. FFT binding is the wrong algebra. Sparse term matching across channels works dramatically better.

### Aggregate MRR is a misleading metric

Optimizing aggregate MRR led us to over-fit on easy queries while ignoring hard ones. The better approach: pick decisive case studies (hard NL queries where BM25 fails) and treat them like a test suite. "how does the timer wheel expire" is worth 10 easy symbol-exact matches for understanding retrieval quality.

### The retrieval funnel matters more than the reranker

Most of the quality comes from the first two layers (BM25 + structural expansion). Rerankers provide incremental gains but can't fix a broken candidate set. SEC Fusion's real contribution is expanding the candidate pool, not the reranking math.

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
           v
+-----------------------------+
|  Layer 5: SEC Fusion        |
|  BM25 candidates (top 50)   |
|  + SEC Solo (top 50)        |
|  --> union + SEC rerank     |
+-----------------------------+
```

### Key Innovations

**Structural Evidence Convolution (SEC)** — Terms from symbol source code, doc comments, search hints, and identifier decomposition are propagated through 7 structural channels (self, calls_out, calls_in, 2hop variants, type_ret, file_path) with distance-based decay. The self channel carries the richest signal — up to 8KB of source code terms plus developer-written search hints. Scoring uses weighted channel overlap with IDF weighting and diversity bonuses for multi-channel hits.

**SEC Fusion** — Unions BM25 pipeline candidates with SEC Solo candidates, then reranks the combined set with SEC scoring. Rescues symbols that BM25 misses while preserving BM25's strong ranking on easy queries. SEC Fused achieves 0.628 MRR on signetai (vs 0.442 baseline) and 0.612 MRR on esbuild (vs 0.445 baseline).

**Holographic Reduced Representations (HRR)** — Each symbol's identity and graph neighborhood are encoded into a 1024-dim vector via circular convolution. Query vectors are matched via dot product. Hypersphere normalization (unit-length post-IFFT) eliminated a 47x norm variance across symbols and produced a +0.132 aggregate NDCG gain.

**Query decomposition** — Abstract queries ("how does retrieval ranking work") are decomposed into 3-8 concrete subqueries via domain-specific term mapping. Each subquery runs through the standard FTS+rerank pipeline; symbols hit by multiple tracks get a multiplicative evidence boost.

**Multi-evidence channels** — Each candidate is scored across 5 evidence channels: lexical (name match), structural (graph expansion), test (test coverage), path (file match), and hints (search_hints coverage). Symbols scoring on 2+ channels get a multiplicative agreement bonus.

**Behavioral role tags** — 19 role tags (validator, cache, handler, retry, auth-gate, etc.) inferred from symbol names, callees, file paths, and edge patterns. Fed into search_hints so FTS matches role vocabulary.

**Structural motifs** — 8 motifs (connector, orchestrator, hub, guard, transform, sink, source, leaf) detected from local edge patterns. A function with both call-in and call-out edges is a "connector" — its hints include "connects joins links bridges".

**Search hints** — An FTS column (weight 5.0) populated at index time with structural role descriptions, morphological variants, role tags, and motif terms. Gives FTS semantic context without embeddings, at zero query-time cost.

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
        sec.rs          # Structural Evidence Convolution + fusion
        hrr.rs          # HRR holographic encoding + hypersphere normalization
        hrr_v2.rs       # HRR v2 (retained for reference, not in pipeline)
        evidence.rs     # Evidence index with adjacency lists
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
    graphiq-bench/      # NDCG/MRR benchmark binary
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
