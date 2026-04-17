# HRR Roadmap: Bivector Expansion & Beyond

Current state: **HRR-Biv5 = 1.857 aggregate** (+0.076 over BM25, +0.057 over HRR-Rerank).

## Current Best Scores

| Method | Self | Tokio | Signetai | Aggregate |
|---|---|---|---|---|
| BM25 | 0.715 | 0.539 | 0.527 | 1.781 |
| HRR-Rerank (2-hop) | 0.719 | 0.546 | 0.535 | 1.800 |
| **HRR-Biv5 (seeds=5, w=0.5)** | **0.777** | 0.528 | **0.552** | **1.857** |
| HRR-Pure (2-hop) | 0.633 | 0.500 | 0.429 | 1.562 |

Biv5 per-DB optimal weights: Self w=1.0 → 0.826, Tokio w=0.3 → 0.528, Signetai w=0.3 → 0.552.

## The Problem: Tokio Regression

Biv5 regresses tokio by -0.018 (0.546→0.528). The bivector expansion dilutes BM25's correct results for symbol-exact queries (`spawn` 1.000→0.678, `block on` 1.000→0.678). The gain comes from NL queries (`periodic interval` 0.431→0.539, `split tcp` 0.512→0.639). The fundamental tension: bivector expansion helps where BM25 is weak but hurts where BM25 is already correct.

## Avenue 1: Conditional Bivector Activation (HIGH IMPACT, TARGET: +0.02 aggregate)

Only apply bivector expansion when BM25's confidence is low. Skip it for high-confidence queries.

**Confidence signals to test:**
- **BM25 top-1 score vs top-10 score ratio**: if the top result is much better than the 10th, BM25 is confident → skip bivector. If scores are flat, BM25 is uncertain → use bivector.
- **BM25 top-1 vs top-2 gap**: a big gap means one dominant result → skip bivector.
- **Query class detection**: exact symbol queries (`Runtime`, `spawn`) should skip bivector entirely. NL queries should use it.

**Implementation:**
```
let confidence = bm25_scores[0] / bm25_scores[9].max(0.01);
if confidence > threshold {
    // BM25 is confident — just use HRR-Rerank
    hrr_rerank(...)
} else {
    // BM25 is uncertain — use bivector expansion
    bivector_expand + RRF + hrr_rerank
}
```

**Why it could work**: The per-query data shows that bivector *always* helps NL queries and *sometimes* hurts exact queries. A simple threshold on score ratio could capture this pattern without explicit query classification.

**Expected impact**: If we recover tokio's exact-query scores (1.000 for `spawn`, `block on`) while keeping the NL gains, tokio goes from 0.528 → ~0.560. Aggregate → ~1.889.

## Avenue 2: Bivector Expansion with Top-K Pruning (HIGH IMPACT, TARGET: +0.01 aggregate)

Instead of injecting ALL top-50 bivector candidates into the RRF merge, only inject candidates that pass a quality filter.

**Filter criteria:**
- **HRR query similarity floor**: candidate must have `dot(query_vec, candidate_hologram) > threshold`. Currently we inject structurally related symbols regardless of whether they match the query text at all.
- **Bivector score threshold**: only inject candidates with bivector dot product above the mean.
- **Deduplication via structural overlap**: if a bivector candidate shares 3+ graph neighbors with an existing BM25 candidate, skip it — it's likely redundant.

**Why**: Current Biv5 injects 50 candidates, most of which are noise on large codebases. The tokio regression is driven by low-quality bivector candidates pushing correct BM25 results out of top-10. A quality gate would let through only the genuinely relevant structural matches.

## Avenue 3: Multi-Bivector Composition (MEDIUM-HIGH IMPACT, TARGET: +0.02 aggregate)

Instead of a single bivector from the top-5 BM25 results, compute **multiple bivectors from different structural neighborhoods**:

- **Call-graph bivector**: pairwise rejection between BM25 results that share call relationships
- **Type-hierarchy bivector**: pairwise rejection between results that share type ancestry
- **File-local bivector**: pairwise rejection between results in the same file/directory

Each bivector captures a different structural plane. Score expansion candidates by their projection onto the COMBINED bivector (weighted average based on which bivector has highest coherence).

**Why**: The current single bivector mixes all structural relationships. For a query like "tcp accept connections", the call-graph bivector should dominate (finding `accept()` and `incoming()`). For "rate limit middleware", the type-hierarchy bivector should dominate (finding `Middleware` trait implementors). Separating them gives cleaner directional signal.

**Implementation**: Group BM25 top-K results by their structural relationships (call neighbors, type ancestors, file siblings). Compute one bivector per group. Use coherence to weight groups. Expand from the highest-coherence bivector first.

## Avenue 4: Iterative Holographic Query Refinement (MEDIUM IMPACT, TARGET: +0.01 aggregate)

The "text → geometry → math → text" loop:

1. BM25 top-5 → bivector expand → top-50 structural matches
2. Extract terms from structural matches (via `hrr_expand_query`)
3. Feed expanded terms back into BM25 as enriched query
4. RRF merge original BM25 + enriched BM25 + bivector expansion
5. HRR rerank

This is different from our current approach because step 2→3 converts structural discoveries back into the text domain where BM25 can actually use them. Currently we inject structural matches via RRF, but BM25 never gets to see the vocabulary they contain.

**Why**: The term vocabulary of structurally related symbols often contains the exact words the user was looking for but couldn't express. `periodic interval timer` → bivector finds `Interval::new_periodic()` → extract terms ["interval", "new", "periodic"] → enriched BM25 query now matches `Interval` with high confidence.

**Risk**: Term injection can also inject noise terms that dilute the query. Mitigate with term scoring (only inject terms that appear in 3+ structural matches).

## Avenue 5: Enriched Identity Vectors (MEDIUM IMPACT)

Already tested: file path + qualified name terms into identity vectors. Result: HRR-Pure dropped from 0.632 to 0.591 on self. The enrichment diluted the signal because file path terms are too generic.

**Revised approach**: Instead of adding ALL context terms, selectively bind high-signal context:
- File stem as a binding: `file_db ⊛ identity` (not just "db" as a term)
- Module path as a binding: `module_core ⊛ identity`
- Symbol kind as a binding: `is_function ⊛ identity`

Bindings are additive through circular convolution, so they encode STRUCTURE (this symbol IS in db.rs) rather than just TEXT (this symbol mentions "db"). The binding `file_db ⊛ identity` means "the identity of this symbol when it's in the db file" — a role-filler pair that preserves distinctiveness.

**Expected impact**: Marginal. The enrichment approach already failed once. Bindings might work better than term injection, but the fundamental problem is that file/module terms are too common.

## Avenue 6: Query-Side Structural Parsing (MEDIUM IMPACT)

Detect structural intent in queries and encode it as holographic bindings:
- "calls X" → `calls_vec ⊛ X_identity`
- "what contains X" → `contains_vec ⊛ X_identity`
- "X implements Y" → `implements_vec ⊛ X_identity`
- Default: plain term superposition (current)

Then use circular CORRELATION (the inverse of convolution) to unbind: `query_correlation ⊛ candidate_hologram ≈ filler`. This is HRR's killer feature — we can ask "what is bound to X through relation R?" which BM25 fundamentally cannot do.

**Why**: 3 of our 26 tokio queries and 3 of 25 signetai queries are structural ("what connects callers to callees", "how does the runtime schedule"). These query types map naturally to binding queries.

**Expected impact**: Small on aggregate (few structural queries in benchmark) but huge for the queries it helps. Also a differentiator — no other code search tool does structural query parsing.

## Avenue 7: Adaptive Seed Count per Query (LOW-MEDIUM IMPACT)

Instead of fixed seeds=5, adapt the seed count based on BM25 result quality:

**Heuristic**: Use seeds=N where N is the number of BM25 results with score > 50% of the top result. If only 1 result dominates, seeds=1 (skip bivector — one result can't form a bivector). If 8 results are clustered, seeds=8 (rich structural plane).

**Why**: seeds=5 is a compromise. For queries where BM25 finds 2 strong results and 48 weak ones, using seeds=5 includes 3 noise seeds. For queries where BM25 finds 15 equally-good results, seeds=5 leaves signal on the table.

## Avenue 8: Heat Kernel Diffusion on Graph Laplacian (DEFERRED)

Build the graph Laplacian L, compute heat kernel K_t(u,v) = Σ_k e^{-tλ_k} φ_k(u)φ_k(v). Time parameter t controls locality. Replace HRR's graph encoding with diffusion-based proximity. Deferred until HRR/bivector approaches are fully explored.

## Recommended Order

1. **Conditional bivector activation** (Avenue 1) — highest expected impact, addresses tokio regression directly
2. **Bivector candidate pruning** (Avenue 2) — complementary to #1, reduces noise
3. **Benchmark both together** — should push aggregate past 1.880
4. **Multi-bivector composition** (Avenue 3) — if #1 and #2 saturate
5. **Iterative text↔geometry loop** (Avenue 4) — if structural approaches plateau
6. **Query-side structural parsing** (Avenue 6) — unlock HRR's binding queries
7. **Enriched identity via bindings** (Avenue 5) — marginal expected value
8. **Adaptive seeds** (Avenue 7) — fine-tuning
9. **Heat kernel** (Avenue 8) — if all HRR approaches plateau

## Target

| Method | Self | Tokio | Signetai | Aggregate |
|---|---|---|---|---|
| BM25 | 0.715 | 0.539 | 0.527 | 1.781 |
| HRR-Rerank | 0.719 | 0.546 | 0.535 | 1.800 |
| HRR-Biv5 | 0.777 | 0.528 | 0.552 | 1.857 |
| **Target (Avenues 1+2)** | **~0.777** | **~0.565** | **~0.560** | **~1.900** |
| **Stretch (Avenues 1-4)** | **~0.800** | **~0.600** | **~0.580** | **~1.980** |
