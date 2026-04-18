# GraphIQ Roadmap

## Current State

**GooberV5** — per-candidate holographic name gating. Wired into `graphiq search` and `graphiq-mcp`. FFT-based cosine similarity thresholded at 0.25, scaled by query specificity. Beats all prior versions on all 3 codebases.

| Codebase | BM25 | **GooberV5** | Delta |
|---|---|---|---|
| signetai | 0.556 | **0.681** | +0.125 |
| tokio | 0.583 | **0.517** | -0.066 |
| esbuild | 0.675 | **0.799** | +0.124 |

### Completed

- [x] Wire GooberV5 into search pipeline (`search.rs`)
- [x] Remove dead HRR/evidence/SEC branching from `search.rs` (-521 lines)
- [x] Clean unused index fields (`top_idf` dead code removed)
- [x] Latency profiling (p50/p99 across all codebases)
- [x] Fuzz testing (53 adversarial queries, zero panics)
- [x] Multi-language demo codebase (Rust, TS, Python, Go) + self-test benchmark

## Next Steps

### 1. Tokio regression

Generic function names make structural signals unreliable. Possible approaches:
- Seed-only fallback for queries with low average IDF
- Name specificity bonus for seeds matching high-IDF terms

### 2. Expand benchmarks

- Add Python (Django/Flask) and Java (Spring) codebases
- Bootstrap resampling for statistical significance

### 3. Production polish

- Wire GooberV5 into `graphiq demo` command
- Benchmark CI integration
- Binary size optimization
