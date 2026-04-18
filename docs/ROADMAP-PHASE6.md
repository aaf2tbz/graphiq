# Phase 6→7: Latent Semantic Geometry → Anisotropic Hypersphere

**Phase 6 (isotropic) completed** — LSA infrastructure built, SVD working, angular
scoring implemented. Did not improve retrieval because isotropic normalization
treats all latent dimensions equally, capturing patterns already in BM25.

**Phase 7 (anisotropic) is the continuation** — warps the sphere before projection
so discriminative dimensions dominate and generic/noisy ones are suppressed.

Full design: `docs/DESIGN-LSA.md` (updated for anisotropic model)
Full roadmap: `docs/ROADMAP.md` (Phase 7 section)

## Phase 6 Deliverables (Completed)

- [x] Term-symbol matrix extraction with structural augmentation (`build_tfidf_matrix`)
- [x] Randomized SVD to k=96 (`randomized_svd`)
- [x] Isotropic hyperspherical normalization (`normalize_to_sphere`)
- [x] Angular distance and spherical cap search (`spherical_cap_search`, `blade_search`)
- [x] Query projection (`project_query`)
- [x] Centroid projection for multi-concept queries

## Phase 7 Steps

See `docs/ROADMAP.md` → Phase 7 for the full step-by-step plan.

### Quick Summary

| Step | What | Key Change |
|---|---|---|
| A | Per-dimension discriminativity analysis | Compute discᵢ = 1 - |mean|/std per dimension |
| B | Diagonal weight matrix | wᵢ = (specᵢ/max(spec))^α + ε |
| C | Anisotropic normalization | `normalize_anisotropic(vecs, weights)` |
| D | Anisotropic angular search | Wire warped vectors into search functions |
| E | Reranker integration | LSA reranker behind GooberV5 |
| F | Alpha tuning + ablation | Systematic α sweep |
| G | Geometric MISS recovery | Spherical cap on warped sphere |
| H | Centroid projection | Multi-concept queries on warped sphere |

## Success Criteria

| Metric | Target |
|---|---|
| Tokio NDCG@10 | > 0.58 (+0.04 over baseline) |
| Signetai NDCG@10 | > 0.56 (+0.03 over baseline) |
| Self NDCG@10 | >= 0.71 (no regression) |
| Index time (LSA step) | < 5 seconds |
| Storage overhead | < 15MB for signetai |
| Query latency | < 2ms additional |

## Key Files

- `crates/graphiq-core/src/lsa.rs` — Modified: anisotropic weights, warped normalization
- `crates/graphiq-core/src/search.rs` — Modified: LSA reranker behind GooberV5
- `crates/graphiq-core/src/cruncher.rs` — Modified: expose S_k and sigma for anisotropy
- `crates/graphiq-core/src/db.rs` — Modified: latent vector storage
- `crates/graphiq-core/src/index.rs` — Modified: anisotropic LSA computation after indexing
- `docs/DESIGN-LSA.md` — The full design document (updated for anisotropic model)
- `docs/ROADMAP.md` — The full roadmap (Phases 7-9)
