# GraphIQ

Structural code intelligence for AI agents. Drop a codebase in and get accurate, ranked answers — not just symbol names, but structural facts about what each symbol does, what connects to it, and where it sits in the architecture.

No embeddings. No LLM. No model dependencies. Pure math, physics, and graph theory — spectral heat diffusion on the code graph, holographic name matching, predictive surprise scoring. Everything runs locally in a single SQLite file.

## Why GraphIQ Instead of Grep

Grep is the strongest possible naive baseline — it searches every symbol name and every line of source code with `LIKE %term%`. It's brutally effective for exact lookups and substring matching. But grep has a fundamental limitation: **it has no understanding of code structure**. It doesn't know what calls what, what imports what, or which symbols are structurally important.

GraphIQ preserves grep's lexical strengths (BM25 full-text search is Layer 1 of the pipeline) and adds structural analysis on top. The result:

| Codebase | Language | Symbols | MRR@10 GraphIQ | MRR@10 Grep | Win |
|---|---|---|---|---|---|
| signetai | TypeScript | 20,870 | **0.404** | 0.154 | 2.6x |
| tokio | Rust | 17,867 | **0.667** | 0.360 | 1.9x |
| esbuild | Go | 12,040 | **0.475** | 0.173 | 2.7x |
| flask | Python | 1,971 | **0.615** | 0.523 | 1.2x |
| junit5 | Java | 34,273 | **0.420** | 0.159 | 2.6x |

**GraphIQ wins first-hit accuracy (MRR) on all 5 codebases, 1.2-2.7x over grep.** This is the metric that matters for agents — when an AI asks "how does this codebase handle rate limiting?", it scans the top 3 results and picks one. Getting the right answer at position 1 instead of position 7 is the difference between a useful response and a hallucination.

NDCG (ranking quality across all relevant results) is a split — GraphIQ wins on 3/5 codebases. The losses are on tokio (generic function names like `run`, `handle`, `poll` where grep's raw substring matching is harder to beat) and flask (small codebase where the structural advantage is minimal). Full results: [docs/benchmarks.md](docs/benchmarks.md).

GraphIQ is also **1,300-11,700x faster** on warm cache. Grep's `LIKE %term%` does a full table scan on every symbol's name and source for each query term. GraphIQ uses BM25's inverted index (O(1) lookup) plus pre-computed graph structures:

| Codebase | Symbols | G IQ MRR | Grep MRR | G IQ median | Grep median | Speedup |
|---|---|---|---|---|---|---|
| signetai | 21K | **0.247** | 0.025 | **18us** | 124ms | 6,900x |
| tokio | 18K | **0.558** | 0.186 | **13us** | 79ms | 6,100x |
| esbuild | 12K | **0.150** | 0.100 | **19us** | 94ms | 4,900x |
| flask | 2K | **0.646** | 0.557 | **7us** | 9ms | 1,300x |
| junit5 | 34K | **0.445** | 0.084 | **16us** | 187ms | 11,700x |

### What "beats grep" actually means

Grep returns a flat list of matching lines. GraphIQ returns ranked symbols with structural context:

```
graphiq search "rate limit middleware" →

  1. RateLimiter                          score: 0.94
     callers: setupMiddleware, chain.execute
     callees: checkLimit, TokenBucket.consume
     role: [hub]

  2. TokenBucket                          score: 0.71
     callers: RateLimiter.checkLimit
     callees: Clock.now
     role: [connector]
```

The agent doesn't just get a name. It gets a fact about the codebase — what the symbol is connected to, what role it plays, and how relevant it is to the query. That's the product difference.

## How It Works

GraphIQ is a 6-layer retrieval pipeline. Every layer is deterministic — no neural networks, no learned weights, no GPU required.

```
Query: "how does the timer wheel expire deadlines"
                    |
                    v
         Query Family Router
         (classifies into 8 families)
                    |
                    v
         Layer 1: BM25/FTS (~5ms)
         identifier-aware text search
         → 30 seed symbols
                    |
                    v
         Layer 2: Spectral Expansion
         Chebyshev heat diffusion on graph Laplacian
         → ~100 candidate symbols
                    |
                    v
         Layer 3: Query Deformation
         predictive surprise + channel capacity + MDL
         → adaptive per-query scoring
                    |
                    v
         Layer 4: SEC + NG Scoring
         structural evidence convolution + non-Gaussianity
         → scored candidates
                    |
                    v
         Layer 5: Holographic Name Gate
         FFT cosine similarity, threshold-gated
         → name-verified ranking
                    |
                    v
         Layer 6: Confidence Fusion
         BM25 lock, kind boosts, diversity
         → top_k results
```

### Layer 1: BM25 Full-Text Search

The foundation. SQLite FTS5 with per-column weights — symbol names get 10x weight, identifier decomposition gets 8x, source code gets 1x. Returns top 30 seeds in ~5ms.

The secret weapon is the **hints column**: at index time, GraphIQ infers behavioral role tags (validator, cache, handler, retry, auth-gate, etc.) and structural motifs (connector, orchestrator, hub, guard) from symbol names, call patterns, and file paths. These get written into FTS so BM25 matches role vocabulary at zero query-time cost. A function named `ensureFreshness` gets hints like "cache validate check verify" — so "validate cache entry" finds it even though the name has nothing in common.

**This is what grep already does well.** GraphIQ doesn't replace it — grep is Layer 1. The difference is what happens next.

### Layer 2: Spectral Heat Diffusion

BM25 seeds are expanded through the code graph using Chebyshev polynomial approximation of the graph Laplacian's heat kernel. Heat propagates from seed symbols across structural edges — calls, imports, type flow, shared error types, shared data shapes — so symbols structurally connected to the seeds get discovered even if their names share no terms with the query.

The graph Laplacian L = D^(-1/2)(D - W)D^(-1/2) captures connectivity. The heat kernel e^(-tL) propagates signal from seed nodes. Direct computation needs eigendecomposition (O(n^3)). Chebyshev approximation computes it in O(K|E|) per query where K=15 is the polynomial order. Only one parameter matters — heat_t and walk_weight are remarkably insensitive (673 combinations tested on esbuild).

**This is what grep can't do.** Grep only finds symbols whose names or source contain query terms. If the answer to "how does the timer wheel process deadlines" is a function named `advance_clock` that calls `fire_timer`, grep won't find it unless the query contains "advance" or "clock" or "fire" or "timer". Heat diffusion finds it because `advance_clock` is structurally adjacent to `process_expired_timers` (which BM25 did find).

### Layer 3: Query Deformation

Three adaptive signals that reshape scoring based on each query's structural context:

**Predictive surprise** — For each symbol, a conditional term model built from its 1-hop graph neighborhood measures how surprising the query is given the symbol's context (KL divergence). High surprise = the query's terms are unexpected in this neighborhood, suggesting a novel, relevant match. Disambiguates short generic words like "cache" or "channel" that BM25 can't distinguish.

**Channel capacity routing** — Symbols have structural roles (orchestrator, library, boundary, worker, isolate) computed from edge-type distributions. The scoring weights adapt: orchestrator symbols get more structural coverage weight (they call many things), library symbols get more BM25 weight (they're self-contained).

**MDL explanation sets** — A greedy set cover tracks which query terms each result explains, stopping when marginal information gain drops below threshold. Diversity bonus for spanning multiple structural roles.

### Layer 4: Structural Evidence Scoring

SEC propagates query terms through 7 structural channels (self, calls_out, calls_in, 2-hop out, 2-hop in, return type, file path) with distance-based decay. Non-Gaussianity scoring boosts symbols where query terms concentrate in specific channels over symbols with flat distributions.

### Layer 5: Holographic Name Gate

FFT-based circular convolution encodes symbol identifiers as holographic vectors. The cosine similarity between query and candidate name holograms has 6.8x separation between correct and incorrect matches — a strong signal. But it's **gated**: only candidates with similarity > 0.25 receive any boost. On codebases with descriptive names (esbuild), correct matches pass the gate easily. On codebases with generic names (tokio), they don't — and the gate adapts without any codebase-specific tuning.

### Layer 6: Confidence Fusion

Final ranking applies BM25 confidence lock (when BM25's rank-1 has a >1.2x gap, lock it — demoting confident BM25 results is almost always wrong), kind boosts (functions and types over variables and imports), and per-file diversity limits.

### The Query Family Router

Before any of the above, the query is classified into one of 8 families. Each family routes to the best retrieval method for that query type. No fusion, no stacking — one method per query:

| Family | Detection | Search Method | Example |
|---|---|---|---|
| SymbolExact | Exact name, PascalCase | GooV5 (holographic name) | `RateLimiter` |
| SymbolPartial | Short fragment | GooV5 | `rate limit` |
| NaturalDescriptive | Action verbs | Geometric (heat diffusion) | `encode a value in VLQ` |
| NaturalAbstract | "how does", "what controls" | Deformed (max exploration) | `how does auth work` |
| ErrorDebug | Panic/error/timeout | Deformed (predictive model) | `timeout in channel send` |
| CrossCuttingSet | "all", "every", plural | Deformed (high diversity) | `all connector implementations` |
| Relationship | "vs", "relationship" | Geometric (neighborhood) | `AsyncFd vs readiness guard` |
| FilePath | Paths, extensions | Geometric (file-adjacent) | `scheduler/worker.rs` |

The classifier inverts the typical cascade: instead of trying to detect NL patterns and defaulting to symbol, it detects code-shaped tokens and defaults everything else to NaturalDescriptive. This prevents 65% of queries from falling through to a wrong default.

### The Code Graph

GraphIQ builds a rich structural graph during indexing. Beyond calls and imports:

| Edge Type | Signal | Example |
|---|---|---|
| Calls | Function calls | `authenticate()` calls `hashPassword()` |
| Imports | Module imports | `use tokio::sync::Mutex` |
| Contains | Scope containment | `RateLimiter` struct contains `check()` method |
| SharesType | Shared type tokens in signatures | Two functions both take `Arc<Mutex<Bar>>` |
| SharesErrorType | Shared error parameters | Functions that both return `Result<T, io::Error>` |
| SharesDataShape | Shared field access patterns | Functions that both access `self.config.host` |
| SharesConstant | Shared numeric/string literals | Functions that both reference `429` or `"timeout"` |
| StringLiteral | Shared error-related string constants | Functions containing `"connection refused"` |
| CommentRef | Symbol mentions in comments | `// delegates to processExpiredTimers` |

These edges feed into the spectral diffusion, predictive model, and evidence scoring. The result is a graph where heat diffusion can find symbols that are behaviorally related even when their names share no terms.

## Installation

### Homebrew (macOS + Linux)

```bash
brew tap aaf2tbz/graphiq
brew install graphiq
```

Installs four binaries:
- `graphiq` — CLI (index, search, blast, status, reindex, demo, setup)
- `graphiq-mcp` — MCP server for LLM integration (stdio JSON-RPC)
- `graphiq-bench` — NDCG/MRR benchmarking and parameter tuning
- `graphiq-locomo` — LoCoMo-style benchmarking

### From Source

```bash
git clone https://github.com/aaf2tbz/graphiq.git
cd graphiq
cargo build --release
```

Requires Rust 1.70+. No other dependencies — no Python, no Node, no system libraries. The binary is statically linked.

### Quick Start

```bash
# Zero setup — generates a sample project and shows GraphIQ vs BM25
graphiq demo

# Index a real project and configure MCP integrations
graphiq setup --project /path/to/project
```

`graphiq setup` detects your installed harnesses, writes MCP server configs, indexes the project, and reports the active search mode.

## CLI

### Search

```bash
graphiq search "rate limit middleware"
graphiq search "authenticateUser" --top 20 --file src/auth/
graphiq search "error handler" --debug
```

`--debug` prints per-result score breakdowns and the active search mode.

### Blast Radius

Compute what a symbol affects (forward) and what depends on it (backward):

```bash
graphiq blast RateLimiter
graphiq blast RateLimiter --depth 5 --direction forward
graphiq blast RateLimiter --direction both
```

### Indexing

```bash
graphiq index /path/to/project              # fresh index
graphiq reindex /path/to/project            # reindex existing
graphiq status                               # index stats, active mode, artifact freshness
graphiq doctor                               # diagnose index issues
graphiq upgrade-index                        # rebuild stale artifacts
```

### Setup

One-command onboarding for MCP integration:

```bash
graphiq setup                          # current directory
graphiq setup --project /path/to/proj  # specify project
graphiq setup --skip-index             # configure without indexing
```

## MCP Server

`graphiq-mcp` speaks JSON-RPC 2.0 over stdio. Exposes five tools to AI agents:

| Tool | Description |
|---|---|
| `search` | Ranked symbol search with file filter and top_k (max 50) |
| `blast` | Blast radius — forward/backward/both, depth 1-10 |
| `context` | Full source + structural neighborhood (callers, callees, members) |
| `status` | Index stats, project root, database size, active search mode |
| `index` | (Re)index the project on demand |

```bash
graphiq-mcp /path/to/project
```

### Supported Harnesses

`graphiq setup` auto-configures these harnesses:

| Harness | Config Location | Status |
|---|---|---|
| opencode | `~/.config/opencode/opencode.json` | Auto-detected |
| Claude Desktop | `~/Library/Application Support/Claude/claude_desktop_config.json` | Auto-detected |
| Codex | `~/.codex/config.toml` | Auto-detected |
| Hermes | `~/.hermes/config.yaml` | Auto-detected |

Manual configuration for any MCP-compatible client:

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

34 languages detected. 16 with dedicated TreeSitter parsers for full symbol extraction (functions, classes, methods, structs, enums, traits, interfaces):

**Full parsing (symbols + structure):** TypeScript, TSX, JSX, JavaScript, Rust, Python, Go, Java, C, C++, Ruby, YAML, TOML, JSON, HTML, CSS

**File tracking + FTS:** Kotlin, Swift, C#, PHP, Lua, Dart, Scala, Haskell, Elixir, Zig, GraphQL, Protobuf, Shell, SQL, Markdown, XML, SCSS

Languages with full parsing get the complete pipeline: structural graph edges, spectral diffusion, predictive model, holographic matching. Languages with file tracking get BM25 full-text search with path-aware ranking.

## Architecture

Single-file SQLite database at `.graphiq/graphiq.db`. Rust, edition 2021. Zero runtime dependencies beyond the OS. No Python, no Node, no shared libraries. The binary is self-contained.

```
graphiq/
  crates/
    graphiq-core/       # Core library
    graphiq-cli/        # CLI binary
    graphiq-mcp/        # MCP server binary
    graphiq-bench/      # Benchmark binary
```

### Key Components

| Component | Source | Purpose |
|---|---|---|
| SearchEngine | `search.rs` | Main entry point, query family dispatch, seed expansion, artifact negotiation |
| CruncherIndex | `cruncher.rs` | GooV5 search, holographic encoding, SEC scoring, evidence convolution |
| SpectralIndex | `spectral.rs` | Chebyshev heat diffusion, Ricci curvature, channel fingerprints |
| QueryFamily | `query_family.rs` | 8-family query classifier, retrieval policy generation |
| DeepGraph | `deep_graph.rs` | Type flow, error type, data shape, string literal, comment ref edges |
| RepoSelfModel | `self_model.rs` | Deterministic concept nodes for abstract queries |
| EdgeEvidence | `edge_evidence.rs` | Edge profile classification (direct, structural, reinforcing, boundary) |
| PredictiveModel | Built at index time | Conditional term models for surprise scoring, 5K-term vocabulary |

### Storage Layout

```
.graphiq/graphiq.db
  ├── symbols          # name, kind, signature, source, file, line, importance
  ├── edges            # source, target, kind (calls, imports, contains, shares_*, ...)
  ├── files            # path, language, symbol_count
  ├── edges_fts        # FTS5 index on symbols (name, decomposed, hints, sig, source)
  ├── manifest.json    # artifact freshness tracking
  └── [computed at query time]
      ├── spectral     # Chebyshev heat diffusion infrastructure
      ├── cruncher     # GooV5 adjacency lists, term sets, IDF
      ├── predictive   # Conditional term models per symbol
      └── fingerprints # Channel fingerprint vectors per symbol
```

### Operating System Support

| OS | Status | Notes |
|---|---|---|
| macOS (x86_64, aarch64) | Fully supported | Homebrew and source |
| Linux (x86_64) | Fully supported | Homebrew and source |
| Linux (aarch64) | Source only | Requires Rust toolchain |

## Benchmarks

See [docs/benchmarks.md](docs/benchmarks.md) for full results including per-category breakdowns, deep graph edge counts, and router performance analysis.

### NDCG@10 (ranking quality, 20 queries per codebase)

| Codebase | GraphIQ | Grep | Delta |
|---|---|---|---|
| signetai (TS) | **0.406** | 0.343 | +18% |
| tokio (Rust) | 0.205 | **0.326** | -37% |
| esbuild (Go) | **0.411** | 0.277 | +48% |
| flask (Python) | 0.426 | **0.432** | -1% |
| junit5 (Java) | **0.198** | 0.181 | +9% |

### MRR@10 (first-hit accuracy, 20 queries per codebase)

| Codebase | GraphIQ | Grep | Delta |
|---|---|---|---|
| signetai (TS) | **0.404** | 0.154 | +162% |
| tokio (Rust) | **0.667** | 0.360 | +85% |
| esbuild (Go) | **0.475** | 0.173 | +175% |
| flask (Python) | **0.615** | 0.523 | +18% |
| junit5 (Java) | **0.420** | 0.159 | +164% |

## Documentation

- [docs/retrieval.md](docs/retrieval.md) — Full pipeline details: SEC, NG scoring, holographic name gate, heat diffusion, query deformation
- [docs/benchmarks.md](docs/benchmarks.md) — Complete results: 5 codebases, per-category NDCG, deep graph edge stats, router analysis
- [docs/research.md](docs/research.md) — Experimental history: 22 phases of research, lessons learned, what didn't work
- [docs/ROADMAP.md](docs/ROADMAP.md) — Current state and next steps

## License

MIT
