# AFMO v2 Roadmap: Quantum-Inspired Propagator & Spectral Bandpass

Source: Bekenstein "Advanced Quantum Theory A" — mapped to GraphIQ by Alex & Squirt, April 2026.

## Current State (AFMO v1)

Poincaré ball model with hierarchy from file/qualified-name structure. Net +0.008 aggregate NDCG across 3 codebases (0.717/0.543/0.529 vs BM25 0.715/0.539/0.527). Conservative gate-based boosts.

### What's proven
- Hierarchy is real: modules(depth 0) → structs/functions(depth 1) → methods/constants(depth 2)
- Self: 42/550/9, Tokio: 487/7167/68, Signetai: 169/5500/7212
- Hyperbolic distance gives meaningful spread (0.2-0.8 range) unlike hyperspherical (crammed at 0.2-0.3)
- Gate-based multiplicative boosts preserve BM25 ranking while adding signal
- Hyp_sim distribution is genuinely discriminative with real hierarchy

### What's weak
- Boost coefficients are very conservative — can't push hard without regression
- Graph proximity is simple (just "has edge to seed node") — wastes structural info
- SVD weighting is flat σ_k — doesn't adapt to query confidence
- No multi-hop path information in reranking

## Three Ideas from Quantum Mechanics

### Idea 1: Path Integral Propagator (HIGH IMPACT)

**Physics:** The Feynman propagator K(r,t;r',t') = ∫ D[path] exp(iS/ħ) sums over ALL paths between states, weighted by action.

**Mapping:**
- "State" = symbol in SVD space
- "Path" = sequence of graph edges from FTS seed to candidate
- "Action" = -log(edge_weight × semantic_coherence)
- "Propagator" P(q→s) = Σ over all paths from FTS seeds to s of Π(edge_weight × coherence)

**Current approach (Born approximation, 1st order):**
```
graph_boost = 1 + 0.06 * (avg edge weight to seed nodes)
```

**Propagator approach (full Born series):**
```
propagator(s) = Σ over seed nodes s_i [
    w(s_i → s) * cos(v_s, v_q)                           // 1-hop
    + Σ over intermediate k [
        w(s_i → k) * cos(v_k, v_q) * w(k → s) * cos(v_s, v_q)  // 2-hop
    ]
]
```

**Why it's better:**
- Captures multi-hop semantic coherence (path stays "on topic")
- Rewards paths through semantically relevant intermediaries
- Naturally handles indirect relationships (A calls B which references C)
- The PRODUCT of weights ensures path quality (one bad edge kills the path)

**Implementation:**
- Replace `graph_boost` in `afmo_rerank` with `propagator_score`
- Compute 1-hop and 2-hop paths using existing `gravity` graph
- Semantic coherence = max(0, cosine(v_node, v_query)) on top-16 σ-weighted dims
- Normalize propagator by dividing by max across candidates
- Apply as multiplicative boost: `1.0 + α * propagator_score`

### Idea 2: Spectral Bandpass (MEDIUM IMPACT)

**Physics:** The propagator in energy representation: K(E) = Σ_k |k⟩⟨k| / (E - σ_k). The parameter E creates a natural bandpass — components near E are amplified, far ones suppressed.

**Mapping:**
- "Energy representation" = SVD basis (already there)
- σ_k = singular values (already there)
- E = bandpass center (query-adaptive)

**Current approach:**
```
sim(q, s) = Σ_k σ_k * q_k * s_k  (σ-weighted dot product)
```

**Bandpass approach:**
```
sim(q, s) = Σ_k q_k * s_k / (E - σ_k)
```

**Query-adaptive E:**
- Count matching query terms N_match
- High N_match (confident query): E = σ_1 * 1.2 → narrow bandpass, top components dominate
- Low N_match (uncertain query): E = σ_1 * 2.0 → wide bandpass, more components contribute
- Formula: E = σ_1 * (1.2 + 0.8 / (1 + N_match))

**Why it might help:**
- Currently, top SVD components dominate (σ decays from ~3.2)
- For uncertain queries, we should listen to more components
- For precise queries, we should focus on the strongest signal
- This is principled, not ad hoc

**Implementation:**
- Modify `project_query` to accept an E parameter
- Compute bandpass-weighted query vector
- Use in both `afmo_search` and `afmo_rerank`
- Alternatively: add as a separate signal alongside existing σ-weighted projection

### Idea 3: Fock Space Validation (ALREADY DONE)

**Physics:** Multi-particle states in occupation number representation. Overlap ⟨symbol|query⟩ = Π_i ⟨s_i|q_i⟩.

**Mapping:** This is EXACTLY our `geometric_mean(per_term_similarities)`. The geometric mean is the Fock space inner product. This validates the approach — no change needed.

## Implementation Order

1. **Propagator scoring** — replace graph_boost in afmo_rerank
2. **Spectral bandpass** — add as alternative query projection
3. **Benchmark self**, then tokio/signetai
4. **If net positive**: tune coefficients and wire into search pipeline
5. **If not**: try Berry curvature (neighborhood structural complexity)

## Files to Modify

- `crates/graphiq-core/src/afmo.rs` — main changes
  - Add `propagator_score()` function
  - Modify `afmo_rerank()` to use propagator
  - Add `project_query_bandpass()` with E parameter
- `crates/graphiq-bench/src/main.rs` — already wired, just rebuild

## Key Reference Equations

- Propagator: K(E) = Σ_k |k⟩⟨k| / (E - E_k) — Eq from Bekenstein Ch 2
- Born series: T = V + V G₀ V + V G₀ V G₀ V + ... — Ch 3, perturbation theory
- Path integral: K = ∫ D[path] exp(iS/ħ) — Ch 2.1
- Berry connection: A = i⟨n|∂/∂R|n⟩ — Ch 1.3.6
- Fock space: |n₁,n₂,...⟩ with a†ᵢ|...,nᵢ,...⟩ = √(nᵢ+1)|...,nᵢ+1,...⟩ — Ch 5.1

## Baseline to Beat

| Codebase | BM25 | AFMO v1 |
|---|---|---|
| Self | 0.715 | 0.717 |
| Tokio | 0.539 | 0.543 |
| Signetai | 0.527 | 0.529 |
| **Aggregate** | **1.781** | **1.789** |

Target: aggregate ≥ 1.800 (net +0.019 over BM25)
