# Benchmarks

## Methodology

v3 pipeline (BM25 + graph walk + gated name overlap + specificity scaling + per-family routing + neighbor fingerprints) benchmarked on 3 codebases with fresh indexes and new query sets. 50 NDCG queries and 50 MRR queries per codebase (300 total), covering 7 categories. Competitor is Grep — symbol-level `LIKE %term%` search across names and source code.

### Codebases

| Codebase | Language | Symbols | Edges | Characteristics |
|---|---|---|---|---|
| signetai | TypeScript | 23,215 | 51,310 | Domain-specific names, deep call graphs |
| tokio | Rust | 17,867 | 39,103 | Generic function names (`run`, `handle`, `poll`) |
| esbuild | Go | 12,040 | 39,941 | Descriptive names (`convertOKLCHToOKLAB`) |

### Query Categories (NDCG, 50 per codebase)

| Category | Count | Description |
|---|---|---|
| nl-descriptive | 8 | NL with action verbs (`compute the hash of a string`) |
| nl-abstract | 8 | How/what questions (`how does the retention system decide what to delete`) |
| error-debug | 8 | Error/panic queries (`ollama embedding preflight fails`) |
| relationship | 7 | Connections between functions (`how are purgeDeadJobs and deadLetterPendingExtractionJobs related`) |
| cross-cutting | 7 | Enumeration queries (`all functions involved in embedding operations`) |
| file-path | 6 | File/module paths (`src/mcp/scope.ts`) |
| symbol-exact | 6 | Exact symbol names (`extractStructured`) |

### MRR Query Design

50 queries per codebase. Mix of exact-name lookups, natural language descriptions, error scenarios, and relationship queries. Tests single-target retrieval.

### Evaluation Metrics

- **NDCG@K**: Normalized Discounted Cumulative Gain at K. Graded relevance (3/2/1). Reported at K=3, 5, 10.
- **MRR@10**: Mean Reciprocal Rank. 1/first_correct_rank.
- **P@10**: Precision at 10 (fraction of top 10 that are relevant).
- **R@10**: Recall at 10 (fraction of relevant items found in top 10).
- **H@K**: Hit rate at K — fraction of queries where a relevant result appears in top K.

## Results (v3 — Gated Overlap + Specificity + Neighbor Fingerprints)

v3 adds 4 targeted signals on top of v2's BM25 + graph walk pipeline: (1) gated name overlap — only applied when BM25 is confident and the query is specific, (2) query-specificity-weighted coverage — rare-term queries get more name-matching weight, (3) per-family ScoreConfig — 8 family-specific signal routing parameters, (4) neighbor term fingerprints — 1-hop graph terms disambiguate generic names via exact match. Determinism fixes reduced benchmark variance from ±0.05 to ±0.015.

### NDCG@10 (50 queries per codebase)

| Codebase | GraphIQ | Grep | Δ |
|---|---|---|---|
| signetai | **0.339** | 0.137 | **+147%** |
| esbuild | **0.365** | 0.210 | **+74%** |
| tokio | 0.183 | **0.196** | -7% |
| **Overall** | **0.296** | **0.181** | **+63%** |

### MRR@10 (50 queries per codebase)

| Codebase | GraphIQ | Grep | Δ |
|---|---|---|---|
| signetai | **0.437** | 0.168 | **+160%** |
| esbuild | **0.498** | 0.256 | **+95%** |
| tokio | **0.348** | 0.306 | **+14%** |
| **Overall** | **0.428** | **0.243** | **+76%** |

### Per-Category NDCG@10

**Signetai (50 queries):**

| Category | GraphIQ | Grep |
|---|---|---|
| symbol-exact | **0.807** | 0.807 |
| nl-descriptive | **0.458** | 0.052 |
| relationship | **0.703** | 0.031 |
| error-debug | **0.399** | 0.175 |
| nl-abstract | 0.000 | 0.000 |
| cross-cutting | **0.048** | 0.000 |
| file-path | 0.000 | 0.000 |

**Esbuild (50 queries):**

| Category | GraphIQ | Grep |
|---|---|---|
| symbol-exact | 0.630 | 0.630 |
| relationship | **0.806** | 0.239 |
| nl-descriptive | **0.428** | 0.219 |
| error-debug | **0.405** | 0.023 |
| file-path | 0.176 | **0.297** |
| nl-abstract | 0.057 | **0.129** |
| cross-cutting | **0.093** | 0.044 |

**Tokio (50 queries):**

| Category | GraphIQ | Grep |
|---|---|---|
| symbol-exact | 0.727 | **0.762** |
| nl-descriptive | **0.096** | 0.066 |
| relationship | **0.209** | 0.195 |
| nl-abstract | **0.089** | 0.012 |
| file-path | **0.081** | 0.000 |
| error-debug | 0.129 | **0.344** |
| cross-cutting | 0.043 | **0.070** |

### Category Averages (3 codebases)

| Category | Grep | GraphIQ | Winner |
|---|---|---|---|
| symbol-exact | **0.733** | 0.721 | Grep (marginal) |
| relationship | 0.155 | **0.573** | GraphIQ (3.7x) |
| nl-descriptive | 0.112 | **0.327** | GraphIQ (2.9x) |
| error-debug | 0.181 | **0.311** | GraphIQ (1.7x) |
| nl-abstract | **0.047** | 0.049 | GraphIQ (marginal) |
| file-path | 0.099 | **0.086** | Mixed |
| cross-cutting | **0.038** | **0.061** | GraphIQ (1.6x) |

### MRR Hit Rates

| Codebase | G H@1 | G H@10 | Gr H@1 | Gr H@10 |
|---|---|---|---|---|
| signetai | 19/50 | 27/50 | 7/50 | 12/50 |
| esbuild | 21/50 | 33/50 | 7/50 | 25/50 |
| tokio | 11/50 | 22/50 | 14/50 | 20/50 |

### v3 vs v2 Comparison

v2 used 25 queries/codebase for MRR and 20 for NDCG with smaller query sets. v3 uses 50/50 with fresh queries and re-indexed codebases. Direct numerical comparison is not apples-to-apples, but the directional shift is clear:

| Change | v2 → v3 Effect |
|---|---|
| 2x more queries | Harder — more edge cases, more NL queries |
| New query sets | Tests generalization, not overfitting |
| Re-indexed codebases | Fresh edges, no stale artifacts |
| Gated name overlap | +relationship, +nl-descriptive |
| Per-family routing | +error-debug on descriptive-name codebases |
| Neighbor fingerprints | Disambiguates generic names (tokio partially) |

## Analysis

GraphIQ's structural signals dominate grep on codebases with descriptive names. The relationship category is GraphIQ's strongest signal (3.7x over grep) — the graph walk finds structurally connected symbols that no substring search can discover. Error-debug is 1.7x better on signetai/esbuild where error types are distinctive.

### Remaining Weaknesses

**Tokio**: Generic names remain the hard case. GraphIQ wins MRR (+14%) but Grep edges NDCG (-7%). Tokio's `poll`, `read`, `write` functions are too generic for name overlap to help, and the graph walk's structural signal is weaker in a runtime library where everything calls everything.

**Abstract NL queries**: Both GraphIQ and Grep score near zero on "how does X work" queries across all codebases. These require semantic understanding beyond structural graph signals.

**File-path queries**: Neither system scores well. Grep's substring matching occasionally wins when the path contains query terms.

## Previous Results

<details>
<summary>v2 results (25 MRR / 20 NDCG queries per codebase)</summary>

### MRR@10

| Codebase | GraphIQ | Grep | Δ |
|---|---|---|---|
| signetai | **0.900** | 0.888 | +1.4% |
| esbuild | **0.940** | 0.950 | -1.1% |
| tokio | **0.848** | 0.943 | -10% |

### NDCG@10

| Codebase | GraphIQ | Grep | Δ |
|---|---|---|---|
| signetai | **0.330** | 0.279 | +18% |
| esbuild | **0.405** | 0.288 | +41% |
| tokio | **0.221** | 0.278 | -20% |

</details>

<details>
<summary>v1 results (v7 SNP Structural Fallback)</summary>

### MRR@10

| Codebase | GraphIQ | Grep | Δ |
|---|---|---|---|
| signetai | 0.847 | **0.888** | -4.6% |
| esbuild | **0.950** | 0.950 | tied |
| tokio | **0.970** | 0.943 | +2.9% |

### NDCG@10

| Codebase | GraphIQ | Grep | Δ |
|---|---|---|---|
| signetai | **0.323** | 0.279 | +16% |
| esbuild | **0.403** | 0.288 | +40% |
| tokio | **0.291** | 0.278 | +4.7% |

</details>

## Running Benchmarks

```bash
cargo build --release -p graphiq-bench

# NDCG + MRR (both run on the same query file)
./target/release/graphiq-bench <db> <ndcg-queries.json>

# MRR only (separate file)
./target/release/graphiq-bench <db> '' <mrr-queries.json>

# Both
./target/release/graphiq-bench <db> <ndcg-queries.json> <mrr-queries.json>

# Speed benchmark
./target/release/graphiq-bench speed <db> <mrr-queries.json>
```

### Query File Format

**NDCG:**
```json
[
  {
    "query": "how does memory extraction process conversation transcripts",
    "category": "nl-descriptive",
    "relevance": {
      "extractFromConversation": 3,
      "process_extract": 3,
      "enqueueExtractionJob": 2
    }
  }
]
```

**MRR:**
```json
[
  {
    "query": "repairReEmbed",
    "expected_symbol": "repairReEmbed"
  },
  {
    "query": "read pipeline pause state",
    "expected_symbol": "readPipelinePauseState"
  }
]
```
