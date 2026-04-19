# GraphIQ Finishing Roadmap

_Drop a codebase in. Get instant structural knowledge. No embeddings, no LLM. Just math, physics, and dimensions._

## Where We Are

21 research phases. 12 retrieval methods. 33K lines of Rust. The retrieval engine is mature:

| Method | esbuild NDCG | tokio NDCG | signetai NDCG |
|---|---|---|---|
| BM25 | 0.299 | 0.272 | 0.287 |
| Routed | **0.514** | **0.413** | **0.405** |
| CARE | 0.496 | 0.363 | 0.384 |

The system works. BM25 seeds, spectral heat diffusion expands, deformation reshapes, query family gates signals, holographic name matching boosts, CARE fuses. Six layers, zero neural, deterministic.

**What's not done:** The system retrieves symbols well. But the vision isn't just retrieval — it's a codebase that knows itself. An agent drops in and can ask structural questions, trace bridges, understand architecture. The remaining work is in three areas: closing retrieval gaps on hard categories, making numbers/symbols into first-class bridging entities, and building the self-describing recall surface that lets the codebase explain itself.

## The New Theory: Numbers and Symbols as Bridge Entities

### Observation

Code is full of numbers and symbolic constants that act as structural glue:

- `200`, `404`, `500` appear across HTTP handlers, response builders, middleware, tests
- `0.5`, `0.25`, `0.08` appear in scoring, thresholds, decay constants
- `DEFAULT_TIMEOUT`, `MAX_RETRIES`, `BUFFER_SIZE` are named constants used across modules
- `f64`, `f32`, `usize`, `i32` are type constraints that connect implementations
- `3850` (daemon port) appears in config, CLI, tests, dashboard
- `O(n)`, `O(1)` appear in doc comments and performance-critical code

Currently these are noise in FTS — just more tokens. The identifier decomposition splits `HTTP_404` into `http 404`, but the `404` is treated as a generic term with low IDF because it appears "everywhere." The system doesn't understand that `404` is a **bridge** — it connects the error handler to the response builder to the test case.

### The Insight

Numbers and symbolic constants are **implicit edges** in the code graph. They're structural identifiers just like function names, but they cross boundaries that named edges don't:

1. **Cross-type**: `404` appears in an enum variant (Error), a response builder (Response), and a test (rateLimit.test)
2. **Cross-layer**: `0.5` appears in a decay constant, a similarity threshold, and a configuration default
3. **Cross-file**: `DEFAULT_PORT = 3850` connects config.rs, server.rs, cli.rs, and health_test.rs
4. **Cross-language**: `200` means the same thing whether the codebase has Rust, TypeScript, or Go

Named edges (Calls, Imports, Extends) connect symbol A to symbol B through explicit relationships. Number/symbol bridges connect symbol A to symbol B through **shared structural constants** — they're the "dark matter" of the code graph.

### How to Exploit It

#### Phase N1: Numeric Literal Extraction

At index time, extract all numeric literals from symbol sources:

```
fn handle_rate_limit() -> Response {
    if requests > MAX_REQUESTS {          // MAX_REQUESTS is a named constant
        return Response::new(429)         // 429 is a numeric bridge
    }
    Response::new(200)                    // 200 is a numeric bridge
}
```

Extract: `429`, `200`, plus the constant name `MAX_REQUESTS`.

Create a new edge type: `SharesConstant`. Two symbols get a `SharesConstant` edge if they reference the same numeric literal or named constant. Weight by:

```
weight = (count_of_shared_literals / min(total_literals_a, total_literals_b)) * rarity_boost
```

where `rarity_boost = log(total_symbols / symbols_containing_this_literal)`. A literal like `0` (everywhere) gets low boost. A literal like `3850` (specific) gets high boost.

**Deliverable**: `extract_numeric_literals()` in `index.rs`, `SharesConstant` edge kind in `edge.rs`, bridge computation in `index.rs`.
**Files**: `index.rs`, `edge.rs`, `db.rs`
**Risk**: Low. Additive edges that only participate in spectral diffusion if the query family permits it. No changes to existing scoring.

#### Phase N2: Symbolic Constant Indexing

Extract named constants (PascalCase ALL_CAPS identifiers assigned literal values):

```
const DEFAULT_PORT: usize = 3850;
const MAX_RETRIES: usize = 3;
const SIMILARITY_THRESHOLD: f64 = 0.25;
```

Treat these as **named numeric entities** — they have both a symbolic identity (`DEFAULT_PORT`) and a value identity (`3850`). Symbols that reference `DEFAULT_PORT` get a `ReferencesConstant` edge to the constant definition. Symbols that contain the literal `3850` get a `SharesConstant` edge to the constant definition AND to other symbols containing `3850`.

This creates a two-hop bridge: function A uses `DEFAULT_PORT` → constant `DEFAULT_PORT = 3850` → function B contains literal `3850`. The constant acts as a hub connecting the named reference to the numeric value.

**Deliverable**: Constant extraction in `index.rs`, `ReferencesConstant` edge kind, hub detection for constants.
**Files**: `index.rs`, `edge.rs`

#### Phase N3: Numeric Bridge Scoring

Add numeric bridge awareness to spectral diffusion. When heat diffusion propagates from FTS seeds, `SharesConstant` edges carry weight proportional to the constant's rarity:

```
edge_weight(SharesConstant, literal) = 0.3 * log(N_symbols / N_symbols_with_literal)
```

A rare literal like `3850` (3 symbols) gets high weight. A common literal like `0` (2000 symbols) gets near-zero weight. This is the IDF principle applied to numeric literals.

**Gate**: Only activate for `NaturalAbstract`, `CrossCuttingSet`, and `ErrorDebug` query families. Symbol-exact queries don't need numeric bridges — they already match by name.

**Deliverable**: Modified heat diffusion in `spectral.rs`, query family gating in `query_family.rs`.
**Benchmark**: 3-codebase NDCG/MRR. nl-abstract should improve (queries like "what port does the server listen on" should find the constant).

#### Phase N4: Numeric Entity Surface

New MCP tool: `constants`. Given a query, return all numeric literals and named constants that appear in the codebase with their usage sites:

```bash
graphiq constants "timeout"
# DEFAULT_TIMEOUT = 30000  (used in: server.rs::start, client.rs::connect, config.rs::defaults)
# TIMEOUT_MS = 5000        (used in: retry.rs::backoff, health.rs::check)
```

This gives the agent instant structural knowledge about what constants glue the code together.

**Deliverable**: `constants` MCP tool, CLI command.
**Files**: `graphiq-mcp/src/main.rs`, `graphiq-cli/src/main.rs`

### Expected Impact

| Category | Current (Routed) | Expected with N1-N4 |
|---|---|---|
| nl-abstract (tokio) | 0.000 | 0.10-0.20 |
| nl-abstract (signetai) | 0.098 | 0.15-0.25 |
| cross-cutting (signetai) | 0.083 | 0.15-0.25 |
| error-debug (signetai) | 0.941 | 0.941 (maintained) |

Numeric bridges won't help symbol-exact or symbol-partial (those are name-matching problems). But they should meaningfully help nl-abstract and cross-cutting — exactly the categories that remain unsolved.

---

## Closing Retrieval Gaps

### The Unsolved Categories

| Category | Best NDCG | Issue |
|---|---|---|
| nl-abstract (tokio) | 0.000 | Generic function names make structural signals unreliable |
| nl-abstract (signetai) | 0.098 | Concept nodes help but insufficient |
| cross-cutting (signetai) | 0.083 | Per-package limit too restrictive |
| symbol-partial (signetai) | 0.040 | Single tokens match hundreds |

### Phase R1: CARE Integration into Search Pipeline

CARE currently exists only in the benchmark harness. Wire it into the production search pipeline as the default search mode, replacing Routed.

**Why**: CARE beats Routed on MRR (tokio 0.493 vs 0.348, signetai 0.696 vs 0.691) while matching Routed on NDCG. For agent recall (H@3), CARE is superior.

**Implementation**: CARE needs to run both GooV5 and Routed search internally and fuse results. This means `SearchEngine::search()` runs two passes. The latency cost is ~2x but both passes share the same BM25 seeds and spectral index, so actual overhead is ~1.4x.

**Deliverable**: `SearchMode::CARE` in `search.rs`, production wiring.
**Files**: `search.rs`, `cruncher.rs`

### Phase R2: Relaxed Cross-Package Expansion

Phase 5 Step W was identified but never implemented. Increase per-package limit from 1 to 2, include FTS hits in cross-cutting results (currently discarded), fall through to normal path when expansion finds nothing.

**Deliverable**: Modified `directory_expand.rs`.
**Files**: `directory_expand.rs`, `search.rs`

### Phase R3: Adaptive Fusion Weights

CARE currently uses fixed weights (0.6/0.4 for convergent, 0.7 for lexical-only, 0.45 for structural-only). Make these adapt based on query family:

| Family | GooV5 Weight | Routed Weight | Rationale |
|---|---|---|---|
| SymbolExact | 0.85 | 0.15 | Name matching dominates |
| SymbolPartial | 0.70 | 0.30 | Name still matters more |
| NaturalAbstract | 0.30 | 0.70 | Structure is the answer |
| CrossCuttingSet | 0.20 | 0.80 | Must traverse the graph |
| ErrorDebug | 0.40 | 0.60 | Error context is structural |
| NaturalDescriptive | 0.55 | 0.45 | Balanced |

This is family-based gating applied to fusion — the same principle that worked for edge evidence (Phase 17) and holographic gating (Phase 5).

**Deliverable**: Family-aware fusion weights in `search.rs`.
**Risk**: Medium. The research record shows that per-category tuning can overfit. Validate on all 3 codebases.

---

## Self-Describing Recall Surface

The ultimate goal: the codebase can answer questions about itself without an agent reading source files. This is Phase 8 from the original roadmap, now with 10 more phases of learning behind it.

### Phase S1: Subsystem Detection (Revival)

Phase 7 defined subsystem detection via edge density + modularity maximization. It was implemented (`subsystems.rs`, 1275 lines) but never wired into the agent surface. Wire it.

The subsystem detector produces:

```
Subsystem {
    name: "HTTP Middleware",
    boundary_symbols: [RateLimiter, AuthMiddleware, CorsHandler],
    internal_symbols: [TokenBucket, checkLimit, validateToken],
    imports_in: [setupMiddleware, server::configure],
    exports_out: [RateLimiter::handle],
}
```

New MCP tool: `topology`. Given a region or subsystem name, return its boundaries, internal structure, and external connections.

**Deliverable**: Wire `subsystems.rs` into `SearchEngine`, add `topology` MCP tool.
**Files**: `search.rs`, `self_model.rs`, `graphiq-mcp/src/main.rs`

### Phase S2: Structural Role Materialization

Phase 7 Step B defined structural roles (entry point, hub, guard, leaf, orchestrator). The `roles.rs` module (582 lines) computes these. Materialize them as first-class data that the `explain` and `why` tools consume.

When an agent asks "what is RateLimiter?", the answer should include:

```
RateLimiter (struct)
  Role: Entry Point + Guard (boundary of middleware subsystem)
  Calls: checkLimit(), TokenBucket.consume(), getConfig()
  Called by: setupMiddleware(), chain.execute()
  Contains: handle(), checkLimit(), reset()
  Constants: MAX_REQUESTS (shared with: RateLimitError, loadConfig)
```

The constants line is the bridge from Phase N1-N4. This is where it all connects.

**Deliverable**: `explain` MCP tool that composes subsystem + role + constants + evidence.
**Files**: `graphiq-mcp/src/main.rs`, `role_query.rs`

### Phase S3: Self-Knowledge Validation

The validation protocol from Phase 8 Step D. 10 structural questions per codebase, agent uses only GraphIQ MCP tools, human judges correctness. Target: 7/10 on each codebase.

**Questions should include:**
1. "Where is the main entry point for the HTTP server?"
2. "What module handles authentication and what does it depend on?"
3. "What numeric constants connect the timeout subsystem to the retry subsystem?"
4. "Where are the error boundaries?"
5. "What is the deepest call chain?"
6. "Where does the codebase violate its own conventions?"
7. "What symbols bridge the middleware and routing subsystems?"
8. "What constants are shared across the most modules?"
9. "Where is rate limiting configured and who consumes that configuration?"
10. "What is the structural role of the daemon module?"

**Deliverable**: Validation harness + results document.
**Files**: `benches/validation/`

---

## Codebase Cleanup

The codebase has 47 source files. Many are historical artifacts from abandoned approaches:

### Dead Code (Remove)

| File | Lines | Status | Reason |
|---|---|---|---|
| `af26.rs` | 869 | Dead | 26-dim feature vector scoring — overfitting (research.md) |
| `afmo.rs` | 816 | Dead | Adaptive Feature Map — net +0.008, abandoned |
| `hrr.rs` | 1500 | Dead | HRR v1 — net negative (research.md) |
| `hrr_v2.rs` | 1453 | Dead | HRR v2 — slightly less negative |
| `qhrr.rs` | 777 | Dead | Quantum HRR — abandoned |
| `windtunnel.rs` | 775 | Dead | Experimental retrieval system |
| `hypergraph.rs` | 477 | Dead | Unused hypergraph experiment |
| `motifs.rs` | ~200 | Dead | Superseded by roles.rs |

**Total removal**: ~6,867 lines. This would bring the codebase from 33K to ~26K lines — cleaner, easier to navigate, less confusing for contributors.

### Keep But Clean

| File | Lines | Action |
|---|---|---|
| `cruncher.rs` | 3348 | Keep — core retrieval (GooV5). Remove dead v1-v4 branches |
| `sec.rs` | 1232 | Keep — SEC channel scoring. Remove unused CRv1/CRv2 paths |
| `evidence.rs` | 1022 | Keep — evidence profiles. Clean dead BFS multiplicity |
| `lsa.rs` | 1516 | Keep — LSA infrastructure. Remove isotropic-only paths |

### Production Polish

1. **Wire CARE as default search mode** (Phase R1)
2. **Update `graphiq demo`** to use CARE/Routed instead of GooberV5
3. **Benchmark CI** — run NDCG/MRR on every PR via GitHub Actions
4. **Binary size optimization** — strip dead code, check `cargo bloat`
5. **Expand benchmarks** — add Python (Django) and Java (Spring) codebases
6. **Bootstrap resampling** for statistical significance on benchmark differences

---

## Execution Order

```
Phase N1 (Numeric Literal Extraction)        → index-time, no scoring changes
Phase N2 (Symbolic Constant Indexing)        → extends N1 with named constants
Phase N3 (Numeric Bridge Scoring)            → query-time, gated by family
Phase N4 (Numeric Entity Surface)            → MCP tool + CLI command
    ↓
Phase R1 (CARE Integration)                  → wire CARE into production
Phase R2 (Relaxed Cross-Package)             → easy tuning win
Phase R3 (Adaptive Fusion Weights)           → family-aware fusion
    ↓
Phase S1 (Subsystem Detection Wiring)        → MCP topology tool
Phase S2 (Structural Role Materialization)   → MCP explain tool
Phase S3 (Self-Knowledge Validation)         → prove it works
    ↓
Cleanup (remove 7K lines of dead code)
Production Polish (CI, benchmarks, binary size)
```

After each phase: benchmark all 3 codebases, commit only if self-benchmark does not regress and at least one external benchmark improves.

---

## Success Criteria

### Retrieval

| Metric | Current | Target |
|---|---|---|
| esbuild NDCG@10 | 0.514 | >= 0.530 |
| tokio NDCG@10 | 0.413 | >= 0.450 |
| signetai NDCG@10 | 0.405 | >= 0.450 |
| tokio MRR | 0.493 | >= 0.520 |
| signetai MRR | 0.696 | >= 0.720 |
| nl-abstract (any) | 0.000-0.156 | >= 0.20 |
| cross-cutting (any) | 0.083-0.282 | >= 0.25 |

### Self-Knowledge

| Metric | Target |
|---|---|
| Structural question accuracy | >= 7/10 per codebase |
| `constants` tool usefulness | Manual: 4/5 queries produce correct bridge lists |
| `explain` tool accuracy | Manual: 4/5 queries produce correct role + subsystem + bridges |

### Production

| Metric | Target |
|---|---|
| Codebase size | <= 27K lines (from 33K) |
| Index time (20K symbols) | < 8 seconds |
| Query latency (cold) | < 50ms |
| CARE latency | < 80ms |
| Benchmark CI | Every PR |
| Languages benchmarked | 5 (TS, Rust, Go, Python, Java) |

---

## The Deeper Thesis

The convergence observation from the self-describing physics roadmap was correct: five frameworks independently converged on "the code graph is a manifold." But the missing piece was always: what lives on that manifold?

The answer: **numbers and symbols**. Function names are the named vertices. Edges are the named connections. But numbers — the constants, the error codes, the thresholds, the port numbers, the decay rates — are the unnamed bonds that hold the structure together. They're the hydrogen bonds of code: weaker than the covalent edges (Calls, Imports), but collectively responsible for the shape of the entire molecule.

GraphIQ has been computing geometry on the named structure. Adding numeric bridges means computing geometry on the full structure — named edges plus unnamed bonds. The manifold doesn't change. But the diffusion across it becomes more honest, because heat can now propagate along the paths that developers actually use to navigate code: "where's the number 429 used?" is how humans debug. It should be how agents debug too.

The finishing work isn't more scoring heuristics. It's completing the physics.
