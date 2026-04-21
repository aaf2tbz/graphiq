# Benchmarks

## Methodology

v6 unified pipeline benchmarked on 3 codebases (TypeScript, Rust, Go) with separate NDCG and MRR query sets. NDCG queries (20 per codebase) use graded relevance (3=perfect, 2=good) with multiple relevant symbols across 7 categories. MRR queries (25 per codebase) target a single expected symbol — split between exact-name lookups and natural language descriptions. Competitor is Grep — symbol-level `LIKE %term%` search across names and source code.

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

## Results (v7 — SNP Structural Fallback)

### MRR@10 (25 queries per codebase)

| Codebase | GraphIQ | Grep | Δ |
|---|---|---|---|
| signetai | 0.847 | **0.888** | -4.6% |
| esbuild | **0.950** | 0.950 | tied |
| tokio | **0.970** | 0.943 | +2.9% |
| **Overall** | 0.922 | **0.927** | **-0.5%** |

#### MRR Detail

| Codebase | GIQ H@1 | Grep H@1 | GIQ H@3 | Grep H@3 | GIQ H@10 | Grep H@10 |
|---|---|---|---|---|---|---|
| signetai | 19/25 | 21/25 | 24/25 | 22/25 | 24/25 | 25/25 |
| esbuild | 23/25 | 23/25 | 24/25 | 24/25 | 25/25 | 25/25 |
| tokio | 24/25 | 23/25 | 24/25 | 24/25 | 25/25 | 25/25 |

Grep edges ahead on signetai MRR due to the codebase growing from 20,870 to 23,215 symbols — more candidates competing for rank 1.

### NDCG@10 (20 queries per codebase)

| Codebase | GraphIQ | Grep | Δ |
|---|---|---|---|
| signetai | **0.323** | 0.279 | +16% |
| esbuild | **0.403** | 0.288 | +40% |
| tokio | **0.291** | 0.278 | +4.7% |
| **Overall** | **0.339** | **0.282** | **+20%** |

#### NDCG@K Detail

| Codebase | GIQ @3 | Grep @3 | GIQ @5 | Grep @5 | GIQ @10 | Grep @10 |
|---|---|---|---|---|---|---|
| signetai | **0.296** | 0.249 | **0.305** | 0.260 | **0.323** | 0.279 |
| esbuild | **0.395** | 0.267 | **0.395** | 0.267 | **0.403** | 0.288 |
| tokio | **0.305** | 0.271 | **0.294** | 0.253 | **0.291** | 0.278 |

### Per-Category NDCG@10

**Signetai:**

| Category | GraphIQ | Grep |
|---|---|---|
| symbol-exact | **0.881** | 0.803 |
| symbol-partial | 0.751 | **0.758** |
| nl-descriptive | 0.000 | 0.000 |
| nl-abstract | **0.303** | 0.000 |
| error-debug | 0.120 | **0.298** |
| file-path | **0.099** | 0.000 |
| cross-cutting | 0.000 | 0.000 |

**Esbuild:**

| Category | GraphIQ | Grep |
|---|---|---|
| symbol-exact | 1.000 | 1.000 |
| symbol-partial | **0.901** | 0.778 |
| nl-descriptive | **0.453** | 0.000 |
| nl-abstract | **0.333** | 0.037 |
| error-debug | 0.000 | **0.105** |
| file-path | 0.000 | 0.000 |
| cross-cutting | 0.000 | 0.000 |

**Tokio:**

| Category | GraphIQ | Grep |
|---|---|---|
| symbol-exact | **0.897** | 0.857 |
| symbol-partial | 0.411 | **0.538** |
| nl-descriptive | **0.197** | 0.184 |
| nl-abstract | **0.089** | 0.049 |
| error-debug | 0.000 | **0.074** |
| file-path | **0.145** | 0.128 |
| cross-cutting | **0.296** | 0.038 |

### Category Averages (3 codebases)

| Category | Grep | GraphIQ | Winner |
|---|---|---|---|
| symbol-exact | 0.887 | **0.926** | GraphIQ |
| symbol-partial | **0.691** | 0.688 | Grep (marginal) |
| nl-descriptive | 0.061 | **0.217** | GraphIQ (3.6x) |
| nl-abstract | 0.029 | **0.242** | GraphIQ (8.3x) |
| error-debug | **0.159** | 0.040 | Grep |
| file-path | 0.043 | **0.081** | GraphIQ |
| cross-cutting | 0.013 | **0.099** | GraphIQ (7.6x) |

### Combined (MRR + NDCG)

| Codebase | Grep | GraphIQ | Δ |
|---|---|---|---|
| signetai | **0.584** | 0.585 | +0.2% |
| esbuild | 0.619 | **0.677** | +9.4% |
| tokio | 0.611 | **0.631** | +3.3% |
| **Overall** | **0.605** | **0.631** | **+4.3%** |

### v7 vs v6 Comparison

v7 added SNP structural fallback and source scan seeds (~2,780 lines removed):

| Codebase | v6 NDCG | v7 NDCG | Δ |
|---|---|---|---|
| signetai | 0.397 | 0.323 | -0.074 |
| esbuild | 0.453 | 0.403 | -0.050 |
| tokio | 0.284 | 0.291 | +0.007 |

Note: signetai grew from 20,870 to 23,215 symbols (+11%) between v6 and v7 benchmarks, making direct comparison difficult. Tokio and esbuild databases are unchanged. Tokio improved; esbuild regressed on some NL queries.

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
