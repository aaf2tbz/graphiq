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

<br>

<table>
<tr>
<td width="54%" valign="top">

<h2>Local code search with structural memory</h2>

GraphIQ turns a repository into a searchable code graph: symbols, files,
calls, imports, type flow, error surfaces, comments, constants, and the
relationships between them.

Substring search finds what you typed. GraphIQ finds what the code is
connected to.

Ask for `"rate limit middleware"` and GraphIQ can land on
<code>rateLimitMiddleware</code>, then follow the graph to
<code>TokenBucket</code>, <code>ThrottleConfig</code>, and
<code>checkRateLimit</code> even when those names do not
share the same words.

</td>
<td width="46%" valign="top">

<pre>
       query
         |
   lexical seeds
         |
   structural graph
    /    |     \
 calls imports constants
    \    |     /
   ranked symbols
</pre>

<code>zero network</code> · <code>single SQLite file</code> · <code>no LLM required</code>

</td>
</tr>
</table>

## Start in 20 seconds

```bash
graphiq index /path/to/project
graphiq search "rate limit middleware"
```

Or wire it into an editor/agent harness:

```bash
graphiq setup --project /path/to/project
```

Use `graphiq setup --harness cursor` or any supported harness name to
target one integration.

## Install

**Homebrew**

```bash
brew tap aaf2tbz/graphiq
brew install graphiq
```

**Install script**

```bash
curl -fsSL https://raw.githubusercontent.com/aaf2tbz/graphiq/main/install.sh | bash
```

**From source**

```bash
git clone https://github.com/aaf2tbz/graphiq.git
cd graphiq
cargo build --release
```

Installs `graphiq`, `graphiq-mcp`, and `graphiq-bench`.

## Why it works

<table>
<tr>
<td width="33%" valign="top">

<h3>01. Lexical precision</h3>

BM25 FTS5 handles exact symbol names, identifiers, file paths, and
decomposed camelCase/snake_case terms.

</td>
<td width="33%" valign="top">

<h3>02. Graph recall</h3>

Seed results expand through calls, imports, constants, type edges, error
surfaces, and local neighborhoods.

</td>
<td width="33%" valign="top">

<h3>03. Query routing</h3>

Eight query families tune scoring for symbols, natural language,
relationships, errors, files, constants, and architecture questions.

</td>
</tr>
</table>

```text
BM25 name match  +  graph walk  +  structural aliases  +  family weights
       |                |                  |                    |
       +----------------+------------------+--------------------+
                                |
                         stable ranked results
```

The result is a compact local index with the codebase's structure baked
in, so agents can search by intent without shipping your source to a
remote embedding service.

## Benchmark Signal

Current v3.1 benchmarks cover 300 queries across signetai, esbuild, and
tokio. Full methodology lives in [docs/benchmarks.md](docs/benchmarks.md).

| Codebase | Grep NDCG@10 | GraphIQ NDCG@10 | Grep MRR@10 | GraphIQ MRR@10 |
|---|---:|---:|---:|---:|
| signetai | 0.143 | **0.286** (+100%) | 0.144 | **0.450** (+213%) |
| esbuild | 0.200 | **0.318** (+59%) | 0.145 | **0.551** (+280%) |
| tokio | **0.193** | 0.192 (-1%) | 0.330 | **0.411** (+25%) |
| **Overall** | **0.179** | **0.265** (+48%) | **0.206** | **0.471** (+128%) |

| Query shape | Result vs grep | Signal |
|---|---:|---|
| Relationship queries | **3.9x** | Graph traversal finds connected symbols substring search misses |
| Natural language queries | **2.0x** | Identifier decomposition plus family-aware scoring |
| Error/debug queries | **1.2x** | Error surfaces and shared constants become searchable structure |
| Exact symbol queries | tied | BM25 is already excellent when names are known |

## Agent Tools

`graphiq-mcp` exposes 14 JSON-RPC tools over stdio:

| Tool | Use it for |
|---|---|
| `briefing` | Project overview and starting context |
| `search` | Ranked symbol search with filters |
| `context` | Source plus structural neighborhood |
| `blast` | Forward/backward impact analysis |
| `interrogate` | Deep symbol inspection |
| `topology` | Local code topology |
| `why` | Ranking explanation |
| `explain` | Natural-language symbol explanation |
| `dead_code` | Unreachable symbols grouped by file |
| `constants` | Numeric/string constant lookup |
| `status` | Index stats and health |
| `doctor` | Artifact validation |
| `index` | Manual reindex |
| `upgrade_index` | Rebuild stale artifacts |

```bash
graphiq-mcp /path/to/project
graphiq-mcp /path/to/project --watch   # auto-reindex on file changes
```

The MCP server lazily builds its in-memory index on first search and
detects/recreates corrupted databases automatically.

### Harnesses

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

## What gets indexed

| Layer | Examples |
|---|---|
| Symbols | functions, methods, classes, interfaces, traits, structs, enums, modules |
| Structure | calls, imports, type flow, references, constants, containment |
| Context | comments, signatures, file paths, sibling symbols, error surfaces |
| Maintenance | dead code, blast radius, topology, index health |

## System Shape

```text
Query
  → Query Family Router (8 families)
  → Seed Generation (BM25 FTS5 → per-term expansion → graph walk → numeric bridges)
  → Scoring (IDF coverage + name overlap + neighbor fingerprints + specificity scaling + structural aliases)
  → Ranked results
```

The full architecture is documented in
[How GraphIQ works](docs/how-graphiq-works.md).

## Languages

**Full parsing:** TypeScript, TSX, JavaScript, JSX, Rust, Python, Go, Java,
C, C++, Ruby, YAML, TOML, JSON, HTML, CSS

**File tracking:** Kotlin, Swift, C#, PHP, Lua, Dart, Scala, Haskell,
Elixir, Zig, GraphQL, Protobuf, Shell, SQL, Markdown, XML, SCSS, CMake,
Dockerfile, Makefile, Meson

## Performance

| Mode | Latency |
|---|---|
| Cold CLI (first run) | ~5–10s |
| Warm CLI (cached) | ~50ms |
| In-process (MCP) | <strong style="color:#3fb950">~18μs</strong> |

Index size for a ~20K symbol codebase: ~6.5MB.

## Docs

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
cargo bench
graphiq index .
graphiq search "query family router"
```

Requirements: Rust 1.75+, macOS or Linux.

## Uninstall

```bash
curl -fsSL https://raw.githubusercontent.com/aaf2tbz/graphiq/main/install.sh | bash -s -- uninstall
```

## License

MIT.

---

[GitHub](https://github.com/aaf2tbz/graphiq) ·
[Homebrew](https://github.com/aaf2tbz/homebrew-graphiq) ·
[crates.io](https://crates.io/crates/graphiq) ·
[discussions](https://github.com/aaf2tbz/graphiq/discussions) ·
[issues](https://github.com/aaf2tbz/graphiq/issues)
