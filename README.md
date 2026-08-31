<h1 align="center"> GraphIQ </h1>

<p align="center"> 
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/assets/graphiq-logo-dark.png">
    <source media="(prefers-color-scheme: light)" srcset="docs/assets/graphiq-logo-light.png">
    <img src="docs/assets/graphiq-logo-light.png" alt="GraphIQ" width="132">
  </picture>
</p>

<p align="center">
  <a href="https://github.com/aaf2tbz/graphiq/releases"><img src="https://img.shields.io/github/v/release/aaf2tbz/graphiq?include_prereleases&style=for-the-badge" alt="GitHub release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg?style=for-the-badge" alt="MIT License"></a>
  <a href="https://github.com/aaf2tbz/graphiq/actions/workflows/linux-smoke.yml"><img src="https://img.shields.io/github/actions/workflow/status/aaf2tbz/graphiq/linux-smoke.yml?branch=main&label=linux&style=for-the-badge" alt="Linux smoke"></a>
</p>


<p align="center">
  Local code intelligence for agents and developers.** GraphIQ indexes a repository into a structural graph of symbols,       calls, imports, constants, and type flow, then searches it with ranked retrieval that understands how code is connected—    not just what strings it contains. No embeddings. No LLM. No network calls. Everything lives in a single SQLite file.
</p>

## Quickstart

### Install

```bash
brew tap aaf2tbz/graphiq
brew install graphiq
```

Or install a prebuilt binary on macOS or Linux:

```bash
curl -fsSL https://raw.githubusercontent.com/aaf2tbz/graphiq/main/install.sh | bash
```

### Index and search

```bash
graphiq index /path/to/project
graphiq search "rate limit middleware"
graphiq search "rateLimitMiddleware"      # exact symbol name
graphiq context rateLimitMiddleware        # source + structural neighborhood
graphiq blast rateLimitMiddleware          # what depends on it?
```

The first index walks the project while respecting `.gitignore`, parses supported source files, and writes `.graphiq/graphiq.db`. Re-indexing is incremental: unchanged files are skipped.

### Connect an agent harness

```bash
graphiq setup --project /path/to/project
graphiq sync
```

`setup` configures the MCP server for supported harnesses such as Claude Code, Codex CLI, OpenCode, Cursor, Windsurf, Gemini CLI, Aider, and Pi. Indexes are temporary-backed by default; use `graphiq setup --persistent` for a project-local index.

## How GraphIQ works

GraphIQ has two cooperating phases: **indexing**, which turns source code into a searchable structural model, and **search**, which combines lexical retrieval with bounded graph exploration.

```text
Source files
    │
    ▼
File discovery → Tree-sitter parsing → symbols + structural edges
                                      │
                                      ▼
                         SQLite FTS5 + CruncherIndex
                                      │
Query → family routing → seeds → graph walk → scoring → ranked results
```

### 1. Indexing

#### File discovery and symbol extraction

GraphIQ recursively discovers project files and respects `.gitignore`. Tree-sitter parsers extract functions, methods, classes, structs, enums, interfaces, traits, modules, variables, and imports, along with their names, kinds, signatures, documentation, source, and locations. Symbols are deduplicated by file, name, and starting line.

#### Structural graph construction

The index connects symbols with relationships discovered from syntax and semantic analysis:

| Relationship | What it captures |
|---|---|
| `calls` | A function or method invokes another symbol |
| `references` | A symbol name is referenced from another symbol |
| `imports` | A module or file imports another symbol |
| `contains` | A type or module contains a member |
| `extends` / `implements` | Inheritance and interface boundaries |
| `tests` | A test exercises a subject symbol |
| `shares_type` | Symbols use related type tokens |
| `shares_error_type` | Symbols produce or handle the same error type |
| `shares_data_shape` | Symbols access related fields or data shapes |
| `shares_constant` | Symbols share meaningful literals or constants |
| `comment_ref` | Comments refer to another known symbol |

The result is stored in `.graphiq/graphiq.db`, a SQLite database containing:

- `symbols` — extracted code symbols and their source metadata
- `edges` — weighted relationships between symbols
- `files` — file metadata and content hashes for incremental indexing
- `symbols_fts` — an FTS5 index for lexical retrieval

GraphIQ also builds a compressed `CruncherIndex` containing adjacency lists, term sets, IDF weights, name lookup, neighbor terms, and structural degree. This keeps repeated searches fast without requiring a server or external service.

#### Search hints

During indexing, GraphIQ derives behavioral role vocabulary and structural motifs from names, paths, signatures, and call patterns. Hints such as `cache`, `validate`, `handler`, `retry`, `connector`, and `orchestrator` are added to the FTS index. This lets a descriptive query find relevant code even when the exact query words are absent from a symbol name.

### 2. Search

Every query passes through a family router before retrieval. The family determines which signals are trusted and how much structural expansion is allowed:

| Query family | Typical query | Primary behavior |
|---|---|---|
| Symbol exact | `RateLimiter` | Trust exact lexical matches |
| Symbol partial | `rate_limit` | Match decomposed identifier terms |
| File path | `scheduler/worker.rs` | Prioritize path components |
| Error/debug | `timeout in channel send` | Search error phrases and error relationships |
| Natural descriptive | `encode a value in VLQ` | Combine lexical and structural evidence |
| Natural abstract | `how does authentication work` | Explore related concepts |
| Cross-cutting | `all connector implementations` | Favor coverage and file diversity |
| Relationship | `callers of authenticate` | Follow graph relationships |

#### Seed generation

Search begins with SQLite FTS5 BM25 retrieval over symbol names, decomposed identifiers, qualified names, hints, signatures, paths, documentation, and source. Natural-language and debugging queries can additionally activate:

- per-term expansion with stemming and small synonym sets
- source scanning for distinctive error phrases
- numeric and constant bridges
- graph-aware expansion through type, error, and data-shape relationships

Symbol-like queries stay conservative and rely primarily on exact or partial lexical evidence. This prevents a structurally related but lexically unrelated symbol from displacing a clear name or path match.

#### Graph walk expansion

For query families that benefit from structural context, GraphIQ performs a bounded breadth-first walk from the strongest seeds. It follows incoming and outgoing edges through the in-memory adjacency lists, applies term-coverage and IDF gates, decays evidence by distance, and limits both depth and fan-out. The walk can reveal a nearby implementation, handler, or adapter that BM25 alone would miss without expanding across the entire repository.

#### Scoring and safeguards

Candidates are scored using a family-specific combination of:

- BM25 relevance and query-term coverage
- exact and partial name overlap
- evidence accumulated during graph walks
- terms from one-hop neighbors
- symbol kind, test-file, and structural adjustments
- file diversity and deterministic tie-breaking

Confidence gates keep weak secondary signals from overriding strong lexical matches. Exact symbol matches receive a final promotion, while generic utilities and test-only results are controlled by family-specific policies. Scores and IDs are quantized or ordered deterministically so repeated searches produce stable results.

The MCP `search` tool presents these results as a compact Markdown list by default. Use `format: "detailed"` when source previews and caller/callee lists are needed, then use `context` to read one implementation in full or `blast` to trace its impact.

## Documentation

### Start here

- **[Install](docs/install.md)** — Homebrew, install script, or build from source
- **[Quickstart](docs/quickstart.md)** — index, search, and wire GraphIQ into a harness
- **[How GraphIQ works](docs/how-graphiq-works.md)** — the complete indexing and search pipeline
- **[What gets indexed](docs/indexing.md)** — languages, symbol types, and graph edges

### Using GraphIQ

- **[CLI reference](docs/cli.md)** — every command and flag
- **[MCP tools](docs/mcp.md)** — the tool surface agents see
- **[Supported harnesses](docs/supported-harnesses.md)** — Claude Code, Codex, OpenCode, Cursor, Pi, and more

### Reliability and performance

- **[Reliability and resource control](docs/reliability.md)** — CPU caps, dormancy, and crash safety
- **[Performance](docs/performance.md)** — runtime and storage characteristics

### Going deeper

- **[Signet plugin](docs/signet-plugin.md)** — the managed plugin for Signet users
- **[Research notes](docs/research.md)** — experimental history and design lessons
- **[Hardening roadmap](docs/hardening-roadmap.md)** — reliability work, past and planned
- **[Contributing](docs/contributing.md)** — building, testing, and CI

## License

MIT — see [LICENSE](LICENSE).
