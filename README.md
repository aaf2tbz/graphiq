# GraphIQ

Structural code intelligence for AI agents. Drop a codebase in and get accurate, ranked answers — not just symbol names, but structural facts about what each symbol does, what connects to it, and where it sits in the architecture.

No embeddings. No LLM. No model dependencies. Pure math, physics, and graph theory — spectral heat diffusion on the code graph, holographic name matching, predictive surprise scoring. Everything runs locally in a single SQLite file.

## Why GraphIQ Instead of Grep

Grep is the strongest possible naive baseline — it searches every symbol name and every line of source code with `LIKE %term%`. It's brutally effective for exact lookups and substring matching. But grep has a fundamental limitation: **it has no understanding of code structure**. It doesn't know what calls what, what imports what, or which symbols are structurally important.

GraphIQ preserves grep's lexical strengths (BM25 full-text search is Layer 1 of the pipeline) and adds structural analysis on top. The result:

### MRR@10 (first-hit accuracy, 25 queries per codebase)

| Codebase | Grep | GraphIQ | Δ |
|---|---|---|---|
| signetai | 0.941 | **0.960** | +2% |
| esbuild | 0.943 | **0.947** | +0.4% |
| tokio | 0.940 | **0.970** | +3% |
| **Overall** | **0.941** | **0.959** | **+1.9%** |

### NDCG@10 (ranking quality, 20 queries per codebase)

| Codebase | Grep | GraphIQ | Δ |
|---|---|---|---|
| signetai | 0.276 | **0.397** | +44% |
| esbuild | 0.298 | **0.453** | +52% |
| tokio | **0.290** | 0.284 | -2% |
| **Overall** | **0.288** | **0.378** | **+31%** |

### Combined (MRR + NDCG)

| Codebase | Grep | GraphIQ | Δ |
|---|---|---|---|
| signetai | 0.609 | **0.679** | +11% |
| esbuild | 0.621 | **0.700** | +13% |
| tokio | 0.615 | **0.627** | +2% |
| **Overall** | **0.615** | **0.669** | **+8.7%** |

### NDCG@10 by Category (3-codebase average)

| Category | Grep | GraphIQ |
|---|---|---|
| symbol-exact | 0.887 | **0.899** |
| symbol-partial | **0.711** | 0.708 |
| nl-descriptive | 0.069 | **0.289** |
| nl-abstract | 0.030 | **0.216** |
| error-debug | 0.159 | **0.268** |
| file-path | **0.066** | 0.048 |
| cross-cutting | 0.000 | **0.137** |

GraphIQ dominates on 5/7 categories. The remaining gaps — file-path and tokio's natural language queries — are the frontier for v7.

### What "beats grep" actually means

Grep returns a flat list of matching lines. GraphIQ returns ranked symbols with structural context:

```
graphiq search "rate limit middleware" →

  1. RateLimiter                          score: 0.94
     callers: setupMiddleware, chain.execute
     callees: checkLimit, TokenBucket.consume
     role: [hub]

  2. TokenBucket                          score: 0.71
     callers: RateLimiter.checkLimit
     callees: Clock.now
     role: [connector]
```

The agent doesn't just get a name. It gets a fact about the codebase — what the symbol is connected to, what role it plays, and how relevant it is to the query. That's the product difference.

## How It Works

GraphIQ is a unified retrieval pipeline. Every component is deterministic — no neural networks, no learned weights, no GPU required.

```
Query: "how does the timer wheel expire deadlines"
                |
                v
     Query Family Router
     (classifies into 8 families)
                |
                v
     Seed Generation (seeds.rs)
     BM25/FTS → name lookup → graph walk → numeric bridges → self-model
     → ~100 seed candidates
                |
                v
     Spectral Expansion (pipeline.rs)
     Chebyshev heat diffusion on graph Laplacian
     → ~200 candidate symbols
                |
                v
     Unified Scoring (scoring.rs)
     SEC + holographic name gate + predictive surprise + MDL
     → scored, ranked candidates
                |
                v
     Confidence Fusion (pipeline.rs)
     BM25 lock → kind boosts → file diversity
     → top_k results
```

### The Unified Pipeline (v6)

v6 consolidated ~3,000 lines of near-duplicate scoring code across 5 search methods into a single `unified_search()` function parameterized by `ScoreConfig`. The pipeline has four stages:

**1. Seed Generation** (`seeds.rs`) — BM25 full-text search produces initial seeds, then expands them through name lookup (identifier decomposition), structural graph walks (calls, imports, type flow), numeric bridges (shared constants), and self-model concept nodes. `SeedConfig::for_family()` controls which expansions activate based on query family.

**2. Spectral Expansion** (`pipeline.rs`) — Seeds are expanded through Chebyshev polynomial approximation of the graph Laplacian's heat kernel. Heat propagates from seed symbols across structural edges — calls, imports, type flow, shared error types, shared data shapes — so symbols structurally connected to the seeds get discovered even if their names share no terms with the query. Direct computation needs eigendecomposition (O(n³)). Chebyshev computes it in O(K|E|) per query where K=15.

**3. Unified Scoring** (`scoring.rs`) — A single `score_candidates()` function handles all search modes. The `ScoreConfig` struct parameterizes behavior: IDF-weighted coverage fractions, predictive surprise (KL divergence from conditional term models), MDL explanation sets (greedy set cover for query term diversity), and holographic name gating (FFT cosine similarity with threshold 0.25).

**4. Confidence Fusion** — BM25 confidence lock (when BM25's rank-1 has a >1.2x gap, lock it), kind boosts (functions and types over variables and imports), and per-file diversity limits.

### The Query Family Router

Before the pipeline runs, the query is classified into one of 8 families. Each family routes to tuned `ScoreConfig` parameters — no fusion, no stacking, one configuration per query:

| Family | Detection | Config | Example |
|---|---|---|---|
| SymbolExact | Exact name, PascalCase | name-gated, no surprise | `RateLimiter` |
| SymbolPartial | Short fragment | name-gated, light expansion | `rate limit` |
| NaturalDescriptive | Action verbs | full spectral + surprise + MDL | `encode a value in VLQ` |
| NaturalAbstract | "how does", "what controls" | max exploration, high walk weight | `how does auth work` |
| ErrorDebug | Panic/error/timeout | predictive model + fingerprints | `timeout in channel send` |
| CrossCuttingSet | "all", "every", plural | high diversity, set cover | `all connector implementations` |
| Relationship | "vs", "relationship" | neighborhood-centric | `AsyncFd vs readiness guard` |
| FilePath | Paths, extensions | file-adjacent | `scheduler/worker.rs` |

### The Code Graph

GraphIQ builds a rich structural graph during indexing:

| Edge Type | Signal | Example |
|---|---|---|
| Calls | Function calls | `authenticate()` calls `hashPassword()` |
| Imports | Module imports | `use tokio::sync::Mutex` |
| Contains | Scope containment | `RateLimiter` struct contains `check()` method |
| SharesType | Shared type tokens in signatures | Two functions both take `Arc<Mutex<Bar>>` |
| SharesErrorType | Shared error parameters | Functions that both return `Result<T, io::Error>` |
| SharesDataShape | Shared field access patterns | Functions that both access `self.config.host` |
| SharesConstant | Shared numeric/string literals | Functions that both reference `429` or `"timeout"` |
| StringLiteral | Shared error-related string constants | Functions containing `"connection refused"` |
| CommentRef | Symbol mentions in comments | `// delegates to processExpiredTimers` |

## Installation

### Homebrew (macOS + Linux)

```bash
brew tap aaf2tbz/graphiq
brew install graphiq
```

Installs three binaries:
- `graphiq` — CLI (index, search, blast, status, reindex, demo, setup)
- `graphiq-mcp` — MCP server for LLM integration (stdio JSON-RPC)
- `graphiq-bench` — NDCG/MRR benchmarking

### From Source

```bash
git clone https://github.com/aaf2tbz/graphiq.git
cd graphiq
cargo build --release
```

Requires Rust 1.70+. No other dependencies — no Python, no Node, no system libraries.

### Quick Start

```bash
# Zero setup — generates a sample project and shows GraphIQ vs BM25
graphiq demo

# Index a real project and configure MCP integrations
graphiq setup --project /path/to/project
```

## CLI

### Search

```bash
graphiq search "rate limit middleware"
graphiq search "authenticateUser" --top 20 --file src/auth/
graphiq search "error handler" --debug
```

`--debug` prints per-result score breakdowns and the active search mode.

### Blast Radius

```bash
graphiq blast RateLimiter
graphiq blast RateLimiter --depth 5 --direction forward
graphiq blast RateLimiter --direction both
```

### Indexing

```bash
graphiq index /path/to/project
graphiq reindex /path/to/project
graphiq status
graphiq doctor
graphiq upgrade-index
```

### Setup

```bash
graphiq setup
graphiq setup --project /path/to/proj
graphiq setup --skip-index
```

## MCP Server

`graphiq-mcp` speaks JSON-RPC 2.0 over stdio. Exposes five tools:

| Tool | Description |
|---|---|
| `search` | Ranked symbol search with file filter and top_k (max 50) |
| `blast` | Blast radius — forward/backward/both, depth 1-10 |
| `context` | Full source + structural neighborhood (callers, callees, members) |
| `status` | Index stats, project root, database size, active search mode |
| `index` | (Re)index the project on demand |

```bash
graphiq-mcp /path/to/project
```

### Supported Harnesses

| Harness | Config Location | Status |
|---|---|---|
| opencode | `~/.config/opencode/opencode.json` | Auto-detected |
| Claude Desktop | `~/Library/Application Support/Claude/claude_desktop_config.json` | Auto-detected |
| Codex | `~/.codex/config.toml` | Auto-detected |
| Hermes | `~/.hermes/config.yaml` | Auto-detected |

## Supported Languages

34 languages detected. 16 with dedicated TreeSitter parsers for full symbol extraction:

**Full parsing:** TypeScript, TSX, JSX, JavaScript, Rust, Python, Go, Java, C, C++, Ruby, YAML, TOML, JSON, HTML, CSS

**File tracking + FTS:** Kotlin, Swift, C#, PHP, Lua, Dart, Scala, Haskell, Elixir, Zig, GraphQL, Protobuf, Shell, SQL, Markdown, XML, SCSS

## Architecture

Single-file SQLite database at `.graphiq/graphiq.db`. Rust, edition 2021. Zero runtime dependencies.

```
graphiq/
  crates/
    graphiq-core/       # Core library
      seeds.rs          # Seed generation (BM25, name lookup, graph walk)
      scoring.rs        # Unified scoring (SEC, holographic, surprise, MDL)
      pipeline.rs       # unified_search() — single pipeline for all modes
      search.rs         # Router, query family dispatch
      cruncher.rs       # Adjacency lists, term sets, IDF, legacy methods
      spectral.rs       # Chebyshev heat diffusion, Ricci curvature
      query_family.rs   # 8-family query classifier
      deep_graph.rs     # Type flow, error type, data shape edges
      self_model.rs     # Deterministic concept nodes
    graphiq-cli/        # CLI binary
    graphiq-mcp/        # MCP server binary
    graphiq-bench/      # Benchmark binary
```

### Key Components

| Component | Source | Purpose |
|---|---|---|
| SearchEngine | `search.rs` | Main entry point, query family dispatch |
| unified_search | `pipeline.rs` | Single pipeline for all search modes |
| generate_seeds | `seeds.rs` | BM25 → name lookup → graph walk → bridges → self-model |
| score_candidates | `scoring.rs` | SEC + holographic + surprise + MDL scoring |
| CruncherIndex | `cruncher.rs` | Adjacency lists, term sets, IDF, holographic encoding |
| SpectralIndex | `spectral.rs` | Chebyshev heat diffusion, channel fingerprints |
| QueryFamily | `query_family.rs` | 8-family classifier, retrieval policy |
| DeepGraph | `deep_graph.rs` | Type flow, error type, data shape, string literal edges |
| RepoSelfModel | `self_model.rs` | Deterministic concept nodes for abstract queries |

### Storage Layout

```
.graphiq/
├── graphiq.db              SQLite database (symbols, edges, FTS5 index)
├── manifest.json           artifact freshness tracking
└── cache/                  precomputed artifacts (zstd-compressed)
    ├── cruncher.bin.zst
    ├── holo_f32.bin.zst
    ├── spectral.bin.zst
    ├── predictive_compact.bin.zst
    ├── fingerprints.bin.zst
    └── self_model.bin.zst
```

### Operating System Support

| OS | Status | Notes |
|---|---|---|
| macOS (x86_64, aarch64) | Fully supported | Homebrew and source |
| Linux (x86_64) | Fully supported | Homebrew and source |
| Linux (aarch64) | Source only | Requires Rust toolchain |

## Benchmarks

v6 unified pipeline. 3 codebases (TypeScript, Rust, Go). Separate NDCG (20 queries/codebase) and MRR (25 queries/codebase) query sets.

### Accuracy vs Grep

| Metric | Grep | GraphIQ | Δ |
|---|---|---|---|
| MRR@10 | 0.941 | **0.959** | +1.9% |
| NDCG@10 | 0.288 | **0.378** | +31% |
| Combined | 0.615 | **0.669** | +8.7% |

| Category | Grep | GraphIQ | Winner |
|---|---|---|---|
| symbol-exact | 0.887 | **0.899** | GraphIQ |
| symbol-partial | **0.711** | 0.708 | Grep (marginal) |
| nl-descriptive | 0.069 | **0.289** | GraphIQ (4.2x) |
| nl-abstract | 0.030 | **0.216** | GraphIQ (7.2x) |
| error-debug | 0.159 | **0.268** | GraphIQ (1.7x) |
| file-path | **0.066** | 0.048 | Grep |
| cross-cutting | 0.000 | **0.137** | GraphIQ |

### Search Speed

All artifacts cached to disk after first query. Subsequent CLI searches load from cache.

| Metric | Time |
|---|---|
| Cold search (first run) | ~30s |
| Warm search (cached) | **~850ms** |
| In-process query (MCP/bench) | ~18μs |

### Artifact Cache

```
.graphiq/cache/           Total: ~75MB
├── cruncher.bin.zst      6.5MB   adjacency lists, term sets, IDF
├── holo_f32.bin.zst      42MB    holographic name vectors (f32)
├── spectral.bin.zst      17MB    Chebyshev heat diffusion
├── predictive_compact.bin.zst  8.8MB  conditional term models (top-200/symbol)
├── fingerprints.bin.zst  78KB    channel fingerprint vectors
└── self_model.bin.zst    356KB   deterministic concept nodes
```

Cache is validated against DB stats on every search — stale cache is transparently rebuilt. `graphiq reindex` and `graphiq upgrade-index` invalidate the cache automatically.

See [docs/benchmarks.md](docs/benchmarks.md) for full per-codebase breakdowns and methodology.

## Documentation

- [docs/retrieval.md](docs/retrieval.md) — Full pipeline details
- [docs/benchmarks.md](docs/benchmarks.md) — Complete benchmark results
- [docs/research.md](docs/research.md) — Experimental history: 24 phases of research
- [docs/ROADMAP.md](docs/ROADMAP.md) — Current state and next steps

## License

MIT
