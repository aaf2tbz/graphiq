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

<table align="center">
<tr>
<td width="54%" valign="top" align="center">

<h2 align="center">Local code search with structural memory</h2>

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
<td width="46%" valign="top" align="center">

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

<h2 align="center">⚡ Start in 20 seconds</h2>

```bash
graphiq index /path/to/project
graphiq search "rate limit middleware"
```

<p align="center">Or wire it into an editor/agent harness:</p>

```bash
graphiq setup --project /path/to/project
```

<p align="center">
Use <code>graphiq setup --harness cursor</code> or any supported harness name to target one integration.
</p>

<h2 align="center">📦 Install</h2>

<p><strong>Homebrew</strong></p>

```bash
brew tap aaf2tbz/graphiq
brew install graphiq
```

<p><strong>Install script</strong></p>

```bash
curl -fsSL https://raw.githubusercontent.com/aaf2tbz/graphiq/main/install.sh | bash
```

<p><strong>From source</strong></p>

```bash
git clone https://github.com/aaf2tbz/graphiq.git
cd graphiq
cargo build --release
```

<p align="center">Installs <code>graphiq</code>, <code>graphiq-mcp</code>, and <code>graphiq-bench</code>.</p>

<h2 align="center">Why it works</h2>

<table align="center">
<tr>
<td width="33%" valign="top" align="center">

<h3><small>🎯 [01]</small><br>Lexical precision</h3>

BM25 FTS5 handles exact symbol names, identifiers, file paths, and
decomposed camelCase/snake_case terms.

</td>
<td width="33%" valign="top" align="center">

<h3><small>🕸️ [02]</small><br>Graph recall</h3>

Seed results expand through calls, imports, constants, type edges, error
surfaces, and local neighborhoods.

</td>
<td width="33%" valign="top" align="center">

<h3><small>🧭 [03]</small><br>Query routing</h3>

Eight query families tune scoring for symbols, natural language,
relationships, errors, files, constants, and architecture questions.

</td>
</tr>
</table>

<table align="center">
<tr>
<td align="left">
<pre>
BM25 name match  +  graph walk  +  structural aliases  +  family weights
       |                |                  |                    |
       +----------------+------------------+--------------------+
                                |
                         stable ranked results
</pre>
</td>
</tr>
</table>

<p align="center">
The result is a compact local index with the codebase's structure baked in, so agents can search by intent without shipping your source to a remote embedding service.
</p>

<h2 align="center">📈 Benchmark Signal</h2>

<p align="center">
Current v3.1 benchmarks cover 300 queries across signetai, esbuild, and tokio. Full methodology lives in <a href="docs/benchmarks.md">docs/benchmarks.md</a>.
</p>

<table align="center" width="100%">
<tr>
<th align="center">Codebase</th>
<th align="center">Grep NDCG@10</th>
<th align="center">GraphIQ NDCG@10</th>
<th align="center">Grep MRR@10</th>
<th align="center">GraphIQ MRR@10</th>
</tr>
<tr><td align="center">signetai</td><td align="center">0.143</td><td align="center"><strong>0.286</strong> (+100%)</td><td align="center">0.144</td><td align="center"><strong>0.450</strong> (+213%)</td></tr>
<tr><td align="center">esbuild</td><td align="center">0.200</td><td align="center"><strong>0.318</strong> (+59%)</td><td align="center">0.145</td><td align="center"><strong>0.551</strong> (+280%)</td></tr>
<tr><td align="center">tokio</td><td align="center"><strong>0.193</strong></td><td align="center">0.192 (-1%)</td><td align="center">0.330</td><td align="center"><strong>0.411</strong> (+25%)</td></tr>
<tr><td align="center"><strong>Overall</strong></td><td align="center"><strong>0.179</strong></td><td align="center"><strong>0.265</strong> (+48%)</td><td align="center"><strong>0.206</strong></td><td align="center"><strong>0.471</strong> (+128%)</td></tr>
</table>

<table align="center" width="100%">
<tr>
<th align="center">Query shape</th>
<th align="center">Result vs grep</th>
<th align="center">Signal</th>
</tr>
<tr><td align="center">Relationship queries</td><td align="center"><strong>3.9x</strong></td><td align="center">Graph traversal finds connected symbols substring search misses</td></tr>
<tr><td align="center">Natural language queries</td><td align="center"><strong>2.0x</strong></td><td align="center">Identifier decomposition plus family-aware scoring</td></tr>
<tr><td align="center">Error/debug queries</td><td align="center"><strong>1.2x</strong></td><td align="center">Error surfaces and shared constants become searchable structure</td></tr>
<tr><td align="center">Exact symbol queries</td><td align="center">tied</td><td align="center">BM25 is already excellent when names are known</td></tr>
</table>

<h2 align="center">🛠️ Agent Tools</h2>

<p align="center"><code>graphiq-mcp</code> exposes 14 JSON-RPC tools over stdio:</p>

<h3 align="center">Tools</h3>

<table align="center" width="100%">
<tr><th align="center">Tool</th><th align="center">Use it for</th></tr>
<tr><td align="center"><code>briefing</code></td><td align="center">Project overview and starting context</td></tr>
<tr><td align="center"><code>search</code></td><td align="center">Ranked symbol search with filters</td></tr>
<tr><td align="center"><code>context</code></td><td align="center">Source plus structural neighborhood</td></tr>
<tr><td align="center"><code>blast</code></td><td align="center">Forward/backward impact analysis</td></tr>
<tr><td align="center"><code>interrogate</code></td><td align="center">Deep symbol inspection</td></tr>
<tr><td align="center"><code>topology</code></td><td align="center">Local code topology</td></tr>
<tr><td align="center"><code>why</code></td><td align="center">Ranking explanation</td></tr>
<tr><td align="center"><code>explain</code></td><td align="center">Natural-language symbol explanation</td></tr>
<tr><td align="center"><code>dead_code</code></td><td align="center">Unreachable symbols grouped by file</td></tr>
<tr><td align="center"><code>constants</code></td><td align="center">Numeric/string constant lookup</td></tr>
<tr><td align="center"><code>status</code></td><td align="center">Index stats and health</td></tr>
<tr><td align="center"><code>doctor</code></td><td align="center">Artifact validation</td></tr>
<tr><td align="center"><code>index</code></td><td align="center">Manual reindex</td></tr>
<tr><td align="center"><code>upgrade_index</code></td><td align="center">Rebuild stale artifacts</td></tr>
</table>

<h3 align="center">Supported Harnesses</h3>

<p align="center">
<code>Claude Code</code> · <code>Claude Desktop</code> · <code>OpenCode</code> · <code>Codex CLI</code> · <code>Cursor</code> · <code>Windsurf</code> · <code>Gemini CLI</code> · <code>Hermes Agent</code> · <code>Aider</code>
</p>

<h3 align="center">Config / Setup</h3>

<p><strong>Configure all supported harnesses</strong></p>

```bash
graphiq setup --project /path/to/project
```

<p><strong>Configure one harness only</strong></p>

```bash
graphiq setup --harness cursor
```

<p><strong>Run the MCP server with file watching</strong></p>

```bash
graphiq-mcp /path/to/project --watch
```

<p align="center">
The MCP server lazily builds its in-memory index on first search and detects/recreates corrupted databases automatically.
</p>

<h2 align="center">🗂️ What gets indexed</h2>

<table align="center">
<tr><th align="center">Layer</th><th align="center">Examples</th></tr>
<tr><td align="center">Symbols</td><td align="center">functions, methods, classes, interfaces, traits, structs, enums, modules</td></tr>
<tr><td align="center">Structure</td><td align="center">calls, imports, type flow, references, constants, containment</td></tr>
<tr><td align="center">Context</td><td align="center">comments, signatures, file paths, sibling symbols, error surfaces</td></tr>
<tr><td align="center">Maintenance</td><td align="center">dead code, blast radius, topology, index health</td></tr>
</table>

<h2 align="center">🧩 System Shape</h2>

```text
Query
  → Query Family Router (8 families)
  → Seed Generation (BM25 FTS5 → per-term expansion → graph walk → numeric bridges)
  → Scoring (IDF coverage + name overlap + neighbor fingerprints + specificity scaling + structural aliases)
  → Ranked results
```

<p align="center">
The full architecture is documented in <a href="docs/how-graphiq-works.md">How GraphIQ works</a>.
</p>

<h2 align="center">🌐 Languages</h2>

<p align="center">
<strong>Full parsing:</strong> TypeScript, TSX, JavaScript, JSX, Rust, Python, Go, Java, C, C++, Ruby, YAML, TOML, JSON, HTML, CSS
</p>

<p align="center">
<strong>File tracking:</strong> Kotlin, Swift, C#, PHP, Lua, Dart, Scala, Haskell, Elixir, Zig, GraphQL, Protobuf, Shell, SQL, Markdown, XML, SCSS, CMake, Dockerfile, Makefile, Meson
</p>

<h2 align="center">⚙️ Performance</h2>

<table align="center">
<tr><th align="center">Mode</th><th align="center">Latency</th></tr>
<tr><td align="center">Cold CLI (first run)</td><td align="center">~5–10s</td></tr>
<tr><td align="center">Warm CLI (cached)</td><td align="center">~50ms</td></tr>
<tr><td align="center">In-process (MCP)</td><td align="center"><strong style="color:#3fb950">~18μs</strong></td></tr>
</table>

<p align="center">Index size for a ~20K symbol codebase: ~6.5MB.</p>

<h2 align="center">📚 Docs</h2>

<p align="center">
<a href="docs/how-graphiq-works.md">How GraphIQ works</a> ·
<a href="docs/benchmarks.md">Benchmarks</a> ·
<a href="docs/research.md">Research notes</a>
</p>

<h2 align="center">🧪 Development</h2>

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

<p align="center">Requirements: Rust 1.75+, macOS or Linux.</p>

<h2 align="center">🔌 Agent Skill</h2>

<p align="center">
GraphIQ is available as an installable agent skill on <a href="https://skills.sh/aaf2tbz/graphiq/graphiq"><strong>skills.sh</strong></a>.
</p>

```bash
npx skills add aaf2tbz/graphiq
```

<p align="center">
Installs the GraphIQ skill into Claude Code, Cursor, OpenCode, Codex, Windsurf, and <a href="https://github.com/vercel-labs/skills#supported-agents">40+ other agents</a>.
</p>

<h2 align="center">🧼 Uninstall</h2>

```bash
curl -fsSL https://raw.githubusercontent.com/aaf2tbz/graphiq/main/install.sh | bash -s -- uninstall
```

<h2 align="center">License</h2>

<p align="center">MIT.</p>

---

<p align="center">
[GitHub](https://github.com/aaf2tbz/graphiq) ·
[Homebrew](https://github.com/aaf2tbz/homebrew-graphiq) ·
[skills.sh](https://skills.sh/aaf2tbz/graphiq/graphiq) ·
[crates.io](https://crates.io/crates/graphiq) ·
[discussions](https://github.com/aaf2tbz/graphiq/discussions) ·
[issues](https://github.com/aaf2tbz/graphiq/issues)
</p>
