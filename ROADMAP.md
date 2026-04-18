# GraphIQ Roadmap

## Current State (GooberV3 — NG-augmented)

### 30-query MRR benchmark

| Codebase | BM25 MRR | Goober v2 MRR | **GooberV3 MRR** | vs v2 | vs BM25 |
|---|---|---|---|---|---|
| signetai | 0.556 | 0.626 | **0.676** | **+0.050** | **+0.120** |
| tokio | 0.583 | 0.513 | **0.512** | ~same | -0.071 |
| esbuild | 0.675 | 0.777 | **0.806** | **+0.029** | **+0.131** |

GooberV3 adds Non-Gaussianity (NG) scoring on top of Goober v2. NG measures how far a symbol's 7-channel SEC score vector deviates from a uniform/Gaussian distribution. Symbols with spiky channel profiles (a few channels dominate) are more specific matches. Channel coherence (bispectrum analog) measures whether the same query terms hit multiple channels simultaneously.

The NG boost uses negentropy (entropy gap from uniform) + excess kurtosis weighting, with `1.0 + 0.25 * ng_norm + 0.15 * coherence_norm` as the multiplicative factor.

### 10-query benchmark (legacy reference)

| Codebase | BM25 MRR | Goober MRR | GooberV3 MRR |
|---|---|---|---|
| signetai | 0.720 | 0.764 | 0.659 |
| tokio | 0.508 | 0.393 | 0.483 |
| esbuild | 0.562 | 0.681 | 0.798 |

---

## Priority 1: Fix the Tokio Regression — PARTIALLY DONE

**Completed**: Capped structural norms at 0.5 to prevent BM25 override when seed scores are close. This improved tokio from 0.343 to 0.393 (+0.050 on 10-query) and esbuild from 0.631 to 0.681 (+0.050).

**Tested and reverted** (no improvement or regression):
- sqrt(name_norm) — no effect on tokio
- Adaptive BM25 weight by gap ratio — no effect (BM25 gap already tiny)
- Multiplicative BM25 formula — hurt esbuild
- Confidence lock threshold 1.05 (from 1.2) — hurt esbuild

### Remaining actionable experiments:

1. **Non-linear name scoring** — Replace additive name score with `sqrt(sum)` or `max(term_scores)` to penalize many weak matches vs few strong ones. This directly addresses the 3-weak > 2-strong problem.

2. **Adaptive BM25 weight** — Scale the BM25 weight by the BM25 gap: when rank-1 and rank-2 are close, increase BM25 dominance to preserve seed ordering. When the gap is large, allow coverage/name to differentiate. Formula: `W_BM25 = 3.0 + 5.0 * (1.0 - min(bm25_gap, 1.0))`.

3. **Seed-only fallback for uncertain queries** — When BM25 rank-1/rank-2 ratio < 1.2 (no confidence lock), skip the walk entirely and use pure BM25 ordering with just coverage/name tiebreaking. The walk only adds value when it can find candidates BM25 misses, which happens primarily on domain-specific codebases (signetai) not generic utility codebases (tokio).

4. **Name specificity bonus** — For seeds where the name matches high-IDF terms (above median), apply an additional multiplicative bonus. This rewards seeds with specific name matches (`tcp_linger`) over seeds with generic name matches (`configure_socket`).

---

## Priority 2: Expand the Benchmark — DONE

Added 30-query MRR benchmark sets for all 3 codebases. The 30-query results confirm Goober's improvements are stable (not artifacts of small sample size):
- signetai: +0.069 MRR over BM25 (10-query: +0.044)
- esbuild: +0.102 MRR over BM25 (10-query: +0.119)
- tokio: regression reduced to -0.070 (10-query suggested -0.115)

### Remaining actions:

1. **Expand to 30+ queries per codebase** — Cover more query categories: cross-cutting concerns, multi-hop relationships, behavioral queries, error handling patterns.

2. **Add more codebases** — Test on Python (Django/Flask), Go (Kubernetes client), Java (Spring). Different language ecosystems may have different graph topology characteristics.

3. **Stratified evaluation** — Report MRR by query category (symbol-exact, nl-abstract, error-debug, cross-cutting) to understand where each approach wins.

4. **Statistical significance testing** — Use bootstrap resampling to determine whether MRR differences are statistically significant at the 10-query scale.

---

## Priority 3: Query Understanding — IN PROGRESS (NG scoring done)

GooberV3 added Non-Gaussianity scoring derived from SEC channel analysis. The remaining query understanding work:

### Completed:
- **Non-Gaussianity (NG) scoring** — Negentropy + excess kurtosis of 7-channel SEC score vectors. Symbols with non-Gaussian channel profiles (spiky, specific) get boosted over symbols with flat/uniform profiles. Implemented as multiplicative `ng_boost` in `goober_v3_search`.
- **Channel coherence (bispectrum analog)** — Measures whether the same query terms hit multiple SEC channels simultaneously. Second-order correlation that linear scoring can't capture.

### Remaining actions:

1. **Query type detection** — Classify queries as navigational (symbol name lookup), informational ("how does X work"), or structural ("callers of X"). Each type may benefit from different scoring weights.

2. **Intent-aware walk strategy** — Navigational queries should use minimal walk (BM25 is sufficient). Informational queries may benefit from deeper walks. Structural queries should follow specific edge types (calls/references).

3. **Query expansion via search_hints** — The `search_hints` column already contains behavioral role tags and structural motifs. Use these for query expansion when the query matches a role pattern.

---

## Priority 4: Walk Quality Improvements

The IDF-gated walk was Goober's key innovation. It can be improved.

### Actions:

1. **Edge-type-aware walk** — Currently all edge types have the same walk behavior. For queries about behavior, follow call edges. For queries about type relationships, follow extends/implements edges. For queries about file organization, follow contains edges.

2. **Walk candidate rescoring** — After the walk, rescore candidates using their local graph density. Candidates in sparse regions (few neighbors matching query terms) are likely noise. Candidates in dense regions (many neighbors matching) are likely in a relevant cluster.

3. **Adaptive walk depth** — Use seed quality to determine walk depth. High-quality seeds (strong name match + high BM25) get deeper walks. Low-quality seeds get shallow walks to minimize noise.

---

## Priority 5: Architecture Cleanup

The codebase has accumulated dead code from 9+ retrieval experiments.

### Actions:

1. **Remove dead retrieval systems** — HRR, HRR v2, AFMO, Spectral, LSA, AF26, Holo, Windtunnel are all superseded by Goober. Keep them in a `legacy/` archive if needed for reference, but remove from the build.

2. **Consolidate CruncherV1/V2/Goober** — CruncherV1 and V2 are superseded by Goober. Keep Goober as the primary engine. V1/V2 can remain as references.

3. **Remove unused index fields** — `bridging`, `sig_terms`, `top_idf` are computed but never used by Goober. Clean up `build_cruncher_index`.

4. **Simplify the bench tool** — Remove CRv1/CRv2 columns from the bench output once Goober is confirmed as the default. Or keep for regression tracking.

---

## Priority 6: Production Readiness

### Actions:

1. **Wire Goober into the search pipeline** — Replace CruncherV2 with Goober in `search.rs` so `graphiq search` uses it.

2. **Latency profiling** — Measure Goober's p50/p99 latency vs CruncherV2. The reduced walk breadth should make it faster.

3. **Memory usage** — Goober's simpler candidate structure should use less memory. Profile peak allocation during search.

4. **Fuzz testing** — Run arbitrary query strings through Goober to catch panics on edge cases (empty queries, single-character queries, very long queries, Unicode).
