# GraphIQ Pipeline Simplification Spec

**Status**: Approved, not yet started
**Goal**: Remove ~2000 lines of dead code and proven-noise signals. Keep only what the research shows actually improved results. The unified pipeline (`pipeline.rs`) is the live path — everything else in `cruncher.rs` past line ~480 is dead.

## What the research says

### Keep — these improved results

| Signal | Evidence | Source |
|---|---|---|
| BM25 seed retrieval + confidence lock | "BM25 retrieves, structural math reranks" — every system that replaced BM25 failed | research.md §1 |
| SEC negentropy + channel coherence | Core structural scoring since GooberV3, never removed | research.md §3 |
| Holographic name matching with 0.25 gate | 6.8x separation, V5 beat V4 on all 3 codebases after gating | research.md §5 |
| Chebyshev heat diffusion | Geometric matched V5 with zero tuning, parameterized by `cheb_order=15` | research.md §9 |
| Predictive surprise | Removing it regressed esbuild by 0.044 | research.md §24 Phase 6 experiment 1 |
| MDL explanation set | Removing it regressed esbuild by 0.050 | research.md §24 Phase 6 experiment 3 |
| QueryFamily routing | Replaced binary Nav/Info intent with 8-family classifier + RetrievalPolicy | research.md §14 |
| Channel capacity adjustments (additive) | Replacing weights entirely caused regressions; additive adjustments preserved gains | research.md §11B |
| BM25 confidence lock | Demoting confident BM25 results is almost always wrong | research.md §3 |
| File diversity cap | In pipeline via 3-per-file limit | pipeline.rs:444-457 |
| Kind boost + test penalty | Standard scoring adjustments | scoring.rs:223-224 |

### Remove — these are dead or proven noise

| Target | Why | Source |
|---|---|---|
| 8 dead search methods in cruncher.rs | Replaced by unified pipeline in Phase 24. Not called from live path. | research.md §24 |
| 5 dead Candidate structs | Only used by dead methods | verified: grep shows no usage outside dead methods |
| `QueryIntent` enum + `classify_query` | Replaced by `QueryFamily` + `classify_query_family`. Still lingers in `pipeline.rs` and `scoring.rs`. | research.md §14 |
| Ricci curvature boost in pipeline | "Compute geometry, don't score it" — tested as scoring feature, no improvement | research.md §10A, §6 |
| `build_seed_candidates` (scoring.rs:68-154) | Not called from pipeline.rs or search.rs. Only used by dead methods. | grep verified |
| `apply_file_diversity` (scoring.rs:283-305) | Only called from dead `goober_v5_search` (cruncher.rs:2660). Pipeline has its own inline version. | grep verified |
| `ENERGY_DEPTH`, `ENERGY_BREADTH`, `ENERGY_DECAY`, `INTERFERENCE_MIN_ENERGY` | Only used by dead `cruncher_v2_search` | cruncher.rs:767-770 |
| `per_term_energy`, `interference_score` | Only used by dead `cruncher_v2_search` | cruncher.rs:772, 797 |
| Bench `ALL_METHODS` + `run_method` + `cmd_diagnose` | References all dead methods | bench/main.rs:369-525 |
| Bench `diagnostic.rs` | Calls `cruncher_search_standalone` and `cruncher_search` directly | diagnostic.rs:80-81 |

---

## Phase 1: Delete dead search methods from cruncher.rs

**Net removal: ~2200 lines**

Delete from `crates/graphiq-core/src/cruncher.rs`:
- `cruncher_search` (lines 489-756)
- `cruncher_v2_search` (lines 831-1093)
- `cruncher_search_standalone` (lines 1095-1168)
- `goober_search` (lines 1181-1397)
- `goober_v3_search` (lines 1590-1836)
- `goober_v4_search` (lines 1913-2179)
- `goober_v5_search` (lines 2365-2661)
- `geometric_search` (lines 2663-3041)
- `Candidate` struct (lines 478-487)
- `V2Candidate` struct (lines 758-765)
- `GooberCandidate` struct (lines 1170-1179)
- `GooberV3Candidate` struct (lines 1577-1588)
- `V5Candidate` struct (lines 2349-2363)
- Constants: `ENERGY_DEPTH`, `ENERGY_BREADTH`, `ENERGY_DECAY`, `INTERFERENCE_MIN_ENERGY` (lines 767-770)
- Helpers: `per_term_energy` (line 772), `interference_score` (line 797)

Update fuzz tests in `cruncher.rs` (lines 3081-3086):
- Replace dead method calls with pipeline call or remove fuzz block

## Phase 2: Remove QueryIntent from live pipeline

QueryIntent is the old binary Navigational/Informational classifier, superseded by QueryFamily in Phase 14 but never cleaned out of the pipeline and scoring code.

**In `cruncher.rs`:**
- Delete `QueryIntent` enum (lines 1838-1842)
- Delete `classify_query` function (lines 1844-1911)

**In `pipeline.rs`:**
- Remove `QueryIntent` from imports (line 4)
- Remove `classify_query` from imports (line 6)
- Remove `classify_query` call (line 40)
- Remove `intent_weights` match (lines 51-54) — replace with fixed default weights `[4.0, 1.0, 1.2, 0.15, 0.08]` (the middle ground that works for all families; QueryFamily's `RetrievalPolicy` already handles family-specific gating)
- Remove `if matches!(intent, QueryIntent::Informational)` gate on graph walks (line 169) — `RetrievalPolicy` already gates `allow_spectral` per family. All families that reach the pipeline (non-FTS) benefit from graph expansion.
- Remove `QueryIntent` parameter from `score_candidates` call (line 407)

**In `scoring.rs`:**
- Remove `QueryIntent` from imports (line 3)
- Remove `intent` parameter from `score_candidates` (line 159)
- Remove intent-based branching (lines 189-193) — use `use_idf_coverage_frac` path for all cases (it's already the better formula, used for geometric/deformed modes)
- Remove `_intent` from `ScoreConfig::for_goober_v5` (line 39)

**In `search.rs`:**
- No changes needed — it already uses `QueryFamily`, not `QueryIntent`

## Phase 3: Remove Ricci curvature boost from pipeline

Research lesson #6: "Compute geometry, don't score it." The Ricci boost in `pipeline.rs` adds complexity for zero ranking improvement.

**In `pipeline.rs`, within the heat diffusion block (lines 282-391):**
- Delete `avg_ricci` computation (lines 282-301)
- Delete `ricci_max` / `ricci_min` (lines 303-304)
- Delete `ricci_boost` calculation (lines 326-331)
- Change line 384 from `entry.walk_evidence = entry.walk_evidence.max(cov_score * normalized_heat * ricci_boost * config.evidence_weight)` to `entry.walk_evidence = entry.walk_evidence.max(cov_score * normalized_heat * config.evidence_weight)`

**Keep** `compute_ricci_curvature` in `spectral.rs` — it's structural infrastructure.

## Phase 4: Extract holographic name encoding to holo_name.rs

Code hygiene — the holographic name matching functions live in cruncher.rs but are a self-contained subsystem. Extracting them makes cruncher.rs about pure indexing + term matching.

Move from `cruncher.rs` to new `crates/graphiq-core/src/holo_name.rs`:
- `HOLO_DIM` constant
- `holo_hash_seed`
- `holo_random_unit`
- `holo_fft_inplace`
- `holo_ifft_inplace`
- `holo_to_freq`
- `holo_from_freq`
- `holo_normalize`
- `holo_cosine`
- `HoloIndex` struct (with `name_holos` and `term_freq` fields)
- `build_holo_index`
- `holo_query_name_cosine`

Note: `holo.rs` has a DIFFERENT `HoloIndex` with different fields (`boundary_traces`, `freq_mul_add`, etc). Keep separate. The cruncher version is for name-matching only.

Update imports in: `cruncher.rs`, `pipeline.rs`, `scoring.rs`, `search.rs`, `graphiq-bench/src/main.rs`
Add `pub mod holo_name;` to `lib.rs`.

## Phase 5: Remove dead scoring functions

After Phase 1, these are unreferenced:

**In `scoring.rs`:**
- Delete `build_seed_candidates` (lines 68-154) — not called from pipeline or search
- Delete `apply_file_diversity` (lines 283-305) — pipeline has its own inline version at lines 444-457

## Phase 6: Clean up bench

**In `graphiq-bench/src/main.rs`:**
- Delete `ALL_METHODS` array (line 369)
- Delete `run_method` function (lines 387-416)
- Delete `cmd_diagnose` function (lines 427-525)
- Remove dead method fields from `FullEngine` struct that only `run_method` used
- Trim imports
- Keep `run_router` (exercises the live pipeline) and speed bench

**In `graphiq-bench/src/diagnostic.rs`:**
- Delete file entirely or rewrite to use the unified pipeline. Currently calls `cruncher_search_standalone` and `cruncher_search` which are dead methods.

## Post-simplification targets

| File | Current | Target | Notes |
|---|---|---|---|
| `cruncher.rs` | 3198 | ~700 | Index building + term matching + sec channels + negentropy/coherence |
| `pipeline.rs` | 460 | ~380 | Unified search without QueryIntent/Ricci |
| `scoring.rs` | 305 | ~180 | Unified scoring without intent param + dead functions |
| `query_family.rs` | 425 | 425 | Unchanged — already clean |
| `seeds.rs` | 285 | 285 | Unchanged — already clean |
| `holo_name.rs` | new | ~200 | Extracted holographic name matching |
| `search.rs` | 644 | ~630 | Minor import cleanup |
| Bench `main.rs` | 771 | ~550 | Dead methods/diagnose removed |
| Bench `diagnostic.rs` | ~100 | 0 | Deleted |

**Total removal: ~2500 lines**

## Verification

After each phase:
1. `cargo build` — must compile
2. `cargo test` — all tests pass
3. `cargo run --bin graphiq-bench -- bench <db> <queries.json>` — GraphIQ MRR/NDCG unchanged (scores identical to 3 decimal places for the live pipeline path)

The dead methods being removed don't affect the live pipeline, so phases 1, 5, and 6 are zero-risk by construction. Phases 2-4 modify the live pipeline and need bench verification.
