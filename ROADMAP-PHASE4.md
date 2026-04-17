# GraphIQ Phase 4: Structural Retrieval

_Phase 3 closed the easy gaps with heuristics. Phase 4 is about using the graph we already have._

## The Insight

The system indexes 14,547 edges in signetai — Implements, Calls, Imports, Extends, Contains, Tests, References. But at query time, the only way structural information surfaces is through:
1. `search_hints` (FTS text matching on structural descriptions)
2. BFS expansion from the top 20 FTS hits (generic traversal)

Neither is targeted. When someone asks "all connector implementations", the system runs a bag-of-words FTS query instead of following Implements edges from the Connector interface. When someone queries "embed" in a 20K codebase, the system has no way to know that `embed_row` is the most-called function with "embed" in its name.

**The graph already knows the answer. We just aren't asking it the right questions.**

## Current State (after Phase 3)

| Codebase | MRR | Hit@1 | Hit@3 | Hit@10 |
|---|---|---|---|---|
| graphiq (self, 859 sym) | 0.870 | 78% | 93% | 100% |
| tokio (17,867 sym) | 0.683 | 65% | 69% | 73% |
| signetai (20,870 sym) | 0.410 | 32% | 44% | 60% |

### Per-Category Breakdown (signetai — the hard target)

| Category | MRR | Problem |
|---|---|---|
| symbol-exact | 1.000 | Solved |
| nl-abstract | 0.400 | Decomposition works for "how/what" queries |
| nl-descriptive | 0.312 | FTS recall gap — correct symbols not in top 200 |
| file-path | 0.222 | File stem matches multiple files, wrong symbol wins |
| symbol-partial | 0.111 | Single tokens match hundreds in 20K codebase |
| cross-cutting | 0.083 | "all X implementations" is a type-hierarchy query, not FTS |

## Phase 4 Steps

### Step P: Implements-Aware Retrieval

**Target:** cross-cutting 0.083 → 0.50+
**Files:** `search.rs`, `graph.rs`
**Effort:** Small-Medium

Parse "all X implementations/subclasses" as a type-hierarchy query:
1. Extract the concept token X from the query (e.g., "connector" from "all connector implementations")
2. FTS-find the interface/trait/base class matching X (e.g., `Connector`, `BaseConnector`)
3. Follow Implements/Extends edges *incoming* to find all implementors
4. Return the implementors directly — bypass the normal rerank pipeline

This is a dedicated retrieval path, not a heuristic bolt-on. The data is already in the edges table. For signetai:
- "all connector implementations" → FTS finds `Connector` → Implements edges yield `OpenCodeConnector`, `ClaudeCodeConnector`, etc.
- "all autograd operations" → FTS finds `Tape` (struct) → follow outgoing Calls/Contains from Tape to find `forward`, `backward`, `alloc`, etc.

Implementation:
```
fn is_hierarchy_query(query: &str) -> Option<&str> {
    let lower = query.to_lowercase();
    let patterns = ["all ", "every "];
    if !patterns.iter().any(|p| lower.starts_with(p)) { return None; }
    // Strip prefix, extract first content token
    let rest = lower.trim_start_matches("all ").trim_start_matches("every ");
    rest.split_whitespace().next()  // "connector" from "all connector implementations"
}
```

Then in `search()`: if `is_hierarchy_query` returns Some(concept), run a targeted BFS with only Implements/Extends edge filter, incoming direction, from the FTS top-1 result for the concept token.

For "all autograd operations" where `Tape` has no Implements edges: fall through to cross-package expansion (existing Step N logic) or follow outgoing Calls/Contains edges.

**Risk:** Low. Only activates for explicit "all/every X" patterns. Falls back gracefully.

**Verify:** signetai cross-cutting should hit 0.50+. Self cross-cutting must not regress (currently 0.625).

---

### Step Q: Structural Centrality Boost for Partial Queries

**Target:** symbol-partial 0.111 → 0.30+
**Files:** `rerank.rs`
**Effort:** Small

When `query_tokens.len() == 1` (single-token partial queries), aggressively boost symbols by their structural centrality. The `importance` score already exists in the DB but its heuristic range is too narrow: `0.5 + 0.5 * importance` gives a range of [0.5, 1.0] — at most 2x discrimination.

For single-token queries where 50+ symbols match, that 2x isn't enough. The fix:
1. When `query_tokens.len() == 1`, use a wider importance multiplier: `1.0 + 2.0 * importance`
2. This gives range [1.0, 3.0] — enough to surface `Connector` (importance ~0.8, boost 2.6x) over local helpers (importance ~0.3, boost 1.6x)
3. Additionally check Implements/Extends incoming edges: symbols that are the target of many "Implements" edges (i.e., they're an interface with many implementors) get an extra 1.5x

The heuristic is only for single-token queries. Multi-token queries already have enough discrimination from name_coverage.

```
// In apply_heuristics:
let centrality_boost = if query_tokens.len() == 1 {
    let implements_count = count_incoming_implements(sym.id);
    let interface_bonus = if implements_count >= 2 { 1.5 } else { 1.0 };
    (1.0 + 2.0 * sym.importance) * interface_bonus
} else {
    1.0
};
```

The `count_incoming_implements` can be precomputed at index time (stored in metadata or a small lookup table loaded into memory at search time).

**Risk:** Low. Only affects single-token queries where the current system is at 0.111 MRR — any change is likely an improvement.

**Verify:** signetai symbol-partial must improve. Self symbol-partial must not regress (currently 0.792, single-token queries like "cache" → HotCache, "blast" → blast).

---

### Step R: File Representative Indexing

**Target:** file-path 0.222 → 0.60+
**Files:** `index.rs`, `db.rs`, `rerank.rs`
**Effort:** Medium

At index time, compute a "file representative" for each file: the public symbol most imported/called by other files. Store this as a boolean flag `is_file_representative` in the symbol metadata.

When a file-path query comes in (e.g., "knn.rs"), the `file_path_boost` heuristic should give 3.0x to the file representative (instead of the current 2.0x for primary definitions). This ensures the most important export of a file wins.

Additionally, extend `is_primary_definition` to include file representatives — currently it only matches struct/class/enum/trait/interface, but functions that are a file's main export should also get the boost.

Implementation:
1. At end of `index_project()`, after importance scores are computed:
   - For each file, find all public symbols
   - For each public symbol, count incoming Imports edges from OTHER files
   - The symbol with the most incoming Imports is the file representative
   - Store `"is_file_representative": true` in metadata JSON
2. In `file_path_boost`:
   - If symbol is the file representative: 3.0x
   - Else if exact name match + primary definition: 2.0x
   - Else if decomposed name contains stem + primary definition: 1.5x
   - Else: current logic

For signetai "knn.rs":
- FTS finds all symbols in files matching "knn"
- `build_knn_edges` is likely the most-imported function from that file
- With file_representative boost, it would beat any other symbol in the file

For "daemon.ts":
- Multiple files match, but `registerDaemonCommands` might be the representative of its specific daemon.ts
- If not, the importance-based ranking within the file should help

**Risk:** Low-Medium. Requires a post-indexing pass (but importance already does this). Could shift file-path results on small codebases.

**Verify:** signetai file-path must improve. Self file-path must not regress (currently 0.833).

---

### Step T: Edge-Propagated Hints

**Target:** nl-descriptive 0.312 → 0.50+, general recall improvement
**Files:** `index.rs`
**Effort:** Medium

The single highest-impact change. Currently, `search_hints` are generated from a symbol's own properties: its name, doc comment, source terms, signature, role tags, and motifs. But a symbol's callers and importers often describe it in richer vocabulary than its own name.

Example: `cosine_similarity` has hints from its own properties: "similarity", "cosine", "math". But `merge_hybrid_scores` (which calls `cosine_similarity`) has a doc comment saying "Merges hybrid scores using vector similarity and keyword matching." If we propagate relevant terms from callers to callees, `cosine_similarity` would also get hints like "vector", "hybrid", "scores", "keyword", "matching" — directly addressing the FTS recall gap.

Implementation (at index time, after all symbols and edges are indexed):
1. For each symbol S, find all symbols that call S (incoming Calls edges)
2. Extract source terms from each caller (up to 10 most significant)
3. For each caller term, if it's NOT already in S's hints, add it as a propagated hint
4. Cap propagated hints at 20 terms per symbol
5. Only propagate from symbols in different files (avoids noise from same-module helpers)

This is index-time only — zero query-time cost. It enriches the FTS hints column so that `search_hints LIKE '%vector%'` matches `cosine_similarity` even though its name doesn't contain "vector."

Similarly, propagate hints along Implements edges: if `OpenCodeConnector` implements `Connector`, and `Connector` has hints "base interface", then `OpenCodeConnector` gets hints from `Connector` too.

**Why this is better than Step U for nl-descriptive:** Step U tries to fix recall at query time by routing through decomposition. This fixes recall at index time by giving FTS more vocabulary to match on. Same latency, more robust.

**Risk:** Low. Only enriches an existing FTS column. Could slightly increase index size (hints column gets longer). No query-time changes.

**Verify:** signetai nl-descriptive must improve. Self must not regress. Check that "compute vector similarity between embeddings" now matches `cosine_similarity` via propagated "vector" hint.

---

### Step U: Decomposition Without Prefix Gate

**Target:** nl-descriptive improvement (complement to Step T)
**Files:** `decompose.rs`, `search.rs`
**Effort:** Medium

Currently, decomposition only activates for queries starting with "how/what/where/why/when". The nl-descriptive queries like "compute vector similarity between embeddings" or "split documents into chunks for indexing" don't trigger decomposition — they go through plain FTS OR fallback, where the correct symbol is buried.

Fix: Extend `is_decomposable_query` to also match:
- Queries starting with action verbs: "compute", "find", "split", "start", "build", "create", "detect", "extract", "parse", "validate", "normalize", "get", "set", "check", "run"
- These are imperative NL queries that the decomposition engine's domain maps can handle

The decomposition engine's `domain_map` and `phrase_map` already contain mappings like "vector similarity" → "cosine_similarity". The issue is they're never consulted for nl-descriptive queries.

Additionally, add a low-confidence fallback: after FTS, if the top-1 BM25 score is below a threshold AND the query has >= 4 content tokens, run decomposition as a second pass.

**Risk:** Medium. Decomposition runs 3-8 subqueries. Could affect latency p95 for NL queries.

**Verify:** signetai nl-descriptive must improve. Self nl-descriptive must not regress (currently 0.900). Tokio nl-descriptive must not regress.

---

## Execution Order

```
Step P (Implements-Aware Retrieval)   → cross-cutting, low risk, clear win
Step Q (Structural Centrality Boost)  → symbol-partial, low risk, small change
Step R (File Representative)           → file-path, low-medium risk
Step T (Edge-Propagated Hints)         → nl-descriptive, low risk, index-time only
Step U (Decomposition Without Gate)    → nl-descriptive complement, medium risk
```

Steps P and Q are independent. Step T is the highest-impact index-time change.

After each step: benchmark all 3 codebases, record before/after, commit only if aggregate MRR improves.

## Success Criteria

| Metric | Current | Phase 4 Target |
|---|---|---|
| Self MRR | 0.870 | >= 0.870 (no regression) |
| Tokio MRR | 0.683 | >= 0.700 |
| Signetai MRR | 0.410 | >= 0.520 |
| Signetai cross-cutting | 0.083 | >= 0.50 |
| Signetai symbol-partial | 0.111 | >= 0.30 |
| Signetai file-path | 0.222 | >= 0.50 |
| Signetai nl-descriptive | 0.312 | >= 0.50 |

## Non-Goals

- Embedding models / vector search (explicitly out of scope)
- LLM-based query rewriting or extraction
- Changes to FTS column weights (proven radioactive)
- Type-discriminating reranks (proven regressive in Phase 2)

## Design Constraints

- Zero model dependencies for core search
- Cold latency must stay under 5ms for non-decomposed queries
- Index size growth must stay under 20% (no embedding columns)
- Every new retrieval path must be toggleable and logged in debug mode
