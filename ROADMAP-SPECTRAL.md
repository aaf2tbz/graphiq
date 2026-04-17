# Phase 6.2: Spectral Manifold Embedding

Calabi-Yau-inspired code retrieval via graph Laplacian harmonic analysis.

## Why

LSA factorizes term co-occurrence. Abstract queries ("how does the runtime schedule") share zero terms with target symbols, so LSA scores 0.000. The code *graph* captures the actual relationships — call chains, type hierarchies, import paths. Spectral embedding factorizes the graph topology itself, discovering structural communities that term analysis can never see.

## Architecture

```
Code graph (edges table)
    ↓
Weighted adjacency W (symbols × symbols)
    - structural edges: calls, imports, extends, implements (weight by kind)
    - semantic edges: term-Jaccard overlap between symbol text (weight by similarity)
    ↓
Normalized Laplacian L = D⁻¹ᐟ²(D - W)D⁻¹ᐟ²
    ↓
k smallest eigenvectors (harmonic modes)
    ↓
Spectral coordinates per symbol (k-dim embedding)
    ↓
Query: FTS seeds → spectral neighbors by Euclidean distance in embedding space
```

## Steps

### A. Build Weighted Adjacency Matrix
- Load all edges from DB (calls, imports, extends, implements)
- Map symbol IDs to matrix indices
- Edge weights: calls=1.0, imports=0.8, extends=1.2, implements=1.1
- Add term-overlap edges: for each pair of symbols sharing ≥2 terms, add edge weighted by Jaccard similarity (capped at 0.3 to not overpower structural edges)
- Output: sparse symmetric adjacency matrix W

### B. Construct Normalized Laplacian
- Degree matrix D = diag(row sums of W)
- L_normalized = I - D⁻¹ᐟ² W D⁻¹ᐟ²
- Store as sparse matrix

### C. Compute Harmonic Modes (Eigendecomposition)
- Find k smallest non-trivial eigenvectors of L
- Skip eigenvector for eigenvalue ≈ 0 (constant vector — trivial)
- Use inverse iteration / Lanczos (sparse eigensolver)
- k = 6 to start (CY 3-fold analogy: 6 real dimensions)
- Store eigenvector coordinates per symbol

### D. Spectral Search
- At query time: FTS finds seed symbols (top 5-10)
- Compute centroid of seed embeddings
- Rank all symbols by Euclidean distance to centroid in spectral space
- Return top-10
- For zero-FTS-hit queries: use query term matching against symbol names to find seeds

### E. Benchmark Against Baselines
- Pure spectral NDCG@10 on all 3 codebases
- Compare with: BM25 baseline, LSA centroid, LSA cap
- If spectral beats LSA: tune k (sweep 4-16)
- If spectral beats BM25 on any query class: that's the win

### F. Geodesic Refinement
- Replace Euclidean distance in embedding with geodesic distance on the manifold
- Approximate via shortest-path on k-nearest-neighbor graph in spectral space
- This is the true "distance on the Calabi-Yau manifold"

### G. Mirror Symmetry Exploitation
- Build a second manifold from the term-symbol bipartite graph
- Find symbols where both manifolds agree (intersection of spectral neighborhoods)
- Agreement boost: symbols close in BOTH manifolds are high-confidence hits

## Success Criteria

- Spectral beats LSA centroid on all 3 codebases
- Spectral + FTS seeds beats BM25 on nl-abstract and nl-descriptive at scale (tokio, signetai)
- No regression on symbol-exact (should match BM25 via FTS seeds)
- Indexing stays under 2 minutes for 20K symbols
- Query latency stays under 50ms

## Key Math

**Normalized Laplacian:**
L = I - D⁻¹ᐟ² A D⁻¹ᐟ²

**Lanczos iteration** for sparse eigendecomposition:
- Builds tridiagonal matrix T from Krylov subspace
- Eigenvalues of T approximate eigenvalues of L
- O(nk) for k eigenvectors of n-node graph

**Spectral embedding:**
φ(i) = [v₂(i), v₃(i), ..., vₖ₊₁(i)]
where vⱼ is the j-th smallest eigenvector of L

**Spectral distance:**
d(i,j) = ||φ(i) - φ(j)||₂

## Files

- `crates/graphiq-core/src/spectral.rs` — NEW: adjacency construction, Laplacian, eigensolver, spectral search
- `crates/graphiq-core/src/lib.rs` — add `pub mod spectral`
- `crates/graphiq-bench/src/main.rs` — add spectral evaluation alongside LSA
- `crates/graphiq-core/src/db.rs` — add spectral_coords table
