# GraphIQ

Instant, accurate code search powered by BM25 + structural graph analysis + spectral heat diffusion + query family routing + CARE fusion. No embeddings. No model dependencies. Drop a codebase in and search.

**Routed: NDCG@10 0.514 (esbuild), 0.413 (tokio), 0.405 (signetai) | CARE: MRR 0.493 (tokio)**

## How It Works

```
Query: "rate limit middleware"
        |
        v
  Query Family Router (8 families)
        |
        v
  BM25/FTS  -->  30 seeds
        |
        v
  [Geo] Chebyshev Heat Diffusion (spectral graph)
        |
        v
  Predictive Surprise (D_KL query vs graph context)
  Channel Capacity Routing (structural role blending)
  MDL Explanation Sets (greedy coverage + stopping)
        |
        v
  SEC + NG Scoring  -->  structural rerank
        |
        v
  Holographic Name Gate  -->  confidence-filtered boost
        |
        v
  [CARE] Fusion: GooV5 (lexical) + Routed (structural)
  Confidence-anchored score fusion + BM25 anchor
        |
        v
  Confidence Lock  -->  top_k results
```

BM25 retrieves seeds. The query family router classifies the query and gates which downstream signals may influence ranking. Chebyshev heat diffusion propagates relevance across the graph's structural topology. Three deformation signals adapt scoring per-query. For CARE mode, GooV5's lexical precision and Routed's structural recall are fused via normalized score fusion with convergence bonuses. See [docs/retrieval.md](docs/retrieval.md) for full pipeline details.

## Benchmarks

NDCG@10 across 3 codebases (v4 queries, 12 methods):

| Codebase | BM25 | GooV5 | Geometric | Deformed | **Routed** | CARE |
|---|---|---|---|---|---|---|
| esbuild (Go) | 0.299 | 0.430 | 0.480 | 0.483 | **0.514** | 0.496 |
| signetai (TS) | 0.287 | 0.375 | 0.367 | 0.367 | **0.405** | 0.384 |
| tokio (Rust) | 0.272 | 0.305 | 0.353 | 0.355 | **0.413** | 0.363 |

MRR across 3 codebases (v4 queries, disjoint from NDCG):

| Codebase | BM25 | GooV5 | Geometric | Routed | **CARE** |
|---|---|---|---|---|---|
| esbuild | 0.575 | 0.713 | 0.763 | **0.740** | 0.693 |
| signetai | 0.650 | **0.721** | 0.700 | 0.691 | 0.696 |
| tokio | 0.375 | 0.467 | 0.425 | 0.348 | **0.493** |

12 retrieval methods tested: BM25, CRv1, CRv2, Goober, GooV3, GooV4, GooV5, Geometric, Curved, Deformed, Routed, CARE.

Full results including per-category breakdowns, H@1-10, P@10, R@10: [docs/benchmarks.md](docs/benchmarks.md)

## Quick Start

```bash
# Install
brew tap aaf2tbz/graphiq && brew install graphiq

# Try it — no project needed
graphiq demo

# Index a real project
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
- `graphiq` — CLI (index, search, blast, status, reindex, demo, setup, spectral)
- `graphiq-mcp` — MCP server for LLM integration (stdio JSON-RPC)
- `graphiq-bench` — NDCG/MRR benchmarking and parameter tuning
- `graphiq-locomo` — LoCoMo-style benchmarking

### From source

```bash
git clone https://github.com/aaf2tbz/graphiq.git
cd graphiq
cargo build --release
```

## CLI

### `graphiq setup`

One-command onboarding. Detects git root, writes MCP configs, indexes the project.

```bash
graphiq setup                          # current directory
graphiq setup --project /path/to/proj  # specify project
graphiq setup --skip-index             # configure without indexing
```

Configures these harnesses automatically:

| Harness | Config |
|---|---|
| opencode | `~/.config/opencode/opencode.json` |
| Codex | `~/.codex/config.toml` |
| Claude Desktop | `~/Library/Application Support/Claude/claude_desktop_config.json` |
| Hermes | `~/.hermes/config.yaml` |

### `graphiq search`

```bash
graphiq search "rate limit middleware"
graphiq search "authenticateUser" --top 20 --file src/auth/
graphiq search "error handler" --debug
```

`--debug` prints per-result score breakdowns and the active search mode (Routed, GooV5, etc.).

### `graphiq blast`

Compute blast radius — what a symbol affects and what depends on it.

```bash
graphiq blast RateLimiter
graphiq blast RateLimiter --depth 5 --direction forward
```

### `graphiq spectral`

Compute the spectral embedding (Laplacian eigendecomposition + Chebyshev heat kernel infrastructure). Required for the Geometric, Deformed, and Routed search modes.

```bash
graphiq spectral --db .graphiq/graphiq.db
```

### `graphiq demo`

Generates a multi-language sample project (Rust, Java, Ruby), indexes it, and runs a side-by-side comparison of BM25 (FTS-only) vs GraphIQ (GooberV5). Shows where structural graph analysis promotes the right symbol above pure text search results.

```
$ graphiq demo
Indexed in 41ms: 13 files, 119 symbols, 48 edges

── BM25 (FTS) vs GraphIQ (GooberV5) ──

  "maximum concurrent connections"  [target: ConnectionPool]
  BM25 rank:  #3   GraphIQ rank:  #1   GraphIQ promotes target

  "scheduler shutdown cleanup"  [target: shutdown]
  BM25 rank:  #5   GraphIQ rank:  #2   GraphIQ promotes target

  Result: GraphIQ 3/8 | BM25 1/8 | Tied 4/8
```

Zero setup. No project needed.

### Other commands

```bash
graphiq index /path/to/project     # index a project
graphiq reindex /path/to/project   # reindex
graphiq status                      # show index stats, active search mode, artifact freshness
graphiq doctor                      # diagnose index issues
```

### `graphiq-bench`

Benchmark and tune retrieval methods.

```bash
# Run full benchmark suite (12 methods, NDCG + MRR)
graphiq-bench <db> <ndcg-queries.json> <mrr-queries.json>

# Parameter tuning (outputs CSV)
graphiq-bench tune <db> <ndcg-queries.json> <mrr-queries.json>

# Latency profiling
graphiq-bench profile <db> <mrr-queries.json>

# Fuzz testing
graphiq-bench fuzz <db>
```

## MCP Server

`graphiq-mcp` speaks JSON-RPC 2.0 over stdio. Five tools:

| Tool | Description |
|---|---|
| `search` | Ranked symbol search with file filter and top_k (max 50) |
| `blast` | Blast radius — forward/backward/both, depth 1-10 |
| `context` | Full source + structural neighborhood |
| `status` | Index stats, project root, database size, active search mode |
| `index` | (Re)index the project on demand |

```bash
graphiq-mcp /path/to/project
```

Manual configuration for any MCP client:

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

## Supported Languages

34 languages detected, 14 with dedicated TreeSitter parsers for symbol extraction:

**Full parsing:** TypeScript, TSX, JSX, JavaScript, Rust, Python, Go, Java, C, C++, Ruby, YAML, TOML, JSON, HTML, CSS

**File tracking + FTS:** Kotlin, Swift, C#, PHP, Lua, Dart, Scala, Haskell, Elixir, Zig, GraphQL, Protobuf, Shell, SQL, Markdown, XML, SCSS

## Documentation

- [docs/retrieval.md](docs/retrieval.md) — Retrieval pipeline, SEC, NG scoring, holographic name gate, geometric search, deformation, query family routing
- [docs/benchmarks.md](docs/benchmarks.md) — Full benchmark results (v3 + v4), 12 methods, H@1-10, P@10, R@10, per-category breakdowns
- [docs/research.md](docs/research.md) — Experimental history, 21 phases of research, lessons learned, what didn't work
- [ROADMAP.md](docs/ROADMAP.md) — Current state and next steps

## Architecture

Single-file SQLite database at `.graphiq/graphiq.db`. Rust, edition 2021. No runtime dependencies beyond the OS.

```
graphiq/
  crates/
    graphiq-core/       # Core library (search, cruncher, spectral, self_model, trace, query_family)
    graphiq-cli/        # CLI binary
    graphiq-mcp/        # MCP server binary
    graphiq-bench/      # Benchmark binary (12 methods including CARE fusion)
```

### Key Components

| Component | File | Purpose |
|---|---|---|
| SearchEngine | `search.rs` | Main search entry point, query family dispatch, artifact negotiation |
| CruncherIndex | `cruncher.rs` | GooV5 search, holographic encoding, SEC scoring |
| SpectralIndex | `spectral.rs` | Chebyshev heat diffusion, channel fingerprints |
| QueryFamily | `query_family.rs` | 8-family query classifier, retrieval policy generation |
| RepoSelfModel | `self_model.rs` | Deterministic concept nodes for abstract queries |
| RetrievalTrace | `trace.rs` | Proof-carrying search results for `why` tool |

## License

MIT
