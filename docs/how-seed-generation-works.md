# How Seed Generation Works

Source: [`crates/graphiq-core/src/seeds.rs`](../crates/graphiq-core/src/seeds.rs)

Seed generation is the first stage of the unified pipeline. It produces a set of candidate symbol IDs with associated confidence scores, which are then expanded through spectral diffusion and scored.

The entry point is `generate_seeds()`, which takes a query string, a `SeedConfig`, and an optional self-model. It runs up to 5 expansion strategies in sequence, each adding new candidates that weren't found by the previous stages.

## The Expansion Pipeline

### 1. BM25 Full-Text Search (`bm25_seeds`)

Every query starts with SQLite FTS5 BM25 search over the symbol index. This returns up to 200 candidates ranked by BM25 score. For natural language queries (NaturalAbstract, NaturalDescriptive, ErrorDebug, CrossCuttingSet), the FTS config uses relaxed matching (lower token thresholds, synonym expansion). For symbol-like queries, it uses strict matching.

Output: up to 200 `(symbol_id, bm25_score)` pairs.

### 2. Per-Term Expansion (`per_term_fts_expansion`)

Only activated for natural language queries.

The query is split into terms, filtered by length (>= 3 chars) and a stopword list (45 common English words). For each surviving term, two expansions happen:

- **Stemming**: `stem_word()` produces a stemmed variant (e.g. "running" -> "run"). Both original and stem are searched.
- **Synonyms**: `get_synonyms()` returns known synonyms (e.g. "error" -> ["err", "fault"]). Up to 3 synonyms per term.

Each variant is searched individually via FTS with a limit of 50. Results are aggregated: a symbol that matches multiple query terms accumulates score across all terms, divided by the number of query terms (coverage normalization).

Symbols already in the BM25 seed set are excluded. Output: additional candidates not found by the whole-query BM25.

### 3. Numeric Bridge Seeds (`numeric_bridge_seeds`)

Only activated for natural language queries.

Extracts numeric literals from the query: decimal numbers > 1, hex literals (0x...), floats. For each number, it queries the `edges` table for `shares_constant` and `references_constant` edges whose metadata contains that literal.

This lets a query like "timeout after 30 seconds" discover symbols that share the constant `30` — connecting code that uses the same magic numbers.

Output: symbols connected through shared numeric literals.

### 4. Graph-Aware Expansion (`graph_aware_expansion`)

Only activated for natural language queries.

Uses the structural graph to expand candidates through specific edge types, chosen per query family:

| Family | Edge types queried |
|---|---|
| ErrorDebug | `shares_error_type` |
| CrossCuttingSet | `shares_type`, `shares_data_shape` |
| NaturalAbstract/Descriptive | `shares_error_type`, `shares_type`, `shares_data_shape` |
| Relationship | `shares_type`, `shares_error_type` |

Takes the top 30 existing seeds, follows matching edges (limit 30 per seed), and aggregates weights. Output is truncated to top 50 by accumulated weight.

### 5. Self-Model Expansion (`self_model_expansion`)

Only activated for NaturalAbstract queries.

The `RepoSelfModel` contains deterministic concept nodes — clusters of symbols that share structural roles or naming patterns. `expand_query()` matches query terms against concept node labels and returns associated symbol IDs.

Scores are multiplied by 5.0 to give self-model matches competitive weight against BM25 seeds.

## SeedConfig

`SeedConfig::for_family()` controls which expansions fire:

| Family | per_term | graph | numeric | self_model |
|---|---|---|---|---|
| SymbolExact | no | no | no | no |
| SymbolPartial | no | no | no | no |
| FilePath | no | no | no | no |
| ErrorDebug | yes | yes | yes | no |
| NaturalDescriptive | yes | yes | yes | no |
| NaturalAbstract | yes | yes | yes | yes |
| CrossCuttingSet | yes | yes | yes | no |
| Relationship | no | no | no | no |

Symbol-like queries rely entirely on BM25 + name lookup in the pipeline. Natural language queries activate all available expansion paths to maximize recall.
