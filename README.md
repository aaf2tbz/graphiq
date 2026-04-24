<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/graphiq-logo-dark.png">
  <source media="(prefers-color-scheme: light)" srcset="docs/assets/graphiq-logo-light.png">
  <img src="docs/assets/graphiq-logo-light.png" alt="GraphIQ" width="120">
</picture>

# G R A P H I Q

**Local code search that understands how your code is connected**

<a href="https://github.com/aaf2tbz/graphiq/releases"><img src="https://img.shields.io/github/v/release/aaf2tbz/graphiq?include_prereleases&style=for-the-badge" alt="GitHub release"></a>
<a href="https://github.com/aaf2tbz/graphiq/blob/main/LICENSE"><img src="https://img.shields.io/badge/License-MIT-blue.svg?style=for-the-badge" alt="MIT License"></a>
<a href="https://github.com/aaf2tbz/homebrew-graphiq"><img src="https://img.shields.io/badge/Homebrew-Install-green?style=for-the-badge&logo=homebrew" alt="Homebrew"></a>
<a href="docs/benchmarks.md"><img src="https://img.shields.io/badge/NDCG%4010%20%2B48%25%20%7C%20MRR%4010%20%2B128%25%20-%20vs%20grep-black?style=for-the-badge" alt="NDCG@10 +48% | MRR@10 +128% vs grep"></a>

<strong style="color:#58a6ff">+48% NDCG@10</strong>, <strong style="color:#58a6ff">+128% MRR@10</strong> vs grep across 300 benchmark queries<br>
<strong style="color:#f0883e">Structural graph indexing</strong> · zero network · single SQLite file · ~18μs query latency

[Docs](docs/how-graphiq-works.md) · [Benchmarks](docs/benchmarks.md) · [Research](docs/research.md) · [Discussions](https://github.com/aaf2tbz/graphiq/discussions)

</div>

---

> **Substring search finds what you typed. <span style="color:#f0883e">Graph search finds what you meant.</span>**

GraphIQ is a local code search engine that indexes your codebase into a
structural graph — calls, imports, type flow, error surfaces, shared
constants — then searches that graph with ranked retrieval instead of
plain substring matching.

You ask <strong style="color:#a5d6ff">"rate limit middleware"</strong> and it finds `rateLimitMiddleware`
through name decomposition, then walks the graph to discover
`TokenBucket`, `ThrottleConfig`, and `checkRateLimit` even though none
of those names contain "middleware."

Everything runs <strong style="color:#3fb950">locally</strong>. No embeddings, no LLM, no network requests. A
single SQLite file (~6.5MB for 20K symbols). <strong style="color:#3fb950">~18μs query latency</strong> from an
MCP server.

## Install

**Homebrew (macOS & Linux):**

```bash
brew tap aaf2tbz/graphiq
brew install graphiq
```

**One-line install script (macOS & Linux):**

```bash
curl -fsSL https://raw.githubusercontent.com/aaf2tbz/graphiq/main/install.sh | bash
```

**Uninstall:**

```bash
curl -fsSL https://raw.githubusercontent.com/aaf2tbz/graphiq/main/install.sh | bash -s -- uninstall
```

**From source:**

```bash
git clone https://github.com/aaf2tbz/graphiq.git
cd graphiq
cargo build --release
```

Installs three binaries: `graphiq` (CLI), `graphiq-mcp` (MCP server), `graphiq-bench` (benchmarking).

## Quick start

```bash
graphiq index /path/to/project
graphiq search "rate limit middleware"
```

### First proof of value

Index a project, then try a natural language search:

```bash
graphiq index /path/to/project
graphiq search "encode a value in VLQ"
```

Compare with grep — GraphIQ finds `encodeVLQ` through identifier
decomposition and graph expansion, even if the exact term "VLQ" doesn't
appear in the function name.

### Multi-agent setup

Configure GraphIQ for your editor or agent in one command:

```bash
graphiq setup --project /path/to/project
```

Use `graphiq setup --harness cursor` to configure a specific harness only.

## Benchmarks (v3.1 — 300 queries, 3 codebases)

| Codebase | Grep NDCG@10 | <span style="color:#58a6ff">GraphIQ NDCG@10</span> | Grep MRR@10 | <span style="color:#58a6ff">GraphIQ MRR@10</span> |
|---|---|---|---|---|
| signetai (TypeScript, 23K syms) | 0.143 | **<span style="color:#3fb950">0.286</span> (+100%)** | 0.144 | **<span style="color:#3fb950">0.450</span> (+213%)** |
| esbuild (Go, 12K syms) | 0.200 | **<span style="color:#3fb950">0.318</span> (+59%)** | 0.145 | **<span style="color:#3fb950">0.551</span> (+280%)** |
| tokio (Rust, 18K syms) | **0.193** | 0.192 (-1%) | 0.330 | **<span style="color:#3fb950">0.411</span> (+25%)** |
| **Overall** | **0.179** | **<span style="color:#58a6ff">0.265</span> (+48%)** | **0.206** | **<span style="color:#58a6ff">0.471</span> (+128%)** |

| Query type | vs Grep | Why |
|---|---|---|
| <span style="color:#f0883e">**Relationships**</span> ("what calls RateLimiter") | **<span style="color:#3fb950">3.9x</span>** | Graph walk finds structurally connected symbols no substring search can discover |
| <span style="color:#f0883e">**Natural language**</span> ("encode a value in VLQ") | **<span style="color:#3fb950">2.0x</span>** | Identifier decomposition + per-family signal routing |
| <span style="color:#f0883e">**Error/debug**</span> ("timeout in channel send") | **<span style="color:#3fb950">1.2x</span>** | Error-type edge routing + shared constant discovery |
| **Symbol exact** ("authenticateUser") | ~tied | BM25 is already excellent for exact name lookups |
| **Abstract NL** ("how does auth work") | ~tied | Requires semantic understanding beyond structural graph signals |

Full methodology and per-codebase breakdowns in [docs/benchmarks.md](docs/benchmarks.md).

## Core capabilities

| Core | What it does |
|---|---|
| <strong style="color:#f0883e">Structural graph indexing</strong> | Calls, imports, type flow, error surfaces, shared constants, comment references |
| <strong style="color:#58a6ff">8-family query routing</strong> | Symbol lookup, NL description, error debug, relationship, etc. — each with its own scoring profile |
| <strong style="color:#58a6ff">Graph walk expansion</strong> | Seed results expand through structural edges to discover related symbols grep cannot reach |
| <strong style="color:#f0883e">Blast radius analysis</strong> | Forward/backward impact tracing with configurable depth (1–10) |
| <strong style="color:#3fb950">Zero network</strong> | No embeddings, no LLM, no API calls. Everything runs in a single SQLite file |
| <strong style="color:#3fb950">14 MCP tools</strong> | Full editor/agent integration — search, blast, context, interrogate, topology, explain, and more |
| <strong style="color:#f0883e">Dead code detection</strong> | Find unreachable symbols with zero-caller analysis and 8 exemption rules |
| <strong style="color:#f0883e">File watcher</strong> | Auto-reindex on file changes with `--watch` flag on MCP server |
| <strong style="color:#3fb950">Multi-agent setup</strong> | One-command config for Claude Code, Cursor, Windsurf, Codex, Gemini CLI, and more |
| <strong style="color:#3fb950">Signet integration</strong> | Works with Signet (local memory for agents) to combine graph-aware code search with persistent agent context |

## MCP Server

`graphiq-mcp` exposes <strong style="color:#58a6ff">14 tools</strong> over JSON-RPC 2.0 (stdio) for editor and agent integration:

| Tool | Purpose |
|---|---|
| <span style="color:#58a6ff">`briefing`</span> | Project overview — start here |
| <span style="color:#58a6ff">`search`</span> | Ranked symbol search with file filter and top_k |
| <span style="color:#f0883e">`blast`</span> | Change impact analysis (forward/backward/both, depth 1–10) |
| <span style="color:#58a6ff">`context`</span> | Full source + structural neighborhood |
| `why` | Explain why a result ranked where it did |
| `interrogate` | Deep structural interrogation of a symbol |
| `topology` | Code topology around a symbol |
| `explain` | Natural language symbol explanation |
| `status` | Index stats and health |
| <span style="color:#3fb950">`index`</span> | (Re)index the project |
| `doctor` | Artifact health check |
| `upgrade_index` | Rebuild stale artifacts |
| `constants` | Numeric/string constant lookup |
| <span style="color:#f0883e">`dead_code`</span> | Find unreachable symbols grouped by file |

```bash
graphiq-mcp /path/to/project
graphiq-mcp /path/to/project --watch   # auto-reindex on file changes
```

The server lazily builds its index on first search (~1s from SQLite). Corrupted databases are detected and recreated automatically.

### Supported Harnesses

| Harness | Config | Setup |
|---|---|---|
| Claude Code | `.claude/.mcp.json` | `graphiq setup` |
| Claude Desktop | `~/Library/Application Support/Claude/claude_desktop_config.json` | `graphiq setup` |
| OpenCode | `~/.config/opencode/opencode.json` | `graphiq setup` |
| Codex CLI | `~/.codex/config.toml` | `graphiq setup` |
| Cursor | `.cursor/mcp.json` | `graphiq setup` |
| Windsurf | `.windsurf/mcp.json` | `graphiq setup` |
| Gemini CLI | `~/.gemini/settings.json` | `graphiq setup` |
| Hermes Agent | `~/.hermes/config.yaml` | `graphiq setup` |
| Aider | `.aider.conf.yml` | `graphiq setup` |

Use `graphiq setup --harness <name>` to configure a specific harness only.

## How It Works

```text
Query
  → Query Family Router (8 families)
  → Seed Generation (BM25 FTS5 → per-term expansion → graph walk → numeric bridges)
  → Scoring (IDF coverage + name overlap + neighbor fingerprints + specificity scaling + structural aliases)
  → Ranked results
```

The pipeline classifies every query into one of <strong style="color:#58a6ff">8 families</strong> (symbol lookup, NL description, error debug, relationship, etc.), then routes it through family-specific scoring parameters. Symbol lookups trust BM25. NL queries expand through the graph. Relationship queries lean into structural adjacency. Each family gets its own walk depth, expansion strategy, and signal weights.

Graph edges capture calls, imports, type flow, shared error types, shared constants, and comment references. The full architecture is documented in [How GraphIQ works](docs/how-graphiq-works.md).

## Languages

<strong style="color:#3fb950">**Full parsing (16 variants):**</strong> TypeScript, TSX, JavaScript, JSX, Rust, Python, Go, Java, C, C++, Ruby, YAML, TOML, JSON, HTML, CSS

**File tracking (20+):** Kotlin, Swift, C#, PHP, Lua, Dart, Scala, Haskell, Elixir, Zig, GraphQL, Protobuf, Shell, SQL, Markdown, XML, SCSS, CMake, Dockerfile, Makefile, Meson

## Performance

| Mode | Latency |
|---|---|
| Cold CLI (first run) | ~5–10s |
| Warm CLI (cached) | ~50ms |
| In-process (MCP) | <strong style="color:#3fb950">~18μs</strong> |

Index size for a ~20K symbol codebase: ~6.5MB.

## Documentation

- [How GraphIQ works](docs/how-graphiq-works.md) — full system explanation
- [Benchmarks](docs/benchmarks.md) — methodology and results
- [Research notes](docs/research.md) — experimental history

## Development

```bash
git clone https://github.com/aaf2tbz/graphiq.git
cd graphiq
cargo build --release
cargo test
```

```bash
cargo bench          # Run benchmarks
graphiq index .      # Index GraphIQ on itself
graphiq search "query family router"
```

Requirements: Rust 1.75+, macOS or Linux.

## License

MIT.

---

[GitHub](https://github.com/aaf2tbz/graphiq) ·
[Homebrew](https://github.com/aaf2tbz/homebrew-graphiq) ·
[crates.io](https://crates.io/crates/graphiq) ·
[discussions](https://github.com/aaf2tbz/graphiq/discussions) ·
[issues](https://github.com/aaf2tbz/graphiq/issues)
