# GraphIQ

Instant, accurate code search powered by BM25 + structural graph analysis. No embeddings. No model dependencies. Drop a codebase in and search.

**0.827 MRR on esbuild, 0.681 on signetai, 0.511 on tokio** — 30-query benchmark, 46K symbols, ~1ms p50 latency.

## How It Works

```
Query: "rate limit middleware"
        |
        v
  BM25/FTS  -->  30 seeds
        |
        v
  Graph Walk  -->  ~100 candidates (IDF-gated, depth 2)
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

Five layers, all deterministic. BM25 retrieves seeds, the code graph expands candidates, structural scoring reranks, holographic matching adds name similarity signal when it's confident, and the confidence lock prevents wrong promotions. See [docs/retrieval.md](docs/retrieval.md) for the full pipeline details.

## Benchmarks

30-query MRR across 3 codebases:

| Codebase | BM25 | **GraphIQ** | Delta |
|---|---|---|---|
| esbuild (Go) | 0.675 | **0.827** | +0.152 |
| signetai (TS) | 0.556 | **0.681** | +0.125 |
| tokio (Rust) | 0.583 | **0.511** | -0.072 |

46K symbols total. GraphIQ beats BM25 on 2 of 3 codebases. Tokio remains hard — generic function names (`run`, `handle`, `poll`) make structural signals unreliable.

Full results including NDCG@10, accuracy, and per-query breakdowns: [docs/benchmarks.md](docs/benchmarks.md)

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
- `graphiq` — CLI (index, search, blast, status, reindex, demo, setup)
- `graphiq-mcp` — MCP server for LLM integration (stdio JSON-RPC)
- `graphiq-bench` — NDCG/MRR benchmarking
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

### `graphiq demo`

Generates a sample project, indexes it, runs showcase queries. Zero setup.

### Other commands

```bash
graphiq index /path/to/project     # index a project
graphiq reindex /path/to/project   # reindex
graphiq status                      # show index stats
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

- [docs/retrieval.md](docs/retrieval.md) — Retrieval pipeline, SEC, NG scoring, holographic name gate
- [docs/benchmarks.md](docs/benchmarks.md) — Full benchmark results and methodology
- [docs/research.md](docs/research.md) — Experimental history and lessons learned
- [ROADMAP.md](ROADMAP.md) — Current state and next steps

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
