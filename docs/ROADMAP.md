# GraphIQ Roadmap

## Current State

**GooberV5** — per-candidate holographic name gating. Wired into `graphiq search`
and `graphiq-mcp`. FFT-based cosine similarity thresholded at 0.25, scaled by
query specificity. Beats all prior versions on 2 of 3 codebases.

| Codebase | BM25 | **GooberV5** | Delta |
|---|---|---|---|
| signetai | 0.556 | **0.681** | +0.125 |
| tokio | 0.583 | **0.517** | -0.066 |
| esbuild | 0.675 | **0.799** | +0.124 |

### Completed

- [x] Wire GooberV5 into search pipeline (`search.rs`)
- [x] Remove dead HRR/evidence/SEC branching from `search.rs` (-521 lines)
- [x] Clean unused index fields (`top_idf` dead code removed)
- [x] Latency profiling (p50/p99 across all codebases)
- [x] Fuzz testing (53 adversarial queries, zero panics)
- [x] Multi-language demo codebase (Rust, TS, Python, Go) + self-test benchmark
- [x] LSA infrastructure: structurally-augmented TF-IDF matrix, randomized SVD,
     isotropic hyperspherical normalization, angular scoring (`lsa.rs`)
- [x] Holographic boundary tracing with FFT circular convolution and
     relation-type binding (`holo.rs`)
- [x] SEC 7-channel term propagation with inverted index (`sec.rs`)

## Thesis

GraphIQ should move toward a codebase that can recognize itself — not just a
search engine that ranks symbols. The real target is still grep, because grep
is what developers trust when they want to touch reality. The way to beat it is
not by becoming more ornate than grep. It is by turning a repo into a
self-describing recall field where BM25 gives you the fast lexical entry point
and the graph tells you how the system holds itself together.

A query like "how does auth work" should not feel like throwing words into a
box. It should feel like deforming the codebase's own internal geometry until
the right structures light up. The win is not novelty — it is making code
mathematically legible to itself so an agent can reach the truth faster than a
human chaining ripgrep, sort, and intuition.

From that perspective, the next step is not another clever scorer. It is
building a substrate where edges carry explanation and candidates carry evidence
profiles — so the graph knows not only that A connects to B, but what kind of
support that relation represents, whether it is direct, causal, structural,
reinforcing, contradictory, or boundary-defining.

Then the agent-facing surface — through MCP and CLI — becomes a way to
interrogate that self-knowledge directly: not just "what matches," but "why does
this part of the system recognize this question as its own reflection?"

GraphIQ should not become a memory system or a personal notebook. It should
become a recall surface for code where structure is explicit, support is
inspectable, and the repo can explain how it works from within its own internal
physics.

---

## Phase 7: Edge Evidence Profiles

**Goal**: Make edges first-class knowledge carriers. Extend `Edge` with an
evidence profile that captures what kind of support the relation represents.
This is the main arc — the hypersphere reranker is a supporting component
folded in at Step E.

### Why

The current edge model has 9 types (Calls, Contains, Implements, Extends,
Overrides, References, Imports, Tests, ReExports) but edges carry only `kind`
and `weight`. The `metadata` field is `serde_json::Value::Null` everywhere in
practice.

A `Calls` edge between `main() → authenticate()` and between
`authenticate() → hashPassword()` are the same type with the same weight.
But one is an orchestration boundary and the other is an internal
implementation detail. The retrieval pipeline treats them identically.

### Step A: Evidence Taxonomy

Define the evidence types an edge can carry:

| Evidence | Meaning | Example |
|---|---|---|
| `direct` | A literally calls/contains/references B | `main() → run_server()` |
| `structural` | A's position in the graph depends on B's position | `RateLimiter` contains `checkLimit` |
| `reinforcing` | Multiple paths confirm the A→B relationship | Both import graph and call graph connect them |
| `boundary` | A is the public interface, B is the implementation | `trait Auth → impl Auth` |
| `incidental` | The connection exists but carries little semantic signal | A references type B in a generic way |

**Deliverable**: `EvidenceKind` enum in `edge.rs`.

### Step B: Index-Time Evidence Inference

After indexing, compute evidence profiles for each edge:

1. **Direct**: Default for Calls, Contains, Imports where source directly names target.
2. **Structural**: Edge participates in a structural motif (hub, guard, orchestrator).
3. **Reinforcing**: Multiple distinct paths exist between source and target.
   Computed via `bfs_multiplicity` — reuse the BFS machinery from `evidence.rs`
   (`bfs_distances`) but at per-edge granularity instead of per-candidate.
4. **Boundary**: Edge crosses a visibility or module boundary (public → private, interface → impl).
5. **Incidental**: Catch-all for low-signal edges (References with no other connection).

Store as JSON in the existing `metadata` column.

**Precedent**: `evidence.rs` already computes multi-path convergence at the
candidate level (`bfs_distances` + convergence scoring). The edge-level version
reuses the same BFS but queries "how many distinct paths connect source to
target" rather than "how many query-term seeds reach this candidate." Different
question, same traversal engine.

**Deliverable**: `infer_edge_evidence(db, edges) -> Vec<EvidenceKind>`
**Verify**: Manual inspection on signetai. Boundary edges should cluster at
module interfaces.

### Step C: Evidence-Aware Retrieval

Modify the Goober walk to weight edges by evidence type:
- `direct` and `boundary` edges carry the most retrieval signal
- `reinforcing` edges get a multiplicity bonus
- `incidental` edges get a penalty

This replaces the current flat `edge_weight` per kind.

**Precedent**: Research notes say "walk tuning (edge types, density, adaptive
depth) produced zero improvement." That was tuning existing edge-type weights.
Evidence profiles are a different signal — they add multiplicity (reinforcing),
boundary crossing (boundary), and motif membership (structural) as new
dimensions that were not tested in the walk-tuning experiments. Still, this
step should be validated against the walk-tuning null result. If evidence
weights produce no improvement over flat edge weights, stop here — the walk
is already well-tuned.

**Deliverable**: Modified walk scoring in `cruncher.rs`.
**Benchmark**: 3-codebase NDCG comparison.

### Step D: Agent-Facing Interrogation Surface

New MCP tools:

- `explain` — given a symbol, return its structural role, evidence-bearing edges,
  and the subsystem it belongs to
- `topology` — given a region, return motifs and boundary-defining symbols
- `why` — given a search result, return the evidence chain that caused it to rank

These don't require new indexing — they expose what the graph already knows.

**Deliverable**: 3 new MCP tools in `graphiq-mcp/src/main.rs`.

### Step E: Anisotropic Hypersphere Reranker

A supporting component, not the main arc. The anisotropic sphere warps latent
space so geometric recovery is more precise, but it serves the evidence-aware
retrieval pipeline — it is not the point itself.

**Why the isotropic sphere was insufficient**: The existing LSA pipeline
(Phase 6, `lsa.rs`) computes a structurally-augmented TF-IDF matrix, applies
randomized SVD to k=96 dimensions, then isotropic normalization (`v̂ = v/||v||₂`)
with angular scoring. This "captured patterns already in BM25" (research.md)
because high-σ dimensions encode generic patterns BM25 already handles, and the
isotropic sphere gives them equal weight.

**The fix: anisotropic weighting.** Warp the sphere before projection:

```
ṽ = Wv / ||Wv||₂
```

where W is diagonal. The i-th diagonal entry combines two signals:

**1. Singular value mass** σᵢ — how much co-occurrence structure dimension i
captures. High σ alone isn't enough — top singular values capture generic
patterns.

**2. Per-dimension discriminativity** — how non-uniform dimension i is across
symbols:

```
discᵢ = 1 - |mean(sᵢ)| / std(sᵢ)
```

**Combined specificity:**

```
specᵢ = σᵢ × discᵢ
```

**Diagonal weights:**

```
wᵢ = (specᵢ / max(spec))^α + ε     α ∈ [0.5, 2.0], ε ≈ 0.1
```

- α = 0 recovers the isotropic sphere
- α = 1 is the natural weighting
- α = 2 aggressively suppresses noisy dimensions
- ε prevents collapsing weak dimensions to zero

**Precedent**: AFMO (`afmo.rs:82-130`) attempted sigma-based diagonal weighting
via `project_query_bandpass`. It failed because it used only σᵢ (no
discriminativity term) and had an inversion bug where low-σ dimensions were
amplified 100x. The key addition is `discᵢ` — it suppresses dimensions that
have high σ but are generically distributed across all symbols.

**Risk flag**: `discᵢ` is conceptually similar to the entropy weighting from V9
(research.md), which "helped tokio, hurt signetai." If discᵢ produces the same
pattern, the ε floor and α parameter are critical. If per-dimension weighting
isn't robust across codebases, fall back to σ-only weighting (AFMO showed it's
safe but weak) or keep W = identity (isotropic).

**Sub-steps:**

1. **Per-dimension analysis** — Compute discᵢ, specᵢ for all 96 dimensions.
   Diagnose that generic dims get low specificity, domain-specific dims get
   high. Deliverable: `analyze_latent_dimensions() -> DimensionProfile`.

2. **Weight matrix** — Implement weight derivation with α=1.0, ε=0.1. Store W
   alongside the LSA index. Deliverable: `compute_anisotropy_weights()`.

3. **Anisotropic normalization** — Replace `normalize_to_sphere` with
   `normalize_anisotropic(vecs, weights)`. Apply at index and query time.
   Deliverable: modified `lsa.rs`.

4. **Reranker wiring** — Place behind GooberV5 for NL / low-confidence queries
   only:
   ```
   BM25/FTS → GooberV5 → Anisotropic LSA rerank (top_k)
   ```
   Activation: query is informational, GooberV5 top-1 confidence < threshold,
   ≥2 terms in LSA basis. Blend: `goober * 0.7 + lsa * 0.3`.
   Deliverable: `lsa_rerank()` in `search.rs`.

5. **Ablation** — 30 configs (3 codebases × 5 α values × 2 augmentation modes).
   Proves whether anisotropy is what makes the difference vs isotropic LSA.

6. **Geometric MISS recovery** — When BM25 returns nothing, project query onto
   anisotropic sphere and return all symbols within angular radius θ.
   Deliverable: `geometric_expand(query, theta)`.

7. **Multi-concept queries** — Decompose NL queries into sub-concepts, project
   each independently, score by angular distance to weighted centroid.

### Success Criteria

| Metric | Target |
|---|---|
| Tokio NDCG@10 | > 0.58 |
| Signetai NDCG@10 | > 0.56 |
| Self NDCG@10 | >= 0.71 (no regression) |
| Evidence coverage | ≥ 80% of edges have inferred evidence (not `incidental`) |
| `why` tool accuracy | Manual: ≥ 4/5 test queries produce correct evidence chains |
| Index time (evidence + LSA) | < 8 seconds |
| Query latency | < 2ms additional |

### Key Files

- `crates/graphiq-core/src/edge.rs` — Modified: `EvidenceKind` enum
- `crates/graphiq-core/src/evidence.rs` — Modified: per-edge BFS multiplicity
- `crates/graphiq-core/src/cruncher.rs` — Modified: evidence-aware walk scoring
- `crates/graphiq-core/src/lsa.rs` — Modified: anisotropic weights, warped normalization
- `crates/graphiq-core/src/search.rs` — Modified: LSA reranker behind GooberV5
- `crates/graphiq-core/src/db.rs` — Modified: latent vector + evidence storage
- `crates/graphiq-core/src/index.rs` — Modified: evidence inference + anisotropic LSA
- `crates/graphiq-mcp/src/main.rs` — Modified: `explain`, `topology`, `why` tools
- `docs/DESIGN-LSA.md` — Updated: anisotropic hypersphere math

---

## Phase 8: Self-Describing Recall Surface

**Goal**: The graph should be able to explain how it works from within its own
internal physics. Not just "what matches," but "why does this part of the system
recognize this question as its own reflection?"

This is the culmination. The evidence profiles from Phase 7 give edges
meaning. Now the graph uses those meanings to detect its own structure, name
its own patterns, and answer questions about itself.

### Step A: Subsystem Detection

Detect subsystem boundaries from edge evidence profiles + module structure.
A subsystem is a cluster of symbols with dense internal edges and sparse
external edges.

**Implementation note**: Do NOT use spectral graph coordinates (graph Laplacian
eigendecomposition). Spectral was tried (`spectral.rs`) and produced
"interesting, not useful" results — 6 dimensions weren't enough to capture
meaningful code structure. Instead, use a simpler method: thresholded edge
density from the evidence profiles computed in Phase 7. Two symbols belong to
the same subsystem if they share enough `direct` or `boundary` edges, with
modularity maximization to find clean cuts. This is community detection on a
weighted graph, not eigen-decomposition.

**Deliverable**: `detect_subsystems(db) -> Vec<Subsystem>`

### Step B: Structural Role Materialization

Promote `search_hints` from FTS text into first-class graph structure.
Each symbol gets a materialized `structural_role` derived from its motif,
evidence-bearing edges, and subsystem membership.

Roles to detect:
- **Entry point**: Called from outside its subsystem, high `boundary` edge count
- **Hub**: High degree, connects many symbols within its subsystem
- **Guard**: Sits at subsystem boundary, validates/preconditions before internal calls
- **Leaf**: Minimal outbound edges, performs concrete work
- **Orchestrator**: Calls many internal symbols, high `direct` edge count, rarely called from outside

**Deliverable**: `structural_role` field on `Symbol`, computed at index time.

### Step C: Self-Knowledge Queries

Agent-facing queries that interrogate the graph's own structure:

- "What are the boundary-defining symbols?"
- "What does the graph believe about how authentication works?"
- "Where are the structural contradictions?" (inconsistent error handling,
  mixed patterns, divergent conventions)

These are not search queries — they are questions the graph answers about
itself by tracing evidence-bearing edges and subsystem boundaries.

**Deliverable**: `interrogate` MCP tool that accepts structural queries.

### Step D: Recall Surface Validation

The real benchmark for a self-describing recall surface is not NDCG on search
results. It is whether an agent can answer structural questions about the
codebase by interrogating the graph, without reading source files.

**Validation protocol**:

1. Select 10 structural questions per codebase (30 total):
   - "Where is the main entry point for the HTTP server?"
   - "What module handles authentication and what does it depend on?"
   - "Where are error boundaries — where does the code transition from
     validated to unvalidated state?"
   - "What is the deepest call chain in the codebase?"
   - "Where does the codebase violate its own conventions?"

2. For each question, the agent uses only GraphIQ MCP tools (no file reads).

3. Human judges whether the answer is correct and complete.

4. Target: ≥ 7/10 correct on each codebase.

**Deliverable**: Validation results + benchmark harness for automated re-runs.

---

## Standing Priorities

These run in parallel with the phased work above.

### Tokio Regression

Generic function names make structural signals unreliable. The evidence-aware
walk (Phase 7 Step C) and anisotropic reranker (Phase 7 Step E) should both
help — evidence profiles penalize incidental connections between generic names,
and the anisotropic sphere downweights dimensions where generic terms dominate.
If they don't, fallback approaches:
- Seed-only fallback for queries with low average IDF
- Name specificity bonus for seeds matching high-IDF terms

### Expand Benchmarks

- Add Python (Django/Flask) and Java (Spring) codebases
- Bootstrap resampling for statistical significance

### Production Polish

- Wire GooberV5 into `graphiq demo` command
- Benchmark CI integration
- Binary size optimization
