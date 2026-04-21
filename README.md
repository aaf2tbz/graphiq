# GraphIQ

Local code intelligence. Index a codebase into a structural graph, then search it with ranked retrieval that understands calls, imports, type flow, and architectural roles — not just string matching.

No embeddings. No LLM. No network. Everything runs in a single SQLite file.

## How It Compares to Grep

Grep (BM25 `LIKE %term%` over all symbol names and source lines) is a strong baseline for exact lookups. GraphIQ wraps BM25 as layer 1, then adds graph structure on top. Benchmarked on 3 codebases (TypeScript, Rust, Go), 25 queries each for MRR, 20 each for NDCG:

| Metric | Grep | GraphIQ | Delta |
|---|---|---|---|
| MRR@10 | 0.911 | 0.896 | -1.6% |
| NDCG@10 | 0.291 | 0.319 | +10% |

Per-category NDCG@10 (3-codebase average):

| Category | Grep | GraphIQ |
|---|---|---|
| symbol-exact | 0.953 | 0.845 |
| symbol-partial | 0.718 | 0.603 |
| nl-descriptive | 0.061 | 0.137 |
| nl-abstract | 0.032 | 0.238 |
| error-debug | 0.158 | 0.171 |
| file-path | 0.062 | 0.065 |
| cross-cutting | 0.000 | 0.098 |

GraphIQ wins 5 of 7 categories. Grep retains an edge on exact-name lookups. See [docs/benchmarks.md](docs/benchmarks.md) for per-codebase breakdowns and methodology.

## Pipeline

```
Query -> Query Family Router (8 families)
      -> Seed Generation: BM25 FTS5 -> name lookup -> graph walk -> numeric bridges (~100 candidates)
      -> Scoring: IDF coverage + name match + graph walk evidence
      -> Confidence Fusion: BM25 lock -> kind boosts -> file diversity -> top_k results
```

No spectral diffusion. No holographic matching. No predictive models. BM25 retrieves, graph walk expands, scoring ranks.

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

Each family routes to tuned seed expansion strategies. No stacking, no fusion — one path per query.

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
GRAPHIQ_DB=/tmp/graphiq.db graphiq index /path/to/project
graphiq reindex /path/to/project
graphiq status
graphiq doctor
graphiq upgrade-index
graphiq constants
graphiq subsystems --roles
graphiq roles --top 30

graphiq setup --project /path/to/project
graphiq demo
```

`--debug` on search prints per-result score breakdowns, active search mode, and query family. `GRAPHIQ_DB` overrides the database path for all CLI commands.

## MCP Server

`graphiq-mcp` speaks JSON-RPC 2.0 over stdio. 13 tools:

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
graphiq-mcp /path/to/project --db /custom/path/graphiq.db
GRAPHIQ_DB=/custom/graphiq.db graphiq-mcp /path/to/project
```

The MCP server lazily indexes — it starts immediately and only builds the CruncherIndex when you call `search` (or explicitly `index`). If the database is empty, all query tools return an error directing you to index first. The CruncherIndex builds in ~1 second from SQLite.

On startup, the server resolves the database path in this order:
1. `--db` flag (absolute or relative to cwd)
2. `GRAPHIQ_DB` environment variable
3. `.graphiq/graphiq.db` inside the project root
4. Auto-discovery of nested indexes (e.g. monorepo with a single child index)

The project root is stored in the DB at index time. If the server is given a different root than what was indexed, it uses the stored root automatically. Corrupted databases are detected and recreated on startup.

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
  graphiq.db                SQLite (symbols, edges, FTS5 index, meta table)
  manifest.json             artifact freshness tracking
  cache/                    precomputed artifacts (zstd-compressed)
    cruncher.bin.zst        adjacency lists, term sets, IDF
```

Cache size for a ~20K symbol codebase: ~6.5MB. `graphiq reindex` and `graphiq upgrade-index` invalidate automatically.

## Search Speed

| Mode | Time |
|---|---|
| Cold CLI search (first run) | ~5-10s |
| Warm CLI search (cached) | ~50ms |
| In-process query (MCP/bench) | ~18us |

## Source Layout

```
graphiq/
  crates/
    graphiq-core/
      seeds.rs          seed generation (BM25, name lookup, graph walk, bridges)
      scoring.rs        scoring (IDF coverage + name match + walk evidence)
      pipeline.rs       unified_search()
      search.rs         search engine, query dispatch
      query_family.rs   8-family query classifier
      cruncher.rs       adjacency lists, term sets, IDF
      deep_graph.rs     type flow, error type, data shape edges
      blast.rs          blast radius computation
      index.rs          indexing pipeline, TreeSitter parsing
      tokenize.rs       identifier decomposition, term extraction
      cache.rs          in-process LRU cache (neighborhoods, results, blast)
      briefing.rs       project briefing generation
      db.rs             SQLite schema and queries
      languages/        14 TreeSitter grammars
    graphiq-cli/        CLI binary
    graphiq-mcp/        MCP server binary
    graphiq-bench/      benchmark binary
```

## Documentation

- [docs/how-seed-generation-works.md](docs/how-seed-generation-works.md)
- [docs/how-scoring-works.md](docs/how-scoring-works.md)
- [docs/benchmarks.md](docs/benchmarks.md) — full results and methodology
- [docs/retrieval.md](docs/retrieval.md) — pipeline details
- [docs/research.md](docs/research.md) — experimental log
- [docs/ROADMAP-V2.md](docs/ROADMAP-V2.md) — v2 simplification roadmap

## License

MIT
