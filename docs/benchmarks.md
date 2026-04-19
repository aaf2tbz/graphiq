# Benchmarks

## Methodology

v3 benchmark queries across 3 codebases covering different languages and codebase characteristics:

| Codebase | Language | Symbols | Edges | NDCG Queries | MRR Queries | Characteristics |
|---|---|---|---|---|---|---|
| signetai | TypeScript | 20,870 | 14,547 | 47 | 20 | Domain-specific names, deep call graphs |
| tokio | Rust | 17,867 | 15,686 | 46 | 20 | Generic function names (`run`, `handle`, `poll`) |
| esbuild | Go | 12,040 | 24,826 | 40 | 20 | Descriptive names (`convertOKLCHToOKLAB`) |

Two evaluation metrics:
- **NDCG@10**: graded relevance across top 10 results. Queries have 1-3 graded relevant symbols each.
- **MRR** (Mean Reciprocal Rank): rank-1 correctness. 1.0 = perfect.

NDCG and MRR use completely disjoint query sets targeting different symbols. 7 query categories: `symbol-exact`, `symbol-partial`, `nl-descriptive`, `nl-abstract`, `file-path`, `error-debug`, `cross-cutting`.

## Results

### NDCG@10 (graded relevance)

| Codebase | BM25 | CRv1 | CRv2 | Goober | V3 | V4 | V5 | Geometric | Curved | **Deformed** |
|---|---|---|---|---|---|---|---|---|---|---|
| signetai | 0.334 | 0.364 | 0.385 | 0.383 | 0.388 | 0.388 | 0.415 | 0.441 | 0.441 | 0.440 |
| tokio | 0.249 | 0.233 | 0.253 | 0.278 | 0.284 | 0.247 | 0.315 | 0.368 | 0.368 | **0.371** |
| esbuild | 0.315 | 0.342 | 0.415 | 0.378 | 0.411 | 0.380 | 0.455 | 0.501 | 0.501 | **0.510** |

### MRR (rank-1 correctness)

| Codebase | BM25 | CRv1 | CRv2 | Goober | V3 | V4 | V5 | Geometric | Curved | **Deformed** |
|---|---|---|---|---|---|---|---|---|---|---|
| signetai | 0.843 | 0.749 | 0.822 | 0.843 | 0.885 | 0.810 | 0.885 | **0.924** | **0.924** | **0.924** |
| tokio | 0.627 | 0.627 | 0.627 | 0.627 | 0.627 | 0.627 | 0.612 | 0.637 | 0.637 | **0.674** |
| esbuild | 0.624 | 0.624 | 0.624 | 0.624 | 0.624 | 0.624 | 0.639 | 0.676 | 0.668 | **0.728** |

### Hit@1 / Hit@3 / Hit@5 / Hit@10 (Deformed)

| Codebase | H@1 | H@3 | H@5 | H@10 |
|---|---|---|---|---|
| signetai (NDCG) | 18/47 | 21/47 | 23/47 | 24/47 |
| tokio (NDCG) | 17/46 | 22/46 | 23/46 | 23/46 |
| esbuild (NDCG) | 20/40 | 25/40 | 25/40 | 25/40 |

## Per-Category Breakdown (Deformed)

### esbuild

| Category | BM25 | GooV5 | Geometric | Deformed |
|---|---|---|---|---|
| symbol-exact | 0.594 | 0.789 | 0.900 | 0.900 |
| symbol-partial | 0.205 | 0.232 | 0.284 | **0.313** |
| nl-descriptive | 0.439 | 0.833 | 0.833 | 0.833 |
| error-debug | 0.559 | 0.790 | 0.790 | 0.790 |
| nl-abstract | 0.076 | 0.146 | 0.111 | 0.111 |
| file-path | 0.000 | 0.000 | 0.148 | 0.148 |
| cross-cutting | 0.000 | 0.000 | 0.000 | 0.000 |

### tokio

| Category | BM25 | GooV5 | Geometric | Deformed |
|---|---|---|---|---|
| symbol-exact | 0.611 | 0.665 | 0.884 | 0.881 |
| symbol-partial | 0.230 | 0.305 | 0.323 | **0.326** |
| nl-descriptive | 0.102 | 0.178 | 0.166 | **0.182** |
| error-debug | 0.511 | 0.738 | 0.774 | 0.770 |
| nl-abstract | 0.000 | 0.000 | 0.000 | 0.000 |
| file-path | 0.000 | 0.000 | 0.000 | 0.000 |
| cross-cutting | 0.000 | 0.013 | 0.021 | 0.021 |

### signetai

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
- **GooV5**: GooberV4 + per-candidate holographic name gating
- **Geometric**: Chebyshev heat diffusion on graph Laplacian + V5 scoring framework
- **Curved**: Geometric + Ricci curvature-weighted diffusion (no measurable difference)
- **Deformed**: Geometric + predictive surprise (D_KL free energy) + channel capacity routing + MDL explanation sets

## Running Benchmarks

```bash
cargo build --release -p graphiq-bench

# Full benchmark suite
./target/release/graphiq-bench <db> <ndcg-queries.json> <mrr-queries.json>

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
    "category": "nl-descriptive",
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

Query categories: `symbol-exact`, `symbol-partial`, `nl-descriptive`, `nl-abstract`, `file-path`, `error-debug`, `cross-cutting`.

## Fuzz Testing

53 adversarial query strings tested across all codebases with zero panics:
- Empty, whitespace-only, single-character queries
- Special characters (`()&&||.+-*[]{}<>=::;,\'\"\`)
- Unicode (CJK, Cyrillic, emoji)
- 1000-term queries, repeated terms, only-stopword queries
- CamelCase, snake_case, kebab-case identifiers
- Numeric strings, hex literals

Run with: `graphiq-bench fuzz <db-path>`
