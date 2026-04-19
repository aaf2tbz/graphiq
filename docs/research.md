# Research Notes

What we learned building GraphIQ's retrieval engine. This document covers the experimental history and the lessons that shaped the current system.

## Experimental Timeline

### Phase 1: Can we beat BM25?

We built 9 standalone retrieval systems. None beat BM25 on MRR across all codebases.

| System | Approach | Verdict |
|---|---|---|
| SEC | Structural Evidence Convolution (inverted index) | Good on specific codebases, can't beat BM25 generally |
| Evidence | Adjacency-based evidence propagation | Net negative |
| HRR | Holographic Reduced Representations (1024-dim) | Net negative |
| HRR v2 | Improved HRR with hypersphere normalization | Slightly less negative |
| AFMO | Adaptive Feature Map Optimization | No improvement |
| Spectral | Spectral graph coordinates (Lanczos) | Interesting, not useful |
| LSA | Truncated SVD / Latent Semantic Analysis | Capture patterns already in BM25 |
| AF26 | 26-dimensional feature vector scoring | Overfitting |
| Holo | Full holographic encoding + matching | Signal too noisy standalone |

**Lesson**: BM25's inverted index is O(1) — no full-scan system can compete on speed, and its ranking is remarkably hard to beat on correctness. The winning pattern is always: **BM25 retrieves, structural math reranks**.

### Phase 2: Goober (BM25 + structural reranking)

Stripped everything from CruncherV2 that wasn't helping:
- Removed: energy vectors, cosine interference, hub dampening, bridging potential, yoyo validation
- Kept: BM25-dominant seed scoring, IDF-gated walk, confidence lock

Result: simpler system that strictly outperformed CruncherV2 on all 3 codebases. **Removing complexity improved results.**

### Phase 3: V3→V4 — SEC channel analysis

**GooberV3** added Non-Gaussianity scoring. SEC's 7 channels produce a score vector per candidate. Candidates with non-Gaussian (spiky, specific) channel profiles get boosted over flat/uniform ones. Negentropy + channel coherence as the boost formula.

**GooberV4** added query intent classification. Navigational queries (symbol lookups) and informational queries ("how does X work") get different scoring weights. Navigational queries cap structural norms lower to preserve BM25 ordering. This helped tokio slightly.

### Phase 4: V5–V11 — Holographic name matching experiments

The core signal: holographic cosine similarity between query and candidate name terms has **6.8x separation** between correct and incorrect matches. Clearly a strong signal. But how to use it without causing false promotions?

| Version | Strategy | signetai | tokio | esbuild | Verdict |
|---|---|---|---|---|---|
| V4 (baseline) | SEC negentropy + query intent | 0.676 | 0.499 | 0.784 | Best all-around |
| V6 | Multiplicative HSECRR boost | 0.642 | 0.497 | 0.773 | Boost just reshuffles |
| V7 | Additive ch_name + ch_calls cosine | 0.642 | 0.521 | **0.839** | Real signal, context-dependent |
| V8 | Channel resonance profiles | 0.642 | 0.496 | 0.753 | Weaker than V4 |
| V9 | Entropy-weighted channels | 0.604 | **0.528** | 0.773 | Tokio best, signetai regressed |
| V10 | Entropy-gated V7 | 0.625 | 0.507 | 0.779 | Nowhere |
| V11 | Character-level bigram HRR | 0.623 | 0.470 | 0.771 | Too granular, noisy |

### Phase 5: Final V5 — Per-candidate gating

The breakthrough: don't add the holographic signal to every candidate. **Gate it.**

Only candidates with cosine > 0.25 receive the holographic boost, scaled by query specificity. Below the threshold, contribution is exactly 0.

Result: V5 beats V4 on all 3 codebases simultaneously — the first version to do so.

### Phase 6: Spectral graph infrastructure

Upgraded spectral.rs: SPECTRAL_DIM 6→50, added eigenvalue/lambda_max tracking, Chebyshev polynomial heat kernel (O(K|E|) per query without eigendecomposition), harmonic extension (Jacobi iterative Dirichlet solver). Built `SparseGraph` with structural edge tracking separate from term-overlap edges.

### Phase 9: Geometric search pipeline

Replaced V5's BFS walks with Chebyshev heat diffusion on the graph Laplacian. Same V5 scoring framework, but candidates come from spectral diffusion instead of graph walks. Ran 673 parameter combinations on esbuild — discovered chebyshev_order=15 is the only meaningful parameter. Heat_t (0.3–5.0) and walk_weight (1.0–10.0) are remarkably insensitive.

**Geometric matched GooberV5 on first pass with zero tuning.** Then surpassed it on tokio (0.368 vs 0.367) and signetai (0.443 vs 0.444) after parameter tuning.

### Phase 10: Structural geometry

**10A: Ricci curvature.** Implemented Ollivier-Ricci curvature on structural edges. Fixed O(n²) hang by separating structural from term-overlap edges (5.6M → 6.8K on tokio). Tested as curvature-weighted matvec and post-diffusion reranker — no improvement. **Lesson: compute geometry, don't score it.** Ricci is structural infrastructure, not a scoring feature.

**10B: Channel fingerprints.** 7-dim per-symbol edge-type distribution vector + entropy + role classification (orchestrator/library/boundary/isolate/worker). Query-independent infrastructure for Phase 11.

### Phase 11: Query as Deformation

Three new signals that make the retrieval pipeline adaptive to each query's structural context:

**11A: Predictive Surprise (Free Energy).** For each symbol, built a conditional term model from its 1-hop structural neighborhood (calls, imports, etc.) with Laplace smoothing over a 5K-term vocabulary. At query time, D_KL(query || symbol_predicted_terms) measures how surprising the query is given the symbol's graph context. High surprise = the query's terms are unexpected in this symbol's neighborhood, suggesting a novel/relevant match. Applied as `surprise_boost` at 0.08 weight.

**11B: Channel Capacity Routing.** Replaced the binary Navigational/Informational classifier with data-driven weight adjustments based on the structural roles of seed symbols. Orchestrator seeds get more coverage weight (they're calling into many things), library seeds get more BM25 weight (they're self-contained). Uses ChannelFingerprint roles weighted by BM25 score influence. Applied as additive adjustments to the intent-based weights — not replacement, augmentation.

**11C: MDL Explanation Sets.** Greedy set cover over ranked results tracking which query terms each symbol explains. Stops when marginal information gain per symbol cost drops below 0.05 (efficiency threshold). Includes diversity bonus from role variety. Applied as a multiplier on final scores.

**Result: no regressions, gains on weak categories.**

| Codebase | Geometric NDCG | Deformed NDCG | Delta |
|---|---|---|---|
| Signetai | 0.441 | 0.440 | -0.001 |
| Tokio | 0.368 | 0.371 | +0.003 |
| Esbuild | 0.501 | 0.510 | +0.009 |

`symbol-partial` improved most (+0.029 on esbuild) — the surprise signal disambiguates short common words like "embedding", "cache", "channel".

**Key lesson: additive beats replacement.** Initial attempt used channel capacity to replace Nav/Info weights entirely → NDCG regression on all 3 codebases. Switching to additive adjustments eliminated regressions and preserved gains.

## Key Lessons

### 1. BM25 is hard to beat

Every system that tried to replace BM25 failed. The winning pattern is always BM25 retrieves + structural math reranks. BM25's inverted index is O(1) and its ranking is remarkably good for code search where identifiers carry meaning.

### 2. Simpler is better

CruncherV2 had 6 scoring mechanisms. Goober has 3. Goober wins everywhere. The complex interference mechanics captured patterns already captured by simpler coverage + name scoring, while introducing noise on codebases with generic function names.

### 3. Confidence matters

Two forms of confidence preservation:
- **BM25 confidence lock**: When BM25 rank-1 has a >1.2x gap, lock it. Demoting confident BM25 results is almost always wrong.
- **Signal confidence gate**: When a secondary signal (holographic, structural) is only moderately confident, don't use it. Only apply signals when they're strongly confident.

### 4. Additive beats multiplicative

Multiplicative boosts (V6) just reshuffle existing rankings. Additive contributions (V7, V5) can genuinely promote candidates the base score would miss. But additive contributions need gating to prevent false promotions from moderate-similarity noise. This lesson recurred in Phase 11: replacing weights entirely caused regressions; adding adjustments preserved gains.

### 5. Gate your signals

The raw holographic cosine has 6.8x separation — strong signal. But adding it indiscriminately caused false promotions. The gate (threshold 0.25 + query specificity scaling) turned a net-negative feature into a net-positive across all codebases. The gate adapts to the codebase: descriptive names (esbuild) pass the gate, generic names (tokio) don't. No codebase-specific tuning required.

### 6. Compute geometry, don't score it

Ricci curvature is a real structural signal (Ollivier-Ricci on the code graph). But using it as a scoring feature — curvature-weighted diffusion, per-node average curvature as a boost — produced no improvement. The geometry is infrastructure, not a ranking signal. The heat diffusion that exploits the graph topology is useful; the curvature of that topology is not (at least not for ranking).

### 7. Chebyshev order is the dominant parameter

Ran 673 parameter combinations for heat diffusion. Only chebyshev_order matters (15 is best). Heat_t, walk_weight, and heat_top_k are remarkably insensitive across their full ranges. This suggests the diffusion's sensitivity is in the polynomial approximation quality, not the physical time constant or expansion breadth.

### 8. Codebase characteristics matter more than query characteristics

Tokio is hard because its function names are generic. Esbuild is easy because its names are descriptive. Signetai is in between. The retrieval system needs to be robust across all three — a system that overfits to one codebase's characteristics will fail on another.

### 9. Aggregate MRR is misleading

Optimizing aggregate MRR led to over-fitting on easy queries while ignoring hard ones. Better approach: pick decisive case studies (hard NL queries where BM25 fails) and treat them like a test suite.

## What Didn't Work

- **Walk tuning** (edge types, density, adaptive depth): The walk pipeline is well-tuned. All modifications produced zero improvement.
- **Character-level encoding** (bigram HRR): Too granular, introduced noise. Term-level encoding works better.
- **Channel resonance profiles** (shape matching): Weaker than simple negentropy.
- **Entropy weighting**: Helped tokio, hurt signetai. Not robust across codebases.
- **Ricci curvature scoring**: Real geometric signal, but not useful as a ranking feature. Compute it, don't score with it.
- **LSA reranker**: Helps signetai MRR +0.025, hurts tokio NDCG -0.020. Removed from pipeline.
- **SEC reranker**: Hurts ALL three codebases. Removed from pipeline.
- **Channel capacity weight replacement**: Replacing Nav/Info weights with channel-derived weights regressed NDCG on all codebases. Additive adjustments work.

## Open Questions

- **Weak categories**: file-path (0.000 everywhere), nl-abstract (~0.000–0.098), and cross-cutting (~0.000–0.282) remain unsolved. These require fundamentally different approaches (file path routing, semantic abstraction, cross-cutting concern detection).
- **More codebases**: Current benchmark covers TS, Rust, Go. Need Python, Java to validate generalizability.
- **Statistical significance**: 30–47 queries per codebase is small. Bootstrap resampling would help determine whether differences are real.

## Cross-References to Roadmap

These precedents from failed experiments are directly relevant to future phases:

| Failed Experiment | Relationship |
|---|---|
| AFMO bandpass (σ-only weighting + 100x bug) | Any variance-based weighting must be robust across codebases |
| V9 entropy weighting | `discᵢ` is conceptually similar — must validate against V9's "helps tokio, hurts signetai" pattern |
| Evidence BFS | Multi-path convergence at candidate level — reuseable pattern |
| Walk tuning null result | Edge-type weights are well-tuned; new signals must be orthogonal |
| Spectral eigen-decomposition | "Interesting, not useful" standalone — useful as diffusion substrate only |
| Isotropic LSA | Captured patterns already in BM25; anisotropic correction didn't help either |
| Ricci curvature scoring | Geometry as infrastructure, not ranking signal |
| Channel capacity replacement | Additive adjustments work; full replacement causes regressions |
