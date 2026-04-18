# GraphIQ Roadmap

## Current State

**GooberV5** — per-candidate holographic name gating. FFT-based cosine similarity thresholded at 0.25, scaled by query specificity. Beats all prior versions on all 3 codebases.

| Codebase | BM25 | **GooberV5** | Delta |
|---|---|---|---|
| signetai | 0.556 | **0.681** | +0.125 |
| tokio | 0.583 | **0.517** | -0.066 |
| esbuild | 0.675 | **0.799** | +0.124 |

## Next Steps

### 1. Tokio regression

Generic function names make structural signals unreliable. Possible approaches:
- Seed-only fallback for queries with low average IDF
- Name specificity bonus for seeds matching high-IDF terms

### 2. Expand benchmarks

- Add Python (Django/Flask) and Java (Spring) codebases
- Bootstrap resampling for statistical significance

### 3. Architecture cleanup

- Remove dead retrieval systems (HRR, AFMO, Spectral, LSA, etc.)
- Clean unused index fields (`bridging`, `sig_terms`, `top_idf`)
- Wire GooberV5 into `search.rs` as the default

### 4. Production readiness

- Latency profiling (p50/p99)
- Memory profiling
- Fuzz testing
