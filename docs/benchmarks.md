# Benchmarks

## Methodology

v4 benchmark queries across 3 codebases with separate NDCG and MRR query sets (20 queries each, disjoint). NDCG queries use graded relevance (3=perfect, 2=good, 1=related) with multiple relevant symbols. MRR queries target a single expected symbol. Competitor is Grep — a symbol-level `LIKE %term%` search across names and source code. This is the strongest possible naive baseline.

### Codebases

| Codebase | Language | Symbols | Edges | Characteristics |
|---|---|---|---|---|
| signetai | TypeScript | 20,870 | 28,017 | Domain-specific names, deep call graphs |
| tokio | Rust | 17,867 | 21,378 | Generic function names (`run`, `handle`, `poll`) |
| esbuild | Go | 12,040 | 27,124 | Descriptive names (`convertOKLCHToOKLAB`) |

### Query Categories

| Category | Count | Description |
|---|---|---|
| symbol-exact | 3 | Exact symbol names (`spawn_blocking`, `Renamer`) |
| symbol-partial | 3 | Short fragments (`blocking shutdown`, `acquire semaphore permit`) |
| nl-descriptive | 4 | NL with action verbs (`retrieve memories using vector similarity search`) |
| nl-abstract | 3 | How/what questions (`what determines the retention and compaction of memories`) |
| error-debug | 3 | Error/panic queries (`panic in blocking task after runtime shutdown`) |
| file-path | 2 | File/module names (`tokio sync mpsc`) |
| cross-cutting | 2 | Enumeration queries (`all ways to send data across tokio channels`) |

### Evaluation Metrics

- **NDCG@10**: Normalized Discounted Cumulative Gain at 10. Graded relevance (3/2/1).
- **MRR@10**: Mean Reciprocal Rank. 1/first_correct_rank.

## Results

### NDCG@10

| Codebase | GraphIQ | Grep | Winner |
|---|---|---|---|
| signetai | **0.399** | 0.343 | GraphIQ (+16%) |
| tokio | 0.179 | **0.322** | Grep (+80%) |
| esbuild | **0.420** | 0.277 | GraphIQ (+52%) |

### MRR@10

| Codebase | GraphIQ | Grep | Winner |
|---|---|---|---|
| signetai | **0.393** | 0.154 | GraphIQ (+155%) |
| tokio | **0.717** | 0.317 | GraphIQ (+126%) |
| esbuild | **0.368** | 0.185 | GraphIQ (+99%) |

### Per-Category NDCG

**Signetai:**

| Category | GraphIQ | Grep |
|---|---|---|
| symbol-exact | 1.000 | 1.000 |
| symbol-partial | 0.960 | 0.989 |
| nl-descriptive | 0.199 | 0.123 |
| nl-abstract | 0.025 | 0.000 |
| error-debug | 0.244 | 0.105 |
| file-path | 0.190 | 0.068 |
| cross-cutting | 0.063 | 0.000 |

**Tokio:**

| Category | GraphIQ | Grep |
|---|---|---|
| symbol-exact | 0.830 | 0.830 |
| symbol-partial | 0.038 | 0.235 |
| nl-descriptive | 0.197 | 0.000 |
| nl-abstract | 0.000 | 0.585 |
| error-debug | 0.000 | 0.494 |
| file-path | 0.128 | 0.000 |
| cross-cutting | 0.000 | 0.000 |

**Esbuild:**

| Category | GraphIQ | Grep |
|---|---|---|
| symbol-exact | 0.901 | 0.901 |
| symbol-partial | 0.765 | 0.722 |
| nl-descriptive | 0.568 | 0.111 |
| nl-abstract | 0.049 | 0.000 |
| error-debug | 0.412 | 0.111 |
| file-path | 0.000 | 0.000 |
| cross-cutting | 0.161 | 0.000 |

### Analysis

GraphIQ's strength is MRR — finding the right answer quickly. On signetai it's 2.6x better, on tokio 2.3x, on esbuild 2x. This matters because agents scan top-3 results.

NDCG weakness is concentrated in two areas:
- **symbol-exact**: GraphIQ should match Grep but sometimes doesn't (e.g. `ReadBuf` scores 0.490 vs 0.490 — tied, but `AppendSourceMapChunk` scores 1.000 vs 1.000 while `spawn_blocking` scores 0.542 vs 1.000). The GooberV5 router for exact lookups has room to improve.
- **tokio nl-abstract/error-debug**: Deformed mode produces 0.000 on these categories for tokio. The spectral index may not capture tokio's structural patterns well.

## Router Performance

The query family router achieves 2 wins, 17 ties, 1 loss vs the best individual method per query (signetai). Routing is not the bottleneck — search method quality is.

### Routing Table

| Query Family | Search Mode | Rationale |
|---|---|---|
| SymbolExact | GooberV5 | Holographic name matching for exact lookups |
| SymbolPartial | GooberV5 | Fuzzy name matching for fragments |
| NaturalDescriptive | Geometric | Structural context for action-oriented NL |
| NaturalAbstract | Deformed | Maximum exploration for how/what questions |
| ErrorDebug | Deformed | Predictive model + fingerprints for error patterns |
| CrossCuttingSet | Deformed | High diversity for enumeration queries |
| Relationship | Geometric | Structural neighborhood for call graph queries |
| FilePath | Geometric | File-adjacent symbol discovery |

### Classifier Design

The classifier inverts the typical cascade: instead of trying to detect NL patterns and defaulting to symbol, it detects symbols (code-shaped tokens) and defaults everything else to NaturalDescriptive. This prevents 65% of queries from falling through to a wrong default.

Priority order: CrossCutting > ErrorDebug > Relationship > FilePath > Symbol > NaturalAbstract > NaturalDescriptive (default).

## Running Benchmarks

```bash
cargo build --release -p graphiq-bench

# NDCG (GraphIQ vs Grep)
./target/release/graphiq-bench <db> <ndcg-queries.json>

# MRR only
./target/release/graphiq-bench <db> "" <mrr-queries.json>

# Both
./target/release/graphiq-bench <db> <ndcg-queries.json> <mrr-queries.json>
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
    "query": "how does the runtime schedule tasks",
    "category": "behavioral",
    "expected_symbol": "scheduler"
  }
]
```
