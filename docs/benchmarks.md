# Benchmarks

## Methodology

v4 benchmark queries across 3 codebases with separate NDCG and MRR query sets. NDCG queries use graded relevance (3=perfect, 2=good, 1=related) with multiple relevant symbols. MRR queries target a single expected symbol. Each has easy (name hints in query) and medium (purely behavioral description) subsets.

### Design Philosophy

NDCG and MRR measure fundamentally different things and require different query sets:
- **NDCG@10**: Graded relevance across top 10 results. Queries have 1-3 graded relevant symbols. Measures ranking quality.
- **MRR**: First-hit accuracy. Queries have a single target symbol. Measures "can you find THE answer."

Medium-difficulty NL queries are the real test — they simulate "drop codebase in, ask a real question." ~40-50% miss rate on medium NL is the frontier to improve. **H@3 is the metric that matters for agent recall** — a smart agent scans top 3 results and picks.

### Codebases

| Codebase | Language | Symbols | Edges | NDCG Queries | MRR Queries | Characteristics |
|---|---|---|---|---|---|---|
| signetai | TypeScript | 20,870 | 14,547 | 20 | 20 | Domain-specific names, deep call graphs |
| tokio | Rust | 17,867 | 15,686 | 20 | 20 | Generic function names (`run`, `handle`, `poll`) |
| esbuild | Go | 12,040 | 24,826 | 20 | 20 | Descriptive names (`convertOKLCHToOKLAB`) |

### Query Categories (v4)

v4 queries have a cleaner structure than v3:
- **nl-medium** (10 queries per codebase): Behavioral descriptions without name hints. "how does the SDK authenticate and handle transport" — tests whether the engine finds symbols by structural context alone.
- **symbol-exact** (5 queries): Exact symbol names. Tests BM25 precision preservation.
- **symbol-partial** (5 queries): Single word fragments like `cancel`, `guard`, `budget`. Tests disambiguation of common words.

v3 queries additionally covered `file-path`, `error-debug`, `cross-cutting`, and `nl-descriptive`. v4 focuses on the three categories that differentiate methods most clearly.

### Evaluation Metrics

- **NDCG@10**: Normalized Discounted Cumulative Gain at 10. Graded relevance (3/2/1).
- **MRR**: Mean Reciprocal Rank. 1/first_correct_rank.
- **P@10**: Precision at 10. Fraction of top-10 that are relevant.
- **R@10**: Recall at 10. Fraction of all relevant items found in top 10.
- **H@k**: Hit at k. Did the correct answer appear in top k? (k=1..10)
- **Miss**: Complete miss — correct answer not in top 10 at all.

## Results (v4 Queries)

### NDCG@10 (graded relevance)

| Codebase | BM25 | CRv1 | CRv2 | Goober | GooV3 | GooV4 | GooV5 | Geometric | Curved | Deformed | Routed | **CARE** |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| signetai | 0.287 | 0.275 | 0.282 | 0.315 | 0.320 | 0.298 | 0.375 | 0.367 | 0.367 | 0.367 | **0.405** | 0.384 |
| tokio | 0.272 | 0.267 | 0.281 | 0.269 | 0.282 | 0.269 | 0.305 | 0.353 | 0.353 | 0.355 | **0.413** | 0.363 |
| esbuild | 0.299 | 0.317 | 0.394 | 0.348 | 0.362 | 0.353 | 0.430 | 0.480 | 0.480 | 0.483 | **0.514** | 0.496 |

Routed is the NDCG champion on all 3 codebases. CARE approaches but never beats it.

### MRR (rank-1 correctness)

| Codebase | BM25 | CRv1 | CRv2 | Goober | GooV3 | GooV4 | GooV5 | Geometric | Curved | Deformed | Routed | **CARE** |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| signetai | 0.650 | 0.625 | 0.650 | 0.650 | 0.650 | 0.625 | **0.721** | 0.700 | 0.700 | 0.700 | 0.691 | 0.696 |
| tokio | 0.375 | 0.375 | 0.375 | 0.375 | 0.375 | 0.375 | 0.467 | 0.425 | 0.425 | 0.425 | 0.348 | **0.493** |
| esbuild | 0.575 | 0.575 | 0.575 | 0.575 | 0.575 | 0.575 | 0.713 | 0.763 | 0.763 | 0.763 | **0.740** | 0.693 |

GooV5 preserves rank-1 best for signetai. Routed wins esbuild. CARE wins tokio MRR.

### Hit@1 / Hit@3 / Hit@5 / Hit@10

**Signetai (20 queries):**

| Method | H@1 | H@3 | H@5 | H@10 | Miss |
|---|---|---|---|---|---|
| BM25 | 13/20 | 13/20 | 13/20 | 13/20 | 7/20 |
| GooV5 | 15/20 | 15/20 | 15/20 | 15/20 | 5/20 |
| Routed | 14/20 | 15/20 | 15/20 | 15/20 | 5/20 |
| CARE | 14/20 | 15/20 | 15/20 | 15/20 | 5/20 |

**Tokio (20 queries):**

| Method | H@1 | H@3 | H@5 | H@10 | Miss |
|---|---|---|---|---|---|
| BM25 | 8/20 | 8/20 | 8/20 | 8/20 | 12/20 |
| GooV5 | 10/20 | 11/20 | 11/20 | 11/20 | 9/20 |
| Routed | 8/20 | 9/20 | 9/20 | 9/20 | 11/20 |
| CARE | 11/20 | 12/20 | 12/20 | 12/20 | 8/20 |

**Esbuild (20 queries):**

| Method | H@1 | H@3 | H@5 | H@10 | Miss |
|---|---|---|---|---|---|
| BM25 | 12/20 | 12/20 | 12/20 | 12/20 | 8/20 |
| GooV5 | 15/20 | 17/20 | 17/20 | 17/20 | 3/20 |
| Routed | 16/20 | 16/20 | 16/20 | 16/20 | 4/20 |
| CARE | 15/20 | 16/20 | 16/20 | 16/20 | 4/20 |

### P@10 and R@10 (MRR queries)

| Codebase | Method | P@10 | R@10 |
|---|---|---|---|
| signetai | GooV5 | 0.085 | 0.900 |
| signetai | Routed | 0.080 | 0.850 |
| signetai | CARE | 0.085 | 0.900 |
| tokio | GooV5 | 0.050 | 0.550 |
| tokio | Routed | 0.045 | 0.450 |
| tokio | CARE | 0.055 | 0.600 |
| esbuild | GooV5 | 0.080 | 0.800 |
| esbuild | Routed | 0.080 | 0.800 |
| esbuild | CARE | 0.075 | 0.750 |

## Per-Category Breakdown (v4 NDCG, Top Methods)

### nl-medium (10 queries per codebase)

| Codebase | BM25 | GooV5 | Routed | CARE |
|---|---|---|---|---|
| signetai | 0.094 | 0.195 | **0.275** | 0.247 |
| tokio | 0.154 | 0.184 | **0.334** | 0.292 |
| esbuild | 0.161 | 0.296 | **0.442** | 0.400 |

Routed dominates nl-medium — structural graph traversal finds the right multi-symbol answers for behavioral queries. CARE recovers some of Routed's advantage via convergence bonuses.

### symbol-exact (5 queries per codebase)

| Codebase | BM25 | GooV5 | Routed | CARE |
|---|---|---|---|---|
| signetai | 0.600 | **0.840** | 0.640 | 0.800 |
| tokio | 0.400 | 0.680 | **1.000** | 0.880 |
| esbuild | 0.480 | 0.800 | **0.960** | 0.920 |

Routed excels on symbol-exact in tokio and esbuild where heat diffusion spreads from exact matches to structural neighbors. GooV5 preserves its holographic name gate advantage on signetai.

### symbol-partial (5 queries per codebase)

| Codebase | BM25 | GooV5 | Routed | CARE |
|---|---|---|---|---|
| signetai | 0.100 | 0.040 | 0.100 | 0.040 |
| tokio | 0.180 | 0.240 | **0.280** | 0.240 |
| esbuild | 0.240 | 0.360 | **0.420** | 0.360 |

Symbol-partial remains hard. Single common words like "guard", "budget", "encode" are ambiguous across the codebase. Routed's structural context helps slightly on tokio and esbuild. Signetai symbol-partial is nearly unsolvable — "supersede" and "pipeline" match too many things.

## Historical Results (v3 Queries)

For continuity, v3 results with the original 7-category, 40-47 query benchmark:

### NDCG@10

| Codebase | BM25 | CRv1 | CRv2 | Goober | V3 | V4 | V5 | Geometric | Curved | Deformed |
|---|---|---|---|---|---|---|---|---|---|---|
| signetai | 0.334 | 0.364 | 0.385 | 0.383 | 0.388 | 0.388 | 0.415 | 0.441 | 0.441 | 0.440 |
| tokio | 0.249 | 0.233 | 0.253 | 0.278 | 0.284 | 0.247 | 0.315 | 0.368 | 0.368 | **0.371** |
| esbuild | 0.315 | 0.342 | 0.415 | 0.378 | 0.411 | 0.380 | 0.455 | 0.501 | 0.501 | **0.510** |

### MRR (v3)

| Codebase | BM25 | CRv1 | CRv2 | Goober | V3 | V4 | V5 | Geometric | Curved | **Deformed** |
|---|---|---|---|---|---|---|---|---|---|---|
| signetai | 0.843 | 0.749 | 0.822 | 0.843 | 0.885 | 0.810 | 0.885 | **0.924** | **0.924** | **0.924** |
| tokio | 0.627 | 0.627 | 0.627 | 0.627 | 0.627 | 0.627 | 0.612 | 0.637 | 0.637 | **0.674** |
| esbuild | 0.624 | 0.624 | 0.624 | 0.624 | 0.624 | 0.624 | 0.639 | 0.676 | 0.668 | **0.728** |

### v3 Per-Category Breakdown (Deformed)

#### esbuild

| Category | BM25 | GooV5 | Geometric | Deformed |
|---|---|---|---|---|
| symbol-exact | 0.594 | 0.789 | 0.900 | 0.900 |
| symbol-partial | 0.205 | 0.232 | 0.284 | **0.313** |
| nl-descriptive | 0.439 | 0.833 | 0.833 | 0.833 |
| error-debug | 0.559 | 0.790 | 0.790 | 0.790 |
| nl-abstract | 0.076 | 0.146 | 0.111 | 0.111 |
| file-path | 0.000 | 0.000 | 0.148 | 0.148 |
| cross-cutting | 0.000 | 0.000 | 0.000 | 0.000 |

#### tokio

| Category | BM25 | GooV5 | Geometric | Deformed |
|---|---|---|---|---|
| symbol-exact | 0.611 | 0.665 | 0.884 | 0.881 |
| symbol-partial | 0.230 | 0.305 | 0.323 | **0.326** |
| nl-descriptive | 0.102 | 0.178 | 0.166 | **0.182** |
| error-debug | 0.511 | 0.738 | 0.774 | 0.770 |
| nl-abstract | 0.000 | 0.000 | 0.000 | 0.000 |
| file-path | 0.000 | 0.000 | 0.000 | 0.000 |
| cross-cutting | 0.000 | 0.013 | 0.021 | 0.021 |

#### signetai

| Category | BM25 | GooV5 | Geometric | Deformed |
|---|---|---|---|---|
| symbol-exact | 0.888 | 0.943 | 0.900 | 0.900 |
| symbol-partial | 0.100 | 0.100 | 0.100 | 0.100 |
| nl-descriptive | 0.377 | 0.559 | 0.548 | 0.537 |
| error-debug | 0.126 | 0.443 | **0.941** | **0.941** |
| nl-abstract | 0.049 | 0.098 | 0.098 | 0.098 |
| file-path | 0.000 | 0.000 | 0.000 | 0.000 |
| cross-cutting | 0.323 | 0.294 | 0.279 | 0.259 |

## Method Descriptions

- **BM25**: SQLite FTS5 with per-column weights (name=10, decomposed=8, qualified=6, hints=5, doc=3, file_path=3.5, sig=4, source=1)
- **CR v1**: BM25 seeds + query-conditioned graph walk + multi-signal scoring
- **CR v2**: BM25 seeds + per-term energy field propagation + interference scoring + confidence lock
- **Goober**: BM25-dominant seed scoring + IDF-gated graph walk + confidence lock
- **GooV3**: Goober + NG scoring (negentropy + channel coherence)
- **GooV4**: GooberV3 + query intent classification (navigational vs informational)
- **GooV5**: GooberV4 + per-candidate holographic name gating. Best MRR on lexical-heavy codebases.
- **Geometric**: Chebyshev heat diffusion on graph Laplacian + V5 scoring framework
- **Curved**: Geometric + Ricci curvature-weighted diffusion (no measurable difference from Geometric)
- **Deformed**: Geometric + predictive surprise (D_KL free energy) + channel capacity routing + MDL explanation sets
- **Routed**: SearchEngine with query family routing + Chebyshev heat diffusion. Best NDCG overall. Routes queries to appropriate retrieval policies based on query family classification.
- **CARE**: Confidence-Anchored Reciprocal Expansion. Fuses GooV5 (lexical precision) and Routed (structural recall) via normalized score fusion with convergence bonus and BM25 anchor. Best MRR on tokio.

## Running Benchmarks

```bash
cargo build --release -p graphiq-bench

# Full benchmark suite (NDCG + MRR, all 12 methods)
./target/release/graphiq-bench <db> <ndcg-queries.json> <mrr-queries.json>

# NDCG only
./target/release/graphiq-bench <db> <ndcg-queries.json>

# Parameter tuning (outputs CSV)
./target/release/graphiq-bench tune <db> <ndcg-queries.json> [mrr-queries.json]

# Latency profiling
./target/release/graphiq-bench profile <db> <mrr-queries.json>

# Fuzz testing
./target/release/graphiq-bench fuzz <db>
```

### Adding New Benchmark Queries

Query files are JSON arrays of objects:

**MRR format**:
```json
[
  {
    "query": "encode a value in variable length quantity",
    "category": "nl-easy",
    "expected_symbol": "encodeVLQ"
  }
]
```

**NDCG format**:
```json
[
  {
    "query": "encodeVLQ",
    "category": "symbol-exact",
    "relevance": {
      "encodeVLQ": 3,
      "decodeVLQ": 1,
      "encodeSourceMap": 1
    }
  }
]
```

Query categories: `symbol-exact`, `symbol-partial`, `nl-easy`, `nl-medium`, `nl-descriptive`, `nl-abstract`, `file-path`, `error-debug`, `cross-cutting`.

## Fuzz Testing

53 adversarial query strings tested across all codebases with zero panics:
- Empty, whitespace-only, single-character queries
- Special characters (`()&&||.+-*[]{}<>=::;,\'\"\`)
- Unicode (CJK, Cyrillic, emoji)
- 1000-term queries, repeated terms, only-stopword queries
- CamelCase, snake_case, kebab-case identifiers
- Numeric strings, hex literals

Run with: `graphiq-bench fuzz <db-path>`
