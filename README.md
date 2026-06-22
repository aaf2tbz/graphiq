# GraphIQ

**Local code intelligence for agents and developers.** GraphIQ indexes a repository into a structural graph of symbols, calls, imports, constants, type flow, and error surfaces — then searches it with ranked retrieval that understands how code is connected, not just what strings it contains. No embeddings, no LLM, no network calls. Everything lives in a single SQLite file.

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/assets/graphiq-logo-dark.png">
    <source media="(prefers-color-scheme: light)" srcset="docs/assets/graphiq-logo-light.png">
    <img src="docs/assets/graphiq-logo-light.png" alt="GraphIQ" width="132">
  </picture>
</p>

<p align="center">
  <a href="https://github.com/aaf2tbz/graphiq/releases"><img src="https://img.shields.io/github/v/release/aaf2tbz/graphiq?include_prereleases&style=for-the-badge" alt="GitHub release"></a>
  <a href="https://github.com/aaf2tbz/graphiq/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg?style=for-the-badge" alt="MIT License"></a>
  <a href="docs/benchmarks.md"><img src="https://img.shields.io/badge/NDCG%4010%20%2B48%25%20%7C%20MRR%4010%20%2B128%25-vs%20grep-black?style=for-the-badge" alt="Benchmark signal"></a>
  <a href="https://github.com/aaf2tbz/graphiq/actions/workflows/linux-smoke.yml"><img src="https://img.shields.io/github/actions/workflow/status/aaf2tbz/graphiq/linux-smoke.yml?branch=main&label=linux&style=for-the-badge" alt="Linux smoke"></a>
</p>

---

## Table of Contents

**Start here (gather insight before installing):**
- [What GraphIQ Does](#what-graphiq-does) — the one-paragraph pitch + concrete example
- [Why It's Better Than grep](#why-its-better-than-grep) — the structural advantage
- [How It Works](#how-it-works) — the pipeline at a glance
- [Performance & Benchmarks](#performance--benchmarks) — measured quality vs grep
- [What Gets Indexed](#what-gets-indexed) — languages, symbol types, edges

**Install & use:**
- [Install](#install) — Homebrew, install script, or source
- [Quickstart](#quickstart) — index, search, wire into a harness
- [CLI Reference](#cli-reference) — every command
- [MCP Tools](#mcp-tools) — every tool the agent sees

**Go deeper (linked docs):**
- [📖 How GraphIQ works](docs/how-graphiq-works.md) — full architecture, the 5-stage search pipeline
- [📊 Benchmarks](docs/benchmarks.md) — methodology + per-codebase NDCG/MRR results
- [🔌 Supported harnesses](docs/supported-harnesses.md) — which agents GraphIQ integrates with and how
- [🧩 Signet plugin](docs/signet-plugin.md) — the managed plugin for Signet users
- [🔬 Research notes](docs/research.md) — 29 phases of experimentation and lessons
- [🛣️ Hardening roadmap](docs/hardening-roadmap.md) — reliability work, past and planned

**Reference:**
- [Reliability & Resource Control](#reliability--resource-control) — CPU caps, dormancy, data-file handling
- [Desktop App](#desktop-app) — visual index browser
- [Development](#development) — building, testing, contributing
- [Agent Skill](#agent-skill) · [Uninstall](#uninstall) · [License](#license)

---

## What GraphIQ Does

Substring search finds matching text. GraphIQ finds **related code**.

Ask for `rate limit middleware` and GraphIQ ranks `rateLimitMiddleware`, then connects it to `TokenBucket`, `ThrottleConfig`, `checkRateLimit`, the imported constants, the callers, and nearby files — because the graph carries the *context* with the match. The result is local, explainable search that's particularly effective for agents, who need to understand how a codebase fits together.

```bash
$ graphiq search "rate limit middleware"
#1  src/middleware/rate_limit.rs:42   method::check_rate_limit
#2  src/middleware/rate_limit.rs:7    struct::RateLimiter
#3  src/config/throttle.rs:88         struct::ThrottleConfig
```

### Why It's Better Than grep

grep answers "where does this string appear?" GraphIQ answers "where does this *concept* live, and what's connected to it?"

| Question | grep | GraphIQ |
|---|---|---|
| "where is `RateLimiter` defined?" | ✅ exact text | ✅ exact text |
| "how does rate limiting work?" | ❌ no keywords to grep | ✅ ranks the functions + callers + config |
| "what calls `authenticate`?" | ❌ only with `grep -r authenticate` | ✅ blast radius, one command |
| "what breaks if I change this?" | ❌ can't | ✅ `impact` against the diff |

GraphIQ doesn't replace grep for raw text search — it's a layer above it, ranking grep candidates with structural evidence so you get *meaningful* results first.

---

## How It Works

```text
query
  → query family router (8 families)
  → BM25 lexical seeds (FTS5)
  → graph expansion through calls / imports / constants / neighbors
  → structural aliases and family-specific scoring
  → ranked symbols with source-backed context
```

The core pattern: **BM25 retrieves likely candidates quickly, then the code graph reranks them with structural evidence.** No embeddings, no LLM — the graph (calls, type flow, shared constants, error surfaces) is computed once at index time and reused.

📖 **Full architecture**: [docs/how-graphiq-works.md](docs/how-graphiq-works.md) — the 5-stage pipeline, edge types, scoring weights, and design principles.

---

## Performance & Benchmarks

Measured across 300 queries on three real codebases. Full methodology and per-category breakdowns in [📊 Benchmarks](docs/benchmarks.md).

| Codebase | GraphIQ NDCG@10 | GraphIQ MRR@10 |
|---|---:|---:|
| signetai | 0.286 (+100% vs grep) | 0.450 (+213% vs grep) |
| esbuild | 0.318 (+59% vs grep) | 0.551 (+280% vs grep) |
| tokio | 0.192 (-1% vs grep) | 0.411 (+25% vs grep) |
| **Overall** | **0.265 (+48% vs grep)** | **0.471 (+128% vs grep)** |

- **Index size:** ~6.5 MB for a 20K-symbol codebase.
- **Cold CLI (first run):** ~5–10s (builds the in-memory graph).
- **Warm CLI (cached):** ~50ms.
- **In-process (MCP):** microsecond-scale graph traversal after load.

---

## What Gets Indexed

**16 languages parsed** into symbols: TypeScript, TSX, JavaScript, JSX, Rust, Python, Go, Java, C, C++, Ruby, YAML, TOML, JSON, HTML, CSS.

**20+ more tracked at file level:** Kotlin, Swift, C#, PHP, Lua, Dart, Scala, Haskell, Elixir, Zig, GraphQL, Protobuf, Shell, SQL, Markdown, XML, SCSS, CMake, Dockerfile, Makefile, Meson.

| Layer | Examples |
|---|---|
| **Symbols** | functions, methods, classes, interfaces, traits, structs, enums |
| **Structure** | calls, imports, references, containment, type flow, constants |
| **Context** | comments, signatures, file paths, sibling symbols, error surfaces |
| **Maintenance** | dead code, blast radius, topology, index health |

> Dependency lockfiles and other generated data files (`package-lock.json`, `Cargo.lock`, `pnpm-lock.yaml`, `yarn.lock`, `go.sum`, and oversized JSON/YAML/TOML blobs) are **file-tracked** for freshness but **never symbol-extracted** — so they can't silently dominate the graph with thousands of low-value keys. Wipe an index any time with `graphiq clear`.

---

## Install

### Homebrew

```bash
brew tap aaf2tbz/graphiq
brew install graphiq
```

### Install Script (macOS & Linux)

```bash
curl -fsSL https://raw.githubusercontent.com/aaf2tbz/graphiq/main/install.sh | bash
```

The script downloads a prebuilt binary for your platform, verifies the SHA-256 checksum, and places `graphiq` + `graphiq-mcp` on your PATH. On Linux it detects a missing Vulkan loader and offers to install one (optional — GraphIQ runs on CPU without it).

### From Source

```bash
git clone https://github.com/aaf2tbz/graphiq.git
cd graphiq
cargo build --release
```

**Requirements:** Rust 1.75+ (stable), a C compiler for the tree-sitter grammar builds, and `pkg-config`. SQLite is bundled. For GPU acceleration add `--features gpu` (requires a Vulkan-capable GPU + loader).

### Platform Support

| Platform | Status | Notes |
|---|---|---|
| **macOS** (Apple Silicon) | ✅ Primary | prebuilt release |
| **macOS** (Intel) | ✅ Supported | prebuilt release |
| **Linux** (x86_64) | ✅ Supported | prebuilt release; [smoke-tested in CI](https://github.com/aaf2tbz/graphiq/actions/workflows/linux-smoke.yml) with zero Vulkan installed |
| **Linux** (aarch64) | ✅ Built | prebuilt release; built + RPATH-asserted in CI |
| **Windows** | ⚠️ Builds | not prebuilt; `which`/path checks fall back to `where` |

---

## Quickstart

```bash
# 1. Index a project (creates .graphiq/graphiq.db)
graphiq index /path/to/project

# 2. Search it
graphiq search "rate limit middleware"
graphiq search "rateLimitMiddleware"      # exact symbol
graphiq context rateLimitMiddleware        # read source + neighborhood
graphiq blast rateLimitMiddleware          # what depends on it?

# 3. Wire GraphIQ into every installed agent harness
graphiq setup --project /path/to/project
graphiq sync                               # verify they're attached
```

`setup` configures MCP servers with **temp-backed indexes** by default (so large indexes don't land in your checkout). Use `setup --persistent` when you explicitly want a project-local `.graphiq` database.

**Supported harnesses:** Claude Code, Claude Desktop, OpenCode, Codex CLI, Cursor, Windsurf, Gemini CLI, Hermes Agent, Aider, and Pi. See [🔌 Supported harnesses](docs/supported-harnesses.md) for the integration model of each.

---

## CLI Reference

| Command | What it does |
|---|---|
| `index <path>` | Index a project (incremental by content hash) |
| `search <query>` | Ranked symbol search (name, NL, file path, or error message) |
| `context <symbol>` | Read a symbol's source + structural neighborhood |
| `blast <symbol>` | Trace change impact (forward/backward radius) |
| `impact --project <p>` | Analyze current git changes for affected symbols |
| `briefing [--compact]` | Get oriented: architecture, subsystems, public API, hubs |
| `interrogate` | Ask structural questions (entry points, error boundaries, coupling) |
| `topology <symbol>` | Map the local graph neighborhood |
| `constants [query]` | Find shared numeric/string constants |
| `dead-code` | Find unreachable symbols |
| `status` / `doctor` / `upgrade-index` | Index health, diagnosis, rebuild artifacts |
| `clear` | Delete the index, leave a fresh empty database |
| `setup` | Wire graphiq into agent harnesses |
| `sync [--apply]` | Verify harness attach + write the registry; `--apply` re-configures missing ones |
| `discover` | Scan the system for installed agent harnesses |
| `git` | Current project scope: branch, working tree, recent commits |
| `projects` | List tracked projects + their index health |

`graphiq --help` shows every flag. The full command guide for agents is in [the skill](skills/graphiq/SKILL.md) and the [AGENTS.md template](crates/graphiq-cli/AGENTS.md.template).

---

## MCP Tools

Run the MCP server against a project:

```bash
graphiq-mcp /path/to/project --watch
```

The server auto-binds supported provider workspace roots, lazily builds its index on first search, and detects/recreates corrupted or wrong-project databases automatically.

| Tool | Use |
|---|---|
| `briefing` | Project overview and starting context |
| `search` | Ranked symbol search with structural scoring |
| `context` | Source plus graph neighborhood for a symbol |
| `blast` | Forward or backward impact radius |
| `impact` | Git diff impact report |
| `interrogate` | Structural architecture questions |
| `topology` | Local graph and subsystem structure |
| `constants` | Numeric and string constant lookup |
| `explain` / `why` | Why a symbol ranks where it does |
| `status` / `doctor` / `upgrade_index` | Index health, diagnosis, rebuild |
| `index` / `clear` | Full reindex / wipe to empty |

Tool names are **unprefixed** (`search`, `context`, …) when used natively. Through Signet they get the `signet_code_` prefix.

---

## Reliability & Resource Control

GraphIQ is designed to run unattended in the background without pegging your machine or corrupting state.

**CPU / load control** — the long-running MCP daemon honors thread caps:
- `GRAPHIQ_MAX_THREADS=<n>` — max CPU threads for indexing/search (highest priority)
- `RAYON_NUM_THREADS=<n>` — same effect, standard rayon variable
- In **session/background mode** (`--ephemeral` or `--session-id`) the default caps to 4 threads, so a harness-spawned daemon won't peg every core. Foreground indexing uses all cores.

**Dormancy** — background indexers stop when the session/harness closes. The Signet session hook kills any still-running indexer; the pi extension SIGTERMs its tracked child. Graphiq does no work while no harness/project is active.

**Incremental & crash-safe** — indexing is content-hash incremental (only changed files reparse), and a 0-symbol index degrades gracefully to "No results" instead of crashing.

**SIGPIPE-correct** — graphiq behaves like classic Unix tools: piping to `head`/`grep -q` terminates it cleanly via SIGPIPE (exit 141), never a panic.

**Background indexing profile:**
```bash
GRAPHIQ_INDEX_MODE=background graphiq index /path/to/project  # lower-memory mode
GRAPHIQ_SOURCE_TERM_LIMIT=1200 graphiq index ...              # tune the source window
```

---

## Desktop App

A visual index browser, topology view, connector status, and project manager.

```bash
cd apps/desktop
npm install
npm run dev      # development
npm run package  # build installers → apps/desktop/release/
```

The desktop app follows the active Signet session workspace when Signet writes `$SIGNET_WORKSPACE/.daemon/graphiq/state.json`. Active session indexes are sorted first and marked in the project selector.

---

## Development

```bash
cargo fmt
cargo test --workspace                    # 256 tests
cargo run -p graphiq-cli -- index .
cargo run -p graphiq-cli -- search "query family router"
cargo build --workspace --release --features gpu   # with GPU acceleration
```

**Workspace layout:**

| Crate | Role |
|---|---|
| `graphiq-core` | Indexing, search, and analysis engine |
| `graphiq-cli` | Command-line interface + `graphiq` binary |
| `graphiq-mcp` | MCP server (stdio) + `graphiq-mcp` binary |
| `graphiq-bench` | Benchmark harness |
| `apps/desktop` | Electron desktop shell |
| `signet-plugin/` | Signet managed-plugin manifest + hooks |
| `integrations/pi/` | pi extension (no MCP — shells out to the CLI) |

**CI:** `linux-smoke.yml` builds on ubuntu with `--features gpu`, strips all Vulkan, and proves the binaries start + run the full command surface on CPU. `release.yml` cross-builds macOS + Linux releases with checksums.

Contributing guide: [docs/contributing.md](docs/contributing.md).

---

## Agent Skill

GraphIQ ships as an installable agent skill so AI agents automatically know when and how to use each command:

```bash
npx skills add aaf2tbz/graphiq
```

---

## Uninstall

```bash
curl -fsSL https://raw.githubusercontent.com/aaf2tbz/graphiq/main/install.sh | bash -s -- uninstall
```

This removes `graphiq`, `graphiq-mcp`, and `graphiq-bench`. Project-local `.graphiq/` indexes are left in place (delete them manually if desired).

---

## License

MIT — see [LICENSE](LICENSE).
