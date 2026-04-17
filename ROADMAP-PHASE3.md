# GraphIQ Phase 3: Closing the Scale Gap

_Phase 2 delivered +0.025 tokio and directory expansion for cross-cutting queries.
Phase 3 targets the remaining structural gaps: ambiguous partials, FTS recall on large
codebases, and cross-cutting queries in flat package layouts._

## Current State (after Phase 2 + query calibration fix)

| Codebase | MRR | Hit@1 | Hit@3 | Hit@10 |
|---|---|---|---|---|
| graphiq (self, 849 sym) | 0.847 | 74% | 96% | 100% |
| tokio (17,867 sym) | 0.676 | 65% | 69% | 77% |
| signetai (20,870 sym) | 0.359 | 28% | 44% | 56% |

### Per-Category Breakdown (signetai — the hard target)

| Category | MRR | Problem |
|---|---|---|
| symbol-exact | 1.000 | Solved |
| nl-descriptive | 0.292 | Correct symbols often not in FTS top 200 |
| nl-abstract | 0.114 | Decomposition misses domain terms |
| file-path | 0.222 | "daemon.ts" matches 6 files, wrong one wins |
| symbol-partial | 0.083 | Single tokens match hundreds in 20K codebase |
| cross-cutting | 0.000 | Flat package layout defeats directory expansion |

## What Phase 2 Taught Us

1. **Production/test boost works** — 1.5x production + 0.5x test penalty is net positive
2. **FTS weight changes are radioactive** — touching column weights cascades across all query types
3. **Type-discriminating reranks are too blunt** — Step J's 1.8x type boost caused massive regressions even with guards
4. **Directory expansion works for deep hierarchies** — tokio's `src/sync/` tree benefits; signetai's flat `packages/connector-*/` does not
5. **The real bottleneck is FTS recall, not reranking** — most misses happen because the correct symbol isn't in the top 200 candidates at all

---

## Step K: Name Coverage Heuristic

**Target:** symbol-partial improvement (signetai 0.083 → target 0.20+)
**Files:** `rerank.rs`
**Effort:** Small

For partial queries (<= 2 tokens), compute how much of the symbol's name the query covers.
"embed" covers 100% of `embed` (function) but only 17% of `EmbeddingProjectionErrorResponse`.
Boost symbols where the query covers a higher fraction of their decomposed name tokens.

```
name_coverage = (matching_tokens / total_name_tokens)  [0..1]
if tokens <= 2:
    coverage_boost = 1.0 + (0.5 * name_coverage)
```

This is softer than Step J's type discrimination — it doesn't care about symbol kind,
only about how specifically the query targets this particular symbol. A query for "embed"
with 100% coverage gets 1.5x; a match with 20% coverage gets 1.1x.

**Risk:** Low. This is a continuous signal, not a binary type gate. Even at maximum boost
(1.5x) it's well within the range of other heuristics and shouldn't overwhelm anything.

**Verify:** All 3 benchmarks. Self-benchmark must not regress.

---

## Step L: In-Degree Centrality

**Target:** symbol-partial + nl-descriptive (better ranking of "important" symbols)
**Files:** `db.rs`, `rerank.rs`, `index.rs`
**Effort:** Medium

Compute a simple centrality score per symbol: how many incoming edges does it have?
Symbols with many incoming calls, references, implements, and imports are structurally
important — they're the types and functions the rest of the codebase depends on.

Implementation:
1. At index time, count incoming edges per symbol: `SELECT target_id, COUNT(*) FROM edges GROUP BY target_id`
2. Store as `centrality` float in the symbols table (normalized 0..1)
3. In reranking: `centrality_boost = 1.0 + (0.3 * centrality)`

This naturally surfaces `Connector` (struct) over `connector` (local helper), `DaemonManager`
(trait) over `DaemonFetchFailure` (type alias), and `cosine_similarity` (heavily called)
over `tsCosineSimilarity` (test helper).

**Risk:** Medium. Requires DB schema change (new column). But centrality is a smooth,
continuous signal — unlikely to cause sharp regressions.

**Verify:** All 3 benchmarks after reindex.

---

## Step M: Query-Scoped Cascade

**Target:** FTS recall on large codebases (nl-descriptive signetai misses)
**Files:** `fts.rs`, `search.rs`
**Effort:** Medium

When FTS returns fewer than 30 results AND the query has >= 3 content tokens, run a
second, broader FTS pass that relaxes the AND constraint to OR, and also tries matching
against decomposed name tokens individually.

Current FTS flow:
1. AND query with all content tokens → if < 10 results, fall back to OR

New FTS flow:
1. AND query with all content tokens → if < 10 results, fall back to OR
2. If still < 30 results AND query is NL (>= 3 tokens): decompose each token via
   the same decomposition pipeline used for symbol names, then AND the decomposed tokens
3. Merge and deduplicate

Example: "start and stop the signet daemon process" → content tokens: [start, stop, signet, daemon, process]
→ If AND yields < 30: decompose "signet" → [signet], "daemon" → [daemon], "process" → [process]
→ Also try: OR of individual decomposed tokens to catch partial matches

This specifically targets the signetai nl-descriptive misses where the correct symbol
isn't in the top 200 because its name doesn't contain enough query tokens.

**Risk:** Low-Medium. Only activates when FTS is underperforming. May increase latency
slightly for NL queries on large codebases.

**Verify:** All 3 benchmarks. Focus on signetai nl-descriptive improvement.

---

## Step N: Cross-Package Structural Expansion

**Target:** cross-cutting queries in flat package layouts (signetai 0.000)
**Files:** `directory_expand.rs`, `search.rs`
**Effort:** Medium

Current directory expansion walks the file path hierarchy. This fails for signetai's
monorepo layout where connectors live in `packages/connector-opencode/`,
`packages/connector-claude-code/`, etc. — they're siblings, not parent-child.

New expansion strategy for cross-cutting queries:
1. From FTS seed results, extract package prefixes (e.g., `packages/connector-*`)
2. For each unique prefix pattern, search for all matching package directories
3. Find public exported symbols in those directories
4. Score by: query relevance + export visibility + package name match

This encodes the monorepo pattern: `packages/{scope}-*` directories are related modules.
A query for "all connector implementations" should look at all `packages/connector-*`
directories, not just the parent of one seed result.

**Risk:** Medium. Could over-expand on repos without monorepo layout. Mitigate by
only activating when multiple seed results share a common prefix pattern.

**Verify:** All 3 benchmarks. Cross-cutting should improve on signetai.

---

## Step O: Signature Hints Enrichment

**Target:** nl-descriptive + nl-abstract (signetai 0.292, 0.114)
**Files:** `index.rs`
**Effort:** Small

Enrich search_hints at index time with terms extracted from function signatures and
type annotations. When `block_on<F: Future>(f: F) -> F::Output` is indexed, the hints
should include "future", "output", "async", "await". When `cosine_similarity(a: &[f32],
b: &[f32]) -> f64` is indexed, hints should include "float", "array", "slice", "f64",
"dot product".

The `extract_source_terms` function in `index.rs` already does basic term extraction.
Extend it with:
1. Type name decomposition: `Future` → "future", `TcpStream` → "tcp stream"
2. Common type mapping: `Result<T>` → "result", `Option<T>` → "optional", `Vec<T>` → "vector"
3. Return type hints: function returning `bool` → "check validate predicate"
4. Async detection: `async fn` or `Promise<T>` → "async await promise"

This feeds FTS at zero query-time cost. The hints column (weight 5.0) already exists.

**Risk:** Very low. Only enriches an existing FTS column. No reranking changes.

**Verify:** All 3 benchmarks. Focus on nl-descriptive improvement.

---

## Execution Order

```
Step O (Signature Hints)        → zero-risk enrichment, do first
Step K (Name Coverage)           → small rerank change, targeted at partials
Step L (In-Degree Centrality)    → requires schema change, medium effort
Step M (Query-Scoped Cascade)    → FTS recall improvement for NL queries
Step N (Cross-Package Expansion) → most complex, depends on Step L's centrality
```

After each step: benchmark all 3 codebases, record before/after, commit only if
self-benchmark does not regress and at least one external benchmark improves.

## Success Criteria

| Metric | Current | Phase 3 Target |
|---|---|---|
| Self MRR | 0.847 | >= 0.847 (no regression) |
| Tokio MRR | 0.676 | >= 0.72 |
| Signetai MRR | 0.359 | >= 0.50 |
| Tokio nl-descriptive | 0.200 | >= 0.40 |
| Signetai symbol-partial | 0.083 | >= 0.20 |
| Signetai cross-cutting | 0.000 | >= 0.10 |

## Non-Goals

- Embedding models / vector search (explicitly out of scope per design constraint)
- LLM-based query rewriting or extraction
- Changes to FTS column weights (proven radioactive in Phase 2)
- Type-discriminating reranks (Step J showed these cause cascading regressions)
