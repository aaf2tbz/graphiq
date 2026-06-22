# Performance & Benchmarks

Measured across 300 queries on three real codebases. Full methodology and per-category breakdowns in [Benchmarks](benchmarks.md).

## Search quality vs grep

| Codebase | GraphIQ NDCG@10 | GraphIQ MRR@10 |
|---|---:|---:|
| signetai | 0.286 (+100% vs grep) | 0.450 (+213% vs grep) |
| esbuild | 0.318 (+59% vs grep) | 0.551 (+280% vs grep) |
| tokio | 0.192 (-1% vs grep) | 0.411 (+25% vs grep) |
| **Overall** | **0.265 (+48% vs grep)** | **0.471 (+128% vs grep)** |

## Speed

- **Index size:** ~6.5 MB for a 20K-symbol codebase.
- **Cold CLI (first run):** ~5–10s (builds the in-memory graph).
- **Warm CLI (cached):** ~50ms.
- **In-process (MCP):** microsecond-scale graph traversal after load.

## Reproducing

The benchmark harness compares GraphIQ against grep on real query sets:

```bash
./target/release/graphiq-bench <db-path> <ndcg-queries.json> <mrr-queries.json>
```

Query sets live in `benches/queries/archive/`. Baselines are in `benches/baseline-*.json`. When changing the indexer or scoring, run an **A/B** against `main` to prove no regression.
