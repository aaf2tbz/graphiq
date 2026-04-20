# GraphIQ

Local code intelligence. Index a codebase into a structural graph, then search it with ranked retrieval that understands calls, imports, type flow, and architectural roles — not just string matching.

No embeddings. No LLM. No network. Everything runs in a single SQLite file.

## How It Compares to Grep

Grep (BM25 `LIKE %term%` over all symbol names and source lines) is a strong baseline for exact lookups. GraphIQ wraps BM25 as layer 1, then adds graph structure on top. Benchmarked on 3 codebases (TypeScript, Rust, Go), 25 queries each for MRR, 20 each for NDCG:

| Metric | Grep | GraphIQ | Delta |
|---|---|---|---|
| MRR@10 | 0.941 | 0.959 | +1.9% |
| NDCG@10 | 0.288 | 0.378 | +31% |
| Combined | 0.615 | 0.669 | +8.7% |

Per-category NDCG@10 (3-codebase average):

| Category | Grep | GraphIQ |
|---|---|---|
| symbol-exact | 0.887 | 0.899 |
| symbol-partial | 0.711 | 0.708 |
| nl-descriptive | 0.069 | 0.289 |
| nl-abstract | 0.030 | 0.216 |
| error-debug | 0.159 | 0.268 |
| file-path | 0.066 | 0.048 |
| cross-cutting | 0.000 | 0.137 |

GraphIQ wins 5 of 7 categories. File-path queries remain grep's strength. See [docs/benchmarks.md](docs/benchmarks.md) for per-codebase breakdowns and methodology.

## How It Works

```
Query: "how does the timer wheel expire deadlines"
              |
              v
   Query Family Router (query_family.rs)
   Classifies into one of 8 families
              |
              v
   Seed Generation (seeds.rs)
   BM25 FTS -> name lookup -> graph walk -> numeric bridges -> self-model
   ~100 seed candidates
              |
              v
   Spectral Expansion (pipeline.rs)
   Chebyshev heat diffusion on graph Laplacian (K=15)
   ~200 candidates
              |
              v
   Unified Scoring (scoring.rs)
   IDF coverage + holographic name gate + predictive surprise + MDL
              |
              v
   Confidence Fusion
   BM25 lock -> kind boosts -> file diversity
   -> top_k results
```

**Seed Generation** — BM25 FTS produces initial seeds, expanded through identifier decomposition, structural graph walks (calls, imports, type flow), numeric bridges (shared constants), and self-model concept nodes. `SeedConfig::for_family()` controls which expansions fire based on query family.

**Spectral Expansion** — Chebyshev polynomial approximation of the graph Laplacian's heat kernel. Heat propagates from seed symbols across structural edges so symbols structurally connected to seeds get discovered even if their names share no terms with the query. O(K|E|) per query instead of O(n^3) eigendecomposition.

**Unified Scoring** — Single `score_candidates()` function parameterized by `ScoreConfig`: IDF-weighted coverage fractions, predictive surprise (KL divergence from conditional term models), MDL explanation sets (greedy set cover), and holographic name gating (FFT cosine similarity).

**Confidence Fusion** — BM25 confidence lock (rank-1 gap > 1.2x), kind boosts (functions/types over variables/imports), per-file diversity limits.

### Query Families

| Family | Detection | Example |
|---|---|---|
| SymbolExact | Exact name, PascalCase | `RateLimiter` |
| SymbolPartial | Short fragment | `rate limit` |
| NaturalDescriptive | Action verbs | `encode a value in VLQ` |
| NaturalAbstract | "how does", "what controls" | `how does auth work` |
| ErrorDebug | Panic/error/timeout | `timeout in channel send` |
| CrossCuttingSet | "all", "every", plural | `all connector implementations` |
| Relationship | "vs", "relationship" | `AsyncFd vs readiness guard` |
| FilePath | Paths, extensions | `scheduler/worker.rs` |

Each family routes to a tuned `ScoreConfig`. No stacking, no fusion — one config per query.

### Code Graph Edge Types

| Edge Type | Signal |
|---|---|
| Calls | Function calls |
| Imports | Module imports |
| Contains | Scope containment (struct contains method) |
| SharesType | Shared type tokens in signatures |
| SharesErrorType | Shared error parameters |
| SharesDataShape | Shared field access patterns |
| SharesConstant | Shared numeric/string literals |
| StringLiteral | Shared error-related string constants |
| CommentRef | Symbol mentions in comments |

## Installation

### Homebrew (macOS, Linux)

```bash
brew tap aaf2tbz/graphiq
brew install graphiq
```

Installs three binaries: `graphiq` (CLI), `graphiq-mcp` (MCP server), `graphiq-bench` (benchmarking).

### From Source

```bash
git clone https://github.com/aaf2tbz/graphiq.git
cd graphiq
cargo build --release
```

Rust edition 2021. No system dependencies.

## CLI

```bash
graphiq search "rate limit middleware"
graphiq search "authenticateUser" --top 20 --file src/auth/
graphiq search "error handler" --debug

graphiq blast RateLimiter
graphiq blast RateLimiter --depth 5 --direction forward

graphiq index /path/to/project
graphiq reindex /path/to/project
graphiq status
graphiq doctor
graphiq upgrade-index

graphiq setup --project /path/to/project
graphiq demo
```

`--debug` on search prints per-result score breakdowns, active search mode, and query family.

## MCP Server

`graphiq-mcp` speaks JSON-RPC 2.0 over stdio. 12 tools:

| Tool | What it does |
|---|---|
| `search` | Ranked symbol search (file filter, top_k up to 50) |
| `blast` | Blast radius (forward/backward/both, depth 1-10) |
| `context` | Full source + structural neighborhood (callers, callees, members) |
| `status` | Index stats, project root, database size, active search mode |
| `index` | (Re)index the project |
| `explain` | Symbol explanation |
| `topology` | Code topology |
| `why` | Relevance explanation for a result |
| `interrogate` | Symbol interrogation |
| `doctor` | Artifact health check |
| `upgrade_index` | Rebuild stale artifacts |
| `constants` | Numeric bridge lookup |
| `briefing` | Project briefing |

```bash
graphiq-mcp /path/to/project
```

### Supported Harnesses

| Harness | Config | Setup |
|---|---|---|
| opencode | `~/.config/opencode/opencode.json` | `graphiq setup` |
| Claude Desktop | `~/Library/Application Support/Claude/claude_desktop_config.json` | `graphiq setup` |
| Codex | `~/.codex/config.toml` | `graphiq setup` |
| Hermes | `~/.hermes/config.yaml` | `graphiq setup` |

## Supported Languages

36 languages recognized. 14 have dedicated TreeSitter parsers for full symbol extraction:

**Full parsing (14 grammars, 16 language variants):** TypeScript, TSX, JavaScript, JSX, Rust, Python, Go, Java, C, C++, Ruby, YAML, TOML, JSON, HTML, CSS

**File tracking + FTS only (20):** Kotlin, Swift, C#, PHP, Lua, Dart, Scala, Haskell, Elixir, Zig, GraphQL, Protobuf, Shell, SQL, Markdown, XML, SCSS, CMake, Qml, Dockerfile, Makefile, Meson

## Storage Layout

```
.graphiq/
  graphiq.db                SQLite (symbols, edges, FTS5 index)
  manifest.json             artifact freshness tracking
  cache/                    precomputed artifacts (zstd-compressed)
    cruncher.bin.zst        adjacency lists, term sets, IDF
    holo_f32.bin.zst        holographic name vectors (f32 quantized)
    spectral.bin.zst        Chebyshev heat diffusion index
    predictive_compact.bin.zst  conditional term models (top-200/symbol)
    fingerprints.bin.zst    channel fingerprint vectors
    self_model.bin.zst      deterministic concept nodes
```

Cache sizes for a ~20K symbol codebase: ~75MB total. Validated against DB stats on every search — stale cache is transparently rebuilt. `graphiq reindex` and `graphiq upgrade-index` invalidate automatically.

## Search Speed

| Mode | Time |
|---|---|
| Cold CLI search (first run) | ~30s |
| Warm CLI search (cached) | ~850ms |
| In-process query (MCP/bench) | ~18us |

## Source Layout

```
graphiq/
  crates/
    graphiq-core/
      seeds.rs          seed generation (BM25, name lookup, graph walk, bridges)
      scoring.rs        unified scoring (SEC, holographic, surprise, MDL)
      pipeline.rs       unified_search()
      search.rs         router, query family dispatch
      query_family.rs   8-family query classifier
      cruncher.rs       adjacency lists, term sets, IDF, holographic encoding
      spectral.rs       Chebyshev heat diffusion, predictive model, fingerprints
      deep_graph.rs     type flow, error type, data shape edges
      self_model.rs     deterministic concept nodes
      artifact_cache.rs disk cache (zstd-compressed bincode)
      blast.rs          blast radius computation
      languages/        14 TreeSitter grammars
    graphiq-cli/        CLI binary
    graphiq-mcp/        MCP server binary
    graphiq-bench/      benchmark binary
```

## Documentation

- [docs/retrieval.md](docs/retrieval.md) — pipeline details
- [docs/benchmarks.md](docs/benchmarks.md) — full benchmark results and methodology
- [docs/research.md](docs/research.md) — experimental log

## License

MIT
