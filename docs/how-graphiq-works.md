# How GraphIQ Works

GraphIQ is a structural code intelligence engine. It indexes your codebase into a graph — calls, imports, type flow, error surfaces — and searches it with ranked retrieval that understands how your code is connected, not just what strings it contains.

No embeddings. No LLM. No network requests. Everything lives in a single SQLite file.

## The Short Version

```
Query
  → Query Family Router (8 families)
  → Seed Generation (BM25 FTS5 → per-term expansion → graph walk → numeric bridges → source scan)
  → Graph Walk Expansion (BFS from seeds through structural edges)
  → Scoring (BM25 + IDF coverage + name overlap + neighbor fingerprints + kind/test adjustments)
  → Post-Processing (BM25 confidence lock + file diversity cap)
  → Ranked results
```

## Architecture

GraphIQ has two major phases: **indexing** (builds the graph and search index) and **search** (queries the graph at interactive speed).

### Storage

Everything is stored in a single SQLite database (`.graphiq/graphiq.db`). The schema has four core tables:

- **`symbols`** — every function, type, variable, and import in the codebase, with name, kind, signature, source code, doc comments, and location
- **`edges`** — structural relationships between symbols (calls, imports, type flow, etc.)
- **`files`** — source files with content hashes for incremental reindexing
- **`symbols_fts`** — an FTS5 virtual table for BM25 full-text search

A secondary artifact (`cruncher.bin.zst`, ~6.5MB for 20K symbols) caches the in-memory search index between runs.

---

## Indexing

Source: `crates/graphiq-core/src/index.rs`

### 1. File Discovery

`walk_project()` recursively discovers source files, respecting `.gitignore` and language detection. 16 languages get full Tree-sitter parsing; 20+ more get file-level tracking.

### 2. Symbol Extraction

Each file is parsed with Tree-sitter. The parser extracts:

- Functions, methods, classes, structs, enums, interfaces, traits, modules
- Variable declarations and imports
- Signatures, doc comments, and source code bodies

Symbols are deduplicated by (file, name, start_line).

### 3. Edge Extraction

Edges connect symbols to each other. GraphIQ builds two categories:

**Structural edges** (from syntax analysis):

| Edge | What it captures | Weight |
|---|---|---|
| `calls` | Direct function calls | 1.0 |
| `references` | Symbol name references | 0.8 |
| `imports` | Module imports | 0.6 |
| `contains` | Scope containment (struct → method) | 0.7 |
| `extends` | Class/trait inheritance | 0.9 |
| `implements` | Interface implementation | 0.9 |
| `tests` | Test-to-subject relationship | 0.3 |

**Deep graph edges** (from semantic analysis):

| Edge | What it captures |
|---|---|
| `shares_type` | Functions sharing type tokens in signatures |
| `shares_error_type` | Functions sharing error-type parameters |
| `shares_data_shape` | Functions accessing same field names |
| `shares_constant` | Functions sharing numeric/string literals |
| `comment_ref` | Comments mentioning other symbol names |

Deep graph edges are computed by `crates/graphiq-core/src/deep_graph.rs`. Type tokens are extracted from signatures (filtering keywords and primitives). Error-type edges connect functions that return or handle the same error types. Data-shape edges connect functions that access the same struct fields. Constant edges link functions sharing the same numeric or string literals. Comment-ref edges are extracted from doc comments and inline comments that name other symbols.

### 4. Hints Column

At index time, GraphIQ infers behavioral role tags and structural motifs from symbol names, call patterns, and file paths:

- **19 role tags**: validator, cache, handler, retry, auth-gate, etc.
- **8 structural motifs**: connector, orchestrator, hub, guard, transform, sink, source, leaf

These get written into the FTS `hints` column so BM25 matches role vocabulary at zero query-time cost. A function named `ensureFreshness` that checks cache validity gets hints like "cache validate check verify" — so the query "validate cache entry" finds it even though the function name doesn't contain any of those words.

### 5. CruncherIndex

After SQLite indexing, GraphIQ builds an in-memory `CruncherIndex` (`crates/graphiq-core/src/cruncher.rs`) containing:

- **Adjacency lists** (`outgoing`, `incoming`) — per-symbol edge lists with weights
- **Term sets** — per-symbol term dictionaries with IDF weights, plus separate name and signature term sets
- **Global IDF** — inverse document frequency for every term across the corpus
- **Name index** — lowercase name → symbol indices for exact name lookup
- **Neighbor terms** — union of decomposed terms from each symbol's 1-hop neighbors (used for neighbor fingerprint scoring)
- **Structural degree** — normalized connectivity score per symbol

The CruncherIndex is serialized to `cruncher.bin.zst` (~6.5MB for 20K symbols) and loaded on subsequent searches.

---

## Search Pipeline

The search pipeline has five stages: query classification, seed generation, graph expansion, scoring, and post-processing.

### Stage 1: Query Family Classification

Source: `crates/graphiq-core/src/query_family.rs`

Every query is classified into one of 8 families:

| Family | Detection | Example |
|---|---|---|
| `SymbolExact` | PascalCase, snake_case, `::` separators | `RateLimiter`, `GraphDb::open` |
| `SymbolPartial` | Single lowercase word without code shape | (falls through to `NaturalDescriptive`) |
| `FilePath` | Path separators, file extensions | `scheduler/worker.rs` |
| `ErrorDebug` | Error/panic/failed/timeout/crash signals | `timeout in channel send` |
| `NaturalDescriptive` | Behavioral descriptions (default NL) | `encode a value in VLQ` |
| `NaturalAbstract` | "how does", "what controls" prefixes | `how does auth work` |
| `CrossCuttingSet` | "all", "every", plural nouns | `all connector implementations` |
| `Relationship` | "what calls", "callers of", "relationship between" | `RateLimiter vs TokenBucket` |

The classifier uses a priority cascade: cross-cutting → error-debug → relationship → file-path → symbol-exact → natural-abstract → natural-descriptive. This ordering prevents misclassification (e.g., "all error types" is cross-cutting, not error-debug).

Each family produces a `RetrievalPolicy` (BM25 lock strength, diversity boost, evidence weight) and a `ScoreConfig` (scoring weights and feature gates). This is the key architectural idea: **the classifier doesn't return "intent" — it returns permission boundaries for downstream signals.** Different query types genuinely need different signal combinations.

### Stage 2: Seed Generation

Source: `crates/graphiq-core/src/seeds.rs`

Seeds are the initial set of candidate symbols. They come from multiple strategies, gated per query family:

#### 2a. BM25 Full-Text Search

Every query starts with SQLite FTS5 BM25 search. The FTS index has weighted columns:

| Column | Weight | Content |
|---|---|---|
| name | 10.0 | Symbol name |
| decomposed | 8.0 | Identifier decomposition (`RateLimiter` → `rate`, `limiter`) |
| qualified | 6.0 | Fully qualified name |
| hints | 5.0 | Role tags, motifs, morphological variants |
| sig | 4.0 | Function signatures |
| file_path | 3.5 | Path components |
| doc | 3.0 | Doc comments |
| source | 1.0 | Source code body |

Natural language queries (NaturalAbstract, NaturalDescriptive, ErrorDebug, CrossCuttingSet) use relaxed FTS config with lower token thresholds and synonym expansion. Symbol-like queries use strict matching.

Returns up to 200 `(symbol_id, bm25_score)` pairs.

#### 2b. Per-Term FTS Expansion (NL queries only)

The query is split into terms, filtered by length (≥3 chars) and a 45-word stopword list. Each surviving term gets expanded with:

- **Stemming**: "running" → "run"
- **Synonyms**: "error" → ["err", "fault"] (up to 3 per term)

Each variant is searched individually via FTS (limit 50). Results are aggregated with coverage normalization: a symbol matching multiple query terms accumulates score across all terms, divided by the number of query terms.

#### 2c. Source Scan Seeds (ErrorDebug only)

For error/debug queries, GraphIQ extracts error-specific phrases from the query and scans source code for literal substring matches. Any symbol whose source contains the error phrase becomes a seed candidate, bypassing BM25 entirely.

This is gated to ErrorDebug only because enabling it for other query types causes severe false positives.

#### 2d. Numeric Bridge Seeds (NL queries only)

Extracts numeric literals from the query (decimals >1, hex, floats). For each number, queries the edges table for `shares_constant` and `references_constant` edges whose metadata contains that literal.

A query like "timeout after 30 seconds" discovers symbols that share the constant `30`.

#### 2e. Graph-Aware Expansion (NL queries only)

Uses structural edges to expand candidates through specific edge types, chosen per query family:

| Family | Edge types queried |
|---|---|
| ErrorDebug | `shares_error_type` |
| CrossCuttingSet | `shares_type`, `shares_data_shape` |
| NaturalAbstract/Descriptive | `shares_error_type`, `shares_type`, `shares_data_shape` |
| Relationship | `shares_type`, `shares_error_type` |

Takes the top 30 existing seeds, follows matching edges (limit 30 per seed), aggregates weights, returns top 50 by accumulated weight.

#### Seed Activation Matrix

| Family | per_term | graph | numeric | source_scan |
|---|---|---|---|---|
| SymbolExact | no | no | no | no |
| SymbolPartial | no | no | no | no |
| FilePath | no | no | no | no |
| ErrorDebug | yes | yes | yes | yes |
| NaturalDescriptive | yes | yes | yes | no |
| NaturalAbstract | yes | yes | yes | no |
| CrossCuttingSet | yes | yes | yes | no |
| Relationship | no | no | no | no |

Symbol-like queries rely entirely on BM25 + name lookup. NL queries activate all available expansion paths.

### Stage 3: Graph Walk Expansion

Source: `crates/graphiq-core/src/pipeline.rs`

After seed generation, GraphIQ performs a BFS graph walk from the top 8 seeds through the structural graph. This walk is only enabled for families where structural exploration helps (SymbolExact and FilePath skip it entirely).

The walk operates on the CruncherIndex's adjacency lists:

1. From each seed, follow outgoing and incoming edges (up to 10 per direction)
2. At each neighbor, check two gates:
   - **IDF gate**: The neighbor must match at least one query term with above-median IDF (filters generic utility functions)
   - **Coverage gate**: The neighbor must match at least one query term in its own text (prevents pure structural-only hits)
3. Walk evidence is computed as `coverage_score × proximity_decay × edge_weight`, where proximity decays as `0.5^depth`
4. Continue up to depth 2, max 25 expansions per seed
5. Walk-discovered candidates accumulate evidence from multiple seed paths

The walk discovers symbols that BM25 missed but are structurally connected to relevant seeds. This is GraphIQ's strongest structural signal — relationship queries are 3.7x better than grep because the graph walk finds symbols that share type flow, error surfaces, or data shapes with the seeds, even when they share no text with the query.

### Stage 4: Scoring

Source: `crates/graphiq-core/src/scoring.rs`

Every candidate (seed + walk discovery) gets a composite score. The formula differs for seeds and walk discoveries:

**For seeds:**
```
base = bm25_w × bm25_score + cov_w × min(coverage_norm, 0.4) + name_w × min(name_norm, 0.5)
```

When specificity scaling is enabled (NL queries), weights shift dynamically:
- `bm25_w` decreases with query specificity: `bm25_w × (1 - 0.3 × specificity)`
- `cov_w` increases with query specificity: `cov_w × (1 + 0.5 × specificity)`

**Specificity** is the fraction of query terms with IDF > 1.0 (rare/unusual terms). Specific queries like "encode VLQ source map" get more BM25 weight (the rare terms are discriminative). Broad queries like "handle request" get more coverage weight (need structural expansion).

**For walk discoveries:**
```
base = 1.5 × coverage_norm + 2.0 × name_norm + walk_weight × walk_norm
```

Walk discoveries weight coverage and name matching more heavily, plus accumulated walk evidence.

**Additive boosts:**

1. **Name overlap** — Set-intersection overlap coefficient between query terms and candidate name terms. Gated by a confidence threshold (0.2–0.5 per family) and scaled by query specificity. Below the gate, contribution is exactly 0. The gate adapts to the codebase: descriptive names pass easily, generic names don't.

2. **Neighbor fingerprint** — Exact match overlap between query terms and the union of terms from the candidate's 1-hop graph neighbors. Gated at 0.1 normalized score. This disambiguates generic names: a `poll` function whose neighbors include "frame", "buffer", "codec" gets a boost when the query mentions streaming concepts.

**Multiplicative adjustments:**

```
score = (base + name_overlap + neighbor_boost)
      × coverage_frac^0.3
      × seed_bonus (1.15× for seeds)
      × kind_boost (functions/types boosted, variables/imports penalized)
      × test_penalty (test files penalized for most queries)
```

- `coverage_frac^0.3` — Power-law dampening on the fraction of query terms matched. The 0.3 exponent means matching 50% of terms gives ~82% of the score of matching 100%.
- `kind_boost` — Functions and types get boosted. Variables, imports, and modules get penalized.
- `test_penalty` — Symbols in test files are penalized for most query families.

**Per-family scoring parameters:**

| Family | BM25 w | Cov w | Name w | Walk w | Walk? | Overlap? | Specificity? | Diversity |
|---|---|---|---|---|---|---|---|---|
| SymbolExact | 5.0 | 0.8 | 1.0 | 0.5 | off | yes | no | 3/file |
| SymbolPartial | 4.5 | 1.0 | 1.2 | 0.8 | on | yes | no | 3/file |
| FilePath | 3.0 | 1.5 | 0.8 | 0.3 | off | no | no | 5/file |
| ErrorDebug | 3.5 | 1.5 | 1.5 | 1.2 | on | yes | yes | 3/file |
| NaturalDescriptive | 3.0 | 1.5 | 2.0 | 1.0 | on | yes | yes | 3/file |
| NaturalAbstract | 2.5 | 2.0 | 1.5 | 1.5 | on | yes | yes | 2/file |
| CrossCuttingSet | 2.0 | 2.0 | 1.0 | 1.5 | on | no | yes | 1/file |
| Relationship | 3.0 | 1.5 | 1.0 | 2.0 | on | no | yes | 3/file |

Notice how CrossCuttingSet gets `diversity_max_per_file: 1` — forcing diverse results from different files. Relationship gets the highest walk weight (2.0) because the graph is the primary signal for "what connects X and Y."

### Stage 5: Post-Processing

Two final adjustments:

**BM25 Confidence Lock**: If BM25's top result has a >1.2x score gap over the second result AND the top result's name contains a query term, it's promoted to rank 1 with a +1M score bonus. This prevents structural signals from overriding a clear lexical match.

**File Diversity**: No more than `diversity_max_per_file` results per source file in the final output (varies 1–5 by family). This prevents a single large file from dominating all top-K positions.

**Exact Match Promotion** (SymbolExact only): After scoring, any symbol whose name exactly matches the query is force-promoted to the top of results.

---

## Performance

The entire pipeline is designed for interactive speed:

| Mode | Latency |
|---|---|
| Cold CLI (first run) | ~5-10s (builds CruncherIndex) |
| Warm CLI (cached) | ~50ms |
| In-process (MCP) | ~18μs |

The CruncherIndex is built once at startup and cached to `cruncher.bin.zst`. All graph adjacency, term sets, IDF weights, and neighbor fingerprints are pre-computed. Query-time work is: FTS BM25 lookup (~5ms) + graph walk (microseconds over pre-built adjacency lists) + scoring (linear scan over candidates).

## Design Principles

The current architecture (v3) emerged from 29 phases of experimentation. The key lessons:

1. **BM25 is hard to beat.** Every system that tried to replace BM25 failed. The winning pattern is always BM25 retrieves, structural math reranks.

2. **Simpler is better.** v1 had 5,087 lines of spectral/holographic/predictive math (Chebyshev heat diffusion, FFT holographic encoding, KL-divergence predictive models, Ricci curvature, MDL explanation sets). It used 18GB RAM and produced marginal NDCG improvement (+0.02–0.05). v3 removed it all and actually improved on codebases with descriptive names. The graph walk captures most structural signal; BM25 captures lexical signal. The complex math was refinancing a rounding error.

3. **Confidence matters.** The BM25 confidence lock prevents structural signals from overriding clear lexical matches. Signal confidence gates (name overlap threshold, neighbor score threshold) prevent noisy secondary signals from causing false promotions.

4. **Additive beats multiplicative.** Multiplicative boosts just reshuffle existing rankings. Additive contributions can genuinely promote candidates the base score would miss. But additive contributions need gating to prevent false promotions.

5. **Gate your signals.** The name overlap gate (0.2–0.5 per family) adapts to codebase characteristics without tuning. Descriptive names pass the gate easily. Generic names don't. No codebase-specific configuration required.

6. **Per-family routing is the right abstraction.** Different query types genuinely need different signal combinations. Symbol lookups should trust BM25. Abstract questions should lean into structural expansion. Cross-cutting queries should maximize diversity.

7. **Compute geometry, don't score it.** Ricci curvature is a real structural signal but useless as a ranking feature. The graph topology is infrastructure; the ranking signals are coverage, name matching, and walk evidence.

## Languages

**Full parsing (16 variants):** TypeScript, TSX, JavaScript, JSX, Rust, Python, Go, Java, C, C++, Ruby, YAML, TOML, JSON, HTML, CSS

**File tracking (20+):** Kotlin, Swift, C#, PHP, Lua, Dart, Scala, Haskell, Elixir, Zig, GraphQL, Protobuf, Shell, SQL, Markdown, XML, SCSS, CMake, Dockerfile, Makefile, Meson

## Further Reading

- [Benchmarks](benchmarks.md) — full methodology, per-category results, and comparison with grep
- [Research notes](research.md) — experimental history and lessons from 29 development phases
