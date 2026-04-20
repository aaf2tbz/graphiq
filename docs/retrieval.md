# Retrieval Engine

GraphIQ's retrieval engine is a 6-layer pipeline that combines BM25 text search with structural graph analysis, spectral heat diffusion, and predictive deformation. Every layer is deterministic — no LLMs, no neural embeddings, no learned weights.

## The Pipeline

```
Query: "rate limit middleware"
        |
        +-- Hot Cache hit? --> return (< 1ms)
        v
+---------------------+
|  Layer 1: BM25/FTS  |  ~5ms   --> 30 seeds
|  Identifier-aware   |  rateLimit, rate_limit, middleware all match
+----------+----------+
           v
+------------------------------------------+
|  Layer 2: Spectral Expansion              |  --> ~100 candidates
|  Chebyshev heat diffusion on graph Laplacian |
|  O(K|E|) per query, no eigendecomposition  |
+----------+-------------------------------+
           v
+------------------------------------------+
|  Layer 3: Query Deformation              |
|  Predictive surprise (D_KL free energy)  |
|  Channel capacity routing (role blending) |
|  MDL explanation sets (coverage stopping) |
+----------+-------------------------------+
           v
+------------------------------------------+
|  Layer 4: SEC + NG Scoring                |
|  Intent-adjusted weights                 |
|  Negentropy + channel coherence boost     |
+----------+-------------------------------+
           v
+------------------------------------------+
|  Layer 5: Holographic Name Gate           |
|  FFT cosine similarity per candidate      |
|  Threshold gate (0.25) + specificity      |
+----------+-------------------------------+
           v
+------------------------------------------+
|  Layer 6: Confidence-Preserving Fusion    |  --> top_k
|  BM25 confidence lock, kind boosts,       |
|  test penalties, per-file diversity       |
+------------------------------------------+
```

## Layer 1: BM25/FTS

SQLite FTS5 with per-column weights:

| Column | Weight | Content |
|---|---|---|
| name | 10.0 | Symbol name |
| decomposed | 8.0 | Identifier decomposition (`RateLimiter` → `rate`, `limiter`) |
| qualified | 6.0 | Fully qualified name |
| hints | 5.0 | Structural role descriptions, morphological variants, motif terms |
| sig | 4.0 | Function signatures |
| file_path | 3.5 | Path components |
| doc | 3.0 | Doc comments |
| source | 1.0 | Source code body |

The `hints` column is the secret weapon. At index time, GraphIQ infers 19 behavioral role tags (validator, cache, handler, retry, auth-gate, etc.) and 8 structural motifs (connector, orchestrator, hub, guard, transform, sink, source, leaf) from symbol names, call patterns, and file paths. These get written into the FTS hints column so BM25 matches role vocabulary at zero query-time cost. A function that checks cache validity gets hints like "cache validate check verify" — so the query "validate cache entry" finds it even if the function is named `ensureFreshness`.

BM25 returns the top 30 seeds in ~5ms. These seeds are the starting point for all downstream scoring.

## Layer 2: Spectral Expansion

BM25 seeds are expanded via Chebyshev polynomial approximation of the graph Laplacian's heat kernel. This replaces the BFS graph walk used in earlier versions (GooberV1–V5).

### Chebyshev Heat Diffusion

The graph Laplacian L = D⁻¹ᐟ²(D - W)D⁻¹ᐟ² captures the graph's connectivity structure. The heat kernel e^(-tL) propagates signal from seed nodes across structural distance — symbols close in the call/import/type graph receive more heat than distant ones.

Direct computation requires eigendecomposition (O(n³)). Chebyshev approximation computes the heat kernel in O(K|E|) per query where K is the polynomial order (15):

```
f(T̃) ≈ Σ cₖ Tₖ(T̃)    (Clenshaw quadrature for coefficients)
T̃ = 2L/λ_max - I       (rescaled Laplacian to [-1, 1])
```

Only one parameter matters: `chebyshev_order=15`. Heat_t (0.3–5.0) and walk_weight (1.0–10.0) are remarkably insensitive across their full ranges (673 combinations tested on esbuild).

### Candidate Gating

Heat-diffused candidates must pass two gates before entering scoring:
1. **IDF gate**: Match at least one query term with above-median IDF (filters generic utility functions)
2. **Coverage gate**: Match at least one query term in the symbol's own text (prevents pure structural-only hits)

## Layer 3: Query Deformation

Three adaptive signals that reshape scoring based on each query's structural context. These are computed per-query from pre-built infrastructure (predictive model + channel fingerprints), not at index time.

### 3A: Predictive Surprise (Free Energy)

For each symbol, a conditional term model is built from its 1-hop structural neighborhood — the symbol's own terms (weight 2) plus its neighbors' terms (weight 1), Laplace-smoothed (α=0.1) against a 5K-term background vocabulary.

At query time, the Kullback-Leibler divergence D_KL(query || symbol_predicted) measures how surprising the query is given the symbol's graph context:

```
surprise = Σ_q p(q) × log(p(q) / p_cond(q))
```

High KL means the query terms are unlikely under this symbol's model. After normalization across the candidate pool, this becomes a mild discriminative boost (0.08 weight) — symbols whose neighborhoods align with the query terms get a slight edge over loosely-related candidates.

**What it helps**: `symbol-partial` queries on esbuild (+0.029 NDCG). Short common words like "embedding" or "cache" are ambiguous to BM25 but have very different neighborhood distributions — the surprise signal disambiguates them.

### 3B: Channel Capacity Routing

Replaces the binary Navigational/Informational intent classifier with data-driven weight adjustments. Each symbol has a structural role (from Phase 10B ChannelFingerprints):

| Role | Meaning | Weight Adjustment |
|---|---|---|
| orchestrator | Calls many functions | +0.4 coverage, +0.1 NG |
| library | Called by many, self-contained | +0.5 BM25 |
| boundary | High-entropy edge distribution | +0.3 coverage, +0.1 coherence |
| worker | Moderate call graph | +0.3 BM25 |
| isolate | Few edges | +0.4 BM25 |

The adjustments are **additive** to the intent-based weights, not replacements. This was critical — replacing weights entirely caused NDCG regression on all 3 codebases.

### 3C: MDL Explanation Sets

After scoring and ranking, a greedy set cover tracks which query terms each result explains. It stops when the marginal information gain per symbol cost drops below 0.05:

```
for each result in ranked order:
    new_terms = terms explained by this symbol not yet covered
    if new_terms == 0: skip
    cost = 1 + log(rank) × 0.5
    efficiency = |new_terms| / (n_terms × cost)
    if efficiency < 0.05: STOP
```

A diversity bonus (up to 0.15) rewards explanation sets that span multiple structural roles. The MDL score is applied as a multiplier on final scores, but only when >50% of query terms are covered (otherwise disabled to avoid penalizing short queries).

## Layer 4: SEC + Intent Scoring

### Query Intent Classification

Queries are classified as either **navigational** or **informational**:

- **Navigational**: Symbol lookups, exact name queries, path-based queries. These benefit from preserving BM25 ordering — the user knows what they want, and BM25 is already right.
- **Informational**: "How does X work", "handle Y", abstract descriptions. These benefit from deeper structural exploration — the user doesn't know the exact name, so the graph walk finds relevant symbols BM25 might miss.

Intent provides the base scoring weights (navigational caps structural norms lower). Channel capacity routing from Layer 3 adds additive adjustments on top.

### Structural Evidence Convolution (SEC)

SEC propagates terms through the code graph's 7 structural channels:

```
                    Symbol: "RateLimiter"
                              |
        +---------+---------+---------+---------+---------+---------+
        |         |         |         |         |         |         |
      self    calls_out  calls_in  2hop_out  2hop_in  type_ret  file_path
     (3.0)     (1.5)     (1.5)     (0.7)     (0.7)     (1.0)     (0.5)
        |         |         |         |         |         |         |
    "rate"    "check"   "handle"   "retry"    "api"    "bool"    "middleware"
    "limit"   "enqueue"  "request"  "backoff"           "result"  "rate_limit"
    "limiter" "reject"   "route"
```

Each channel collects terms from the symbol's graph neighborhood with distance-based decay:
- **self** (3.0): the symbol's own identifier decomposition + body terms
- **calls_out** (1.5): terms from functions this symbol calls
- **calls_in** (1.5): terms from functions that call this symbol
- **calls_out_2hop** (0.7): 2-hop call graph traversal
- **calls_in_2hop** (0.7): 2-hop reverse call graph traversal
- **type_ret** (1.0): return type decomposition
- **file_path** (0.5): path components

The result: queries like "how does the timer wheel expire" find `process_expired_timers` because SEC propagated "timer" and "expire" through the call graph even though the query never uses the word "process".

### Non-Gaussianity (NG) Scoring

NG measures how far a symbol's 7-channel SEC score vector deviates from a uniform/Gaussian distribution. Symbols with **spiky** channel profiles (a few channels dominate) are more specific matches than symbols with flat profiles (all channels contribute equally).

Two components:
1. **Negentropy** — The entropy gap between the channel distribution and a uniform distribution. Higher negentropy = more specific.
2. **Channel coherence** — Whether the same query terms hit multiple channels simultaneously. A second-order correlation that linear scoring can't capture.

Applied as a multiplicative boost: `ng_boost = 1.0 + 0.25 * ng_norm + 0.15 * coherence_norm`

This means NG can only promote candidates, never demote them. A symbol with a flat, generic channel profile gets boost 1.0 (no change). A symbol where query terms concentrate in specific channels gets boosted proportionally.

## Layer 5: Holographic Name Gate

The most recent addition and the one that required the most experimentation to get right.

### The Signal

Each symbol's identifier terms are encoded as a holographic vector using FFT-based circular convolution. Given a query, its terms are similarly encoded. The cosine similarity between query and candidate name holograms has **6.8x separation** between correct and incorrect matches — a strong, real signal.

### The Problem

Adding this signal indiscriminately caused false promotions on codebases with generic names. The cosine similarity for moderately-similar but wrong matches (0.1–0.3 range) was enough to promote irrelevant symbols, especially on tokio where function names like `run`, `handle`, `poll` produce spurious holographic matches.

### The Solution: Per-Candidate Gating

The holographic contribution is **gated** per candidate:

```rust
let holo_gate = 0.25;
let holo_max_w = 2.0;

let holo_additive = if candidate.holo_name_sim > holo_gate {
    let excess = (candidate.holo_name_sim - holo_gate) / (1.0 - holo_gate);
    let w = holo_max_w * query_specificity * excess;
    w
} else {
    0.0
};
```

Two controls:

1. **Confidence gate** (0.25 threshold): Only candidates with holographic cosine > 0.25 receive any boost. Below this, the signal is unreliable and the contribution is exactly 0.
2. **Query specificity scaling**: `query_specificity` is the fraction of query terms with IDF > 1.0 (i.e., rare/unusual terms). Specific queries like "encode VLQ source map" have high specificity and get more holographic weight. Broad queries like "handle request" have low specificity and get less.

The holographic boost is **additive** (not multiplicative) to the base score, which matters — multiplicative boosts just reshuffle existing rankings without adding new signal. Additive contributions can genuinely promote candidates that the base score would miss.

### Why This Works

On esbuild (descriptive names like `convertOKLCHToOKLAB`, `lowerAndMinifyCSSColor`), correct matches have high holographic similarity (>0.5) that easily passes the gate. On tokio (generic names like `run`, `poll`), correct matches have low similarity (<0.2) that stays gated out. The gate adapts to the codebase without any codebase-specific tuning.

## Layer 6: Confidence-Preserving Fusion

Final ranking applies kind boosts (functions/types over variables/imports), test penalties (demote test files in production queries), and per-file diversity limits (max 3 results from the same file).

## Why Not Neural Embeddings?

We tested. Neural embeddings at the 137M parameter scale (jina-code, nomic-embed) produced net-negative NDCG when used as rerankers. The signal from identifier decomposition + graph structure is stronger than what small embedding models provide, and the retrieval pipeline exploits it more effectively.

The holographic name matching in Layer 4 is the closest analog to embedding-based similarity, but it's fully deterministic — no model training, no weights to update, no GPU required. Pure FFT operations on hashed term vectors.
