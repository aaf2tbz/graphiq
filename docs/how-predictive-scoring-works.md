# What Predictive Scoring Is

Source: [`crates/graphiq-core/src/spectral.rs`](../crates/graphiq-core/src/spectral.rs) — `PredictiveModel`, `compute_predictive_model()`, `predictive_surprise()` (lines 1063-1251)

## The Idea

For every symbol in the codebase, we can build a simple language model: "given this symbol, what terms are likely to appear nearby?" If a query contains terms that are *surprising* (unlikely) under a symbol's local model, that symbol is probably not relevant. If the query terms are *expected* (likely), the symbol might be relevant even if BM25 didn't find it.

This is called **predictive surprise** — it measures how much a query deviates from what a symbol's neighborhood "predicts."

## Building the Model

`compute_predictive_model()` builds two things:

### Background Model

The top 5000 terms by frequency across all symbols form the vocabulary. Each term gets a probability proportional to its corpus-wide frequency:

```
P_background(t) = count(t) / total_term_count
```

This is the "default" expectation — what you'd predict without knowing anything about a specific symbol.

### Conditional Models (one per symbol)

For each symbol `s`, a local term distribution is built from:

1. The symbol's own terms (name, decomposed name, signature, doc comment, source code) — weight 2.0
2. Its structural neighbors' terms (calls, imports, extends, implements, references, tests edges) — weight 1.0

The raw frequencies are smoothed with the background model (Laplace-like smoothing, alpha=0.1):

```
P(t | s) = (1 - alpha) * P_local(t) + alpha * P_background(t)
```

This prevents zero probabilities for terms that don't appear locally but might still be relevant.

### Compact Storage

The full conditional model for 20K symbols with a 5000-term vocabulary would be ~718MB. Instead, GraphIQ stores only the top 200 terms per symbol ranked by KL divergence from background (i.e., the terms that make this symbol's neighborhood unique). This compresses to ~8.8MB with zstd.

## Predictive Surprise

`predictive_surprise()` computes a KL-divergence-like measure between the query's term distribution and a symbol's conditional model:

```rust
for each query term t:
    q_prob = 1.0 / n_query_terms  // uniform query distribution
    p_q = P(t | symbol)           // symbol's conditional probability
    kl += q_prob * ln(q_prob / p_q)
```

High KL divergence means: "this query's terms are unlikely under this symbol's model" — the symbol is surprised by the query. Low KL means the query terms are expected.

### How Surprise Enters Scoring

Surprise is **inverted** in its effect: high surprise means the symbol is *less* likely to be relevant. But in the actual scoring formula, it's used as a small bonus:

```rust
surprise_bonus = 1.0 + 0.08 * surprise_boost
```

Where `surprise_boost` is the surprise value normalized by the maximum across all candidates. This is a subtle signal — at most 8% bonus — that slightly boosts symbols whose local term neighborhoods align well with the query.

### Why It's Subtle

Benchmarking showed that predictive surprise as a primary signal caused regressions on some codebases (esbuild lost 0.044 NDCG when predictive surprise was removed, but the effect is highly nonlinear). The 0.08 coefficient was tuned to provide consistent small gains without creating pathological cases.

## Related: Channel Capacity Weights

`channel_capacity_weights()` uses the fingerprint system to adjust scoring weights based on the structural roles of the top BM25 seeds. If seeds are mostly "orchestrator" symbols (high outgoing calls), the name matching weight gets boosted. If seeds are "library" symbols (high incoming calls), the BM25 weight gets boosted.

This is a separate mechanism from predictive surprise but operates on the same principle: the structure of initial results informs how we should weight subsequent scoring.

## Related: MDL Explanation Set

`mdl_explanation_set()` computes a greedy set cover: which results collectively cover the most query terms with the fewest symbols? If coverage exceeds 50%, the marginal gain (information per additional result) is used as a small collective bonus.

This rewards diversity in the result set — a set of results that collectively explains all query terms is better than one that explains the same subset repeatedly.
