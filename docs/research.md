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

Multiplicative boosts (V6) just reshuffle existing rankings. Additive contributions (V7, V5) can genuinely promote candidates the base score would miss. But additive contributions need gating to prevent false promotions from moderate-similarity noise.

### 5. Gate your signals

The raw holographic cosine has 6.8x separation — strong signal. But adding it indiscriminately caused false promotions. The gate (threshold 0.25 + query specificity scaling) turned a net-negative feature into a net-positive across all codebases. The gate adapts to the codebase: descriptive names (esbuild) pass the gate, generic names (tokio) don't. No codebase-specific tuning required.

### 6. Codebase characteristics matter more than query characteristics

Tokio is hard because its function names are generic. Esbuild is easy because its names are descriptive. Signetai is in between. The retrieval system needs to be robust across all three — a system that overfits to one codebase's characteristics will fail on another.

### 7. Aggregate MRR is misleading

Optimizing aggregate MRR led to over-fitting on easy queries while ignoring hard ones. Better approach: pick decisive case studies (hard NL queries where BM25 fails) and treat them like a test suite.

## What Didn't Work

- **Walk tuning** (edge types, density, adaptive depth): The walk pipeline is well-tuned. All modifications produced zero improvement.
- **Character-level encoding** (bigram HRR): Too granular, introduced noise. Term-level encoding works better.
- **Channel resonance profiles** (shape matching): Weaker than simple negentropy.
- **Entropy weighting**: Helped tokio, hurt signetai. Not robust across codebases.

## Open Questions

- **Tokio regression**: The structural walk hurts on codebases with generic function names. Possible fix: seed-only fallback for queries with low average IDF.
- **More codebases**: Current benchmark covers TS, Rust, Go. Need Python, Java to validate generalizability.
- **Statistical significance**: 30 queries is small. Bootstrap resampling would help determine whether differences are real.

## Cross-References to New Roadmap

These precedents from failed experiments are directly relevant to the new phases:

| Failed Experiment | New Phase | Relationship |
|---|---|---|
| AFMO bandpass (`afmo.rs:82-130`) | Phase 7 (anisotropic W) | AFMO used σ-only diagonal weighting with a 100x amplification bug. Phase 7 adds `discᵢ` discriminativity term. The key difference: AFMO weighted by variance alone; Phase 7 weights by variance × non-uniformity. |
| V9 entropy weighting | Phase 7 Step B (discᵢ risk) | `discᵢ` is conceptually similar to entropy weighting. V9 "helped tokio, hurt signetai" — not robust across codebases. Phase 7 Step F ablation is designed to catch this same pattern early. |
| Evidence BFS (`evidence.rs`) | Phase 8 Step B (reinforcing) | Evidence computed multi-path convergence at candidate level. Phase 8 reuses the same BFS machinery at per-edge granularity. |
| Walk tuning null result | Phase 8 Step C (evidence-aware walk) | Walk tuning of existing edge-type weights produced zero improvement. Evidence profiles add new signals (multiplicity, boundary, motif) not tested in those experiments. Must validate against the null result. |
| Spectral (`spectral.rs`) | Phase 9 Step A (subsystem detection) | Spectral graph coordinates via Lanczos produced "interesting, not useful" results. Phase 9 must NOT use eigen-decomposition for subsystem detection — use modularity-based community detection on evidence-weighted edges instead. |
| Isotropic LSA (`lsa.rs`) | Phase 7 (anisotropic correction) | Isotropic LSA captured patterns already in BM25. Phase 7 is the same SVD + same structural augmentation but with anisotropic normalization that suppresses generic dimensions. |
