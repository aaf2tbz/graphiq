# GraphIQ

Structural code intelligence. GraphIQ indexes your codebase into a structural graph — calls, imports, type flow, error surfaces — and searches it with ranked retrieval that understands how your code is connected, not just what strings it contains.

No embeddings. No LLM. No network requests. Everything lives in a single SQLite file.

## Benchmarks

Tested against grep (substring search over symbol names and source code) on 3 codebases, 50 queries each for NDCG and MRR (300 total):

| | Grep | GraphIQ |
|---|---|---|
| NDCG@10 | 0.181 | **0.296** (+63%) |
| MRR@10 | 0.243 | **0.428** (+76%) |

GraphIQ wins 5 of 7 query categories. The biggest gains are on relationship queries (3.7x), natural language descriptions (2.9x), and error debugging (1.7x). Grep retains a marginal edge on exact-name lookups.

Full methodology and per-codebase breakdowns in [docs/benchmarks.md](docs/benchmarks.md).

## Install

**Homebrew (macOS, Linux)**

```bash
brew tap aaf2tbz/graphiq
brew install graphiq
```

**From source**

```bash
git clone https://github.com/aaf2tbz/graphiq.git
cd graphiq
cargo build --release
```

Installs three binaries: `graphiq` (CLI), `graphiq-mcp` (MCP server), `graphiq-bench` (benchmarking).

## Quick Start

```bash
# Index a project
graphiq index /path/to/project

# Search
graphiq search "rate limit middleware"
graphiq search "authenticateUser"
graphiq search "how does the auth flow work" --debug

# Blast radius — what does this symbol touch?
graphiq blast RateLimiter
graphiq blast RateLimiter --depth 5 --direction forward

# Diagnostics
graphiq status
graphiq doctor

# Set up MCP integration for your editor/agent
graphiq setup --project /path/to/project
```

`--debug` prints per-result score breakdowns. `GRAPHIQ_DB` overrides the database path.

## MCP Server

`graphiq-mcp` exposes 13 tools over JSON-RPC 2.0 (stdio) for editor and agent integration:

| Tool | Purpose |
|---|---|
| `briefing` | Project overview — start here |
| `search` | Ranked symbol search with file filter and top_k |
| `blast` | Change impact analysis (forward/backward/both, depth 1-10) |
| `context` | Full source + structural neighborhood |
| `why` | Explain why a result ranked where it did |
| `interrogate` | Deep structural interrogation of a symbol |
| `topology` | Code topology around a symbol |
| `explain` | Natural language symbol explanation |
| `status` | Index stats and health |
| `index` | (Re)index the project |
| `doctor` | Artifact health check |
| `upgrade_index` | Rebuild stale artifacts |
| `constants` | Numeric/string constant lookup |

```bash
graphiq-mcp /path/to/project
```

The server lazily builds its index on first search (~1s from SQLite). Corrupted databases are detected and recreated automatically.

### Supported Harnesses

| Harness | Config | Setup |
|---|---|---|
| opencode | `~/.config/opencode/opencode.json` | `graphiq setup` |
| Claude Desktop | `~/Library/Application Support/Claude/claude_desktop_config.json` | `graphiq setup` |
| Codex | `~/.codex/config.toml` | `graphiq setup` |

## How It Works

```
Query
  → Query Family Router (8 families)
  → Seed Generation (BM25 FTS5 → per-term expansion → graph walk → numeric bridges)
  → Scoring (IDF coverage + name overlap + neighbor fingerprints + specificity scaling)
  → Ranked results
```

### Query Families

| Family | Example |
|---|---|
| Symbol (exact/partial) | `RateLimiter`, `rate limit` |
| Natural language | `encode a value in VLQ` |
| Abstract questions | `how does auth work` |
| Error/debug | `timeout in channel send` |
| Cross-cutting sets | `all connector implementations` |
| Relationships | `RateLimiter vs TokenBucket` |
| File paths | `scheduler/worker.rs` |

Each family gets its own scoring configuration — walk depth, expansion strategy, and signal weights are tuned per category.

### Graph Edge Types

| Edge | What it captures |
|---|---|
| Calls | Direct function calls |
| Imports | Module imports |
| Contains | Scope containment (struct → method) |
| SharesType | Matching type tokens in signatures |
| SharesErrorType | Shared error parameters |
| SharesDataShape | Shared field access patterns |
| SharesConstant | Shared numeric/string literals |
| CommentRef | Symbol names mentioned in comments |

## Languages

**Full parsing (16 variants):** TypeScript, TSX, JavaScript, JSX, Rust, Python, Go, Java, C, C++, Ruby, YAML, TOML, JSON, HTML, CSS

**File tracking (20+):** Kotlin, Swift, C#, PHP, Lua, Dart, Scala, Haskell, Elixir, Zig, GraphQL, Protobuf, Shell, SQL, Markdown, XML, SCSS, CMake, Dockerfile, Makefile, Meson

## Performance

| Mode | Latency |
|---|---|
| Cold CLI (first run) | ~5-10s |
| Warm CLI (cached) | ~50ms |
| In-process (MCP) | ~18μs |

Index size for a ~20K symbol codebase: ~6.5MB.

## Documentation

- [How GraphIQ works](docs/how-graphiq-works.md) — full system explanation
- [Benchmarks](docs/benchmarks.md) — methodology and results
- [Research notes](docs/research.md) — experimental history

## License

MIT
