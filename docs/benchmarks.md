# Benchmarks

**Current benchmark: v3** — 3 codebases, 50 NDCG + 50 MRR queries per codebase (300 total), fresh indexes and new query sets. The 5-codebase benchmarks in the [research notes](research.md#phase-22-5-codebase-benchmarks--deep-graph-edges) were run on the v1 pipeline (spectral/holographic artifacts) and do not reflect the current system.

## Methodology

v3.1 pipeline (BM25 + graph walk + gated name overlap + specificity scaling + per-family routing + neighbor fingerprints + structural aliases) benchmarked on 3 codebases with fresh indexes and new query sets. 50 NDCG queries and 50 MRR queries per codebase (300 total), covering 7 categories. Competitor is Grep — symbol-level `LIKE %term%` search across names and source code.

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

## Results (v3.1 — Structural Aliases)

v3.1 adds structural aliases to v3's BM25 + graph walk pipeline. At index time, every collision-prone symbol (≥3 symbols sharing a name) gets a structural fingerprint encoding its edge mix, signature type tokens, 1-hop neighborhood IDF, container context, and behavioral operational context. At query time, these fingerprints disambiguate lexically identical symbols like `poll` (87 instances in tokio), `read` (38 instances), and `handle` (24 instances). Tokio has 13,354 aliased symbols out of 17,867 total (621 collision sets).

### NDCG@10 (50 queries per codebase)

| Codebase | GraphIQ | Grep | Δ |
|---|---|---|---|
| signetai | **0.286** | 0.143 | **+100%** |
| esbuild | **0.318** | 0.200 | **+59%** |
| tokio | 0.192 | **0.193** | -1% |
| **Overall** | **0.265** | **0.179** | **+48%** |

### MRR@10 (50 queries per codebase)

| Codebase | GraphIQ | Grep | Δ |
|---|---|---|---|
| signetai | **0.450** | 0.144 | **+213%** |
| esbuild | **0.551** | 0.145 | **+280%** |
| tokio | **0.411** | 0.330 | **+25%** |
| **Overall** | **0.471** | **0.206** | **+128%** |

### Per-Category NDCG@10

**Signetai (50 queries):**

| Category | GraphIQ | Grep |
|---|---|---|
| symbol-exact | **0.807** | 0.807 |
| relationship | **0.688** | 0.031 |
| error-debug | **0.325** | 0.182 |
| nl-descriptive | **0.243** | 0.079 |
| nl-abstract | 0.000 | 0.000 |
| cross-cutting | **0.017** | 0.000 |
| file-path | 0.000 | 0.000 |

**Esbuild (50 queries):**

| Category | GraphIQ | Grep |
|---|---|---|
| relationship | **0.868** | 0.258 |
| symbol-exact | 0.591 | **0.630** |
| nl-descriptive | **0.382** | 0.219 |
| file-path | 0.139 | **0.241** |
| error-debug | **0.182** | 0.023 |
| nl-abstract | 0.065 | **0.113** |
| cross-cutting | **0.060** | 0.020 |

**Tokio (50 queries):**

| Category | GraphIQ | Grep |
|---|---|---|
| symbol-exact | 0.727 | **0.749** |
| relationship | **0.270** | 0.183 |
| nl-descriptive | **0.101** | 0.065 |
| error-debug | 0.174 | **0.346** |
| nl-abstract | **0.088** | 0.015 |
| cross-cutting | 0.043 | **0.068** |
| file-path | **0.025** | 0.000 |

### Category Averages (3 codebases)

| Category | GraphIQ | Grep | Winner |
|---|---|---|---|
| relationship | **0.609** | 0.157 | GraphIQ (3.9x) |
| symbol-exact | 0.708 | **0.729** | Grep (marginal) |
| nl-descriptive | **0.242** | 0.121 | GraphIQ (2.0x) |
| error-debug | **0.227** | 0.184 | GraphIQ (1.2x) |
| nl-abstract | **0.051** | 0.043 | GraphIQ (marginal) |
| file-path | 0.055 | **0.080** | Mixed |
| cross-cutting | **0.040** | 0.029 | GraphIQ (1.4x) |

### MRR Hit Rates

| Codebase | G H@1 | G H@10 | Gr H@1 | Gr H@10 |
|---|---|---|---|---|
| signetai | 16/50 | 23/50 | 7/50 | 12/50 |
| esbuild | 20/50 | 28/50 | 8/50 | 26/50 |
| tokio | 12/50 | 22/50 | 14/50 | 20/50 |

## Analysis

GraphIQ's structural signals dominate grep on codebases with descriptive names. The relationship category is GraphIQ's strongest signal (3.9x over grep) — the graph walk finds structurally connected symbols that no substring search can discover. Structural aliases improved tokio MRR from +14% to +25%, with the behavioral context fingerprint distinguishing `io-poll` from `parking-poll` from `stream-poll` from `completion-poll`.

### Remaining Weaknesses

**Tokio**: Generic names remain the hard case. GraphIQ wins MRR (+25%) but Grep ties NDCG (-1%). Tokio's `poll`, `read`, `write` functions are too generic for name overlap to help, and the graph walk's structural signal is weaker in a runtime library where everything calls everything. Structural aliases closed the gap from -7% to -1% on NDCG but grep retains an edge on error-debug queries where error messages contain literal function names.

**Abstract NL queries**: Both GraphIQ and Grep score near zero on "how does X work" queries across all codebases. These require semantic understanding beyond structural graph signals.

**File-path queries**: Neither system scores well. Grep's substring matching occasionally wins when the path contains query terms.

## Previous Results

<details>
<summary>v3 results (Gated Overlap + Specificity + Neighbor Fingerprints)</summary>

### NDCG@10

| Codebase | GraphIQ | Grep | Δ |
|---|---|---|---|
| signetai | **0.339** | 0.137 | **+147%** |
| esbuild | **0.365** | 0.210 | **+74%** |
| tokio | 0.183 | **0.196** | -7% |
| **Overall** | **0.296** | **0.181** | **+63%** |

### MRR@10

| Codebase | GraphIQ | Grep | Δ |
|---|---|---|---|
| signetai | **0.437** | 0.168 | **+160%** |
| esbuild | **0.498** | 0.256 | **+95%** |
| tokio | **0.348** | 0.306 | **+14%** |
| **Overall** | **0.428** | **0.243** | **+76%** |

</details>

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
