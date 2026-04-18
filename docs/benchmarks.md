# Benchmarks

## Methodology

30-query benchmark across 3 codebases covering different languages and codebase characteristics:

| Codebase | Language | Symbols | Edges | Characteristics |
|---|---|---|---|---|
| signetai | TypeScript | 20,870 | 14,547 | Domain-specific names, deep call graphs |
| tokio | Rust | 12,892 | 11,520 | Generic function names (`run`, `handle`, `poll`) |
| esbuild | Go | 12,040 | 7,079 | Descriptive names (`convertOKLCHToOKLAB`) |

Two evaluation metrics:
- **MRR** (Mean Reciprocal Rank): rank-1 correctness. 1.0 = perfect (every query's target is rank 1). Primary metric.
- **NDCG@10**: graded relevance across the top 10 results.

NDCG and MRR use completely different query sets targeting different symbols. NDCG queries have graded relevance judgments (1-3) covering 3-6 relevant symbols each. MRR queries target a single expected symbol with binary relevance.

## Results

### MRR (rank-1 correctness — primary metric)

| Codebase | BM25 | CR v1 | CR v2 | Goober | V3 | V4 | **V5** | V5 vs BM25 |
|---|---|---|---|---|---|---|---|---|
| signetai | 0.556 | 0.608 | 0.638 | 0.625 | 0.675 | 0.675 | **0.681** | **+0.125** |
| tokio | 0.583 | 0.492 | 0.511 | 0.513 | 0.506 | 0.499 | **0.511** | -0.072 |
| esbuild | 0.675 | 0.597 | 0.737 | 0.774 | 0.773 | 0.781 | **0.827** | **+0.152** |

### Accuracy (rank-1 correct)

| Codebase | BM25 | GooberV4 | **GooberV5** |
|---|---|---|---|
| signetai | 0.433 | 0.633 | **0.633** |
| tokio | 0.467 | 0.433 | **0.433** |
| esbuild | 0.533 | 0.700 | **0.767** |

### NDCG@10 (graded relevance)

| Codebase | BM25 | CR v1 | CR v2 | Goober | V3 | V4 | **V5** |
|---|---|---|---|---|---|---|---|
| signetai | 0.202 | 0.267 | 0.281 | 0.252 | 0.259 | 0.259 | 0.252 |
| tokio | 0.225 | 0.232 | 0.249 | 0.208 | 0.232 | 0.211 | **0.238** |
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
