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

## Architecture

### Index Time

1. **Extract term-symbol matrix** from FTS data already collected during indexing.
   Each row is a term, each column is a symbol. Entry (t, s) = TF-IDF(t, s).

2. **Augment with structural signals.** Terms inherit co-occurrence from structural
   edges — if symbol A calls symbol B, their term vectors are partially mixed. This
   injects graph structure into the matrix before SVD.

3. **Truncated SVD** to k dimensions (k = 64-128). Produces:
   - T_k: term vectors in latent space (|V| × k)
   - S_k: symbol vectors in latent space (|symbols| × k)
   - Sigma_k: singular values (importance weights)

4. **Normalize to unit hypersphere.** Each row of T_k and S_k is L2-normalized.
   All points now live on S^{k-1}. Angular distance replaces cosine similarity.

5. **Store S_k** in the database as BLOB per symbol (64 floats = 256 bytes each).
   Store T_k as the query projection basis.

### Query Time

1. **Project query** into latent space: q = normalize(T_k^T × tfidf(query_terms))
   This places the query on the same hypersphere as the symbols.

2. **Angular distance**: d(q, s) = arccos(q · s) ∈ [0, π]
   Smaller angle = more relevant. This is the geometric relevance score.

3. **Hybrid scoring**: Blend BM25 score with angular relevance:
   final = bm25_score * (1 - alpha) + (1 - angle/π) * max_bm25 * alpha
   The (1 - angle/π) term maps [0, π] → [1, 0] relevance.

4. **Geometric expansion**: Symbols within angular radius θ of the query form a
   "relevance cap" on the hypersphere. All symbols in the cap are candidates,
   even if BM25 missed them entirely. This solves the MISS problem.

## Why This Works

### The Hyperspherical Structure

After SVD + normalization, the latent space has geometric properties:

- **Clusters**: Semantically related symbols form tight clusters (small intra-cluster
  angular distance). "Mutex", "RwLock", "Semaphore" cluster despite sharing no tokens.

- **Geodesic distance**: The shortest path on the sphere surface between two clusters
  measures their semantic distance. "Timer" and "Runtime" are closer than "Timer"
  and "TcpListener" because timers are managed by the runtime.

- **Caps and cones**: A query defines a spherical cap (all points within angle θ).
  The cap captures the semantic neighborhood — everything the query "means" in code
  space, including synonyms it doesn't share tokens with.

### Why SVD Discovers This

SVD factorizes the term-symbol matrix M ≈ T Σ S^T. The truncated version keeps only
the top k singular values/vectors. These capture the k strongest patterns of
co-occurrence:

- If "mutex" and "rwlock" appear in similar sets of symbols (same files, same callers,
  same imports), they'll share components in the same latent dimensions.
- The singular values weight the dimensions by importance.
- Normalization projects onto the sphere, making only direction (not magnitude) matter.

This is pure linear algebra. No training. No gradient descent. No model weights.
Just matrix factorization.

## Dimensionality

| Codebase | Symbols | Terms (est.) | Matrix Size | SVD to 128 |
|---|---|---|---|---|
| graphiq | ~900 | ~1,500 | 1.5K × 900 | <1s |
| tokio | ~18K | ~8K | 8K × 18K | ~2s |
| signetai | ~21K | ~10K | 10K × 21K | ~3s |

Storage: 128 floats × 4 bytes = 512 bytes per symbol. For signetai: 10.5MB total.
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
discover lexical co-occurrence — it discovers structural semantics. The hyperspherical
geometry then captures relationships that are invisible to both BM25 and neural
embeddings trained only on text.

## Query Projection Variants

### Standard projection
q = normalize(T_k^T × tfidf(query_terms))

Uses the same SVD basis to project query terms. Simple, fast, effective.

### Centroid projection with expansion
For multi-word NL queries like "split tcp stream read write":
1. Decompose into sub-concepts: ["split", "tcp", "stream", "read", "write"]
2. Project each sub-concept independently: q_1, q_2, ..., q_n
3. Compute centroid: c = normalize(mean(q_1, q_2, ..., q_n))
4. Find the spherical cap around c
5. Weight by distance to each sub-concept: closer to more sub-concepts = better

This handles the "intersection of concepts" pattern that BM25 struggles with.

### Pseudo-relevance feedback (geometric)
1. Run initial BM25 search → top 3 results
2. Compute their centroid on the hypersphere
3. Expand to the spherical cap around this centroid
4. These are "things like what you found" — pure geometric relevance feedback

No LLM needed. The feedback signal comes from the code's own structure.

## Integration with Existing Pipeline

```
BM25/FTS (200 candidates)
        ↓
Structural expansion (500)
        ↓
Heuristic rerank (50)     ← Phase 5 (current)
        ↓
LSA angular rerank (top_k) ← Phase 6 (new)
```

The LSA reranker replaces the embed reranker. Same position in the pipeline,
same interface, but powered by linear algebra instead of a GGUF model.

Activates for: NL queries, cross-cutting queries, any query where top heuristic
score < threshold. Does NOT activate for exact/partial symbol matches where
BM25 + heuristics already work.

## Dependencies

- `nalgebra` or `ndarray` + `ndarray-linalg` for SVD
- Or: implement Lanczos iteration directly (no external dependency for the core)
- No model downloads. No network calls. No GPU.

## The Deeper Point

The universe runs on math. Our brains run on ~20 watts and outperform billion-parameter
models at understanding. The gap between BM25 and neural code search isn't a model
gap — it's a representation gap. The right mathematical structure (hyperspherical
geometry on latent semantic space) captures what neural models learn, without learning
anything. The structure was always there in the data. We just weren't looking at it.
