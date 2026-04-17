# GraphIQ Phase 2: Zero-Model External Validation Roadmap

_Bridge the self-benchmark to external-benchmark gap without LLMs or embedding models._

## Baseline (as of 2026-04-16)

| Codebase | MRR | Hit@1 | Hit@3 | Notes |
|---|---|---|---|---|
| graphiq (self) | 0.852 | 78% | 100% | 45 files, 828 symbols |
| tokio | 0.651 | 62% | 69% | 819 files, 17,867 symbols |
| signetai | 0.332 | 28% | 40% | 1,263 files, 20,870 symbols |

## Miss Analysis (28 total across tokio + signetai)

### Category 1: Test Pollution — 13 misses
31% of tokio symbols (5,601/17,867) are in test/bench files. NL query results are dominated by test functions whose names contain query words. Current test penalty (0.8x) is insufficient — test functions with name_exact=1.50x still outrank production code.

**Affected queries:**
- tokio: "periodic interval timer", "tcp accept connections", "block on async task", "join multiple concurrent tasks", "all sync primitives", "all runtime handle methods"
- signetai: "connector" (TestableConnector), "BaseConnector" (TestConnector)

### Category 2: FTS Column Weight Imbalance — 6 misses
Source column weight=1.0, name=10.0. Rich NL descriptions live in source/doc text, not symbol names. TcpListener source says "A TCP socket server, listening for connections. You can accept a new connection" but matches at weight 1.0 while test function names match at weight 10.0.

**Affected queries:**
- tokio: "periodic interval timer" (Interval source mentions period/tick/delay), "how are timers tracked and fired"
- signetai: "split documents into chunks", "find nearest neighbors in embedding space", "what computes similarity scores", "daemon.rs"

### Category 3: Structural Blindness — 5 misses
Edges (calls, references, implements) are indexed but never used at query time for cross-cutting queries. 45 public structs exist in tokio/src/sync/ but "all sync primitives" returns module sections instead of the actual types.

**Affected queries:**
- tokio: "all sync primitives", "all runtime handle methods"
- signetai: "how are connector tools registered", "all autograd operations"

### Category 4: Ambiguous Partial Queries — 6 misses (signetai only)
Single-word queries ("connector", "embed", "vector", "tray", "normalize", "daemon") match hundreds of symbols. Expected targets are never the strongest FTS match in a 20,870-symbol codebase.

**Affected queries:**
- signetai: all 6 symbol-partial queries

---

## Step F: Production Priority Boost

**Target:** Category 1 (test pollution)
**Files:** `rerank.rs`
**Effort:** Small

When `is_nl_query()` returns true, apply a **production boost** of 1.5x to symbols in non-test, non-bench files. Combined with the existing test penalty (0.8x), the effective gap is 1.5/0.8 = 1.875x. Test functions need nearly double the FTS score to outrank production code.

Implementation:
- In `apply_heuristics()`, after the test_file_penalty calculation
- Add `let production_boost = if is_nl_query(&self.query_tokens) && !is_test_file(path) { 1.5 } else { 1.0 };`
- Multiply into heuristic_multiplier

**Expected impact:** Fixes 8-10 of the 13 test pollution misses. The tokio nl-descriptive queries should improve significantly since the top results are currently all test functions.

**Verify:** Run all 3 benchmarks. Self-benchmark should not regress. Tokio nl-descriptive should improve from 0.220.

---

## Step G: NL Column Weight Escalation

**Target:** Category 2 (column weight imbalance)
**Files:** `fts.rs`, `search.rs`
**Effort:** Medium

Make FTS column weights adaptive based on query type. For NL queries, boost source (1.0→4.0) and doc_comment (3.0→6.0) weights. This lets rich text descriptions compete with symbol name matches.

Implementation:
- Add `FtsConfig::for_nl_query()` that returns escalated weights
- In `SearchEngine::search()`, detect NL queries and pass the escalated config to `FtsSearch`
- Keep name weight at 10.0 — exact name matches should still win, but source matches should beat test function names

New weight profiles:
```
Standard:  name=10, decomposed=8, qualified=6, signature=4, source=1, doc=3, hints=5
NL-boost:  name=10, decomposed=8, qualified=6, signature=4, source=4, doc=6, hints=5
```

**Expected impact:** Fixes the remaining 3-5 test pollution misses where the correct symbol's source text matches but at too-low weight. Also helps nl-descriptive queries where the target symbol's source contains the query words.

**Verify:** Run all 3 benchmarks after Step F+G together. Tokio nl-descriptive should improve from baseline 0.269.

---

## Step H: Directory-Scoped Structural Expansion

**Target:** Category 3 (structural blindness)
**Files:** `search.rs`, new module `directory_expand.rs`
**Effort:** Medium-Large

For cross-cutting "all X" queries, after FTS returns seed candidates, expand to find sibling symbols in the same directory subtree. Use the file path hierarchy as a lightweight ontology.

Implementation:
1. New query: `directory_siblings(db, file_path, depth) -> Vec<Symbol>`
   - Given `tokio/src/sync/mod.rs`, return all public exported symbols in `tokio/src/sync/**/*.rs`
   - `depth` controls how many directory levels to walk up (1 = same dir, 2 = parent dir)
2. In `SearchEngine::search()`, after reranking, detect cross-cutting queries (queries starting with "all")
3. For the top 3 FTS results, expand their directories and merge sibling symbols
4. Score siblings by: importance + export_boost + directory proximity (closer = higher)
5. Deduplicate with existing results

New heuristic: `directory_sibling_boost` — when a symbol appears as a sibling of an FTS match in a cross-cutting query context, boost by 1.3x.

**Expected impact:** Fixes "all sync primitives" (should return Mutex, RwLock, Semaphore, etc.) and "all runtime handle methods" (should return Handle struct and its methods). Cross-cutting MRR on tokio should go from 0.000 to >0.3.

**Verify:** Run all 3 benchmarks. Cross-cutting category should improve on both tokio and signetai.

---

## Step I: Signature-Aware NL Matching

**Target:** Category 2 (refinement), Category 4 (partial)
**Files:** `fts.rs`, `index.rs`
**Effort:** Medium

Enrich the search_hints FTS column at index time with extracted signature terms. When `block_on<F: Future>` is indexed, add "future", "async", "await", "output" to hints. When `cosine_similarity(a: &[f32], b: &[f32])` is indexed, add "vector", "float", "array", "similarity", "dot_product".

Implementation:
1. New function `extract_signature_terms(signature: &str, source: &str) -> Vec<String>`
   - Extract type names from signatures: `Future` → "future", `Duration` → "duration", `TcpStream` → "tcp stream"
   - Extract doc comment keywords (first sentence, key nouns)
   - Map common type patterns: `Result<T>` → "result", `Option<T>` → "optional"
2. Append these to `search_hints` at index time
3. No FTS column weight changes needed — hints already have weight 5.0

**Expected impact:** Improves nl-descriptive and nl-abstract queries where the target symbol's signature contains NL-relevant terms. "block on async task" should find `block_on` because its signature mentions `Future`.

**Verify:** Run all 3 benchmarks. Focus on nl-descriptive improvement.

---

## Step J: Export-First Partial Matching

**Target:** Category 4 (ambiguous partials)
**Files:** `rerank.rs`
**Effort:** Small

For partial (1-2 word) queries, dramatically boost public exported structs/classes/traits over functions, methods, type aliases, and imports. A query for "connector" should prefer `BaseConnector` (public class) over `connector()` (test helper function).

Implementation:
- New heuristic: `export_first_partial`
- When query has <= 2 tokens and no file extension:
  - Public struct/class/enum/trait: 1.8x
  - Public function: 1.0x
  - Import/section/module: 0.5x
  - Test file symbol: 0.3x (cumulative with test_file_penalty)

This encodes the intuition that when someone searches a partial term, they usually want the main type, not a helper function or import re-export.

**Expected impact:** Fixes most signetai partial misses. "connector" → BaseConnector (public class, not test helper), "normalize" → normalize_content_for_storage (public function, not import).

**Verify:** Run all 3 benchmarks. Signetai symbol-partial should improve from 0.000.

---

## Execution Order

```
Step F (Production Boost)     → immediate, biggest bang
Step G (NL Column Weights)    → combined with F, medium effort
Step J (Export-First Partial)  → quick win for signetai
Step H (Directory Expansion)  → largest code change
Step I (Signature Terms)       → refinement pass
```

After each step: benchmark all 3 codebases, record before/after, commit only if self-benchmark does not regress and at least one external benchmark improves.

## Success Criteria

| Metric | Current | Target |
|---|---|---|
| Self MRR | 0.852 | >= 0.852 (no regression) |
| Tokio MRR | 0.651 | >= 0.72 |
| Signetai MRR | 0.332 | >= 0.45 |
| Tokio nl-descriptive | 0.220 | >= 0.50 |
| Tokio cross-cutting | 0.000 | >= 0.30 |
| Signetai symbol-partial | 0.000 | >= 0.30 |
