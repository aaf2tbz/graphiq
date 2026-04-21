# Graphiq v2 Roadmap

## Vision

Graphiq is a structural code intelligence tool, not a search engine. Search is table stakes — grep handles it. The value is the graph: blast radius, structural context, architecture understanding, change impact.

v2 removes the spectral/holographic/predictive artifact pipeline (~4,700 lines, 18GB RAM) and simplifies search to BM25 + graph walk + IDF coverage scoring. Structural tools are unchanged — they only need the graph.

## Expected Outcomes

| Metric | v1 (before) | v2 (actual) | Target |
|---|---|---|---|
| Peak RAM (23K symbols) | ~18GB | ~100MB | ~100MB |
| First run time | 30-60s | 5-10s | 5-10s |
| Warm search | 850ms | ~50ms | ~50ms |
| Disk cache | 75MB (7 artifacts) | ~6.5MB (1 artifact) | 0 |
| Core lines removed | — | 5,087 | ~4,700 |
| NDCG@10 | 0.339 | 0.319 | ~0.30-0.32 |
| MRR@10 | 0.922 | 0.896 | ~0.92-0.93 |
| Structural tools | working | unchanged | unchanged |

## Phase 0: Baseline Capture ✅

Captured v1 baseline:
- signetai: NDCG 0.323, MRR 0.847
- tokio: NDCG 0.291, MRR 0.970
- esbuild: NDCG 0.403, MRR 0.950

## Phase 1: Strip Artifacts + Simplify ✅

Two commits:
- `c23bbbb` — Core rewrite: pipeline.rs, scoring.rs, search.rs, seeds.rs simplified. Removed nalgebra. 1,305 lines removed.
- `57e3a3b` — Deleted dead files (spectral.rs, self_model.rs, holo.rs, holo_name.rs, structural_fallback.rs, artifact_cache.rs). 3,782 lines removed.

Total: 5,087 lines removed. Clean build, 202 tests passing.

## Phase 2: Validate ✅

Results within acceptable thresholds on 2/3 codebases:

| Codebase | NDCG Δ | MRR Δ | Status |
|---|---|---|---|
| signetai | +0.007 | +0.053 | Better than v1 |
| esbuild | +0.002 | -0.010 | Within threshold |
| tokio | -0.070 | -0.122 | Exceeds threshold |

tokio regression is the known cost of removing holographic name matching. To recover in future work.

## Phase 3: Cleanup ✅

- [x] Update README.md
- [x] Update benchmarks.md
- [x] Update research.md (Phase 28)
- [x] Update ROADMAP-V2.md
- [x] Delete 6 dead modules: sec.rs, lsa.rs, evidence.rs, router.rs, file_router.rs, topo.rs (4,776 lines)
- [x] Move extract_terms from lsa.rs to tokenize.rs
- [x] Strip dead fields from RetrievalPolicy and TraceScoreBreakdown
- [x] Remove Lsa CLI subcommand
- [x] Remove EvidenceIndex from bench locomos

## Phase 4: Release ✅

- [x] Version bump to 2.0.0
- [x] Merge v2 → main
- [ ] Update homebrew formula (manual)

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
