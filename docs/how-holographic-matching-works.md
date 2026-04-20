# How Holographic Name Matching Works

Sources:
- [`crates/graphiq-core/src/cruncher.rs`](../crates/graphiq-core/src/cruncher.rs) — `HoloIndex`, `build_holo_index()`, `holo_query_name_cosine()` (lines 2296-2367)
- [`crates/graphiq-core/src/hoLO.rs`](../crates/graphiq-core/src/holo.rs) — full holographic encoding with graph structure

## The Problem

BM25 and coverage scoring operate on exact or substring token matches. "RateLimiter" matches "rate" and "limiter" because they're the decomposed tokens. But "throttle" doesn't match "RateLimiter" even though they're conceptually related — different tokens, different character sequences.

Holographic encoding addresses this by representing each name as a high-dimensional vector. Names that share decomposed terms end up with similar vectors (high cosine similarity). Names that share no terms are near-orthogonal (similarity ~0). This provides a graded similarity signal where exact token matching gives a binary match/no-match — "RateLimiter" vs "throttle" gets a low score rather than zero, and partial overlaps like "rate" shared between "rate limit" and "RateLimiter" get proportional credit.

## How It Works

### 1. Deterministic Random Vectors

Each unique term (from symbol names, decomposed identifiers, and IDF vocabulary) gets a 1024-dimensional random unit vector. The vector is deterministic: it's generated from a FNV-1a hash of the term string, used as a seed for a PCG-style PRNG that produces Gaussian-distributed values (Box-Muller transform). Same term always produces the same vector.

```
"rate"  -> hash("term:rate")  -> seed 1024-dim unit vector v_rate
"limit" -> hash("term:limit") -> seed 1024-dim unit vector v_limit
```

Random unit vectors in high dimensions have a useful property: any two random vectors are nearly orthogonal (dot product ~0). This means different terms occupy different "directions" in the 1024-dim space.

### 2. Symbol Holograms

For each symbol, its name's decomposed terms are summed and normalized:

```
h_i = normalize(sum(v_t for t in symbol_i.name_terms))
```

A symbol named "RateLimiter" decomposes to ["rate", "limiter"], so:
```
h_RateLimiter = normalize(v_rate + v_limiter)
```

### 3. Query Hologram

At query time, the query string is decomposed the same way:

```
q = normalize(sum(v_t for t in query_terms))
```

The query "rate limit" produces:
```
q = normalize(v_rate + v_limit)
```

### 4. Cosine Similarity

The similarity between query and symbol is the dot product of their normalized vectors:

```
sim(query, symbol) = dot(q, h_symbol) / (|q| * |h_symbol|)
```

This returns a value in [-1, 1]. For random vectors, expected similarity is ~0. For vectors that share terms, it's positive.

### Why This Helps

The query "rate limit" shares the term "rate" with "RateLimiter" but not "limit" vs "limiter". In exact matching, "limit" != "limiter". But in holographic space:

- "RateLimiter" = v_rate + v_limiter
- "rate limit" = v_rate + v_limit

Since v_limit and v_limiter are different random vectors, the similarity is reduced but not zero — v_rate still contributes. The holographic similarity provides a gradient rather than a binary match/no-match.

### 5. FFT Optimization (in `holo.rs`)

The standalone `holo.rs` module extends this with Fast Fourier Transform encoding for graph-aware holograms. Term vectors are FFT'd into frequency domain, where convolution (binding relationships) becomes element-wise multiplication. This encodes not just the symbol's name terms but its graph neighborhood structure into the hologram.

The pipeline uses the simpler `cruncher.rs` version (`holo_query_name_cosine`) which only encodes name terms — the graph structure is handled separately by the heat kernel and walk evidence.

### 6. Usage in Scoring

Holographic similarity enters the scoring formula as an additive term with a gate:

```rust
if holo_name_sim > 0.25 {
    let excess = (holo_name_sim - 0.25) / (1.0 - 0.25);
    holo_additive = holo_max_w * query_specificity * excess;
}
```

The gate (0.25) filters noise — random similarity is typically < 0.1. Only symbols with meaningful holographic similarity contribute. The contribution scales with query specificity (fraction of high-IDF terms), so specific queries get more benefit from holographic matching than vague ones.

### 7. Disk Cache

Holographic vectors are stored as f32 (4 bytes per dimension) instead of f64, halving memory usage. For 20K symbols at 1024 dimensions, this is ~80MB raw, ~42MB zstd-compressed. f32 quantization of normalized unit vectors is lossless for the precision levels used in cosine similarity comparison.
