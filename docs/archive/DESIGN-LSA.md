# Latent Semantic Geometry

## The Problem

BM25 treats every term as an orthogonal dimension. "start" and "daemon" have zero
relationship in FTS space even though they're structurally entangled in the code.
The semantic gap — where the user's vocabulary diverges from the code's vocabulary —
is a geometry problem, not a model problem.

## The Insight

The term-symbol co-occurrence matrix encodes latent semantic structure. Truncated
SVD projects this matrix into a low-dimensional space where structurally related
concepts cluster. After normalization to unit length, every term and symbol lives on
a hypersphere S^{k-1}. Angular distance (arccos of dot product) is the natural
relevance metric — bounded, scale-invariant, and geometrically meaningful.

This is Deerwester et al. 1988 — LSA — applied to code search. Code's term-symbol
matrix has richer latent structure than general text because:

1. Identifiers carry dense semantic information (compound words, type names)
2. Structural context (imports, callers, type hierarchies) constrains co-occurrence
3. The vocabulary is smaller and more structured than natural language
4. Term-symbol relationships are denser (every function references its domain terms)

No one has applied this to code search with hyperspherical geometry as the primary
framing. The field moved to neural embeddings and abandoned the math. The math was
never the problem — the domain was. Code is the right domain.

## The Isotropic Hypersphere (Phase 6 — Insufficient)

The initial implementation computed:

1. Structurally-augmented TF-IDF matrix
2. Randomized SVD to k=96
3. Isotropic L2 normalization: v̂ = v / ||v||₂
4. Angular distance: d(q̂, ŝ) = arccos(q̂ · ŝ) ∈ [0, π]
5. Relevance: 1 - d/π

**Result**: "Captured patterns already in BM25" (research.md). No retrieval improvement.

**Diagnosis**: The isotropic sphere treats every latent axis equally. The top singular
values capture the most generic patterns (high variance but low discriminativity —
terms like "handle", "process", "get" that appear everywhere). The rare,
discriminative dimensions that capture domain-specific latent structure (auth,
rate-limit, serialization) get drowned out. BM25 already handles the generic
patterns. The isotropic sphere adds nothing BM25 doesn't already see.

## The Anisotropic Hypersphere (Phase 7 — Current)

The fix is not to leave the sphere but to warp it so the sphere reflects the actual
reliability of each direction.

### The Transformation

```
ṽ = Wv / ||Wv||₂
```

where W is a diagonal matrix with entries derived from per-dimension quality metrics.
Then score with the same angular metric:

```
relevance = 1 - arccos(q̃ · s̃) / π
```

This is a Mahalanobis-style angular space where closeness means "close in the
important directions," not just "close after flattening everything equally."

### Deriving W

The singular values σ₁ ≥ σ₂ ≥ ... ≥ σₖ from the SVD tell how much variance each
latent dimension captures. But high σ alone is insufficient — the top singular values
often capture the most generic patterns (common across everything, low discriminativity).

Two signals per dimension:

**1. Singular value mass.** σᵢ from the SVD. How much co-occurrence structure
dimension i captures.

**2. Discriminativity.** How non-uniform dimension i is across symbols:

```
discᵢ = 1 - |mean(sᵢ)| / std(sᵢ)
```

- A dimension where all symbols have similar values → generic → low disc
- A dimension where symbols cluster at extremes → discriminative → high disc

**Combined specificity:**

```
specᵢ = σᵢ × discᵢ
```

**Diagonal weights:**

```
wᵢ = (specᵢ / max(spec))^α + ε     α ∈ [0.5, 2.0], ε ≈ 0.1
```

- α = 0: recovers the isotropic sphere
- α = 1: natural weighting
- α = 2: aggressive suppression of noisy dimensions
- ε: prevents collapsing weak dimensions to zero

### Application

**Index time** (once, after SVD):

```
s̃ᵢ = normalize(W · S_k[i])     for each symbol vector
```

**Query time** (per query):

```
q̃ = normalize(W · T_k^T × tfidf(query_terms))
```

Cost: k multiplications per vector. Negligible overhead.

Then angular scoring on the warped vectors is unchanged from the isotropic case —
the geometry is just more honest about which directions matter.

### Why This Helps

| Scenario | Isotropic | Anisotropic |
|---|---|---|
| Dimension capturing "handle" co-occurrence | Equal weight | Low disc → downweighted |
| Dimension capturing auth/credential/session latent cluster | Equal weight | High disc + decent σ → upweighted |
| Dimension separating functions from types | Equal weight | Moderate disc → moderate weight |
| Noisy dimension from source body terms | Equal weight | Low disc + low σ → suppressed |

The isotropic sphere helped because angular structure survived naming variance.
The anisotropic sphere helps more because it keeps that benefit while suppressing
the exact directions where mush tends to accumulate.

## Architecture

### Index Time

1. **Extract term-symbol matrix** from FTS data already collected during indexing.
   Each row is a term, each column is a symbol. Entry (t, s) = TF-IDF(t, s).

2. **Augment with structural signals.** Terms inherit co-occurrence from structural
   edges — if symbol A calls symbol B, their term vectors are partially mixed. This
   injects graph structure into the matrix before SVD.

3. **Truncated SVD** to k dimensions (k = 96). Produces:
   - T_k: term vectors in latent space (|V| × k)
   - S_k: symbol vectors in latent space (|symbols| × k)
   - Sigma_k: singular values (importance weights)

4. **Compute anisotropy weights.** From S_k and Sigma_k:
   - Per-dimension mean, std
   - Discriminativity: discᵢ = 1 - |mean(sᵢ)| / std(sᵢ)
   - Specificity: specᵢ = σᵢ × discᵢ
   - Weights: wᵢ = (specᵢ / max(spec))^α + ε

5. **Anisotropic normalization.** W · S_k[i] for each symbol, then L2-normalize.
   All points now live on the warped hypersphere.

6. **Store** warped S_k in the database as BLOB per symbol.
   Store T_k and W as the shared projection basis.

### Query Time

1. **Project query** into latent space: q̃ = normalize(W · T_k^T × tfidf(query_terms))
   This places the query on the same warped hypersphere as the symbols.

2. **Angular distance**: d(q̃, s̃) = arccos(q̃ · s̃) ∈ [0, π]
   Smaller angle = more relevant.

3. **Hybrid scoring**: Blend GooberV5 score with angular relevance:
   final = goober_score * (1 - alpha) + (1 - angle/π) * max_goober * alpha

4. **Geometric expansion**: Symbols within angular radius θ on the warped sphere
   form a "relevance cap." All symbols in the cap are candidates, even if BM25
   missed them entirely.

## Dimensionality

| Codebase | Symbols | Terms (est.) | Matrix Size | SVD to 96 |
|---|---|---|---|---|
| graphiq | ~900 | ~1,500 | 1.5K × 900 | <1s |
| tokio | ~18K | ~8K | 8K × 18K | ~2s |
| signetai | ~21K | ~10K | 10K × 21K | ~3s |

Storage: 96 floats × 4 bytes = 384 bytes per symbol. For signetai: 7.8MB total.
Compare to neural embeddings: 768 floats × 4 bytes = 3KB per symbol. 60MB total.

## Structural Augmentation (The Novel Part)

Plain LSA uses raw term-document co-occurrence. For code, we can do better by
injecting graph structure into the matrix before SVD:

1. **Call-graph mixing**: If A calls B, add weighted contributions of B's terms to A's
   row (and vice versa). This propagates semantic information along call edges.

2. **Type hierarchy mixing**: If A implements trait T, add T's terms to A. This
   connects interface language to implementation language.

3. **Import neighborhood**: Symbols in the same file/module share term distributions.
   Weighted by co-location strength.

This pre-SVD augmentation is the key innovation. It means the SVD doesn't just
discover lexical co-occurrence — it discovers structural semantics. The anisotropic
hyperspherical geometry then captures relationships that are invisible to both BM25
and neural embeddings trained only on text.

## Query Projection Variants

### Standard projection (anisotropic)
q̃ = normalize(W · T_k^T × tfidf(query_terms))

Uses the same SVD basis and anisotropy weights. Simple, fast, effective.

### Centroid projection with expansion
For multi-word NL queries like "split tcp stream read write":
1. Decompose into sub-concepts: ["split", "tcp", "stream", "read", "write"]
2. Project each sub-concept independently onto the warped sphere: q̃_1, q̃_2, ..., q̃_n
3. Compute centroid: c̃ = normalize(mean(q̃_1, q̃_2, ..., q̃_n))
4. Find the spherical cap around c̃ on the warped sphere
5. Weight by distance to each sub-concept: closer to more sub-concepts = better

The anisotropic warp means "close to all sub-concepts" is measured in discriminative
dimensions, not generic ones.

### Pseudo-relevance feedback (geometric)
1. Run initial GooberV5 search → top 3 results
2. Compute their centroid on the warped hypersphere
3. Expand to the spherical cap around this centroid
4. These are "things like what you found" — pure geometric relevance feedback

No LLM needed. The feedback signal comes from the code's own structure.

## Integration with Existing Pipeline

```
BM25/FTS (30 seeds)
        ↓
GooberV5 (~100 candidates, structural expansion + holographic gate)
        ↓
Anisotropic LSA rerank (top_k)    ← Phase 7 (new)
```

Activation criteria:
- Query is informational (not navigational)
- GooberV5 top-1 confidence < threshold
- Query has ≥2 terms present in the LSA term basis

Does NOT activate for exact/partial symbol matches where BM25 + heuristics already work.

## Dependencies

- Current: custom randomized SVD in `lsa.rs` (no external linalg dependency)
- No model downloads. No network calls. No GPU.

## The Deeper Point

The isotropic sphere was elegant but too blunt. Every direction treated equally
once projected. The anisotropic sphere keeps the geometry — angular distance,
spherical caps, geodesic clusters — while respecting that not every axis deserves
equal respect. Movement along some directions should matter more than movement
along others. That is the meaning of anisotropy: not the same in every direction.

The universe runs on math. The gap between BM25 and neural code search isn't a
model gap — it's a representation gap. The right mathematical structure
(anisotropic hyperspherical geometry on latent semantic space) captures what
neural models learn, without learning anything. The structure was always there in
the data. We just weren't weighting it correctly.
