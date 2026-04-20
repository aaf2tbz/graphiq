# GraphIQ Unified Pipeline — Implementation Roadmap

## Phase 0: Baseline Capture (before any changes)

**Goal:** Lock current benchmark scores as the regression ceiling.

- [ ] Run NDCG@10 on all 3 codebases with current binary
- [ ] Record per-category scores as `baseline-<codebase>.json` in `benches/`
- [ ] Verify: signetai >= 0.378, esbuild >= 0.428, tokio >= 0.296

## Phase 1: Extract seed generation into `seeds.rs`

**Goal:** Move all seed/expansion methods out of search.rs into a dedicated module. No behavior change.

Create `crates/graphiq-core/src/seeds.rs` with:

- [ ] `fn bm25_seeds(query, fts, policy) -> Vec<(i64, f64)>` — extract from search.rs FTS dispatch
- [ ] `fn per_term_fts_expansion(query, existing, fts) -> Vec<(i64, f64)>` — extract per_term_seed_expansion
- [ ] `fn graph_aware_expansion(db, family, existing) -> Vec<(i64, f64)>` — extract graph_aware_seed_expansion
- [ ] `fn numeric_bridge_seeds(query, existing, db) -> Vec<(i64, f64)>` — extract numeric_bridge_seeds
- [ ] `fn self_model_expansion(query, model) -> Vec<(i64, f64)>` — extract from search.rs self_model dispatch
- [ ] `fn chebyshev_heat_expansion(seeds, spectral, policy) -> Vec<(i64, f64)>` — extract from geometric_search heat block

Wire `search.rs` to call seeds.rs functions. **Build, test, bench. Verify zero regression.**

## Phase 2: Extract scoring formula into `scoring.rs`

**Goal:** One scoring function used by all modes. No behavior change yet.

Create `crates/graphiq-core/src/scoring.rs` with:

- [ ] `struct Candidate` — unified candidate struct (currently duplicated in goober_v5 and geometric)
- [ ] `struct ScoreConfig` — weight profile (bm25_w, cov_w, name_w, ng_w, coh_w, walk_w)
- [ ] `fn score_candidates(candidates, query_terms, config, idx) -> Vec<(usize, f64)>` — the unified scoring formula
- [ ] `fn build_candidates(seeds, walk_results, idx) -> Vec<Candidate>` — dedup + merge seed and walk candidates

Wire goober_v5_search to call scoring.rs. **Build, test, bench. Verify zero regression.**
Wire geometric_search to call scoring.rs. **Build, test, bench. Verify zero regression.**

## Phase 3: Create `pipeline.rs` — the unified search

**Goal:** One function that replaces goober_v5_search and geometric_search.

- [ ] `fn unified_search(query, idx, holo, spectral_opt, self_model_opt, config) -> Vec<(i64, f64)>` — the single entry point
- [ ] Seeds: call seeds.rs methods based on config flags
- [ ] Scoring: call scoring.rs with config weights
- [ ] Post: BM25 lock, file diversity

Wire search.rs router to use `unified_search` instead of mode-specific methods. **Build, test, bench.**

**This is the regression checkpoint.** If scores match within ±0.01, proceed. If not, diff the behavior.

## Phase 4: Delete legacy scoring functions

**Goal:** Remove dead code now that unified_search is the only path.

- [ ] Delete from cruncher.rs: `cruncher_search`, `cruncher_v2_search`, `cruncher_search_standalone`, `goober_search`, `goober_v3_search`, `goober_v4_search`
- [ ] Mark `goober_v5_search` and `geometric_search` as `#[cfg(test)]` or move to `cruncher_legacy.rs` for bench comparisons
- [ ] Simplify `SearchMode` enum: remove `GooberV5`, `Geometric`, `Deformed`, `CARE` → single `Unified` variant
- [ ] Remove `search_goober_v5`, `search_geometric`, `search_deformed`, `search_care` from search.rs

**Build, test, bench. Verify zero regression.**

## Phase 5: Simplify spectral.rs

**Goal:** Remove spectral features that don't contribute.

- [ ] Remove Ricci curvature computation and storage
- [ ] Remove curvature-weighted heat diffusion
- [ ] Remove exact heat kernel
- [ ] Remove harmonic extension
- [ ] Remove `use_curvature` flag from everywhere
- [ ] Keep: Lanczos eigendecomposition, Chebyshev heat diffusion, channel fingerprints, predictive model (for now)

**Build, test, bench. Verify zero regression.**

## Phase 6: Tune the unified pipeline

**Goal:** Now that the pipeline is clean, tune each signal for maximum effect.

- [ ] **Neighbor hints v2:** Increase the enrichment. Currently only looks at callers/callees/file siblings. Add: 2-hop neighbor terms (caller's callers), deep graph edge source names (`shares_type` neighbors), doc comment terms from neighbors. Frequency threshold = 2. Test on tokio.
- [ ] **Test penalty intent-awareness:** For Informational intent, soften test penalty from 0.3 to 0.6. Test file symbols are often the best answers for NL queries ("what demonstrates cooperative yielding").
- [ ] **Coverage frac refinement:** Current: `max(idf_weighted, raw_count/total)`. Try: use idf_weighted as primary, raw_count as floor. This way matching "cooperative" (high IDF) out of 5 terms scores higher than matching "task" (low IDF) out of 5.
- [ ] **Predictive surprise removal test:** Remove the 0.08 surprise bonus. Bench. If no regression, delete it.
- [ ] **MDL removal test:** Remove MDL explanation set. Bench. If no regression, delete it.

Each change: **bench all 3 codebases before and after.** Single-variable changes only.

## Phase 7: Benchmark CI and final validation

**Goal:** Lock in the gains.

- [ ] Create `benches/baseline-v6.json` with final scores
- [ ] Write a bench comparison script: `graphiq-bench-compare <baseline> <current>` that flags any regression > 0.01
- [ ] Update docs/ROADMAP.md with final scores
- [ ] Update docs/benchmarks.md with v6 results
- [ ] Run all 5 codebases (add flask, junit5)

## Estimated Timeline

| Phase | Effort | Risk |
|---|---|---|
| Phase 0 | 15 min | None |
| Phase 1 | 2 hours | Low — pure extraction |
| Phase 2 | 2 hours | Low — pure extraction |
| Phase 3 | 3 hours | Medium — the merge point |
| Phase 4 | 1 hour | Low — dead code deletion |
| Phase 5 | 1 hour | Low — removing unused features |
| Phase 6 | 4 hours | Medium — tuning, may need multiple iterations |
| Phase 7 | 1 hour | Low — documentation |
| **Total** | **~14 hours** | |

## Success Criteria

- [ ] One scoring function replaces 8 legacy functions
- [ ] ~3,000 lines deleted, codebase is navigable
- [ ] No regression on any codebase (within ±0.01 NDCG@10)
- [ ] Tokio NDCG@10 >= 0.340 (overtakes grep at 0.300)
- [ ] Signetai NDCG@10 >= 0.400
- [ ] Esbuild NDCG@10 >= 0.450
