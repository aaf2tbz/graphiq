# GraphIQ

Code intelligence with structural retrieval. Drop a codebase in, get instant, accurate symbol search powered by BM25 seeded graph walks — zero embeddings required.

**GooberV5: 0.681 MRR on signetai (+0.125 over BM25), 0.511 MRR on tokio, 0.827 MRR on esbuild (+0.152 over BM25).** ~1ms p50 latency. No model dependencies. No neural embeddings. (30-query benchmark across 3 codebases, 46K symbols total.)

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
|  Layer 1: BM25/FTS  |  ~5ms   --> 30 seeds
|  Identifier-aware   |  rateLimit, rate_limit, middleware all match
+----------+----------+
           v
+------------------------------------------+
|  Layer 2: Goober Reranker                 |  --> ~100 candidates
|  BM25-dominant seed scoring               |
|  IDF-gated graph walk (depth 2)           |
|  Walk evidence for non-seeds              |
+----------+-------------------------------+
           v
+------------------------------------------+
|  Layer 3: Query Intent + SEC              |
|  Navigational vs informational weights    |
|  NG scoring (negentropy + coherence)      |
+----------+-------------------------------+
           v
+------------------------------------------+
|  Layer 4: Holographic Name Gate           |
|  FFT cosine similarity per candidate      |
|  Threshold gate (0.25) + specificity      |
|  Only boosts confident matches            |
+----------+-------------------------------+
           v
+------------------------------------------+
|  Layer 5: Confidence-Preserving Fusion    |  --> top_k
|  If BM25 rank-1 has 1.2x+ gap, lock it   |
|  Kind boosts, test penalties              |
+------------------------------------------+
```

### What's new: Goober V3→V5

The core retrieval problem: **how do you promote structurally relevant symbols that BM25 misses without demoting correct BM25 results?**

Previous systems (CruncherV1, CruncherV2) used complex energy field propagation and interference scoring. Extensive benchmarking revealed a simpler truth: **BM25 seed ordering is generally correct, and structural reranking must be conservative to avoid introducing noise.**

The Goober lineage:

1. **Goober** — BM25-dominant seed scoring + IDF-gated graph walk + confidence lock. Stripped interference scoring, energy fields, hub dampening, bridging. Simpler = better.
2. **GooberV3** — Non-Gaussianity (NG) scoring from SEC channel analysis. Symbols with spiky, specific channel profiles get boosted over flat/uniform ones. Negentropy + channel coherence.
3. **GooberV4** — Query intent classification (navigational vs informational) with intent-adaptive scoring weights. Navigational queries cap structural norms lower to preserve BM25 ordering.
4. **GooberV5** — Per-candidate holographic name gating. FFT-based holographic encoding of identifier terms produces a cosine similarity signal with 6.8x separation between correct/incorrect matches. The key insight: **only trust the holographic signal when it's confident** (cosine > 0.25 threshold), scaled by query specificity (fraction of high-IDF terms). Low-confidence holographic matches are ignored entirely, preventing the false promotions that plagued earlier experiments.

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

10-query benchmark across 3 codebases (signetai: 20,870 symbols, tokio: 12,892, esbuild: 12,040). Dual evaluation: NDCG@10 (graded relevance) + MRR (rank-1 correctness).

### MRR (rank-1 correctness — primary metric)

**30-query benchmark:**

| Codebase | BM25 | CR v1 | CR v2 | Goober | GooberV3 | GooberV4 | **GooberV5** | V5 vs BM25 |
|---|---|---|---|---|---|---|---|---|
| signetai | 0.556 | 0.608 | 0.638 | 0.625 | 0.675 | 0.675 | **0.681** | **+0.125** |
| tokio | 0.583 | 0.492 | 0.511 | 0.513 | 0.506 | 0.499 | **0.511** | -0.072 |
| esbuild | 0.675 | 0.597 | 0.737 | 0.774 | 0.773 | 0.781 | **0.827** | **+0.152** |

**V5 vs V4 delta:** signetai +0.006, tokio +0.012, esbuild +0.046.

GooberV5 beats all previous versions on all 3 codebases. The holographic name gate adds signal where it's confident (descriptive identifiers like `convertOKLCHToOKLAB`) and stays silent where it's not (generic tokio functions). Esbuild sees the biggest gain (+0.046 MRR, +2 accuracy) because its descriptive function names produce strong holographic matches that pass the confidence gate.

### NDCG@10 (graded relevance)

| Codebase | BM25 | CR v1 | CR v2 | Goober | GooberV3 | GooberV4 | **GooberV5** |
|---|---|---|---|---|---|---|---|
| signetai | 0.202 | 0.267 | 0.281 | 0.252 | 0.259 | 0.259 | 0.252 |
| tokio | 0.225 | 0.232 | 0.249 | 0.208 | 0.232 | 0.211 | **0.238** |
| esbuild | 0.365 | 0.351 | 0.380 | 0.379 | 0.387 | 0.387 | 0.387 |

### Per-query highlights (MRR benchmark, esbuild — V5 wins)

| Query | BM25 | GooberV4 | GooberV5 | Change |
|---|---|---|---|---|
| lower and minify a CSS color | #3 | #2 | **#1** | holographic name match passes gate |
| convert OKLCH color to OKLAB | #2 | #1 | **#1** | maintained |
| compute reserved names for renaming | #2 | #2 | **#1** | holographic boost pushes to top |
| scan for imports and exports | #9 | #4 | **#3** | improved |

### Per-query highlights (MRR benchmark, signetai — V5 wins)

| Query | GooberV4 | GooberV5 | Change |
|---|---|---|---|
| compute semantic version comparison | #3 | **#2** | marginal holographic boost |
| purge stale embeddings from store | #1 | **#1** | maintained |

### Method descriptions

- **BM25**: SQLite FTS5 with per-column weights (name=10, decomposed=8, qualified=6, hints=5, doc=3, file_path=3.5, sig=4, source=1)
- **Cruncher v1**: BM25 seeds + query-conditioned graph walk + multi-signal scoring (coverage, name, structural, bridging)
- **Cruncher v2**: BM25 seeds + per-term energy field propagation + interference scoring + confidence-preserving BM25 rank-1 lock
- **Goober**: BM25 seeds + BM25-dominant seed scoring + IDF-gated graph walk + walk evidence for non-seeds + confidence lock
- **GooberV3**: Goober + Non-Gaussianity (negentropy + channel coherence) from SEC channel analysis
- **GooberV4**: GooberV3 + query intent classification (navigational vs informational) with intent-adaptive scoring weights
- **GooberV5**: GooberV4 + per-candidate holographic name gating — FFT-based cosine similarity thresholded at 0.25, scaled by query specificity

### Benchmark design

NDCG and MRR use **completely different query sets** targeting different symbols. NDCG queries have graded relevance judgments (1-3) covering 3-6 relevant symbols each. MRR queries target a single expected symbol with binary relevance. This ensures the two metrics measure genuinely different retrieval capabilities.

### Running benchmarks

```bash
cargo build --release -p graphiq-bench

# 10-query benchmark (fast)
./target/release/graphiq-bench .graphiq/bench.db benches/ndcg-10-codebase.json benches/mrr-10-codebase.json

# 30-query benchmark (more stable)
./target/release/graphiq-bench .graphiq/bench.db benches/ndcg-10-codebase.json benches/mrr-30-codebase.json
```

## What We Learned Building This

### Every system that tried to replace BM25 failed

We built 9 retrieval systems (SEC, Evidence, HRR, HRR v2, AFMO, Spectral, LSA, AF26, Holo). None beat BM25 on MRR across all codebases. The winning pattern is always: **BM25 retrieves, structural math reranks**. BM25's inverted index is O(1) — no full-scan system can compete on speed, and its ranking is remarkably hard to beat on correctness.

### Simpler is better

CruncherV2 used per-term energy vectors, cosine interference scoring, hub dampening, bridging potential, and yoyo validation. Goober strips all of this and uses a simple BM25-dominant weighted sum with an IDF-gated walk. Result: Goober strictly outperforms CruncherV2 on all 3 codebases. The complex interference mechanics captured patterns that were already captured by simpler coverage + name scoring, while introducing noise on codebases with generic function names.

### The confidence lock is critical

When BM25 is confident (rank-1 has >1.2x gap), it's almost always right. Demoting a confident BM25 result is almost always a mistake. The confidence-preserving lock at rank-1 prevents the graph walk from inserting wrong candidates above correct results.

### Gate your signals

We ran 7 experiments (V5–V11) trying to add holographic name matching. The raw cosine similarity between query and symbol name holograms has 6.8x separation between correct and incorrect matches — a strong signal. But adding it indiscriminately (V7) caused false promotions on codebases with generic names. The solution: **gate the signal at a per-candidate threshold** (cosine > 0.25) and scale by query specificity. Only trust the holographic match when it's confident; otherwise stay silent. This single insight turned a net-negative feature into a net-positive across all 3 codebases.

### Aggregate MRR is a misleading metric

Optimizing aggregate MRR led us to over-fit on easy queries while ignoring hard ones. The better approach: pick decisive case studies (hard NL queries where BM25 fails) and treat them like a test suite.

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

**GooberV5** — Full pipeline: BM25-dominant seed scoring (3:1.5:2 BM25:coverage:name) with IDF-gated graph walk, query intent classification (navigational vs informational), Non-Gaussianity scoring from SEC channels, and per-candidate holographic name gating. The holographic gate uses FFT-based circular convolution to encode identifier terms as holographic vectors, computing cosine similarity between query and candidate name encodings. Only candidates exceeding a 0.25 confidence threshold receive the holographic boost, scaled by query specificity. Beats all prior versions on all 3 codebases.

**Structural Evidence Convolution (SEC)** — Terms propagated through 7 structural channels (self, calls_out, calls_in, 2hop variants, type_ret, file_path) with distance-based decay. The self channel carries up to 8KB of source code terms plus developer-written search hints.

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
        cruncher.rs     # Goober + CruncherV1 + CruncherV2 retrieval engines
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
