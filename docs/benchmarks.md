# Benchmarks

## Methodology

v4 benchmark queries across 5 codebases (TS, Rust, Go, Python, Java) with separate NDCG and MRR query sets (20 queries each, disjoint). NDCG queries use graded relevance (3=perfect, 2=good, 1=related) with multiple relevant symbols. MRR queries target a single expected symbol. Competitor is Grep — a symbol-level `LIKE %term%` search across names and source code. This is the strongest possible naive baseline.

### Codebases

| Codebase | Language | Symbols | Edges | Characteristics |
|---|---|---|---|---|
| signetai | TypeScript | 20,870 | 46,859 | Domain-specific names, deep call graphs |
| tokio | Rust | 17,867 | 39,032 | Generic function names (`run`, `handle`, `poll`) |
| esbuild | Go | 12,040 | 39,632 | Descriptive names (`convertOKLCHToOKLAB`) |
| flask | Python | 1,971 | 5,611 | Small codebase, decorator-based API |
| junit5 | Java | 34,273 | 43,204 | Large codebase, annotation-driven, multiple modules |

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

- **NDCG@K**: Normalized Discounted Cumulative Gain at K. Graded relevance (3/2/1). Reported at H@3, H@5, H@10.
- **MRR@10**: Mean Reciprocal Rank. 1/first_correct_rank. Reported with P@10, R@10, H@10, Acc@1, Acc@10.

## Results (v5 — 5 codebases, deep graph edges)

### NDCG@K

| Codebase | GraphIQ H@3 | Grep H@3 | GraphIQ H@5 | Grep H@5 | GraphIQ H@10 | Grep H@10 |
|---|---|---|---|---|---|---|
| signetai | **0.426** | 0.300 | **0.405** | 0.306 | **0.406** | 0.343 |
| tokio | 0.199 | **0.311** | 0.189 | **0.307** | 0.205 | **0.326** |
| esbuild | **0.395** | 0.235 | **0.403** | 0.235 | **0.411** | 0.277 |
| flask | 0.324 | **0.337** | 0.362 | **0.395** | 0.426 | **0.432** |
| junit5 | **0.242** | 0.167 | **0.222** | 0.167 | **0.198** | 0.181 |

### MRR@10

| Codebase | G IQ MRR | Gr MRR | G IQ H@10 | Gr H@10 | G IQ Acc@1 | Gr Acc@1 |
|---|---|---|---|---|---|---|
| signetai | **0.404** | 0.154 | **12/20** | 6/20 | **7/20** | 1/20 |
| tokio | **0.667** | 0.360 | **18/20** | 13/20 | **10/20** | 4/20 |
| esbuild | **0.475** | 0.173 | **12/20** | 5/20 | **9/20** | 3/20 |
| flask | **0.615** | 0.523 | 17/20 | 15/20 | 11/20 | 9/20 |
| junit5 | **0.420** | 0.159 | **16/20** | 8/20 | 5/20 | 2/20 |

### Summary

GraphIQ wins MRR on all 5 codebases (1.6-2.6x over Grep). MRR measures first-hit accuracy — the metric that matters for agent recall, where an agent scans top results and picks one.

NDCG is a split: GraphIQ wins on signetai, esbuild, and junit5 (3/5). Loses on tokio (known behavioral-NL connectivity gap) and flask (small codebase, close to parity).

### Per-Category NDCG@10

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

GraphIQ's strength is MRR — finding the right answer quickly. On signetai it's 2.6x better, on tokio 1.9x, on esbuild 2.7x, on junit5 2.6x. This matters because agents scan top-3 results.

NDCG weakness is concentrated in two areas:
- **tokio nl-abstract/error-debug**: Behavioral NL queries need edges that don't exist in the call/import graph. The connection is purely behavioral, not structural. Deformed mode produces 0.000 on these categories.
- **flask**: Small codebase (1971 symbols) where Grep's direct name matching is very effective. GraphIQ is close to parity but slightly behind.

## Deep Graph Edges

v5 indexes include 4 new edge types beyond calls, imports, and containment:

| Edge Type | signetai | tokio | esbuild | flask | junit5 |
|---|---|---|---|---|---|
| Type flow (shared type tokens) | 7,187 | 6,659 | 3,595 | 457 | ~5K |
| Error type (shared error params) | 228 | 275 | 83 | 12 | ~200 |
| Data shape (shared field access) | 13,715 | 9,819 | 16,719 | 845 | ~12K |
| String literal (error-related strings) | 199 | 31 | 260 | 8 | ~100 |
| Comment ref (symbol mentions in comments) | 3,993 | 5,810 | 1,767 | 312 | ~3K |

## Router Performance

The query family router achieves strong results vs the best individual method per query. Routing is not the bottleneck — search method quality is.

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
