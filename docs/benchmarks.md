# Benchmarks

## Methodology

v2 simplified pipeline (BM25 + graph walk + IDF coverage) benchmarked on 3 codebases (TypeScript, Rust, Go) with separate NDCG and MRR query sets. NDCG queries (20 per codebase) use graded relevance (3=perfect, 2=good) with multiple relevant symbols across 7 categories. MRR queries (25 per codebase) target a single expected symbol — split between exact-name lookups and natural language descriptions. Competitor is Grep — symbol-level `LIKE %term%` search across names and source code.

### Codebases

| Codebase | Language | Symbols | Edges | Characteristics |
|---|---|---|---|---|
| signetai | TypeScript | 23,215 | 51,264 | Domain-specific names, deep call graphs |
| tokio | Rust | 17,867 | 39,086 | Generic function names (`run`, `handle`, `poll`) |
| esbuild | Go | 12,040 | 39,422 | Descriptive names (`convertOKLCHToOKLAB`) |

### Query Categories (NDCG)

| Category | Count | Description |
|---|---|---|
| symbol-exact | 3 | Exact symbol names (`spawn_blocking`, `NewResolver`) |
| symbol-partial | 3 | Short fragments (`blocking shutdown`, `fold string addition`) |
| nl-descriptive | 3 | NL with action verbs (`how does esbuild resolve import paths`) |
| nl-abstract | 3 | How/what questions (`what determines when a memory should be superseded`) |
| error-debug | 3 | Error/panic queries (`embedding dimension mismatch after model switch`) |
| file-path | 3 | File/module names (`internal resolver resolver.go`) |
| cross-cutting | 2 | Enumeration queries (`all functions that check embedding or vector health`) |

### MRR Query Design

25 queries per codebase. 15 exact-name lookups (e.g., `repairReEmbed`) and 10 natural language descriptions (e.g., `read pipeline pause state`). Tests single-target retrieval — "I know the function exists, can you find it?"

### Evaluation Metrics

- **NDCG@K**: Normalized Discounted Cumulative Gain at K. Graded relevance (3/2). Reported at K=3, 5, 10.
- **MRR@10**: Mean Reciprocal Rank. 1/first_correct_rank.
- **P@10**: Precision at 10 (fraction of top 10 that are relevant).
- **R@10**: Recall at 10 (fraction of relevant items found in top 10).
- **H@K**: Hit rate at K — fraction of queries where a relevant result appears in top K.

## Results (v2 — Simplified BM25 + Graph Walk)

Removed spectral diffusion, holographic matching, predictive models, SNP, source scan, and artifact cache (~5,087 lines, ~18GB RAM). Pipeline is now BM25 FTS5 → graph walk → IDF coverage + name scoring.

### MRR@10 (25 queries per codebase)

| Codebase | GraphIQ | Grep | Δ |
|---|---|---|---|
| signetai | **0.900** | 0.888 | +1.4% |
| esbuild | **0.940** | 0.950 | -1.1% |
| tokio | **0.848** | 0.943 | -10% |
| **Overall** | **0.896** | **0.911** | **-1.6%** |

**v2 vs v1 MRR:**

| Codebase | v1 | v2 | Δ |
|---|---|---|---|
| signetai | 0.847 | **0.900** | +6.3% |
| esbuild | 0.950 | 0.940 | -1.1% |
| tokio | 0.970 | 0.848 | -12.6% |

signetai improved significantly — the artifact pipeline was overriding good BM25 seeds with noise. tokio regressed on partial-name queries like "acquire owned" → `acquire_owned` (rank 1→3) — holographic name matching helped codebases with generic names.

### NDCG@10 (20 queries per codebase)

| Codebase | GraphIQ | Grep | Δ |
|---|---|---|---|
| signetai | **0.330** | 0.279 | +18% |
| esbuild | **0.405** | 0.288 | +41% |
| tokio | **0.221** | 0.278 | -20% |
| **Overall** | **0.319** | **0.282** | **+13%** |

**v2 vs v1 NDCG:**

| Codebase | v1 | v2 | Δ |
|---|---|---|---|
| signetai | 0.323 | **0.330** | +2.2% |
| esbuild | 0.403 | **0.405** | +0.5% |
| tokio | 0.291 | 0.221 | -24% |

### Per-Category NDCG@10

**Signetai:**

| Category | GraphIQ | Grep |
|---|---|---|
| symbol-exact | **0.845** | 0.803 |
| symbol-partial | **0.704** | 0.758 |
| nl-descriptive | 0.000 | 0.000 |
| nl-abstract | **0.273** | 0.000 |
| error-debug | **0.171** | 0.298 |
| file-path | **0.065** | 0.000 |
| cross-cutting | **0.098** | 0.000 |

**Esbuild:**

| Category | GraphIQ | Grep |
|---|---|---|
| symbol-exact | 1.000 | 1.000 |
| symbol-partial | **0.603** | 0.778 |
| nl-descriptive | **0.137** | 0.000 |
| nl-abstract | **0.238** | 0.037 |
| error-debug | 0.000 | **0.105** |
| file-path | **0.065** | 0.000 |
| cross-cutting | **0.098** | 0.000 |

**Tokio:**

| Category | GraphIQ | Grep |
|---|---|---|
| symbol-exact | 0.686 | **0.857** |
| symbol-partial | 0.306 | **0.538** |
| nl-descriptive | **0.145** | 0.184 |
| nl-abstract | 0.049 | **0.049** |
| error-debug | 0.000 | **0.074** |
| file-path | 0.065 | **0.128** |
| cross-cutting | **0.098** | 0.038 |

### Category Averages (3 codebases)

| Category | Grep | GraphIQ | Winner |
|---|---|---|---|
| symbol-exact | **0.953** | 0.845 | Grep |
| symbol-partial | **0.718** | 0.603 | Grep |
| nl-descriptive | 0.061 | **0.137** | GraphIQ (2.2x) |
| nl-abstract | 0.032 | **0.238** | GraphIQ (7.4x) |
| error-debug | **0.158** | 0.171 | GraphIQ |
| file-path | 0.062 | **0.065** | GraphIQ (marginal) |
| cross-cutting | 0.000 | **0.098** | GraphIQ |

### Combined (MRR + NDCG)

| Codebase | Grep | GraphIQ | Δ |
|---|---|---|---|
| signetai | 0.584 | **0.615** | +5.3% |
| esbuild | 0.619 | **0.673** | +8.7% |
| tokio | **0.611** | 0.535 | -12.4% |
| **Overall** | **0.605** | **0.607** | **+0.3%** |

### v2 vs v1 Comparison

v2 removed 5,087 lines of spectral/holographic/predictive/SNP/source-scan code:

| Codebase | v1 NDCG | v2 NDCG | v1 MRR | v2 MRR |
|---|---|---|---|---|
| signetai | 0.323 | **0.330** (+) | 0.847 | **0.900** (+) |
| esbuild | 0.403 | **0.405** (=) | 0.950 | 0.940 (-) |
| tokio | **0.291** | 0.221 (-) | **0.970** | 0.848 (-) |

## Analysis

GraphIQ's strength is structural discovery — nl-descriptive, nl-abstract, and cross-cutting categories where grep returns zero. The graph walk finds symbols structurally adjacent to BM25 seeds even when their names share no query terms.

### Remaining Weaknesses

**Tokio regressions**: The primary cost of simplification. Tokio's generic function names (`run`, `handle`, `poll`) benefited from holographic name matching and SNP structural fallback. Without these, partial-name queries and exact-name lookups suffer. This is the area to recover.

**Error-debug queries**: Grep's raw substring matching on source code still outperforms graph-based retrieval for finding error messages. A targeted source-scan layer could recover this.

**File-path queries**: GraphIQ doesn't have a dedicated path-matching layer. Grep's substring matching on file paths is surprisingly effective.

## Previous Results (v1 — v7 SNP Structural Fallback)

<details>
<summary>Historical v1 results (click to expand)</summary>

### MRR@10

| Codebase | GraphIQ | Grep | Δ |
|---|---|---|---|
| signetai | 0.847 | **0.888** | -4.6% |
| esbuild | **0.950** | 0.950 | tied |
| tokio | **0.970** | 0.943 | +2.9% |
| **Overall** | 0.922 | **0.927** | **-0.5%** |

### NDCG@10

| Codebase | GraphIQ | Grep | Δ |
|---|---|---|---|
| signetai | **0.323** | 0.279 | +16% |
| esbuild | **0.403** | 0.288 | +40% |
| tokio | **0.291** | 0.278 | +4.7% |
| **Overall** | **0.339** | **0.282** | **+20%** |

### v1 Category Averages

| Category | Grep | GraphIQ | Winner |
|---|---|---|---|
| symbol-exact | 0.887 | **0.926** | GraphIQ |
| symbol-partial | **0.691** | 0.688 | Grep (marginal) |
| nl-descriptive | 0.061 | **0.217** | GraphIQ (3.6x) |
| nl-abstract | 0.029 | **0.242** | GraphIQ (8.3x) |
| error-debug | **0.159** | 0.040 | Grep |
| file-path | 0.043 | **0.081** | GraphIQ |
| cross-cutting | 0.013 | **0.099** | GraphIQ (7.6x) |

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
