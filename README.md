# GraphIQ

Instant, accurate code search powered by structural graph analysis + spectral heat diffusion + per-query routing. No embeddings. No LLM. No model dependencies. Drop a codebase in and search.

**GraphIQ vs Grep (symbol-level LIKE search) — 20 queries per codebase, 5 codebases (TS, Rust, Go, Python, Java):**

| Codebase | NDCG@10 | | MRR@10 | |
|---|---|---|---|---|
| | GraphIQ | Grep | GraphIQ | Grep |
| signetai (TS, 20K syms) | **0.406** | 0.343 | **0.404** | 0.154 |
| tokio (Rust, 17K syms) | 0.205 | **0.326** | **0.667** | 0.360 |
| esbuild (Go, 12K syms) | **0.411** | 0.277 | **0.475** | 0.173 |
| flask (Python, 2K syms) | 0.426 | **0.432** | **0.615** | 0.523 |
| junit5 (Java, 34K syms) | **0.198** | 0.181 | **0.420** | 0.159 |

GraphIQ wins MRR on 5/5 (1.6-2.7x) and NDCG on 3/5. MRR measures first-hit accuracy — the metric that matters for agent recall.

## How It Works

```
Query: "rate limit middleware"
        |
        v
  Query Family Router (8 families)
        |
        v
  BM25/FTS  -->  seeds
        |
        v
  Per-query routing:
    SymbolExact/Partial --> GooberV5
    ErrorDebug/Abstract/CrossCutting --> Deformed
    Descriptive/Relationship/FilePath --> Geometric
        |
        v
  Structural rerank  -->  top_k results
```

The query family router classifies each query into one of 8 families and routes it to the best single retrieval method. No fusion, no stacking — one method per query. Symbol lookups get GooV5 (holographic name matching). NL queries get Geometric or Deformed (spectral heat diffusion with predictive surprise). See [docs/benchmarks.md](docs/benchmarks.md) for full results and [docs/retrieval.md](docs/retrieval.md) for pipeline details.

## Benchmarks

GraphIQ vs Grep — our direct competitor. Grep uses `LIKE %term%` across symbol names and source code (the strongest possible naive symbol search). GraphIQ uses structural graph analysis.

### NDCG@10 (graded relevance, 20 queries per codebase)

| Codebase | GraphIQ | Grep | Delta |
|---|---|---|---|
| signetai (TS) | **0.406** | 0.343 | +18% |
| tokio (Rust) | 0.205 | **0.326** | -37% |
| esbuild (Go) | **0.411** | 0.277 | +48% |
| flask (Python) | 0.426 | **0.432** | -1% |
| junit5 (Java) | **0.198** | 0.181 | +9% |

### MRR@10 (first-hit accuracy, 20 queries per codebase)

| Codebase | GraphIQ | Grep | Delta |
|---|---|---|---|
| signetai (TS) | **0.404** | 0.154 | +162% |
| tokio (Rust) | **0.667** | 0.360 | +85% |
| esbuild (Go) | **0.475** | 0.173 | +175% |
| flask (Python) | **0.615** | 0.523 | +18% |
| junit5 (Java) | **0.420** | 0.159 | +164% |

Full results with per-category breakdowns: [docs/benchmarks.md](docs/benchmarks.md)

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
