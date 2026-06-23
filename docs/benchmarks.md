# Benchmarks

## Latest benchmark result: TensorFlow — 6,461,740-line codebase, 433,898 symbols, against baseline Grep

The largest codebase ever benchmarked with GraphIQ (~20× the scale of the v3 codebases below): **TensorFlow**, a multi-language codebase (C/C++, Python, Java, Go, CSS/HTML) spanning **22,812 files / 433,898 symbols / 726,439 edges**. Two separate benchmarks — each with its **own 100-question set** — against baseline Grep. [Full report](benchmark-tensorflow.md) · query sets: [`ndcg-100-tensorflow.json`](../benches/queries/ndcg-100-tensorflow.json), [`mrr-100-tensorflow.json`](../benches/queries/mrr-100-tensorflow.json).

### Headline

| Metric | GraphIQ | Grep | Advantage |
|---|---:|---:|---:|
| **NDCG@10** (100 queries, graded relevance) | **0.201** | 0.123 | **+63%** |
| **MRR@10** (100 queries, single expected symbol) | **0.558** | 0.343 | **+63%** |
| **Hit@10** | **0.85** | 0.52 | +33 pts |
| Categories won (of 10) | **8** | 2 | — |

One-time index cost: **≈27 minutes** (1604s) for a 2.4 GB index. The deep/source-graph phase (type-flow, error-type, data-shape edges) dominates at ~70% of that time — exactly what powers structural retrieval at this scale.

### NDCG@10 by category (100 questions)

| Category | GraphIQ | Grep | Δ | Winner |
|---|---:|---:|---:|:---:|
| behavioral | 0.307 | 0.095 | +0.212 | **GraphIQ** |
| nl-medium | 0.263 | 0.057 | +0.206 | **GraphIQ** |
| cross-cutting | 0.224 | 0.026 | +0.198 | **GraphIQ** |
| file-path | 0.223 | 0.031 | +0.192 | **GraphIQ** |
| symbol-exact | 0.285 | 0.604 | −0.319 | Grep |
| nl-descriptive | 0.171 | 0.025 | +0.146 | **GraphIQ** |
| error-debug | 0.162 | 0.032 | +0.130 | **GraphIQ** |
| symbol-partial | 0.163 | 0.214 | −0.051 | Grep |
| relationship | 0.130 | 0.087 | +0.043 | **GraphIQ** |
| nl-abstract | 0.099 | 0.000 | +0.099 | **GraphIQ** |
| **OVERALL** | **0.201** | **0.123** | **+0.078 (+63%)** | **GraphIQ** |

### MRR@10 by category (100 questions)

| Category | GraphIQ | Grep | Δ | Winner |
|---|---:|---:|---:|:---:|
| file-path | 0.900 | 0.667 | +0.233 | **GraphIQ** |
| descriptive | 0.639 | 0.159 | +0.480 | **GraphIQ** |
| partial | 0.619 | 0.664 | −0.045 | Grep |
| error/debug | 0.607 | 0.125 | +0.482 | **GraphIQ** |
| nl-medium | 0.542 | 0.250 | +0.292 | **GraphIQ** |
| behavioral | 0.525 | 0.216 | +0.309 | **GraphIQ** |
| cross-cutting | 0.458 | 0.350 | +0.108 | **GraphIQ** |
| relationship | 0.341 | 0.176 | +0.165 | **GraphIQ** |
| abstract | 0.322 | 0.024 | +0.298 | **GraphIQ** |
| exact | 0.786 | 1.000 | −0.214 | Grep |
| **OVERALL** | **0.558** | **0.343** | **+0.215 (+63%)** | **GraphIQ** |

### Hit@K (MRR question set)

| Cutoff | GraphIQ | Grep | Δ |
|---|---:|---:|---:|
| Hit@1 | 43/100 | 27/100 | +16 |
| Hit@3 | 61/100 | 40/100 | +21 |
| Hit@5 | 75/100 | 42/100 | +33 |
| Hit@10 | 85/100 | 52/100 | +33 |

**The pattern:** GraphIQ dominates natural-language, behavioral, cross-cutting, error-path, and relationship queries — anywhere a developer is *describing* what they want rather than naming it. On `nl-abstract`, grep scores **0.000** because it cannot match concepts by name at all. Grep only wins when the query is a literal substring of the answer (`symbol-exact` / `exact` / `partial`), where substring matching is trivially optimal. The advantage holds at 433K symbols — ~20× the scale of the v3 codebases below.

---

## v3 benchmarks (3 codebases × 100 queries each)

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
