# Retrieval Engine

GraphIQ's retrieval engine is a 5-layer pipeline that combines BM25 text search with structural graph analysis and holographic name matching. Every layer is deterministic — no LLMs, no neural embeddings, no learned weights.

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
|  Layer 2: Goober Reranker                 |  --> ~100 candidates
|  BM25-dominant seed scoring               |
|  IDF-gated graph walk (depth 2)           |
|  Walk evidence for non-seeds              |
+----------+-------------------------------+
           v
+------------------------------------------+
|  Layer 3: Query Intent + SEC              |
|  Navigational vs informational weights    |
|  NG scoring (negentropy + coherence)      |
+----------+-------------------------------+
           v
+------------------------------------------+
|  Layer 4: Holographic Name Gate           |
|  FFT cosine similarity per candidate      |
|  Threshold gate (0.25) + specificity      |
|  Only boosts confident matches            |
+----------+-------------------------------+
           v
+------------------------------------------+
|  Layer 5: Confidence-Preserving Fusion    |  --> top_k
|  If BM25 rank-1 has 1.2x+ gap, lock it   |
|  Kind boosts, test penalties              |
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

## Layer 2: Goober Reranker

The core insight behind Goober: **BM25 seed ordering is generally correct, and structural reranking must be conservative to avoid introducing noise.**

### Seed Scoring

Each BM25 seed gets scored with a BM25-dominant weighted sum:

```
seed_score = W_bm25 * bm25_norm + W_cov * min(cov_norm, cap) + W_name * min(name_norm, cap)
```

Weights depend on query intent (see Layer 3):
- **Navigational**: bm25=5.0, coverage=0.8, name=1.0 (preserve BM25 ordering)
- **Informational**: bm25=3.0, coverage=1.5, name=2.0 (allow more structural differentiation)

The cap prevents structural norms from overriding BM25 when seed scores are close — this was the key fix for codebases with generic function names.

### IDF-Gated Graph Walk

From the top 8 seeds, a BFS walk explores the code graph:
- **Depth**: 2 hops
- **Breadth**: 25 expanded candidates per seed
- **Gate**: Each candidate must match at least one query term with above-median IDF. This filters generic utility functions that match only common terms.
- **Quality filter**: Non-seed candidates require ≥2 seed paths (reached from multiple seeds). Single-path candidates are discarded.

Walk candidates are scored by coverage + name + walk evidence (coverage × proximity × edge weight).

### Confidence Lock

If BM25's rank-1 result has a >1.2x score gap over rank-2, it gets locked at position 1. Demoting a confident BM25 result is almost always a mistake — the confidence lock prevents the graph walk from inserting wrong candidates above correct results.

## Layer 3: Query Intent + SEC

### Query Intent Classification

Queries are classified as either **navigational** or **informational**:

- **Navigational**: Symbol lookups, exact name queries, path-based queries. These benefit from preserving BM25 ordering — the user knows what they want, and BM25 is already right.
- **Informational**: "How does X work", "handle Y", abstract descriptions. These benefit from deeper structural exploration — the user doesn't know the exact name, so the graph walk finds relevant symbols BM25 might miss.

Intent affects scoring weights (navigational caps structural norms lower) and walk aggressiveness.

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

## Layer 4: Holographic Name Gate

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

## Layer 5: Confidence-Preserving Fusion

Final ranking applies kind boosts (functions/types over variables/imports), test penalties (demote test files in production queries), and per-file diversity limits (max 3 results from the same file).

## Why Not Neural Embeddings?

We tested. Neural embeddings at the 137M parameter scale (jina-code, nomic-embed) produced net-negative NDCG when used as rerankers. The signal from identifier decomposition + graph structure is stronger than what small embedding models provide, and the retrieval pipeline exploits it more effectively.

The holographic name matching in Layer 4 is the closest analog to embedding-based similarity, but it's fully deterministic — no model training, no weights to update, no GPU required. Pure FFT operations on hashed term vectors.
