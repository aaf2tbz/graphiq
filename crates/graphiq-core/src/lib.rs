//! GraphIQ — structural code intelligence engine.
//!
//! Indexes a codebase into a graph (calls, imports, type flow, error surfaces)
//! and searches it with ranked retrieval that understands how code is connected,
//! not just what strings it contains. No embeddings, no LLM, no network.
//! Everything lives in a single SQLite file.
//!
//! # Architecture
//!
//! Two major phases: **indexing** (builds the graph) and **search** (queries it).
//!
//! **Indexing pipeline** (`index`): file discovery → Tree-sitter symbol extraction →
//! edge extraction (structural + deep graph) → hint generation → CruncherIndex.
//!
//! **Search pipeline** (`search`): query family classification → seed generation
//! (BM25 + per-term + graph + numeric + source scan) → graph walk expansion →
//! scoring (BM25 + coverage + name overlap + neighbor fingerprints) → post-processing.
//!
//! # Module map
//!
//! | Module | Purpose |
//! |---|---|
//! | `db` | SQLite storage — symbols, edges, files, FTS5 index |
//! | `index` | Indexing pipeline — walks files, extracts symbols/edges |
//! | `search` | Search engine — query routing, graph walk, orchestration |
//! | `cruncher` | In-memory CruncherIndex — adjacency lists, term sets, IDF |
//! | `fts` | BM25 full-text search over FTS5 virtual table |
//! | `rerank` | Result reranking — heuristics, evidence channels, diversity |
//! | `scoring` | Candidate scoring — BM25 + coverage + name overlap composite |
//! | `pipeline` | Unified search pipeline — seed→walk→score on CruncherIndex |
//! | `graph` | Graph traversal — bounded BFS, structural expansion |
//! | `edge` | Edge types (Calls, Imports, etc.), evidence kinds, blast types |
//! | `blast` | Change impact analysis — forward/backward dependency tracing |
//! | `deep_graph` | Semantic edges — type flow, error surfaces, data shapes |
//! | `query_family` | Query classification — 8 families with retrieval policies |
//! | `decompose` | Query decomposition — split multi-concept queries |
//! | `seeds` | Seed generation — BM25, per-term expansion, graph, numeric |
//! | `trace` | Retrieval trace — debug/diagnostic scoring breakdown |
//! | `subsystems` | Subsystem detection — cluster symbols by file proximity |
//! | `roles` | Structural roles — validator, cache, handler, entry point |
//! | `motifs` | Graph motifs — connector, orchestrator, hub, guard |
//! | `structural_alias` | Ambiguity resolution — disambiguate collision-prone names |
//! | `edge_evidence` | Edge evidence — classify and weight edge quality |
//! | `numeric_bridges` | Constant tracing — find symbols sharing numeric literals |
//! | `files` | File discovery, language detection, content hashing |
//! | `languages` | Per-language Tree-sitter chunkers (16 languages) |
//! | `chunker` | Tree-sitter AST parsing — symbol extraction framework |
//! | `calls` | Call site extraction — function calls across languages |
//! | `symbol` | Symbol types — SymbolKind, Visibility, Symbol, SymbolBuilder |
//! | `tokenize` | Identifier decomposition and term extraction |
//! | `cache` | Hot cache — neighborhoods, results, blasts, assembled context |
//! | `manifest` | Artifact manifest — track cruncher/FTS freshness |
//! | `briefing` | Codebase overview — architecture, subsystems, public API |
//! | `behavioral` | Behavioral descriptors — verb+object phrases from symbol names |

pub mod behavioral;
pub mod blast;
pub mod briefing;
pub mod cache;
pub mod calls;
pub mod chunker;
pub mod db;
pub mod dead_code;
pub mod decompose;
pub mod deep_graph;
pub mod edge;
pub mod edge_evidence;
#[cfg(feature = "embed")]
pub mod embed;
pub mod files;
pub mod fts;
pub mod graph;
pub mod index;
pub mod languages;
pub mod manifest;
pub mod motifs;
pub mod numeric_bridges;
pub mod query_family;
pub mod pipeline;
pub mod rerank;
pub mod scoring;
pub mod seeds;
pub mod roles;
pub mod cruncher;
pub mod search;
pub mod subsystems;
pub mod structural_alias;
pub mod symbol;
pub mod tokenize;
pub mod trace;
