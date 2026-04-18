# GraphIQ Roadmap

## Current State (GooberV5 — per-candidate holographic name gating)

### 30-query MRR benchmark

| Codebase | BM25 | CR v1 | CR v2 | Goober | V3 | V4 | **V5** | V5 vs V4 | V5 vs BM25 |
|---|---|---|---|---|---|---|---|---|---|
| signetai | 0.556 | 0.608 | 0.638 | 0.625 | 0.675 | 0.675 | **0.681** | +0.006 | **+0.125** |
| tokio | 0.583 | 0.492 | 0.511 | 0.513 | 0.506 | 0.499 | **0.511** | +0.012 | -0.072 |
| esbuild | 0.675 | 0.597 | 0.737 | 0.774 | 0.773 | 0.781 | **0.827** | +0.046 | **+0.152** |

### 30-query Accuracy

| Codebase | BM25 | GooberV4 | **GooberV5** |
|---|---|---|---|
| signetai | 0.433 | 0.633 | **0.633** |
| tokio | 0.467 | 0.433 | **0.433** |
| esbuild | 0.533 | 0.700 | **0.767** |

GooberV5 adds per-candidate holographic name gating on top of V4. FFT-based circular convolution encodes identifier terms as holographic vectors. The cosine similarity between query and candidate name encodings has 6.8x separation between correct/incorrect matches. A confidence gate (threshold 0.25) ensures only high-similarity candidates receive the holographic boost, scaled by query specificity (fraction of high-IDF terms). This prevents the false promotions that plagued earlier additive approaches.

### Experimental history

7 experiments (V5–V11) were run and reverted before arriving at the final V5:

| Version | Strategy | signetai | tokio | esbuild | Verdict |
|---|---|---|---|---|---|
| V4 (baseline) | SEC negentropy + query intent | 0.676 | 0.499 | 0.784 | Best all-around |
| V6 | HSECRR multiplicative boost | 0.642 | 0.497 | 0.773 | Boost just reshuffles |
| V7 | Additive ch_name + ch_calls cosine | 0.642 | 0.521 | 0.839 | Real signal, context-dependent |
| V8 | Channel resonance profiles | 0.642 | 0.496 | 0.753 | Weaker than V4 |
| V9 | Entropy-weighted channels | 0.604 | 0.528 | 0.773 | Tokio best, signetai regressed |
| V10 | Entropy-gated V7 | 0.625 | 0.507 | 0.779 | Nowhere |
| V11 | Character-level bigram HRR | 0.623 | 0.470 | 0.771 | Too granular, noisy |

Key findings from the experiments:
- **ch_name cosine has real signal** (6.8x separation) but hurts when applied indiscriminately
- **Additive features beat multiplicative boosts** — reshuffling vs. genuine promotion
- **The walk pipeline is well-tuned** — all walk modifications produced zero improvement
- **Gating is the answer** — only trust signals when they're confident

---

## Priority 1: Tokio Regression — OPEN

Tokio remains the hard case. Generic function names (`run`, `handle`, `poll`) make structural and holographic signals unreliable. The holographic gate correctly stays mostly closed for tokio, but the underlying graph walk still introduces noise.

### Possible experiments:

1. **Seed-only fallback for generic queries** — When query terms have low average IDF (common terms like "task", "handle", "runtime"), skip the walk entirely and use pure BM25 ordering with coverage/name tiebreaking.

2. **Name specificity bonus** — For seeds where the name matches high-IDF terms (above median), apply an additional multiplicative bonus. Rewards seeds with specific name matches (`tcp_linger`) over seeds with generic name matches (`configure_socket`).

---

## Priority 2: Expand the Benchmark — DONE

30-query MRR benchmark sets for all 3 codebases. Stable, statistically meaningful results.

### Remaining:

1. **Add more codebases** — Test on Python (Django/Flask), Go (Kubernetes client), Java (Spring). Different ecosystems have different graph topology characteristics.
2. **Statistical significance testing** — Bootstrap resampling to determine whether MRR differences are significant at 30 queries.

---

## Priority 3: Architecture Cleanup

The codebase has accumulated dead code from 9+ retrieval experiments.

### Actions:

1. **Remove dead retrieval systems** — HRR, HRR v2, AFMO, Spectral, LSA, AF26, Holo, Windtunnel are all superseded by Goober. Archive to `legacy/` or remove from build.

2. **Consolidate CruncherV1/V2** — Superseded by Goober. Can remain as references.

3. **Remove unused index fields** — `bridging`, `sig_terms`, `top_idf` are computed but never used by Goober. Clean up `build_cruncher_index`.

4. **Simplify the bench tool** — Remove CRv1/CRv2 columns once Goober is confirmed as default, or keep for regression tracking.

---

## Priority 4: Production Readiness

### Actions:

1. **Wire GooberV5 into the search pipeline** — Replace CruncherV2 with GooberV5 in `search.rs` so `graphiq search` uses it.

2. **Latency profiling** — Measure GooberV5's p50/p99 latency vs CruncherV2. The holographic index adds build time but minimal query time.

3. **Memory usage** — Profile peak allocation during search with the HoloIndex.

4. **Fuzz testing** — Run arbitrary query strings through GooberV5 to catch panics on edge cases (empty queries, single-character queries, very long queries, Unicode).
