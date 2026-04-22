# GraphIQ

GraphIQ is a local code search engine that understands how your code is connected. It indexes your codebase into a structural graph — calls, imports, type flow, error surfaces, shared constants — then searches that graph with ranked retrieval instead of plain substring matching. You ask "rate limit middleware" and it finds `rateLimitMiddleware` through name decomposition, then walks the graph to discover `TokenBucket`, `ThrottleConfig`, and `checkRateLimit` even though none of those names contain "middleware."

Everything runs locally. No embeddings, no LLM, no network requests. A single SQLite file (~6.5MB for 20K symbols). ~18μs query latency from an MCP server.

### Benchmarks (v3.1 — 50 queries per codebase, 3 codebases, 300 total)

| Codebase | Grep NDCG@10 | GraphIQ NDCG@10 | Grep MRR@10 | GraphIQ MRR@10 |
|---|---|---|---|---|
| signetai (TypeScript, 23K syms) | 0.143 | **0.286** (+100%) | 0.144 | **0.450** (+213%) |
| esbuild (Go, 12K syms) | 0.200 | **0.318** (+59%) | 0.145 | **0.551** (+280%) |
| tokio (Rust, 18K syms) | **0.193** | 0.192 (-1%) | 0.330 | **0.411** (+25%) |
| **Overall** | **0.179** | **0.265** (+48%) | **0.206** | **0.471** (+128%) |

| Query type | vs Grep | Why |
|---|---|---|
| **Relationships** ("what calls RateLimiter") | **3.9x** | Graph walk finds structurally connected symbols no substring search can discover |
| **Natural language** ("encode a value in VLQ") | **2.0x** | Identifier decomposition + per-family signal routing |
| **Error/debug** ("timeout in channel send") | **1.2x** | Error-type edge routing + shared constant discovery |
| **Symbol exact** ("authenticateUser") | ~tied | BM25 is already excellent for exact name lookups |
| **Abstract NL** ("how does auth work") | ~tied | Requires semantic understanding beyond structural graph signals |

Codebases with descriptive names (`convertOKLCHToOKLAB`) see the biggest gains. Codebases with generic names (`run`, `handle`, `poll`) see smaller gains — structural aliases disambiguate collision-prone symbols but generic terms remain hard for IDF to fully resolve.

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
GraphIQ now works with Signet(Local Memory for Agents) to bring together the best of both worlds. 

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
  → Scoring (IDF coverage + name overlap + neighbor fingerprints + specificity scaling + structural aliases)
  → Ranked results
```

The pipeline classifies every query into one of 8 families (symbol lookup, NL description, error debug, relationship, etc.), then routes it through family-specific scoring parameters. Symbol lookups trust BM25. NL queries expand through the graph. Relationship queries lean into structural adjacency. Each family gets its own walk depth, expansion strategy, and signal weights.

Graph edges capture calls, imports, type flow, shared error types, shared constants, and comment references. The full architecture is documented in [How GraphIQ works](docs/how-graphiq-works.md).

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
