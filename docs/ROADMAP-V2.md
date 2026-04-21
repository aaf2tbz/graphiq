# Graphiq v2 Roadmap

## Vision

Graphiq is a structural code intelligence tool, not a search engine. Search is table stakes — grep handles it. The value is the graph: blast radius, structural context, architecture understanding, change impact.

v2 removes the spectral/holographic/predictive artifact pipeline (~4,700 lines, 18GB RAM) and simplifies search to BM25 + graph walk + IDF coverage scoring. Structural tools are unchanged — they only need the graph.

## Expected Outcomes

| Metric | v1 (current) | v2 (target) |
|---|---|---|
| Peak RAM (23K symbols) | ~18GB | ~100MB |
| First run time | 30-60s | 5-10s |
| Warm search | 850ms | ~50ms |
| Disk cache | 75MB (7 artifacts) | 0 |
| Core lines of code | ~15K | ~10K |
| NDCG@10 | 0.339 | ~0.30-0.32 (est.) |
| MRR@10 | 0.922 | ~0.92-0.93 (est.) |
| Structural tools | working | unchanged |

## Phase 0: Baseline Capture

Capture current benchmark numbers as v1 baseline before changes.

Already captured from this session:
- signetai: NDCG 0.323, MRR 0.847
- tokio: NDCG 0.291, MRR 0.970
- esbuild: NDCG 0.403, MRR 0.950

## Phase 1: Strip Artifacts + Simplify

The big bang. Remove spectral, holographic, predictive, and simplify scoring.

### 1a. Delete files
- `spectral.rs` (1,507 lines) — spectral diffusion, Ricci, predictive model, MDL, fingerprints, self-model
- `holo.rs` — holographic encoding
- `holo_name.rs` — holographic name matching
- `structural_fallback.rs` — SNP
- `artifact_cache.rs` — multi-artifact cache (simplify to cruncher-only if needed)

### 1b. Simplify pipeline.rs
Remove: heat diffusion, SNP fallback, source scan seeds, predictive surprise, holographic matching
Keep: BM25 seeds, name lookup, graph walk (BFS), numeric bridges
Result: BM25 → graph walk → score → BM25 lock → file diversity

### 1c. Simplify scoring.rs
Remove: holographic gate, negentropy, coherence, surprise, MDL, source_scan, structural_bonus, SEC channels
Keep: bm25, idf_coverage, name_match, kind_boost, test_penalty
Result: 5-term sum instead of 15-term product

### 1d. Clean CruncherIndex
Remove: SEC channel computation, negentropy, coherence, bridging potential (if unused after scoring simplification)
Keep: adjacency lists, term sets, global IDF, name_to_indices, BM25 search

### 1e. Update consumers
- CLI: remove artifact building calls
- MCP server: remove warmup state, simplify to just CruncherIndex
- Bench: remove spectral/holo/predictive/fingerprint computation
- Remove nalgebra from Cargo.toml

### 1f. Build + test
Verify all 213 tests pass (or update tests for removed code).

## Phase 2: Validate

Run benchmarks on all 3 codebases. Compare to Phase 0 baseline.

Acceptable: up to 0.05 NDCG regression, 0.03 MRR regression.
If regression exceeds threshold: investigate, add back minimal signal.

Verify structural tools: blast, context, explain, topology, briefing, interrogate, constants, why.

## Phase 3: Cleanup

- Remove dead imports and unused code
- Simplify MCP server (no warmup state needed)
- Simplify query family routing (8 → 4-5 families)
- Remove self_model.rs if dead after spectral removal
- Remove lsa.rs if it exists and is dead
- Remove cache.rs (hot cache) if no longer needed
- Update README, benchmarks.md, research.md

## Phase 4: Release

- Version bump to 2.0.0
- Update CI, homebrew
- Release notes documenting the simplification

## What Stays Unchanged

- SQLite schema (symbols, edges, files, FTS5)
- TreeSitter parsing (14 languages)
- Indexing pipeline (index.rs) — symbols, edges, hints, numeric bridges, deep graph
- BM25 FTS5 search
- Graph walk (BFS from seeds)
- IDF coverage scoring
- Name matching with decomposition
- BM25 lock
- File diversity
- All structural tools: blast, context, explain, topology, briefing, interrogate, constants, why
- MCP server (13 tools)
- CLI interface
- AGENTS.md template on install

## What Gets Removed

| Component | Lines | RAM Saved | NDCG Impact |
|---|---|---|---|
| Spectral diffusion | ~800 | ~200MB | +0.02-0.04 |
| Ricci curvature | ~150 | 7.2GB | 0.000 |
| Predictive model | ~200 | 10.5GB | +0.01-0.02 |
| Holographic encoding | ~600 | ~400MB | +0.01-0.02 |
| MDL explanation | ~100 | trivial | ~0.01 |
| Channel fingerprints | ~150 | 2MB | ~0.005 |
| Self-model concepts | ~300 | 5MB | +0.01 (abstract only) |
| SNP fallback | ~200 | trivial | +0.01 (tokio only) |
| Source scan seeds | ~100 | trivial | ~0.01 (errors only) |
| Artifact cache | ~400 | N/A | N/A |
| **Total** | **~4,700** | **~18.3GB** | **~0.02-0.05 total** |

The scoring contribution of these components is small because they were layered on top of an already-strong BM25 + graph walk base. The graph walk captures the structural signal. BM25 captures the lexical signal. Everything else is marginal.
