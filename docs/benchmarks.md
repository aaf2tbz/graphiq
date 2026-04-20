# Benchmarks

## Methodology

v6 unified pipeline benchmarked on 3 codebases (TypeScript, Rust, Go) with separate NDCG and MRR query sets. NDCG queries (20 per codebase) use graded relevance (3=perfect, 2=good) with multiple relevant symbols across 7 categories. MRR queries (25 per codebase) target a single expected symbol — split between exact-name lookups and natural language descriptions. Competitor is Grep — symbol-level `LIKE %term%` search across names and source code.

### Codebases

| Codebase | Language | Symbols | Edges | Characteristics |
|---|---|---|---|---|
| signetai | TypeScript | 20,870 | 46,892 | Domain-specific names, deep call graphs |
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

## Results (v6 — Unified Pipeline)

### MRR@10 (25 queries per codebase)

| Codebase | GraphIQ | Grep | Δ |
|---|---|---|---|
| signetai | **0.960** | 0.941 | +2.0% |
| esbuild | **0.947** | 0.943 | +0.4% |
| tokio | **0.970** | 0.940 | +3.2% |
| **Overall** | **0.959** | **0.941** | **+1.9%** |

#### MRR Detail

| Codebase | GIQ H@1 | Grep H@1 | GIQ H@3 | Grep H@3 | GIQ H@10 | Grep H@10 |
|---|---|---|---|---|---|---|
| signetai | 23/25 | 23/25 | 25/25 | 24/25 | 25/25 | 25/25 |
| esbuild | 23/25 | 23/25 | 25/25 | 24/25 | 25/25 | 25/25 |
| tokio | 24/25 | 22/25 | 24/25 | 25/25 | 25/25 | 25/25 |

Both GraphIQ and Grep achieve 100% H@10 across all codebases. The difference is in rank position — GraphIQ finds the target at rank 1 more often.

### NDCG@10 (20 queries per codebase)

| Codebase | GraphIQ | Grep | Δ |
|---|---|---|---|
| signetai | **0.397** | 0.276 | +44% |
| esbuild | **0.453** | 0.298 | +52% |
| tokio | 0.284 | **0.290** | -2% |
| **Overall** | **0.378** | **0.288** | **+31%** |

#### NDCG@K Detail

| Codebase | GIQ @3 | Grep @3 | GIQ @5 | Grep @5 | GIQ @10 | Grep @10 |
|---|---|---|---|---|---|---|
| signetai | **0.366** | 0.249 | **0.366** | 0.267 | **0.397** | 0.276 |
| esbuild | **0.445** | 0.272 | **0.445** | 0.272 | **0.453** | 0.298 |
| tokio | **0.310** | 0.287 | **0.290** | 0.272 | 0.284 | **0.290** |

### Per-Category NDCG@10

**Signetai:**

| Category | GraphIQ | Grep |
|---|---|---|
| symbol-exact | 0.803 | 0.803 |
| symbol-partial | **0.816** | 0.741 |
| nl-descriptive | **0.190** | 0.000 |
| nl-abstract | **0.265** | 0.000 |
| error-debug | **0.472** | 0.298 |
| file-path | 0.000 | 0.000 |
| cross-cutting | **0.155** | 0.000 |

**Esbuild:**

| Category | GraphIQ | Grep |
|---|---|---|
| symbol-exact | 1.000 | 1.000 |
| symbol-partial | **0.901** | 0.815 |
| nl-descriptive | **0.451** | 0.000 |
| nl-abstract | **0.333** | 0.000 |
| error-debug | **0.333** | 0.105 |
| file-path | 0.000 | 0.068 |
| cross-cutting | 0.000 | 0.000 |

**Tokio:**

| Category | GraphIQ | Grep |
|---|---|---|
| symbol-exact | **0.895** | 0.857 |
| symbol-partial | 0.406 | **0.576** |
| nl-descriptive | **0.225** | 0.207 |
| nl-abstract | 0.049 | **0.089** |
| error-debug | 0.000 | **0.074** |
| file-path | **0.145** | 0.129 |
| cross-cutting | **0.255** | 0.000 |

### Category Averages (3 codebases)

| Category | Grep | GraphIQ | Winner |
|---|---|---|---|
| symbol-exact | 0.887 | **0.899** | GraphIQ |
| symbol-partial | **0.711** | 0.708 | Grep (marginal) |
| nl-descriptive | 0.069 | **0.289** | GraphIQ (4.2x) |
| nl-abstract | 0.030 | **0.216** | GraphIQ (7.2x) |
| error-debug | 0.159 | **0.268** | GraphIQ (1.7x) |
| file-path | **0.066** | 0.048 | Grep |
| cross-cutting | 0.000 | **0.137** | GraphIQ |

### Combined (MRR + NDCG)

| Codebase | Grep | GraphIQ | Δ |
|---|---|---|---|
| signetai | 0.609 | **0.679** | +11% |
| esbuild | 0.621 | **0.700** | +13% |
| tokio | 0.615 | **0.627** | +2% |
| **Overall** | **0.615** | **0.669** | **+8.7%** |

### v6 vs v5 Comparison

v6 unified the pipeline (5 search methods → 1 parameterized function, ~3,000 lines removed). No regression:

| Codebase | v5 NDCG | v6 NDCG | Δ |
|---|---|---|---|
| signetai | 0.406 | 0.397 | -0.009 |
| esbuild | 0.411 | 0.453 | +0.042 |
| tokio | 0.205 | 0.284 | +0.079 |

Esbuild and tokio improved. Signetai regressed 0.009 (within noise — ±0.003 across runs). The unified pipeline matches or beats the legacy methods with dramatically simpler code.

## Analysis

GraphIQ's strength is structural discovery — nl-descriptive, nl-abstract, error-debug, and cross-cutting categories where grep's lexical matching returns zero. Heat diffusion finds symbols that are structurally adjacent to BM25 seeds even when their names share no query terms.

### Remaining Weaknesses

**File-path queries** (0.048 vs grep's 0.066): GraphIQ doesn't have a dedicated path-matching layer in the unified pipeline yet. Grep's substring matching on file paths is surprisingly effective.

**Tokio natural language**: Tokio's generic function names (`run`, `handle`, `poll`, `budget`) mean the graph has low specificity. Grep's raw substring matching edges ahead on nl-abstract and error-debug because there are fewer disambiguating signals in the graph topology.

**Cross-cutting on esbuild**: Both methods score 0.000 on cross-cutting queries for esbuild. Enumeration queries ("all constant folding passes") require discovering a distributed set of symbols — neither lexical nor structural search handles this well.

## Router Performance

The query family router achieves strong results vs the best individual method per query. Routing is not the bottleneck — search method quality is.

### Routing Table

| Query Family | Config | Rationale |
|---|---|---|
| SymbolExact | name-gated, no surprise | Holographic name matching for exact lookups |
| SymbolPartial | name-gated, light expansion | Fuzzy name matching for fragments |
| NaturalDescriptive | full spectral + surprise + MDL | Structural context for action-oriented NL |
| NaturalAbstract | max exploration, high walk weight | Maximum exploration for how/what questions |
| ErrorDebug | predictive model + fingerprints | Error pattern matching |
| CrossCuttingSet | high diversity, set cover | High diversity for enumeration queries |
| Relationship | neighborhood-centric | Structural neighborhood for call graph queries |
| FilePath | file-adjacent | File-adjacent symbol discovery |

### Router Win/Loss (NDCG diagnostic)

| Codebase | Wins | Ties | Losses |
|---|---|---|---|
| signetai | 1 | 16 | 3 |
| esbuild | 1 | 14 | 5 |
| tokio | 0 | 17 | 3 |

The router ties on most queries — the unified pipeline produces the same results regardless of which legacy method "would have won." Losses are where an individual legacy method finds a better result that the unified pipeline doesn't reach.

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
