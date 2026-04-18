# Phase 6: Latent Semantic Geometry

**Goal**: Replace neural embeddings with pure mathematical semantic search via
truncated SVD on a structurally-augmented term-symbol matrix. Hyperspherical
geometry as the relevance framework.

**Baseline** (Phase 5, no embeddings):
- Self: 0.717 | Tokio: 0.540 | Signetai: 0.527

## Steps

### Step A: Term-Symbol Matrix Extraction
Build the TF-IDF matrix during indexing. Extract per-symbol term frequencies
from FTS data (name, signature, doc_comment, source tokens). Compute IDF
across all symbols. Store the sparse matrix for SVD.

**Deliverable**: `extract_term_matrix()` producing a sparse CSR matrix.
**Verify**: Matrix dimensions and sparsity match expectations.

### Step B: Truncated SVD
Implement or integrate truncated SVD. Lanczos iteration on the sparse matrix,
keeping top k=128 singular values/vectors. Produces:
- Term basis T_k (for query projection)
- Symbol vectors S_k (for storage)

**Deliverable**: `compute_lsa(matrix, k=128) -> (T_k, S_k, Sigma_k)`
**Verify**: Reconstruction quality (Frobenius norm ratio).

### Step C: Hyperspherical Normalization + Storage
L2-normalize all vectors to unit length. Store S_k rows in `symbol_latent`
table (symbol_id, latent_vec BLOB, dim). Store T_k as the shared projection
basis.

**Deliverable**: DB schema, storage, retrieval functions.
**Verify**: All vectors have unit L2 norm.

### Step D: Query Projection + Angular Scoring
At query time: project query terms through T_k^T, normalize to unit length,
compute angular distance to all symbol vectors. Return as relevance scores.

**Deliverable**: `angular_search(query, top_k) -> Vec<(symbol_id, angle)>`
**Verify**: Manual spot-checks on known queries.

### Step E: LSA Reranker Integration
Wire into the search pipeline as a reranker (replaces embed reranker).
Hybrid scoring: blend BM25 score with (1 - angle/π) relevance.
Activate for NL queries and low-confidence BM25 results.

**Deliverable**: `lsa_rerank()` in search pipeline.
**Benchmark**: Full 3-codebase NDCG comparison.

### Step F: Structural Augmentation (Pre-SVD)
Inject graph structure into the term-symbol matrix before SVD:
- Call-graph mixing: propagate terms along call edges
- Type hierarchy: mix interface terms into implementations
- Import neighborhood: co-located symbols share term distributions

This is the novel part — the SVD discovers structural semantics, not just
lexical co-occurrence.

**Deliverable**: `augment_matrix(matrix, edges) -> augmented_matrix`
**Benchmark**: Compare F (without augmentation) vs F (with augmentation).

### Step G: Geometric Expansion for MISS Recovery
When BM25 returns zero relevant candidates:
1. Project query onto hypersphere
2. Find all symbols within angular radius θ
3. These are the "semantic neighborhood" — pure geometric recovery

**Deliverable**: `geometric_expand(query, theta) -> Vec<symbol_id>`
**Benchmark**: Track MISS→HIT conversion rate.

### Step H: Centroid Projection for Multi-Concept Queries
For NL queries like "split tcp stream read write":
1. Decompose into sub-concepts
2. Project each independently
3. Compute spherical centroid
4. Weight by distance to each sub-concept

**Deliverable**: Multi-concept query handling.
**Benchmark**: Per-query NDCG on nl-descriptive/abstract categories.

## Success Criteria

| Metric | Target |
|---|---|
| Tokio NDCG@10 | > 0.58 (+0.04 over baseline) |
| Signetai NDCG@10 | > 0.56 (+0.03 over baseline) |
| Self NDCG@10 | >= 0.71 (no regression) |
| Index time (LSA step) | < 5 seconds |
| Storage overhead | < 15MB for signetai |
| Query latency | < 2ms additional |

## Dependencies

- Rust linear algebra: `nalgebra` or `ndarray` + `ndarray-linalg`
- No model downloads, no network calls, no GPU

## Key Files

- `crates/graphiq-core/src/lsa.rs` — New: SVD, matrix ops, angular distance
- `crates/graphiq-core/src/lsa_matrix.rs` — New: TF-IDF extraction, augmentation
- `crates/graphiq-core/src/search.rs` — Modified: LSA reranker replaces embed
- `crates/graphiq-core/src/db.rs` — Modified: latent vector storage
- `crates/graphiq-core/src/index.rs` — Modified: LSA computation after indexing
- `crates/graphiq-core/Cargo.toml` — Modified: add linalg dependency
- `docs/DESIGN-LSA.md` — The full design document
