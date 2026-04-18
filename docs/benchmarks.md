# Benchmarks

## Methodology

30-query benchmark across 4 codebases covering different languages and codebase characteristics:

| Codebase | Language | Symbols | Edges | Characteristics |
|---|---|---|---|---|
| signetai | TypeScript | 20,870 | 14,547 | Domain-specific names, deep call graphs |
| tokio | Rust | 12,892 | 11,520 | Generic function names (`run`, `handle`, `poll`) |
| esbuild | Go | 12,040 | 7,079 | Descriptive names (`convertOKLCHToOKLAB`) |
| demo | Multi (Rust, TS, Python, Go) | 48 | 17 | Small self-test codebase, 4 languages |

Two evaluation metrics:
- **MRR** (Mean Reciprocal Rank): rank-1 correctness. 1.0 = perfect (every query's target is rank 1). Primary metric.
- **NDCG@10**: graded relevance across the top 10 results.

NDCG and MRR use completely different query sets targeting different symbols. NDCG queries have graded relevance judgments (1-3) covering 3-6 relevant symbols each. MRR queries target a single expected symbol with binary relevance.

## Results

### MRR (rank-1 correctness — primary metric)

| Codebase | BM25 | CR v1 | CR v2 | Goober | V3 | V4 | **V5** | V5 vs BM25 |
|---|---|---|---|---|---|---|---|---|
| signetai | 0.556 | 0.608 | 0.646 | 0.625 | 0.675 | 0.658 | **0.681** | **+0.125** |
| tokio | 0.583 | 0.492 | 0.511 | 0.513 | 0.506 | 0.507 | **0.517** | -0.066 |
| esbuild | 0.675 | 0.597 | 0.746 | 0.774 | 0.784 | 0.789 | **0.799** | **+0.124** |

### Accuracy (rank-1 correct)

| Codebase | BM25 | GooberV4 | **GooberV5** |
|---|---|---|---|
| signetai | 0.433 | 0.633 | **0.633** |
| tokio | 0.467 | 0.433 | **0.433** |
| esbuild | 0.533 | 0.700 | **0.767** |

### NDCG@10 (graded relevance)

| Codebase | BM25 | CR v1 | CR v2 | Goober | V3 | V4 | **V5** |
|---|---|---|---|---|---|---|---|
| signetai | 0.202 | 0.267 | 0.281 | 0.252 | 0.259 | 0.259 | 0.244 |
| tokio | 0.225 | 0.232 | 0.249 | 0.208 | 0.232 | 0.213 | **0.238** |
| esbuild | 0.365 | 0.351 | 0.380 | 0.379 | 0.387 | 0.387 | 0.387 |

## Method Descriptions

- **BM25**: SQLite FTS5 with per-column weights (name=10, decomposed=8, qualified=6, hints=5, doc=3, file_path=3.5, sig=4, source=1)
- **Cruncher v1**: BM25 seeds + query-conditioned graph walk + multi-signal scoring
- **Cruncher v2**: BM25 seeds + per-term energy field propagation + interference scoring + confidence lock
- **Goober**: BM25-dominant seed scoring + IDF-gated graph walk + confidence lock
- **GooberV3**: Goober + NG scoring (negentropy + channel coherence)
- **GooberV4**: GooberV3 + query intent classification (navigational vs informational)
- **GooberV5**: GooberV4 + per-candidate holographic name gating

## Per-Query Results

### esbuild — V5 wins

| Query | BM25 | V4 | V5 | Change |
|---|---|---|---|---|
| lower and minify a CSS color | #3 | #2 | **#1** | holographic name match passes gate |
| convert OKLCH color to OKLAB | #2 | #1 | **#1** | maintained |
| compute reserved names for renaming | #2 | #2 | **#1** | holographic boost pushes to top |
| scan for imports and exports | #9 | #4 | **#3** | improved |
| validate log level string is valid | #5 | #1 | **#1** | maintained from V3+ |

### signetai — V5 wins

| Query | V4 | V5 | Change |
|---|---|---|---|
| compute semantic version comparison | #3 | **#2** | marginal holographic boost |
| purge stale embeddings from store | #1 | **#1** | maintained |

## Running Benchmarks

```bash
cargo build --release -p graphiq-bench

# Run on a specific codebase
./target/release/graphiq-bench .graphiq/bench_signetai.db benches/ndcg-10-signetai.json benches/mrr-30-signetai.json

# All three codebases
for db in signetai tokio esbuild; do
  ./target/release/graphiq-bench .graphiq/bench_${db}.db benches/ndcg-10-${db}.json benches/mrr-30-${db}.json
done
```

### Adding New Benchmark Queries

Query files are JSON arrays of objects:

**MRR format** (`mrr-30-*.json`):
```json
[
  {
    "query": "encode a value in variable length quantity",
    "category": "nl-descriptive",
    "expected_symbol": "encodeVLQ"
  }
]
```

**NDCG format** (`ndcg-10-*.json`):
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

## Latency Profile

30 queries x 10 iterations, release build, macOS:

| Codebase | BM25 p50 | GooberV5 p50 | GooberV5 p99 |
|---|---|---|---|
| signetai (20K symbols) | 7.6ms | 17.9ms | 53ms |
| esbuild (12K symbols) | 4.6ms | 19.1ms | 63ms |
| tokio (13K symbols) | 4.9ms | 19.9ms | 67ms |
| demo (48 symbols) | 0.1ms | 0.7ms | 3.2ms |

V5 adds ~2.5ms over V4 from the holographic name computation. FTS is the dominant cost at scale.

## Fuzz Testing

53 adversarial query strings tested across all codebases with zero panics:
- Empty, whitespace-only, single-character queries
- Special characters (`()&&||.+-*[]{}<>=::;,\'\"\`)
- Unicode (CJK, Cyrillic, emoji)
- 1000-term queries, repeated terms, only-stopword queries
- CamelCase, snake_case, kebab-case identifiers
- Numeric strings, hex literals

Run with: `graphiq-bench fuzz <db-path>`
