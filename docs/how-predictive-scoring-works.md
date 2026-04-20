# What Predictive Scoring Is

Source: [`crates/graphiq-core/src/spectral.rs`](../crates/graphiq-core/src/spectral.rs) — `PredictiveModel`, `compute_predictive_model()`, `predictive_surprise()` (lines 1063-1251)

## The Idea

For every symbol in the codebase, we build a simple conditional term model from its structural neighborhood. At query time, we compute how "expected" the query terms are under each symbol's model. Symbols whose neighborhoods align with the query get a small boost — not because alignment means the symbol is the answer, but because alignment is a discriminative signal that separates plausible matches from the long tail of loosely-related candidates.

The mechanism is KL divergence from a background model. After normalization across the candidate pool, it becomes a mild ranking refinement.

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

`predictive_surprise()` computes KL divergence between a uniform query distribution and each symbol's conditional term model:

```rust
for each query term t:
    q_prob = 1.0 / n_query_terms  // uniform query distribution
    p_q = P(t | symbol)           // symbol's conditional probability
    kl += q_prob * ln(q_prob / p_q)
```

A high KL value means the query's terms are unlikely under this symbol's model — the symbol's neighborhood doesn't predict these terms. A low KL value means the query terms are expected.

### How It Actually Works in Scoring

Despite the name "surprise," the signal is used as a **positive discriminative boost** after normalization:

1. Raw KL divergence is computed for every candidate.
2. All values are normalized to [0, 1] by dividing by the maximum across the pool.
3. The normalized value is applied as a small multiplicative bonus: `1.0 + 0.08 * normalized_surprise`.

This means symbols with higher relative alignment (lower raw surprise, which after normalization corresponds to candidates that stand out from the pool) get a slight edge. The 0.08 coefficient keeps it subtle — at most 8% bonus — because the signal is noisy as a primary ranker.

The practical effect: for ambiguous short queries like "cache" or "embedding," where BM25 returns many candidates with similar scores, the surprise signal slightly prefers symbols whose neighborhoods contain related terms (e.g., a `CacheManager` surrounded by `evict`, `ttl`, `invalidate` gets a small boost over an unrelated `Cache` variable in a logging module).

## Related: Channel Capacity Weights

`channel_capacity_weights()` uses the fingerprint system to adjust scoring weights based on the structural roles of the top BM25 seeds. If seeds are mostly "orchestrator" symbols (high outgoing calls), the name matching weight gets boosted. If seeds are "library" symbols (high incoming calls), the BM25 weight gets boosted.

This is a separate mechanism from predictive surprise but operates on the same principle: the structure of initial results informs how we should weight subsequent scoring.

## Related: MDL Explanation Set

`mdl_explanation_set()` computes a greedy set cover: which results collectively cover the most query terms with the fewest symbols? If coverage exceeds 50%, the marginal gain (information per additional result) is used as a small collective bonus.

This rewards diversity in the result set — a set of results that collectively explains all query terms is better than one that explains the same subset repeatedly.
