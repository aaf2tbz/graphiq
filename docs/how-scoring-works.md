# How Unified Scoring and Confidence Fusion Work

Sources:
- [`crates/graphiq-core/src/scoring.rs`](../crates/graphiq-core/src/scoring.rs) — scoring formula
- [`crates/graphiq-core/src/pipeline.rs`](../crates/graphiq-core/src/pipeline.rs) — candidate assembly and post-processing

## The Candidate Struct

Every symbol that enters scoring is a `Candidate`:

```
Candidate {
    idx:            usize,      // index into CruncherIndex
    bm25_score:     f64,        // normalized BM25 (0..1)
    coverage_score: f64,        // IDF-weighted term match score
    coverage_count: usize,      // how many query terms matched
    name_score:     f64,        // name-specific coverage
    is_seed:        bool,       // came from BM25/name lookup
    walk_evidence:  f64,        // accumulated graph walk evidence
    seed_paths:     HashSet,    // which seeds led here
    ng_score:       f64,        // negentropy (channel information)
    coherence_score:f64,        // channel coherence
    holo_name_sim:  f64,        // holographic name similarity
    structural_recall: bool,    // found via name lookup
    surprise_boost: f64,        // predictive surprise
}
```

Candidates come from three sources:
1. **BM25 seeds** — from FTS search, `is_seed = true`
2. **Name lookup** — symbols whose name exactly matches a query term, `structural_recall = true`
3. **Walk/heat expansion** — graph walk neighbors and heat-diffusion-discovered symbols, `is_seed = false`

## The Scoring Formula

`score_candidates()` computes a score for each candidate:

```
base = bm25_w * bm25 + cov_w * coverage + name_w * name_coverage
     (for seeds)
base = 1.5 * coverage + 2.0 * name_coverage + walk_weight * walk_evidence
     (for walk/heat discoveries)

holo_additive = holo_max_w * query_specificity * (holo_sim - gate) / (1 - gate)
              (only if holo_sim > 0.25 gate threshold)

structural_bonus = 2.0 + 3.0 * normalized_degree
                 (for name-lookup candidates with name match)

surprise_bonus = 1.0 + 0.08 * surprise_boost
               (if predictive model available)

score = (base + holo_additive + structural_bonus)
      * coverage_frac^0.3
      * (1 + ng_w * negentropy_norm + coh_w * coherence_norm)
      * seed_bonus        (1.15x for seeds, 1.0x otherwise)
      * kind_boost        (functions/types get boost, variables/imports penalized)
      * test_penalty      (test files penalized for most queries)
      * surprise_bonus
      * mdl_penalty
```

### What each term does

**Base score** — The main retrieval signal. Seeds use a weighted combination of BM25, coverage, and name match. Walk discoveries weight coverage and name match more heavily, plus walk evidence.

**Coverage fraction** (`coverage_frac^0.3`) — Power-law dampening on the fraction of query terms matched. The 0.3 exponent means matching 50% of terms gives ~82% of the score of matching 100%. This prevents results that match many low-value terms from dominating.

**Negentropy + coherence boost** — SEC (Structural Evidence Channels) produces 5 channels of evidence for each candidate. Negentropy measures how much information the channels carry (vs uniform noise). Coherence measures how aligned the channels are. Both are normalized to [0, 1] and added as multiplicative boost.

**Holographic additive** — If holographic name similarity exceeds the 0.25 gate, the excess is scaled by query specificity (fraction of high-IDF terms) and added. This rewards results whose names have high holographic similarity to the query, but only when the query is specific enough to benefit.

**Kind boost** — Functions and types get a boost. Variables, imports, and modules get a penalty. This reflects that search queries typically target callable functions and definable types.

**Test penalty** — Symbols in test files are penalized unless the query has informational intent (where test examples are often the best answer).

**Seed bonus** — 1.15x for seeds, 1.0x for walk discoveries. Slight preference for BM25-confirmed candidates.

## Intent-Dependent Weights

The query is classified as **Navigational** (symbol-like) or **Informational** (NL-like), which determines the base weights:

| Intent | bm25_w | cov_w | name_w | ng_w | coh_w |
|---|---|---|---|---|---|
| Navigational | 5.0 | 0.8 | 1.0 | 0.1 | 0.05 |
| Informational | 3.0 | 1.5 | 2.0 | 0.25 | 0.15 |

These are further adjusted by channel capacity weights from the fingerprint system.

## ScoreConfig

`ScoreConfig` bundles all tuneable parameters. The unified pipeline constructs it in `pipeline.rs` based on intent, channel weights, and whether spectral/predictive models are available. Key parameters:

- `holo_gate: 0.25` — minimum holographic similarity to contribute
- `seed_paths_threshold` — minimum number of seed paths for a walk candidate to be scored (1 with heat diffusion, 2 without)
- `use_idf_coverage_frac` — whether to use IDF-weighted or raw count coverage fractions

## Confidence Fusion (Post-Scoring)

After scoring, two post-processing steps run:

### BM25 Lock (`apply_bm25_lock`)

If BM25's top result has a >1.2x score gap over the second result AND the top result's name contains a query term, it's promoted to rank 1 with a +1e6 bonus. This prevents the structural signals from overriding a clear BM25 match.

### File Diversity (`apply_file_diversity`)

No more than 3 results per source file in the final output. This prevents a single large file from dominating all top-K positions.

### MDL Penalty

After initial scoring, an MDL (Minimum Description Length) explanation set is computed. If the explanation covers >50% of query terms, results get a small bonus proportional to marginal gain. This rewards result sets that collectively explain the query.

## The Full Flow

```
Seeds + Walk + Heat candidates
        |
        v
   score_candidates()
   (formula above)
        |
        v
   apply_bm25_lock()
   (promote confident BM25 #1)
        |
        v
   MDL explanation set
   (collective coverage bonus)
        |
        v
   apply_file_diversity()
   (max 3 per file)
        |
        v
   top_k results
```
