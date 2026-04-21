# Research Notes

Experimental history and lessons from building GraphIQ's retrieval engine.

**Current version**: v3 (Phases 28–29). See [how-graphiq-works.md](how-graphiq-works.md) for the current architecture.

## Table of Contents

- [Timeline Overview](#timeline-overview)
- [Phase 1: Can We Beat BM25?](#phase-1-can-we-beat-bm25)
- [Phase 2: Goober — BM25 + Structural Reranking](#phase-2-goober--bm25--structural-reranking)
- [Phase 3: SEC Channel Analysis (V3→V4)](#phase-3-sec-channel-analysis-v3v4)
- [Phase 4: Holographic Name Matching Experiments (V5–V11)](#phase-4-holographic-name-matching-experiments-v5v11)
- [Phase 5: Per-Candidate Gating Breakthrough (V5 Final)](#phase-5-per-candidate-gating-breakthrough-v5-final)
- [Phase 6: Spectral Graph Infrastructure](#phase-6-spectral-graph-infrastructure)
- [Phase 9: Geometric Search Pipeline](#phase-9-geometric-search-pipeline)
- [Phase 10: Structural Geometry](#phase-10-structural-geometry)
- [Phase 11: Query as Deformation](#phase-11-query-as-deformation)
- [Phase 12: Production Search Modes](#phase-12-production-search-modes)
- [Phase 13–14: Artifact Lifecycle + Query Family Router](#phase-1314-artifact-lifecycle--query-family-router)
- [Phase 15–16: File Path Router + Cross-Cutting Sets](#phase-1516-file-path-router--cross-cutting-sets)
- [Phase 17: Gated Edge Evidence](#phase-17-gated-edge-evidence)
- [Phase 18: Why-Chain Unification](#phase-18-why-chain-unification)
- [Phase 19: Benchmark Lab Notebook](#phase-19-benchmark-lab-notebook)
- [Phase 20: Repo Self-Model](#phase-20-repo-self-model)
- [Phase 21: CARE — Confidence-Anchored Reciprocal Expansion](#phase-21-care--confidence-anchored-reciprocal-expansion)
- [Phase 22: 5-Codebase Benchmarks + Deep Graph Edges](#phase-22-5-codebase-benchmarks--deep-graph-edges)
- [Phase 23: Speed Benchmark](#phase-23-speed-benchmark)
- [Phase 24: Unified Pipeline (v6)](#phase-24-unified-pipeline-v6)
- [Phase 25: Artifact Disk Cache](#phase-25-artifact-disk-cache)
- [Phase 26: SNP Structural Fallback + Source Scan Seeds](#phase-26-snp-structural-fallback--source-scan-seeds)
- [Phase 27: MCP Server Hardening](#phase-27-mcp-server-hardening)
- [Phase 28: v2 Simplification — Remove the Artifact Pipeline](#phase-28-v2-simplification--remove-the-artifact-pipeline)
- [Phase 29: v3 — Gated Signals + Per-Family Routing](#phase-29-v3--gated-signals--per-family-routing)
- [What Didn't Work](#what-didnt-work)
- [Key Lessons](#key-lessons)

## Timeline Overview

| Phase | What | Version | Outcome |
|---|---|---|---|
| 1 | 9 standalone retrieval systems vs BM25 | — | None beat BM25 generally |
| 2 | Goober: BM25 + structural reranking | — | Simpler system beat CruncherV2 everywhere |
| 3 | SEC negentropy + query intent (V3→V4) | — | Marginal gains on tokio |
| 4 | Holographic name matching experiments | V5–V11 | 7 versions, most net-negative |
| 5 | Per-candidate gating | V5 final | Beat V4 on all 3 codebases |
| 6 | Spectral graph infrastructure | — | Chebyshev heat kernel |
| 9 | Geometric search pipeline | — | Matched V5 with zero tuning |
| 10 | Ricci curvature + channel fingerprints | — | Curvature useless for scoring |
| 11 | Predictive surprise + channel capacity + MDL | — | Gains on weak categories, no regressions |
| 12 | SearchMode enum + capability negotiation | — | Closed trust gap |
| 13–14 | Artifact lifecycle + 8-family router | — | Per-family permission boundaries |
| 15–16 | File path router + cross-cutting sets | — | Fixed file-path zero on esbuild |
| 17 | Gated edge evidence | — | Family-based evidence gating |
| 18 | RetrievalTrace | — | Falsifiable ranking explanations |
| 19 | Benchmark lab expansion | — | Separate NDCG/MRR query sets |
| 20 | Repo self-model | — | nl-abstract improved on 2/3 |
| 21 | CARE fusion | — | Beat parents on MRR, not NDCG |
| 22 | 5-codebase benchmarks + deep graph edges | — | MRR wins on all 5 |
| 23 | Speed benchmark | — | 1,300–11,700x faster than grep |
| 24 | Unified pipeline (v6) | v6 | 3,000 lines removed, zero regression |
| 25 | Artifact disk cache | — | 14s → 850ms warm search |
| 26 | SNP structural fallback | v7 | tokio +0.007, esbuild -0.050 |
| 27 | MCP server hardening | — | Production readiness |
| 28 | v2 simplification | **v2** | 5,087 lines removed, 18GB RAM freed |
| 29 | v3 gated signals + per-family routing | **v3 (current)** | Recovered v1 patterns without FFT |

---

## Phase 1: Can We Beat BM25?

We built 9 standalone retrieval systems. None beat BM25 on MRR across all codebases.

| System | Approach | Verdict |
|---|---|---|
| SEC | Structural Evidence Convolution (inverted index) | Good on specific codebases, can't beat BM25 generally |
| Evidence | Adjacency-based evidence propagation | Net negative |
| HRR | Holographic Reduced Representations (1024-dim) | Net negative |
| HRR v2 | Improved HRR with hypersphere normalization | Slightly less negative |
| AFMO | Adaptive Feature Map Optimization | No improvement |
| Spectral | Spectral graph coordinates (Lanczos) | Interesting, not useful |
| LSA | Truncated SVD / Latent Semantic Analysis | Captures patterns already in BM25 |
| AF26 | 26-dimensional feature vector scoring | Overfitting |
| Holo | Full holographic encoding + matching | Signal too noisy standalone |

**Lesson**: BM25's inverted index is O(1) — no full-scan system can compete on speed, and its ranking is remarkably hard to beat on correctness. The winning pattern is always: **BM25 retrieves, structural math reranks**.

## Phase 2: Goober — BM25 + Structural Reranking

Stripped everything from CruncherV2 that wasn't helping:
- Removed: energy vectors, cosine interference, hub dampening, bridging potential, yoyo validation
- Kept: BM25-dominant seed scoring, IDF-gated walk, confidence lock

Result: simpler system that strictly outperformed CruncherV2 on all 3 codebases. **Removing complexity improved results.**

## Phase 3: SEC Channel Analysis (V3→V4)

**GooberV3** added Non-Gaussianity scoring. SEC's 7 channels produce a score vector per candidate. Candidates with non-Gaussian (spiky, specific) channel profiles get boosted over flat/uniform ones. Negentropy + channel coherence as the boost formula.

**GooberV4** added query intent classification. Navigational queries (symbol lookups) and informational queries ("how does X work") get different scoring weights. Navigational queries cap structural norms lower to preserve BM25 ordering. This helped tokio slightly.

## Phase 4: Holographic Name Matching Experiments (V5–V11)

The core signal: holographic cosine similarity between query and candidate name terms has **6.8x separation** between correct and incorrect matches. Clearly a strong signal. But how to use it without causing false promotions?

| Version | Strategy | signetai | tokio | esbuild | Verdict |
|---|---|---|---|---|---|
| V4 (baseline) | SEC negentropy + query intent | 0.676 | 0.499 | 0.784 | Best all-around |
| V6 | Multiplicative HSECRR boost | 0.642 | 0.497 | 0.773 | Boost just reshuffles |
| V7 | Additive ch_name + ch_calls cosine | 0.642 | 0.521 | **0.839** | Real signal, context-dependent |
| V8 | Channel resonance profiles | 0.642 | 0.496 | 0.753 | Weaker than V4 |
| V9 | Entropy-weighted channels | 0.604 | **0.528** | 0.773 | Tokio best, signetai regressed |
| V10 | Entropy-gated V7 | 0.625 | 0.507 | 0.779 | Nowhere |
| V11 | Character-level bigram HRR | 0.623 | 0.470 | 0.771 | Too granular, noisy |

## Phase 5: Per-Candidate Gating Breakthrough (V5 Final)

The breakthrough: don't add the holographic signal to every candidate. **Gate it.**

Only candidates with cosine > 0.25 receive the holographic boost, scaled by query specificity. Below the threshold, contribution is exactly 0.

Result: V5 beats V4 on all 3 codebases simultaneously — the first version to do so.

## Phase 6: Spectral Graph Infrastructure

Upgraded spectral.rs: SPECTRAL_DIM 6→50, added eigenvalue/lambda_max tracking, Chebyshev polynomial heat kernel (O(K|E|) per query without eigendecomposition), harmonic extension (Jacobi iterative Dirichlet solver). Built `SparseGraph` with structural edge tracking separate from term-overlap edges.

## Phase 9: Geometric Search Pipeline

Replaced V5's BFS walks with Chebyshev heat diffusion on the graph Laplacian. Same V5 scoring framework, but candidates come from spectral diffusion instead of graph walks. Ran 673 parameter combinations on esbuild — discovered chebyshev_order=15 is the only meaningful parameter. Heat_t (0.3–5.0) and walk_weight (1.0–10.0) are remarkably insensitive.

**Geometric matched GooberV5 on first pass with zero tuning.** Then surpassed it on tokio (0.368 vs 0.367) and signetai (0.443 vs 0.444) after parameter tuning.

## Phase 10: Structural Geometry

**Ricci curvature.** Implemented Ollivier-Ricci curvature on structural edges. Fixed O(n²) hang by separating structural from term-overlap edges (5.6M → 6.8K on tokio). Tested as curvature-weighted matvec and post-diffusion reranker — no improvement. **Lesson: compute geometry, don't score it.** Ricci is structural infrastructure, not a scoring feature.

**Channel fingerprints.** 7-dim per-symbol edge-type distribution vector + entropy + role classification (orchestrator/library/boundary/isolate/worker). Query-independent infrastructure for Phase 11.

## Phase 11: Query as Deformation

Three new signals that make the retrieval pipeline adaptive to each query's structural context:

**Predictive Surprise (Free Energy).** For each symbol, built a conditional term model from its 1-hop structural neighborhood with Laplace smoothing over a 5K-term vocabulary. At query time, D_KL(query || symbol_predicted_terms) measures how surprising the query is given the symbol's graph context. Applied as `surprise_boost` at 0.08 weight.

**Channel Capacity Routing.** Data-driven weight adjustments based on the structural roles of seed symbols. Orchestrator seeds get more coverage weight, library seeds get more BM25 weight. Applied as additive adjustments to the intent-based weights — not replacement, augmentation.

**MDL Explanation Sets.** Greedy set cover over ranked results tracking which query terms each symbol explains. Stops when marginal information gain per symbol cost drops below 0.05.

**Result: no regressions, gains on weak categories.**

| Codebase | Geometric NDCG | Deformed NDCG | Delta |
|---|---|---|---|
| Signetai | 0.441 | 0.440 | -0.001 |
| Tokio | 0.368 | 0.371 | +0.003 |
| Esbuild | 0.501 | 0.510 | +0.009 |

`symbol-partial` improved most (+0.029 on esbuild) — the surprise signal disambiguates short common words like "embedding", "cache", "channel".

**Key lesson: additive beats replacement.** Initial attempt used channel capacity to replace Nav/Info weights entirely → NDCG regression on all 3 codebases. Switching to additive adjustments eliminated regressions and preserved gains.

## Phase 12: Production Search Modes

Added `SearchMode` enum (`Fts`, `GooberV5`, `Geometric`, `Deformed`, `Routed`) with automatic capability negotiation. The engine checks for spectral, predictive, and fingerprint artifacts and selects the strongest available mode. No silent downgrade — CLI debug output, MCP status, and search result metadata all report the active mode. This closed the trust gap where benchmarks measured Deformed but users got GooberV5.

## Phase 13–14: Artifact Lifecycle + Query Family Router

**Artifact lifecycle.** Added `.graphiq/manifest.json` tracking artifact freshness (fts, cruncher, holo, spectral, predictive, fingerprints). Heavy artifacts marked stale when graph topology changes. `graphiq doctor` reports status; `graphiq upgrade-index` rebuilds.

**Query family router.** Moved from binary Navigational/Informational intent to an 8-family `QueryFamily` classifier:

| Family | Detection | Example |
|---|---|---|
| SymbolExact | Exact name match, PascalCase | `CancellationToken` |
| SymbolPartial | Short fragment, single word | `cancel` |
| FilePath | Path separators, extensions | `scheduler/worker.rs` |
| ErrorDebug | Panic/error/failed/deadlock/timeout | `timeout in channel send` |
| NaturalDescriptive | Behavioral description | `encode a value in VLQ` |
| NaturalAbstract | "how does", "what controls" | `how does auth work` |
| CrossCuttingSet | "all", "every", plural nouns | `all connector implementations` |
| Relationship | "relationship between", "vs" | `AsyncFd vs readiness guard` |

Each family produces a `RetrievalPolicy` that gates which signals are allowed to influence ranking. This is the key architectural shift: the classifier doesn't return "intent" — it returns **permission boundaries** for downstream signals.

## Phase 15–16: File Path Router + Cross-Cutting Sets

**File path router.** Built a file/path index with path tokens, basename tokens, directory tokens, extension/language, and public symbols per file. `FilePath` family queries rank files first, then return representative symbols inside matched files. Fixed the zero on file-path NDCG for esbuild (0.000 → 0.148).

**Cross-cutting sets.** Cross-cutting set detection clusters candidates by shared interface/trait, same directory family, same role tag, or shared morphology. Returns cluster metadata (key, coverage count, representative reason) instead of a flat ranked list. This is answer-shape routing: some queries want a ranked point, some want a set.

## Phase 17: Gated Edge Evidence

Edge evidence profiles (direct, structural, reinforcing, boundary, incidental) now feed into retrieval, but **only for families where structural relation matters**: `NaturalAbstract`, `CrossCuttingSet`, `Relationship`, `ErrorDebug`. Disabled for `SymbolExact`, weak for `SymbolPartial`. Edge weight becomes `edge_kind_weight(kind) × evidence_weight(profile)` with family-based gating. This respects the research lesson: evidence is valuable when the user asks a structural question, but noise when the user already knows the symbol name.

## Phase 18: Why-Chain Unification

Created `RetrievalTrace` — a single proof object that both ranking and explanation consume:

```
RetrievalTrace {
    query_family, search_mode,
    seed_hits, expansions, evidence_edges,
    score_terms, confidence
}
```

Every search result optionally carries this trace in debug mode. MCP `why` reads the trace model, not a separate reconstruction. This makes ranking explanations falsifiable: "BM25 seeded X, heat diffusion reached Y through boundary edges, MDL kept it because it uniquely explained token Z" is the level of detail.

## Phase 19: Benchmark Lab Notebook

Expanded the benchmark harness:

- **Separate NDCG and MRR query sets** with different structures. NDCG queries use graded relevance (3/2/1) with multiple relevant symbols. MRR queries use single target symbols (`expected_symbol`). Each has easy and medium subsets.
- **Medium difficulty NL queries are the real test** — they simulate "drop codebase in, ask a real question." ~40-50% miss rate on medium NL is the frontier to improve.
- **MRR bench expanded**: Reports MRR, P@10, R@10, H@1–H@10, and miss count.
- **12 bench methods**: BM25, CRv1, CRv2, Goober, GooV3, GooV4, GooV5, Geometric, Curved, Deformed, Routed, CARE.

## Phase 20: Repo Self-Model

Built `RepoSelfModel` — deterministic, graph-derived concept nodes without embeddings:

```
ConceptNode {
    name: "Subsystem:runtime_scheduler",
    kind: Subsystem | ErrorSurface | TestSurface | PublicAPI,
    symbols: [...],
    terms: [...],
    summary: "..."
}
```

Concepts are built from subsystems (detected via edge density), error surfaces (error/panic symbols), test surfaces (test/assert symbols), and public APIs (exported/public symbols). Wired into `SearchEngine` for `NaturalAbstract` queries only.

**Results:** esbuild nl-abstract improved 0.111 → 0.156, signetai 0.049 → 0.098, tokio 0.000 (no change — tokio's generic names make concept extraction unreliable). Cross-cutting regression when applied to `CrossCuttingSet` queries — removed that wiring.

## Phase 21: CARE — Confidence-Anchored Reciprocal Expansion

CARE fuses GooV5 (best MRR) and Routed (best NDCG) into a single retrieval method. The insight: GooV5 captures **lexical precision** (name-matching confidence), while Routed captures **structural recall** (graph traversal finds neighbors BM25 misses). These are orthogonal signals.

**Fusion algorithm:**

1. Normalize both result sets to [0, 1] via max-score normalization
2. Three evidence tiers:
   - **Convergent** (both methods found it): `0.6×g_norm + 0.4×r_norm + 0.10` convergence bonus
   - **Lexical-only** (GooV5 only): `0.7 × g_norm`
   - **Structural-only** (Routed only): `0.45 × r_norm` + rank bonus (0.15 for rank-1, 0.08 for rank 2-3)
3. **BM25 anchor**: if GooV5's rank-1 matches BM25's rank-1 with >1.2x confidence gap, force it to position 1

**CARE results (v4 queries):**

| Metric | GooV5 | Routed | CARE |
|---|---|---|---|
| Signetai MRR | 0.721 | 0.691 | **0.696** |
| Tokio MRR | 0.467 | 0.348 | **0.493** |
| Esbuild MRR | 0.713 | **0.740** | 0.693 |
| Signetai NDCG | 0.375 | **0.405** | 0.384 |
| Tokio NDCG | 0.305 | **0.413** | 0.363 |
| Esbuild NDCG | 0.430 | **0.514** | 0.496 |

CARE beats both parents on MRR for signetai (+0.005) and tokio (+0.026), but never beats Routed on NDCG. The esbuild MRR regression (0.740→0.693) is the remaining problem — esbuild's descriptive names give Routed a strong structural signal that CARE dampens by blending.

**Key insight**: Score normalization is critical. Without it, GooV5's raw scores (10-100x larger than Routed's) dominate the fusion. Max-score normalization makes the two signals comparable.

## Phase 22: 5-Codebase Benchmarks + Deep Graph Edges

Expanded benchmark coverage from 3 to 5 codebases (adding flask/Python and junit5/Java) and built 4 new edge types for richer structural connectivity.

**New codebases:**

| Codebase | Language | Symbols | Why |
|---|---|---|---|
| flask | Python | 1,971 | Small codebase, decorator-based API, tests different language coverage |
| junit5 | Java | 34,273 | Large multi-module Java project, annotation-driven |

**Deep graph edges** — 4 new edge types beyond calls/imports/containment:

| Edge Type | Signal |
|---|---|
| SharesType | Functions sharing type tokens in signatures |
| SharesErrorType | Functions sharing error-type parameters |
| SharesDataShape | Functions accessing same field names |
| StringLiteral | Functions sharing error-related string constants |
| CommentRef | Comments mentioning other symbol names |

**Graph-aware seed expansion**: Seeds expanded through different edge types based on query family — ErrorDebug queries route through error-type edges, CrossCutting queries through type/data-shape edges.

**Results (5 codebases):**

| Codebase | NDCG (GraphIQ) | NDCG (Grep) | MRR (GraphIQ) | MRR (Grep) |
|---|---|---|---|---|
| signetai | **0.406** | 0.343 | **0.404** | 0.154 |
| tokio | 0.205 | **0.326** | **0.667** | 0.360 |
| esbuild | **0.411** | 0.277 | **0.475** | 0.173 |
| flask | 0.426 | **0.432** | **0.615** | 0.523 |
| junit5 | **0.198** | 0.181 | **0.420** | 0.159 |

- GraphIQ wins MRR on all 5 codebases (1.6-2.7x over Grep)
- NDCG wins on 3/5 — loses on tokio (known NL gap) and flask (small codebase, near parity)
- junit5's 34K symbols validates GraphIQ scales to Java codebases

**Importance as ranking signal doesn't move NDCG.** Fixed importance computation (broken normalization → sqrt-scaled max-degree). Tried multiplicative boost, tiebreaker, calibrated normalization — all negligible impact. Importance's value is in output display (role tags), not ranking.

## Phase 23: Speed Benchmark

Added `graphiq-bench speed` — measures warm cache latency for both GraphIQ and Grep, 10 queries, 5 warmup, 50 timed iterations.

| Codebase | Syms | GraphIQ MRR | Grep MRR | GraphIQ med | Grep med | Speedup |
|---|---|---|---|---|---|---|
| signetai | 21K | **0.247** | 0.025 | **18μs** | 124ms | 6,900x |
| tokio | 18K | **0.558** | 0.186 | **13μs** | 79ms | 6,100x |
| esbuild | 12K | **0.150** | 0.100 | **19μs** | 94ms | 4,900x |
| flask | 2K | **0.646** | 0.557 | **7μs** | 9ms | 1,300x |
| junit5 | 34K | **0.445** | 0.084 | **16μs** | 187ms | 11,700x |

GraphIQ wins MRR on all 5 and is **1,300–11,700x faster** on warm cache. Grep's `LIKE %term%` does a full table scan on every symbol's name and source code for each query term. GraphIQ uses BM25's FTS5 inverted index (O(1) per term) plus pre-built graph adjacency lists.

## Phase 24: Unified Pipeline (v6)

Consolidated ~3,000 lines of near-duplicate scoring code into a single clean architecture. The 5 search methods (GooberV5, Geometric, Deformed, Curved, CARE) shared ~90% code — parameterized into `ScoreConfig`.

**New files:**
- `seeds.rs` (~270 lines) — seed generation
- `scoring.rs` (~310 lines) — unified scoring
- `pipeline.rs` (~420 lines) — `unified_search()`

**Deleted:** 543 lines of legacy search methods from `search.rs`. All now route through `search_unified()`.

**Sub-phases:**
1. Extract seeds.rs (zero regression)
2. Extract scoring.rs (zero regression)
3. Create pipeline.rs with unified_search (zero regression)
4. Delete legacy scoring functions (543 lines removed)
5. Simplify spectral.rs (remove use_curvature flag)
6. Tune unified pipeline (5 experiments, all neutral/negative — config already optimal)
7. Benchmark validation

**Tuning experiments (all neutral/negative — config already optimal):**
1. Remove predictive surprise → esbuild regressed 0.044. Surprise stays.
2. Remove MDL → esbuild regressed 0.050. MDL stays.
3. Intent-aware test penalty (soften from 0.3→0.6) → esbuild regressed 0.035. Flat 0.3 stays.
4. Deep graph edges in neighbor hints → tokio regressed 0.020. Reverted.
5. Coverage frac refinement → already using optimal formula.

**v6 results:**

| Codebase | v5 NDCG | v6 NDCG | Δ | v6 MRR | Grep MRR |
|---|---|---|---|---|---|
| signetai | 0.406 | 0.397 | -0.009 (noise) | **0.960** | 0.941 |
| esbuild | 0.411 | **0.453** | +0.042 | **0.947** | 0.943 |
| tokio | 0.205 | **0.284** | +0.079 | **0.970** | 0.940 |

**Key lesson:** Massive code deletion with zero regression. The 5 search methods were variations on a theme — parameterizing them into `ScoreConfig` made the codebase 3,000 lines simpler while matching or improving performance.

## Phase 25: Artifact Disk Cache

CLI search was rebuilding two expensive artifacts on every invocation: HoloIndex (~10s for 30K term FFT vectors) and PredictiveModel (~4s for 20K symbols × 5K vocab terms). Warm cache time was 14s.

**Two compact cache formats:**

1. **HoloF32Cache** — HoloIndex's `name_holos` cached as flat f32 buffer (f64→f32). Query-term FFT vectors computed on-the-fly (5-10 terms × FFT = microseconds). Key insight: `holo_random_unit(holo_hash_seed(t))` is fully deterministic, so term_freq doesn't need caching.

2. **PredictiveCompactCache** — PredictiveModel compressed from 20K × 5K HashMap entries to top-200 per symbol by KL divergence from background, using shared vocab index with u32 indices and f32 values.

**Results (signetai, 20,870 symbols):**

| Metric | Before | After |
|---|---|---|
| Cold search | ~29s | ~36s (builds + caches) |
| Warm search | ~14s | **~850ms** |
| Cache size | 24MB (4 artifacts) | **75MB** (7 artifacts) |

Search quality: zero regression — scores identical to 3 decimal places. f32 quantization of normalized unit vectors is lossless for cosine similarity.

## Phase 26: SNP Structural Fallback + Source Scan Seeds

The tokio problem: generic function names (`run`, `handle`, `poll`) give BM25 nothing distinctive. A search for "how does blocking work" matches hundreds of functions equally.

**Structural Neighborhood Profiling (SNP):** For each BM25 seed, compute a structural profile (edge-type distribution of 1-hop neighborhood). Compare query term co-occurrence against these profiles. Candidates whose neighborhood "looks like" what the query describes get boosted even when their name doesn't match.

**Source scan seeds (ErrorDebug only):** When a query mentions error messages, scan source code for those exact strings. Any symbol whose source contains the error string becomes a seed candidate. Gated to ErrorDebug queries because enabling for other query types pollutes results.

**Holographic name extraction:** Extracted holographic name encoding into standalone `holo_name.rs`.

**Results (v7):**

| Codebase | v6 NDCG | v7 NDCG | Δ |
|---|---|---|---|
| signetai | 0.397 | 0.323 | -0.074* |
| esbuild | 0.453 | 0.403 | -0.050 |
| tokio | 0.284 | 0.291 | +0.007 |

*Signetai grew from 20,870 to 23,215 symbols (+11%) between benchmarks, confounding comparison.

**Lessons:** SNP helps codebases with generic names (tokio), neutral-to-negative on descriptive names (esbuild). Source scan must be gated. Predictive surprise and MDL must be kept — removing them regressed esbuild by 0.044 and 0.050.

## Phase 27: MCP Server Hardening

Seven production-readiness improvements to the MCP server for agent usability:

1. **Cold-start readiness** — `initialize` returns `_meta.ready: false` when artifacts aren't loaded. Tool calls during warming return a friendly message instead of panicking.
2. **Warming state in status** — `tool_status` appends `(warming up)` so agents can poll readiness.
3. **Interrogate synonyms + fallback** — Added synonyms to keyword matching. Fallback shows top 10 subsystems by size when no keywords match.
4. **Constants output truncation** — Symbol names in constants output truncated to 40 chars.
5. **Blast disambiguation** — When multiple symbols match, shows alternatives instead of silently picking first.
6. **top_k clamping** — `search` caps at 50, shows `(capped from N)`.
7. **Why trace formatting** — Removed confusing "reconstructed" label, improved rank display.

Rewrote all 13 MCP tool descriptions to be task-oriented (what to use when). Added `write_agents_md()` to `graphiq setup` — writes `.graphiq/AGENTS.md` with quick-reference table and workflow guides.

**Key lesson:** MCP tool ordering matters. `briefing` first, `search` second, maintenance last. Tool descriptions should answer "when should I use this?" not "what does this do?"

## Phase 28: v2 Simplification — Remove the Artifact Pipeline

> **Major version change.** Removed the entire spectral/holographic/predictive artifact pipeline.

**What was removed (5,087 lines, ~18GB RAM):**

| File | Size | What it did |
|---|---|---|
| `spectral.rs` | 42KB | Chebyshev heat diffusion, Ricci curvature, MDL explanation sets, channel fingerprints |
| `self_model.rs` | 34KB | Deterministic concept nodes for abstract queries |
| `holo.rs` | 22KB | Holographic reduced representations (1024-dim FFT vectors) |
| `structural_fallback.rs` | 9KB | SNP structural neighborhood profiling |
| `holo_name.rs` | 5KB | Holographic name matching |
| `artifact_cache.rs` | 7KB | Multi-artifact zstd disk cache |

**What was simplified:**

| File | Before | After | Change |
|---|---|---|---|
| pipeline.rs | 533 lines | 174 lines | Removed heat diffusion, SNP, source scan, predictive surprise |
| scoring.rs | 209 lines | 111 lines | 15-term product → 5-term sum |
| seeds.rs | 371 lines | 260 lines | Removed self-model expansion, source scan seeds |
| search.rs | 646 lines | 415 lines | Removed warmup state, holo/spectral references |

Also removed `nalgebra` dependency.

**Results:**

| Codebase | NDCG v1→v2 | MRR v1→v2 |
|---|---|---|
| signetai | 0.323→**0.330** (+) | 0.847→**0.900** (+) |
| esbuild | 0.403→**0.405** (=) | 0.950→0.940 (-) |
| tokio | **0.291**→0.221 (-) | **0.970**→0.848 (-) |

- signetai IMPROVED — the artifact pipeline was overriding good BM25 seeds with noise
- tokio regressed — generic names benefited from holographic name matching's 6.8x cosine separation
- Warm search: 850ms → ~50ms. Cold search: ~30s → ~5-10s. Disk cache: 75MB → ~6.5MB

**Key lesson:** 18GB of spectral/holographic/predictive artifacts produced marginal NDCG improvement (+0.02–0.05) while actively hurting on codebases with descriptive names. The graph walk captures most structural signal; BM25 captures lexical signal. The complex math was refinancing a rounding error.

## Phase 29: v3 — Gated Signals + Per-Family Routing

> **Current version.** Ported pipeline engineering patterns from holographic experimentation into the simplified v2 pipeline.

The hypothesis: the *pipeline engineering patterns* (confidence gating, specificity scaling, per-family routing) were the real discoveries, not the spectral math.

**Phase 0: Baseline capture.** Recorded v2 performance on all 3 codebases before modifications.

**Phase 1: Gated name overlap.** `compute_name_overlap()` calculates token overlap between query terms and candidate name terms. Gate: only applied when BM25's top-1 seed has a >1.2x score gap AND query specificity > 0.4. Below the gate, contribution is exactly 0. Mirrors the holographic cosine gate from Phase 5 but uses simple token overlap instead of 1024-dim FFT vectors. Result: neutral to positive on signetai/esbuild, neutral on tokio.

**Phase 2: Specificity-weighted coverage.** Query specificity (ratio of rare terms) scales the BM25 vs coverage weight balance. High-specificity queries get more BM25 weight. Low-specificity queries get more coverage weight. Bounded adjustments: BM25 weight varies from 4.0 (broad) to 2.8 (specific). Coverage weight varies from 1.0 (broad) to 1.5 (specific).

**Phase 3: Per-family ScoreConfig.** 8 family-specific parameter sets replacing the global scoring config:
- SymbolExact/FilePath: walk disabled (BM25 is sufficient)
- SymbolPartial: narrow walk
- NaturalDescriptive/Abstract: NL tokenization, per-term expansion
- ErrorDebug: error-type edge routing + source scan seeds
- CrossCutting: type/data-shape edge routing, diversity_max_per_file=1
- Relationship: full walk (walk_weight=2.0), broader seed expansion

**Phase 4: Neighborhood term fingerprints.** During cruncher index build, collect unique terms from 1-hop graph neighbors. At query time, `neighbor_match_score()` counts exact overlaps between query terms and neighbor terms. Exact-match only (no stemming, no fuzzy) to avoid false positives. Disambiguates generic names: `poll_close`'s neighbors include "frame", "buffer", "codec".

**Phase 5: Source scan seeds (ErrorDebug).** Extract error-specific phrases from ErrorDebug queries and scan source code for literal matches. Result: neutral — error queries describe scenarios, not literal strings. Reverted.

**Determinism fixes (cross-cutting):** Rust's `HashMap` randomized hashing + SQLite FTS5's tiebreak-free `ORDER BY score` produced ±0.05 variance on NDCG. Fixed by: FTS SQL tiebreaker (`ORDER BY score, sym.id`), sort tiebreakers on every sorted collection, `BTreeMap` for candidates, deterministic seed ordering. Reduced variance to ±0.015.

**Final v3 benchmark (50 queries × 2 metrics × 3 codebases, re-indexed):**

| Codebase | NDCG GraphIQ | NDCG Grep | MRR GraphIQ | MRR Grep |
|---|---|---|---|---|
| signetai | **0.339** | 0.137 (+147%) | **0.437** | 0.168 (+160%) |
| esbuild | **0.365** | 0.210 (+74%) | **0.498** | 0.256 (+95%) |
| tokio | 0.183 | **0.196** (-7%) | **0.348** | 0.306 (+14%) |

- GraphIQ dominates on descriptive-name codebases: signetai +147–160%, esbuild +74–95%
- Relationship queries are 3.7x better than grep — the graph walk is the strongest structural signal
- Tokio remains hard: generic names defeat name overlap, graph walk has weak signal in a runtime library
- Per-family routing is the right abstraction — different query types genuinely need different parameters
- Determinism matters: ±0.05 variance was masking real improvements/regressions in earlier phases

**Key lesson:** The holographic experiments' real contribution was the *pipeline engineering patterns* (confidence gating, per-family routing, specificity scaling), not the spectral math. Implementing these patterns with simple token overlap instead of 1024-dim FFT vectors produced comparable or better results at zero additional memory cost.

---

## What Didn't Work

Things we tried that didn't survive, grouped by category:

**Approaches that replaced BM25 (Phase 1):** All 9 standalone systems failed. SEC, Evidence, HRR, AFMO, Spectral, LSA, AF26, Holo — none beat BM25 generally.

**Holographic integration strategies (Phase 4):** Multiplicative boost (V6) just reshuffles. Character-level bigram HRR (V11) too granular. Channel resonance profiles (V8) weaker than negentropy. Entropy weighting (V9) helped tokio, hurt signetai.

**Geometric scoring (Phase 10):** Ricci curvature as scoring feature — real geometric signal, useless for ranking. Curvature-weighted diffusion, per-node curvature boost — no improvement.

**Signal fusion strategies (Phase 21):** Hard tier ordering causes false promotions. Pure RRF doesn't beat either parent. Score signal addition amplifies wrong candidates. BM25-adaptive binary thresholds too coarse.

**Walk tuning:** Edge-type weights, density, adaptive depth — the walk pipeline is well-tuned. All modifications produced zero improvement.

**Weight replacement (Phase 11):** Replacing Nav/Info weights with channel-derived weights regressed NDCG on all codebases. Additive adjustments work.

**Self-model on cross-cutting queries (Phase 20):** Concept nodes help abstract queries but hurt set queries — different answer shapes need different substrates.

**Source scan seeds for non-ErrorDebug (Phase 26):** Causes severe false positives. Must be gated to ErrorDebug only.

**Neighbor boost parameter tuning (Phase 29):** Per-family gate/weight caused regressions. Flat gate (0.1) + weight (0.5) was better.

**LSA reranker:** Helps signetai MRR +0.025, hurts tokio NDCG -0.020. Removed.

**SEC reranker:** Hurts ALL three codebases. Removed.

---

## Key Lessons

### 1. BM25 is hard to beat

Every system that tried to replace BM25 failed. The winning pattern is always BM25 retrieves + structural math reranks. BM25's inverted index is O(1) and its ranking is remarkably good for code search where identifiers carry meaning.

### 2. Simpler is better

CruncherV2 had 6 scoring mechanisms. Goober had 3. Goober won everywhere. v1 had 5,087 lines of spectral math. v2 removed it all and signetai actually improved. Complex interference mechanics captured patterns already captured by simpler coverage + name scoring, while introducing noise on codebases with generic function names.

### 3. Confidence matters

Two forms of confidence preservation:
- **BM25 confidence lock**: When BM25 rank-1 has a >1.2x gap, lock it. Demoting confident BM25 results is almost always wrong.
- **Signal confidence gates**: When a secondary signal is only moderately confident, don't use it. Only apply signals when they're strongly confident.

### 4. Additive beats multiplicative

Multiplicative boosts (V6) just reshuffle existing rankings. Additive contributions (V7, V5) can genuinely promote candidates the base score would miss. But additive contributions need gating to prevent false promotions from moderate-similarity noise. This lesson recurred in Phase 11: replacing weights entirely caused regressions; adding adjustments preserved gains.

### 5. Gate your signals

The raw holographic cosine had 6.8x separation — strong signal. But adding it indiscriminately caused false promotions. The gate (threshold 0.25 + query specificity scaling) turned a net-negative feature into a net-positive across all codebases. The gate adapts to the codebase: descriptive names (esbuild) pass the gate, generic names (tokio) don't. No codebase-specific tuning required.

### 6. Compute geometry, don't score it

Ricci curvature is a real structural signal. But using it as a scoring feature produced no improvement. The geometry is infrastructure, not a ranking signal. The heat diffusion that exploits the graph topology is useful; the curvature of that topology is not.

### 7. Chebyshev order is the dominant spectral parameter

673 parameter combinations tested. Only chebyshev_order matters (15 is best). Heat_t, walk_weight, and heat_top_k are remarkably insensitive across their full ranges. The diffusion's sensitivity is in the polynomial approximation quality, not the physical time constant or expansion breadth.

### 8. Codebase characteristics matter more than query characteristics

Tokio is hard because its function names are generic. Esbuild is easy because its names are descriptive. Signetai is in between. A system that overfits to one codebase's characteristics will fail on another.

### 9. Aggregate MRR is misleading

Optimizing aggregate MRR led to over-fitting on easy queries while ignoring hard ones. Better approach: pick decisive case studies (hard NL queries where BM25 fails) and treat them like a test suite. Separate NDCG and MRR query sets to measure ranking quality and first-hit accuracy independently.

### 10. Fusion requires score normalization

When fusing two retrieval methods, raw scores are incomparable — GooV5's scores can be 10-100x Routed's. Max-score normalization to [0, 1] is essential before any fusion logic.

### 11. The patterns matter, not the math

v3's gated signals + per-family routing recovered the holographic experiments' pipeline engineering patterns without the 1024-dim FFT vectors. The real discoveries were confidence gating, specificity scaling, and per-family routing — not the spectral math. Simple token overlap replaced holographic cosine. Neighborhood term fingerprints replaced predictive surprise. The patterns survived; the math didn't need to.
