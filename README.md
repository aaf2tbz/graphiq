# GraphIQ

Instant, accurate code search powered by BM25 + structural graph analysis + spectral heat diffusion. No embeddings. No model dependencies. Drop a codebase in and search.

**GooberV5: NDCG 0.504 (esbuild), 0.444 (signetai), 0.367 (tokio) — Geometric: NDCG 0.503, 0.443, 0.368**

## How It Works

```
Query: "rate limit middleware"
        |
        v
  BM25/FTS  -->  30 seeds
        |
        v
  [V5] Graph Walk (BFS, depth 2, IDF-gated)
  [Geo] Chebyshev Heat Diffusion (spectral graph)
        |
        v
  SEC + NG Scoring  -->  structural rerank
        |
        v
  Holographic Name Gate  -->  confidence-filtered boost
        |
        v
  Confidence Lock  -->  top_k results
```

Two retrieval modes. GooberV5 uses BFS graph walks to expand candidates. Geometric uses Chebyshev polynomial approximation of the graph Laplacian's heat kernel — diffusion-based expansion that naturally propagates relevance across structural distance. Both feed into the same SEC + negentropy + holographic scoring pipeline. See [docs/retrieval.md](docs/retrieval.md) for full pipeline details.

## Benchmarks

NDCG@10 across 3 codebases (v3 queries, 7 categories):

| Codebase | BM25 | GooV4 | GooV5 | **Geometric** |
|---|---|---|---|---|
| esbuild (Go) | 0.315 | 0.383 | **0.504** | 0.503 |
| signetai (TS) | 0.334 | 0.388 | **0.444** | 0.443 |
| tokio (Rust) | 0.249 | 0.246 | 0.367 | **0.368** |

MRR across 3 codebases (v3 queries, disjoint from NDCG):

| Codebase | BM25 | GooV4 | GooV5 | **Geometric** |
|---|---|---|---|---|
| esbuild | 0.624 | 0.652 | 0.669 | **0.676** |
| signetai | 0.843 | 0.810 | **0.924** | 0.924 |
| tokio | 0.627 | 0.560 | **0.637** | 0.636 |

Full results including per-category and per-query breakdowns: [docs/benchmarks.md](docs/benchmarks.md)

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

`--debug` prints per-result score breakdowns.

### `graphiq blast`

Compute blast radius — what a symbol affects and what depends on it.

```bash
graphiq blast RateLimiter
graphiq blast RateLimiter --depth 5 --direction forward
```

### `graphiq spectral`

Compute the spectral embedding (Laplacian eigendecomposition + Chebyshev heat kernel infrastructure). Required for the Geometric search mode.

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
graphiq status                      # show index stats
```

### `graphiq-bench`

Benchmark and tune retrieval methods.

```bash
# Run full benchmark suite
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
| `status` | Index stats, project root, database size |
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

- [docs/retrieval.md](docs/retrieval.md) — Retrieval pipeline, SEC, NG scoring, holographic name gate, geometric search
- [docs/benchmarks.md](docs/benchmarks.md) — Full benchmark results and methodology
- [docs/research.md](docs/research.md) — Experimental history and lessons learned
- [ROADMAP.md](docs/ROADMAP.md) — Current state and next steps

## Architecture

Single-file SQLite database at `.graphiq/graphiq.db`. Rust, edition 2021. No runtime dependencies beyond the OS.

```
graphiq/
  crates/
    graphiq-core/       # Core library
    graphiq-cli/        # CLI binary
    graphiq-mcp/        # MCP server binary
    graphiq-bench/      # Benchmark binary
```

## License

MIT
