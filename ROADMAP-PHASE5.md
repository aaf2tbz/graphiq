# GraphIQ Phase 5: Category Gaps

_Step Q got self to 0.898 but signetai barely moved (+0.002). P was regressive. The remaining gaps are category-specific._

## Baseline (after Step Q)

| Codebase | MRR | Hit@1 | Hit@3 | Hit@10 |
|---|---|---|---|---|
| graphiq (self) | 0.898 | 81% | 96% | 100% |
| tokio | 0.683 | 65% | 69% | 73% |
| signetai | 0.412 | 32% | 48% | 64% |

### Signetai Per-Category

| Category | MRR | Status |
|---|---|---|
| symbol-exact | 1.000 | Solved |
| nl-abstract | 0.444 | 2/3 hit, 1 MISS ("what computes similarity scores") |
| nl-descriptive | 0.292 | 2 MISS, 1 rank-8 |
| symbol-partial | 0.111 | 4 MISS, 1 rank-2, 1 rank-6 |
| file-path | 0.222 | 1 MISS, 1 rank-2, 1 rank-6 |
| cross-cutting | 0.083 | 1 MISS, 1 rank-6 |

## The Problem

Signetai's 20K symbols create massive ambiguity for short queries. The failures cluster into 4 categories, each needing a targeted fix.

**symbol-partial** (0.111): Single tokens like "embed", "tray", "normalize", "daemon" match dozens of symbols. The expected targets are specific functions (`embed_row`, `buildTrayUpdate`) that lose to broader names (`EmbeddingProvider`) on BM25 + structural importance. The name_coverage heuristic gives the same 1.25x boost to both "embed_row" and "EmbeddingProvider" because `dt.contains(qt)` matches "embedding" just as well as "embed".

**file-path** (0.222): "daemon.ts" and "vector.rs" match multiple files. The system doesn't know which symbol is the file's primary export.

**nl-descriptive** (0.292): "find nearest neighbors in embedding space" and "start and stop the signet daemon" never trigger decomposition (only "how/what/where/why/when" prefixes activate it). The phrase_map already has entries that would help, but they're never consulted.

**cross-cutting** (0.083): "all autograd operations" → Tape (MISS). Cross-package expansion from Step N gives per-package limit of 1, which may be too restrictive.

## Steps

### Step R: File Representative Indexing

**Target:** file-path 0.222 → 0.55+
**Files:** `index.rs`, `rerank.rs`
**Effort:** Medium

At index time, after importance scores are computed, identify the "file representative" for each file: the public symbol with the most incoming Imports edges from other files. Store `"is_file_representative": true` in symbol metadata.

In `file_path_boost`, give 3.0x (instead of 2.0x) to the file representative. Extend `is_primary_definition` to also cover functions that are file representatives.

For signetai:
- "knn.rs" → `build_knn_edges` is the most-imported symbol from that file → 3.0x boost → rank 1
- "daemon.ts" → `registerDaemonCommands` may be the representative → surfaces
- "vector.rs" → `cosine_similarity` is likely the representative → surfaces

**Risk:** Low. Only affects file-path queries. Small codebases may shift but their file-path scores are already high.

---

### Step V: Exact Decomp Match for Single-Token Queries

**Target:** symbol-partial 0.111 → 0.35+
**Files:** `rerank.rs`
**Effort:** Small

The name_coverage heuristic uses `dt.contains(qt)` which is too loose for single-token queries. "embedding" contains "embed" so `EmbeddingProvider` gets the same coverage boost as `embed_row`. But "embed" exactly matches one decomposed token of "embed_row" ("embed" | "row") while only partially matching a token of "EmbeddingProvider" ("embedding" | "provider").

For single-token queries, add an exact decomp match bonus:
1. Split the symbol's `name_decomposed` into tokens
2. If the query token **exactly equals** one of those tokens: 1.5x
3. If the query token is only a **substring** of a decomp token (no exact match): 1.0x (no bonus)

This replaces the current loose `dt.contains(qt)` match for single-token queries only. Multi-token queries keep the existing behavior.

Expected effect for "embed":
- `embed_row` (decomp: "embed row") → "embed" exact match → 1.5x
- `EmbeddingProvider` (decomp: "embedding provider") → "embed" is substring only → 1.0x
- `embeddings` (decomp: "embeddings") → "embed" is substring only → 1.0x

For "daemon":
- `registerDaemonCommands` (decomp: "register daemon commands") → "daemon" exact match → 1.5x
- Other daemon-containing symbols → depends on exact match

**Risk:** Low. Only affects single-token queries. Could slightly shift self-bench "cache" (decomp: "hot cache" — "cache" exact match) and "blast" (decomp: "blast" — exact match). Both are already rank 1 so no regression expected.

---

### Step U: Decomposition Without Prefix Gate

**Target:** nl-descriptive 0.292 → 0.50+
**Files:** `decompose.rs`
**Effort:** Small-Medium

Currently `is_decomposable_query` only returns true for "how/what/where/why/when" prefixes. The nl-descriptive queries that MISS never trigger decomposition:

- "find nearest neighbors in embedding space" → no decomposition → plain FTS misses `build_knn_edges`
- "start and stop the signet daemon process" → no decomposition → plain FTS misses `doStart`

But `phrase_map` already has:
- "nearest neighbors embedding" → `["knn"]`
- "connector tools registered" → `["connector register"]`

Fix: Extend `is_decomposable_query` to also activate for:
1. Queries starting with action verbs: "find", "compute", "start", "stop", "split", "build", "detect", "extract", "get", "check", "run"
2. Queries with >= 4 content tokens (lower confidence signal)

Add the guard: after decomposition runs, if top-1 BM25 from the full query is above 5.0, skip decomposition (the FTS already has a strong match). This prevents decomposition from hurting queries that FTS already solves.

**Risk:** Medium. Decomposition runs 3-8 subqueries. Could affect latency for nl-descriptive queries. The BM25 guard limits damage.

---

### Step W: Cross-Package Expansion Relaxation

**Target:** cross-cutting 0.083 → 0.30+
**Files:** `directory_expand.rs`
**Effort:** Small

Step N's cross-package expansion uses a per-package limit of 1. For "all autograd operations" → Tape, Tape might not be in a "package" directory at all (it could be at the top level or in a differently-structured path).

Changes:
1. Increase per-package limit from 1 to 2
2. When the query starts with "all/every" and FTS top-20 results exist, also include the FTS results themselves (not just cross-package siblings) — currently cross-cutting queries ONLY use cross-package expansion, discarding the FTS hits entirely
3. For "all X" queries, if no cross-package expansion found anything, fall through to normal FTS+rerank path

**Risk:** Low. Only affects "all/every" queries. Current scores are near zero so regression risk is minimal.

---

## Execution Order

```
Step V (Exact Decomp Match)        → symbol-partial, smallest change, highest uncertainty
Step R (File Representative)       → file-path, index-time, clean win
Step U (Decomposition Gate Lift)   → nl-descriptive, phrase_map already has the answers
Step W (Cross-Package Relaxation)  → cross-cutting, tuning existing code
```

V first because it's the smallest change and targets the worst category. After each step: benchmark all 3 codebases, commit only if aggregate improves.

## Success Criteria

| Metric | Current | Phase 5 Target |
|---|---|---|
| Self MRR | 0.898 | >= 0.898 |
| Tokio MRR | 0.683 | >= 0.683 |
| Signetai MRR | 0.412 | >= 0.520 |
| Signetai symbol-partial | 0.111 | >= 0.35 |
| Signetai file-path | 0.222 | >= 0.55 |
| Signetai nl-descriptive | 0.292 | >= 0.50 |
| Signetai cross-cutting | 0.083 | >= 0.30 |

## Non-Goals

- Embedding models / vector search
- LLM-based query rewriting
- FTS column weight changes
- Type-discriminating reranks
- Edge-propagated hints (Step T — deferred)
