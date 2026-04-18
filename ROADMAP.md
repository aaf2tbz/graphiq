# GraphIQ Roadmap

## Current State (Goober v1)

| Codebase | BM25 MRR | Goober MRR | Delta |
|---|---|---|---|
| signetai | 0.720 | 0.764 | **+0.044** |
| tokio | 0.508 | 0.343 | -0.165 |
| esbuild | 0.562 | 0.631 | **+0.069** |

Goober beats BM25 on 2/3 codebases and beats CruncherV2 on all 3. The primary unsolved problem is tokio's regression from BM25.

---

## Priority 1: Fix the Tokio Regression

The tokio regression (-0.165 MRR vs BM25) is caused by seed-on-seed competition: the BM25-dominant scoring formula (3.0 * bm25 + 1.5 * coverage + 2.0 * name) can still be overridden when a wrong seed has substantially better coverage + name scores than the correct BM25 rank-1 seed.

**Root cause**: For "configure TCP socket linger timeout", the correct seed (`set_tcp_linger`, BM25 rank 1) matches "tcp" and "linger" (high IDF, 2 terms). A wrong seed matches "configure", "socket", "timeout" (low IDF, 3 terms). Despite IDF weighting, 3 low-IDF name matches accumulate more score than 2 high-IDF matches.

### Actionable experiments (in order):

1. **Non-linear name scoring** — Replace additive name score with `sqrt(sum)` or `max(term_scores)` to penalize many weak matches vs few strong ones. This directly addresses the 3-weak > 2-strong problem.

2. **Adaptive BM25 weight** — Scale the BM25 weight by the BM25 gap: when rank-1 and rank-2 are close, increase BM25 dominance to preserve seed ordering. When the gap is large, allow coverage/name to differentiate. Formula: `W_BM25 = 3.0 + 5.0 * (1.0 - min(bm25_gap, 1.0))`.

3. **Seed-only fallback for uncertain queries** — When BM25 rank-1/rank-2 ratio < 1.2 (no confidence lock), skip the walk entirely and use pure BM25 ordering with just coverage/name tiebreaking. The walk only adds value when it can find candidates BM25 misses, which happens primarily on domain-specific codebases (signetai) not generic utility codebases (tokio).

4. **Name specificity bonus** — For seeds where the name matches high-IDF terms (above median), apply an additional multiplicative bonus. This rewards seeds with specific name matches (`tcp_linger`) over seeds with generic name matches (`configure_socket`).

---

## Priority 2: Expand the Benchmark

10 queries per codebase is too small for statistical confidence. A single query changing rank shifts MRR by 0.05-0.10.

### Actions:

1. **Expand to 30+ queries per codebase** — Cover more query categories: cross-cutting concerns, multi-hop relationships, behavioral queries, error handling patterns.

2. **Add more codebases** — Test on Python (Django/Flask), Go (Kubernetes client), Java (Spring). Different language ecosystems may have different graph topology characteristics.

3. **Stratified evaluation** — Report MRR by query category (symbol-exact, nl-abstract, error-debug, cross-cutting) to understand where each approach wins.

4. **Statistical significance testing** — Use bootstrap resampling to determine whether MRR differences are statistically significant at the 10-query scale.

---

## Priority 3: Query Understanding

Goober currently treats all query terms equally. But "how does periodic memory compaction work" and "set TCP linger" have very different intent.

### Actions:

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
