# GraphIQ Unified Pipeline Specification

## Problem

GraphIQ has two nearly-identical 800-line scoring functions (`goober_v5_search` at cruncher.rs:2378 and `geometric_search` at cruncher.rs:2754) that share ~90% of their code. The only differences are that geometric adds Chebyshev heat diffusion, curvature, predictive surprise, channel capacity weights, MDL, and fingerprints on top of everything goober_v5 does.

The result: 53 distinct signals scattered across 3,500+ lines, with no clear separation between what works and what doesn't. Every change requires editing two nearly-identical scoring blocks and praying the other codebase doesn't regress.

## What Actually Works

Based on all benchmarking across signetai (20K symbols), tokio (18K symbols), and esbuild (12K symbols):

### Tier 1 — Proven, high-signal methods

| # | Method | What it does | Why it works |
|---|---|---|---|
| 1 | **BM25/FTS with adaptive weights** | SQLite FTS5, NL queries get boosted source/hints weights | Universal seed generator. Everything depends on this being right. |
| 2 | **Neighbor hints (index-time)** | Inject distinctive neighbor terms into FTS `search_hints` column | The breakthrough. Moves tokio nl-abstract from 0.000 to 0.087. Makes vocabulary from structural neighborhoods searchable at zero query-time cost. |
| 3 | **Per-term FTS seed expansion** | Each query term → FTS search (with stemming + synonyms) | Finds symbols that mention query terms in source/hints/signature, not just name. Moved nl-descriptive from 0.193 → 0.265 on tokio. |
| 4 | **Graph-aware seed expansion** | Follow `shares_type`, `shares_error_type`, `shares_data_shape` deep edges | Adds candidates connected by structural similarity to existing seeds. |
| 5 | **Coverage score (IDF-weighted)** | Per-term REFT evidence: each query term matched against symbol's full term set | The core scoring signal. IDF weighting means matching "cooperative" (rare) counts more than matching "task" (common). |
| 6 | **Name coverage (IDF-weighted)** | Same as coverage but only against name terms | Strong signal for symbol queries. Names are the most reliable identifier. |
| 7 | **SEC 7-channel negentropy + coherence** | Measures how "peaked" a candidate's signal distribution is across 7 structural channels | Correctly identifies candidates with concentrated signal vs flat generic matches. |
| 8 | **Holographic name gate** | FFT cosine similarity between query and candidate name holograms, gated at 0.25 | 6.8x separation between correct and incorrect on esbuild. Gate prevents false promotions on tokio. |
| 9 | **Structural walk evidence** | 2-hop BFS from seeds, weighted by edge type and depth | Provides evidence for non-seed candidates. The graph topology IS evidence. |
| 10 | **Chebyshev heat diffusion** | Polynomial approximation of graph heat kernel for spectral expansion | Expands beyond text-matched seeds into structurally close symbols. |
| 11 | **BM25 confidence lock (with name gate)** | Promotes top BM25 seed to rank 1 if dominant AND has name match | Prevents graph-walk noise from demoting clearly correct top results. Empty-name symbols no longer lock. |
| 12 | **Kind boost / test penalty / file diversity** | Functions 1.3x, tests 0.3x, max 3 per file | Simple structural priors that consistently help. |

### Tier 2 — Marginal, keep but don't overinvest

| # | Method | Signal strength | Notes |
|---|---|---|---|
| 13 | **Predictive surprise** | Very weak (0.08 weight) | Almost invisible in scoring. Could be removed with no measurable effect. |
| 14 | **Numeric bridge seeds** | Niche | Only helps queries containing numbers. Keep as seed expansion. |
| 15 | **Self-model expansion** | Marginal | Only NaturalAbstract, and those queries are still weak. Keep as seed expansion. |
| 16 | **Channel capacity weights** | Marginal | Additive adjustments to already-tuned weights. Small effect. |
| 17 | **MDL explanation set** | Marginal | Only fires when >50% coverage. Small effect on final ranking. |

### Tier 3 — Dead or harmful, remove

| # | Method | Why remove |
|---|---|---|
| 18 | **Ricci curvature** | Expensive to compute, weak signal, never measured a meaningful contribution |
| 19 | **CARE fusion** | Never routed to by `route_mode`. Dead code path. |
| 20 | **Curvature-weighted heat** | Depends on Ricci curvature. Remove with it. |
| 21 | **Bridging potential** | Only in legacy `cruncher_search`, not goober_v5 or geometric |
| 22 | **Standalone spectral search** | Never called from the pipeline |
| 23 | **Exact heat kernel** | Replaced by Chebyshev approximation |
| 24 | **Harmonic extension** | Never called from the pipeline |

## Architecture

### The Unified Pipeline

One function. Four stages. No mode branching inside the scoring loop.

```
Query
  │
  ├─── Stage 1: CLASSIFY
  │     Family (8 types) → Policy (weights, flags)
  │     Intent (Nav/Info) → Weight profile
  │
  ├─── Stage 2: SEED
  │     BM25 FTS (adaptive weights)
  │     Per-term FTS expansion (stem + synonyms)
  │     Graph-aware expansion (deep edges)
  │     Chebyshev heat diffusion (if spectral index loaded)
  │     Numeric bridge seeds (if query has numbers)
  │     Self-model expansion (if NaturalAbstract)
  │
  ├─── Stage 3: SCORE
  │     For each candidate:
  │       base = bm25_w * bm25 + cov_w * coverage + name_w * name
  │       walk = walk_w * walk_evidence (Info intent only)
  │       holo = gated holographic additive
  │       struct = structural_recall bonus
  │       ng = 1 + ng_w * negentropy + coh_w * coherence
  │       final = (base + walk + holo + struct) * cov_frac^0.3 * ng * kind * test_penalty * seed_bonus
  │
  └─── Stage 4: POST-PROCESS
        BM25 confidence lock (with name gate)
        File diversity cap (max 3/file)
        Exact match promotion (SymbolExact only)
        Diversity boost (family-scaled)
```

### Key Design Principles

1. **Seed generation is separate from scoring.** Currently the two are interleaved — seeds get special scoring paths, walk candidates get different formulas. Unify: all candidates enter the same scoring function regardless of how they were discovered. A flag `is_seed` gives a 1.15x bonus, nothing more.

2. **Spectral is optional, not a mode.** Whether the spectral index is loaded determines whether Chebyshev heat runs as an additional seed expansion. The scoring formula is identical either way. Delete the Deformed/Geometric distinction — it's just "did heat diffusion run?"

3. **Coverage fraction uses IDF, not raw count.** Current: `matched_terms / total_terms`. New: `max(idf_weighted_coverage, raw_count / total_terms)`. This way matching 2 high-IDF terms out of 5 scores higher than matching 2 low-IDF terms out of 5.

4. **Intent affects weights, not code paths.** Navigational and Informational differ only in the 5 weight parameters (bm25_w, cov_w, name_w, ng_w, coh_w). No branching in the scoring loop.

5. **One function, one scoring formula.** Delete `goober_v5_search` (378 lines) and `geometric_search` (594 lines). Replace with `unified_search` (~300 lines) that takes a `SearchConfig` struct specifying which seed methods are available and what the weight profile is.

### The SearchConfig struct

```rust
struct SearchConfig {
    // Seed methods
    use_heat_diffusion: bool,      // spectral index loaded?
    use_self_model: bool,          // self-model loaded?
    heat_t: f64,                   // diffusion time (from policy)
    cheb_order: usize,             // polynomial order

    // Scoring weights (from intent + policy + channel_adj)
    bm25_w: f64,
    cov_w: f64,
    name_w: f64,
    ng_w: f64,
    coh_w: f64,
    walk_w: f64,

    // Modifiers
    diversity_boost: f64,          // from family
    file_diversity_cap: usize,     // 3
}
```

### File Structure After Refactor

```
crates/graphiq-core/src/
  search.rs          — router + post-processing (unchanged)
  pipeline.rs        — NEW: unified_search(config) → Vec<(i64, f64)>
  seeds.rs           — NEW: all seed generation methods
  scoring.rs         — NEW: unified scoring formula
  cruncher.rs        — LEGACY: keep for bench comparisons, not routed to
  fts.rs             — FTS search (unchanged)
  spectral.rs        — Chebyshev heat, simplify (remove curvature, exact kernel, harmonic)
  ...
```

## What Gets Deleted

### From cruncher.rs
- `cruncher_search` (334 lines) — legacy, not routed to
- `cruncher_v2_search` (265 lines) — legacy, not routed to
- `cruncher_search_standalone` (85 lines) — legacy
- `goober_search` (410 lines) — legacy
- `goober_v3_search` (322 lines) — legacy
- `goober_v4_search` (460 lines) — legacy
- `goober_v5_search` (378 lines) — replaced by unified
- `geometric_search` (594 lines) — replaced by unified

**Total: ~2,850 lines of scoring functions → ~300 lines of unified scoring.**

### From spectral.rs
- Ricci curvature computation (~200 lines)
- Curvature-weighted heat (~80 lines)
- Exact heat kernel (~40 lines)
- Harmonic extension (~55 lines)

**Total: ~375 lines removed.**

### From search.rs
- `search_goober_v5` method
- `search_geometric` method
- `search_deformed` method
- `search_care` method
- `SearchMode` enum variants for CARE, GooberV5, Geometric, Deformed → replaced by single `SearchMode::Unified`

**~400 lines simplified.**

### Net: ~3,600 lines deleted, ~600 lines added. Net reduction: ~3,000 lines.

## Benchmark Targets

Current scores (with neighbor hints, after this session's improvements):

| Codebase | Current NDCG@10 | Grep | Target |
|---|---|---|---|
| Signetai | 0.378 | 0.267 | >= 0.400 (maintain lead) |
| Esbuild | 0.428 | 0.286 | >= 0.450 (grow lead) |
| Tokio | 0.296 | 0.300 | >= 0.340 (overtake grep) |

The unified pipeline MUST NOT regress any current score. The simplification should be benchmark-verified at each step.
