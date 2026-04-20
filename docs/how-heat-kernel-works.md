# How the Heat Kernel Works

Source: [`crates/graphiq-core/src/spectral.rs`](../crates/graphiq-core/src/spectral.rs) (lines 619-685), [`crates/graphiq-core/src/pipeline.rs`](../crates/graphiq-core/src/pipeline.rs) (heat diffusion section)

## The Problem

Given a set of seed symbols from BM25, we want to find structurally similar symbols that don't share any text with the query. The mechanism: simulate heat diffusion on the code graph. Heat propagates from seed nodes through graph edges. Symbols that are structurally close to multiple seeds accumulate more heat.

## The Graph

The spectral index builds a normalized Laplacian from the code graph:

1. Each symbol is a node.
2. Structural edges (calls, imports, extends, implements, references, tests) connect nodes with weights.
3. The adjacency matrix `A` is symmetrized.
4. Degree vector `d` computed: `d[i] = sum of edge weights incident to i`.
5. Normalization: `inv_sqrt_d[i] = 1/sqrt(d[i])`.

The **normalized Laplacian** is `L = I - D^{-1/2} A D^{-1/2}`. This maps eigenvalues to [0, 2], which is required for Chebyshev approximation.

The **rescaled Laplacian** is `L_rescaled = (2/lambda_max) * L - I`, which maps eigenvalues to [-1, 1] — the domain of Chebyshev polynomials.

## The Heat Kernel

The heat kernel `h(t, L)` describes how a unit of heat at time 0 distributes across the graph at time `t`. Mathematically:

```
h(t, L) = exp(-t * L)
```

In the eigenbasis of L, this is diagonal: eigenvalue `lambda_i` gets scaled by `exp(-t * lambda_i)`. Large eigenvalues (high-frequency components) decay fast. Small eigenvalues (low-frequency, global structure) persist.

For code search, this means heat preferentially flows along the low-frequency structure of the graph — the architectural backbone — while ignoring noisy local connections.

## Chebyshev Approximation

Direct computation of `exp(-tL)` requires full eigendecomposition: O(n^3). For a codebase with 20K symbols this is expensive and unnecessary.

Instead, GraphIQ uses Chebyshev polynomial approximation:

1. **Compute Chebyshev coefficients**: The heat kernel function `f(x) = exp(-t*x)` is approximated by computing coefficients `c[k]` for `k = 0..K` using numerical integration (1024-point trapezoidal rule on [-1, 1]).

2. **Iterative polynomial evaluation**: Instead of computing the full polynomial explicitly, use the three-term Chebyshev recurrence:
   ```
   T_0(x) = f(x)
   T_1(x) = L_rescaled * f(x)
   T_k(x) = 2 * L_rescaled * T_{k-1}(x) - T_{k-2}(x)
   ```

3. **Accumulate**: `result = sum(c[k] * T_k)` for k = 0..K.

Default parameters: `K = 15` (Chebyshev order), `t = 3.0` (diffusion time).

## Complexity

Each iteration requires one sparse matrix-vector multiply: O(|E|) where |E| is the number of edges. With K iterations, total cost is O(K * |E|). For a graph with 50K edges and K=15, this is ~750K multiply-adds — fast enough for interactive queries.

## How It's Used in the Pipeline

In `unified_search()` (`pipeline.rs`):

1. Seed symbols are mapped from CruncherIndex IDs to SpectralIndex IDs.
2. `chebyshev_heat()` is called with uniform seed weights.
3. The top-K heat results (symbols with highest accumulated heat) are returned.
4. Each heat-discovered symbol is checked: it must have at least one high-IDF query term in its term set (relevance filter — heat alone isn't enough).
5. Heat evidence is combined with coverage score and an optional Ricci curvature boost.

The heat results expand the candidate set beyond what BM25 + name lookup found. These candidates get scored alongside the seed candidates in the unified scoring stage.

## Ricci Curvature Boost

When available, average Ricci curvature (Ollivier-Ricci) for each node is used as a bonus: symbols in high-curvature regions (densely interconnected, "bottleneck" areas of the code graph) get a slight boost (up to 30% additional evidence weight). This preferentially surfaces structurally important symbols.
