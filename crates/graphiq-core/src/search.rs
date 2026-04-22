//! Search engine — orchestrates the full search pipeline.
//!
//! Routes queries through query family classification, seed generation (BM25
//! + graph expansion), graph walk, scoring, and post-processing. Supports two
//! modes: `Fts` (BM25 only) and `GraphWalk` (BM25 + structural expansion).
//!
//! Entry point: [`SearchEngine::search`] — classifies the query, generates
//! seeds, runs graph walk if enabled, scores candidates, and returns ranked
//! results with optional blast radius and retrieval trace.

use std::collections::HashMap;

use crate::blast;
use crate::cache::HotCache;
use crate::cruncher::CruncherIndex;
use crate::db::GraphDb;
use crate::edge::{BlastDirection, BlastRadius};
use crate::fts::{FtsConfig, FtsSearch};
use crate::graph::StructuralExpander;
use crate::rerank::{Reranker, ScoredSymbol};
use crate::query_family::{self, QueryFamily};
use crate::trace::RetrievalTrace;

/// Search mode — determines whether structural graph walking is used.
///
/// `Fts`: BM25 full-text search only (used when cruncher is not built).
/// `GraphWalk`: BM25 + graph walk expansion (used when cruncher is ready).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    Fts,
    GraphWalk,
}

impl std::fmt::Display for SearchMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SearchMode::Fts => write!(f, "FTS"),
            SearchMode::GraphWalk => write!(f, "GraphWalk"),
        }
    }
}

/// Search query configuration.
///
/// Builder-pattern query with options for result count, expansion depth,
/// file filtering, blast radius, and debug tracing.
#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub query: String,
    pub top_k: usize,
    pub max_expansion_depth: usize,
    pub expansion_seeds: usize,
    pub debug: bool,
    pub file_filter: Option<String>,
    pub blast_radius: bool,
    pub blast_depth: usize,
    pub collect_trace: bool,
}

impl SearchQuery {
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            top_k: 10,
            max_expansion_depth: 2,
            expansion_seeds: 20,
            debug: false,
            file_filter: None,
            blast_radius: false,
            blast_depth: 3,
            collect_trace: false,
        }
    }

    pub fn top_k(mut self, k: usize) -> Self {
        self.top_k = k;
        self
    }

    pub fn debug(mut self, d: bool) -> Self {
        self.debug = d;
        self.collect_trace = d;
        self
    }

    pub fn with_trace(mut self) -> Self {
        self.collect_trace = true;
        self
    }

    pub fn with_blast(mut self, depth: usize) -> Self {
        self.blast_radius = true;
        self.blast_depth = depth;
        self
    }

    pub fn file_filter(mut self, filter: impl Into<String>) -> Self {
        self.file_filter = Some(filter.into());
        self
    }
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub results: Vec<ScoredSymbol>,
    pub blast_radius: Option<BlastRadius>,
    pub total_fts_candidates: usize,
    pub total_expanded: usize,
    pub from_cache: bool,
    pub search_mode: SearchMode,
    pub query_family: QueryFamily,
    pub traces: HashMap<i64, RetrievalTrace>,
}

pub struct SearchEngine<'a> {
    db: &'a GraphDb,
    cache: &'a HotCache,
    cruncher_index: Option<&'a CruncherIndex>,
}

impl<'a> SearchEngine<'a> {
    pub fn new(db: &'a GraphDb, cache: &'a HotCache) -> Self {
        Self {
            db,
            cache,
            cruncher_index: None,
        }
    }

    pub fn with_cruncher(mut self, ci: &'a CruncherIndex) -> Self {
        self.cruncher_index = Some(ci);
        self
    }

    fn make_fts(&self, family: QueryFamily) -> FtsSearch<'a> {
        match family {
            QueryFamily::NaturalAbstract
            | QueryFamily::NaturalDescriptive
            | QueryFamily::ErrorDebug
            | QueryFamily::CrossCuttingSet => {
                FtsSearch::with_config(self.db, FtsConfig::for_natural_language())
            }
            _ => FtsSearch::new(self.db),
        }
    }

    pub fn active_mode(&self) -> SearchMode {
        if self.cruncher_index.is_some() {
            SearchMode::GraphWalk
        } else {
            SearchMode::Fts
        }
    }

    pub fn search(&self, query: &SearchQuery) -> SearchResult {
        let query_hash = HotCache::compute_query_hash(&query.query, query.top_k);
        let family = query_family::classify_query_family(&query.query);

        if let Some(cached) = self.cache.get_results(query_hash) {
            return SearchResult {
                results: cached,
                blast_radius: None,
                total_fts_candidates: 0,
                total_expanded: 0,
                from_cache: true,
                search_mode: self.active_mode(),
                query_family: family,
                traces: HashMap::new(),
            };
        }

        let mode = self.active_mode();

        let mut result = match mode {
            SearchMode::GraphWalk => self.search_unified(query, query_hash, family),
            SearchMode::Fts => self.search_fts_fallback(query, query_hash, family),
        };

        if family == QueryFamily::SymbolExact {
            self.promote_exact_matches(&mut result, &query.query);
        }

        result
    }

    fn search_unified(
        &self,
        query: &SearchQuery,
        query_hash: u64,
        family: QueryFamily,
    ) -> SearchResult {
        let ci = self.cruncher_index.unwrap();

        let seed_config = crate::seeds::SeedConfig::for_family(family);
        let (seeds, total_fts, _bm25_original) = crate::seeds::generate_seeds(
            self.db, &query.query, &seed_config,
        );

        let pipeline_config = crate::pipeline::PipelineConfig {
            top_k: query.top_k,
        };

        let raw_results = crate::pipeline::unified_search(
            &query.query,
            ci,
            &seeds,
            &pipeline_config,
            family,
        );

        let file_paths = self.load_file_paths();
        let results: Vec<ScoredSymbol> = raw_results
            .into_iter()
            .filter_map(|(id, score)| {
                let sym = self.db.get_symbol(id).ok()??;
                let fp = file_paths.get(&sym.file_id).cloned();
                if let Some(ref filter) = query.file_filter {
                    if fp.as_deref().map_or(true, |p| !p.contains(filter)) {
                        return None;
                    }
                }
                Some(ScoredSymbol {
                    symbol: sym,
                    score,
                    breakdown: None,
                    is_fts_hit: false,
                    file_path: fp,
                })
            })
            .collect();

        for r in &results {
            self.cache.put_source(r.symbol.id, r.symbol.source.clone());
        }

        let blast_result = self.compute_blast(&results, query);

        self.cache.put_results(query_hash, results.clone());

        SearchResult {
            results,
            blast_radius: blast_result,
            total_fts_candidates: total_fts,
            total_expanded: 0,
            from_cache: false,
            search_mode: SearchMode::GraphWalk,
            query_family: family,
            traces: HashMap::new(),
        }
    }

    fn search_fts_fallback(
        &self,
        query: &SearchQuery,
        query_hash: u64,
        family: QueryFamily,
    ) -> SearchResult {
        let mut results: Vec<ScoredSymbol>;
        let total_fts: usize;
        let total_expanded: usize;

        let fts = self.make_fts(family);
        let fts_results = fts.search(&query.query, Some(200));
        total_fts = fts_results.len();

        if let Some(decomposed) = crate::decompose::decomposed_search(
            self.db,
            &query.query,
            query.top_k,
            query.debug,
            None,
        ) {
            results = if let Some(ref filter) = query.file_filter {
                let mut r = decomposed.results;
                r.retain(|res| {
                    res.file_path
                        .as_deref()
                        .map(|p| p.contains(filter))
                        .unwrap_or(false)
                });
                r
            } else {
                decomposed.results
            };
            total_expanded = 0;
        } else {
            let expander = StructuralExpander::new(self.db);
            let expanded = expander.expand(
                &fts_results,
                query.expansion_seeds,
                query.max_expansion_depth,
            );
            total_expanded = expanded.len();

            let file_paths = self.load_file_paths();
            let reranker = Reranker::new(self.db, query.debug).for_query(&query.query);
            results = reranker.rerank(&fts_results, &expanded, &[], &file_paths, query.top_k);
        }

        if let Some(ref filter) = query.file_filter {
            results.retain(|r| {
                r.file_path
                    .as_deref()
                    .map(|p| p.contains(filter))
                    .unwrap_or(false)
            });
        }

        for r in &results {
            self.cache.put_source(r.symbol.id, r.symbol.source.clone());
        }

        let blast_result = self.compute_blast(&results, query);

        self.cache.put_results(query_hash, results.clone());

        SearchResult {
            results,
            blast_radius: blast_result,
            total_fts_candidates: total_fts,
            total_expanded,
            from_cache: false,
            search_mode: SearchMode::Fts,
            query_family: family,
            traces: HashMap::new(),
        }
    }

    fn compute_blast(&self, results: &[ScoredSymbol], query: &SearchQuery) -> Option<BlastRadius> {
        if !query.blast_radius {
            return None;
        }
        results.first().map(|top| {
            blast::compute_blast_radius(
                self.db,
                top.symbol.id,
                query.blast_depth,
                BlastDirection::Both,
                None,
            )
            .unwrap_or_else(|_| BlastRadius {
                origin_name: top.symbol.name.clone(),
                origin_kind: top.symbol.kind.as_str().to_string(),
                origin_file: String::new(),
                forward: Vec::new(),
                backward: Vec::new(),
                max_depth: query.blast_depth,
            })
        })
    }

    fn load_file_paths(&self) -> HashMap<i64, String> {
        let conn = self.db.conn();
        let mut stmt = match conn.prepare("SELECT id, path FROM files") {
            Ok(s) => s,
            Err(_) => return HashMap::new(),
        };
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .ok();
        match rows {
            Some(r) => r.flatten().collect(),
            None => HashMap::new(),
        }
    }

    fn promote_exact_matches(&self, result: &mut SearchResult, query: &str) {
        let query_lower = query.to_lowercase();
        let conn = self.db.conn();
        let exact_ids: Vec<i64> = conn
            .prepare("SELECT id FROM symbols WHERE LOWER(name) = ?1")
            .ok()
            .and_then(|mut stmt| {
                let rows: Vec<i64> = stmt
                    .query_map([&query_lower], |row| row.get(0))
                    .ok()?
                    .filter_map(|r| r.ok())
                    .collect();
                Some(rows)
            })
            .unwrap_or_default();

        if exact_ids.is_empty() {
            return;
        }

        let exact_set: std::collections::HashSet<i64> = exact_ids.iter().copied().collect();
        let mut promoted: Vec<ScoredSymbol> = Vec::new();
        let mut rest: Vec<ScoredSymbol> = Vec::new();

        for r in result.results.drain(..) {
            if exact_set.contains(&r.symbol.id) {
                promoted.push(r);
            } else {
                rest.push(r);
            }
        }

        let max_existing = promoted.iter().map(|r| r.score).fold(0.0f64, f64::max);
        let boost = (max_existing + 1.0).max(10.0);

        for r in &mut promoted {
            r.score = r.score.max(boost);
        }

        result.results = promoted;
        result.results.extend(rest);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol::{SymbolBuilder, SymbolKind};

    fn setup_engine() -> (GraphDb, HotCache) {
        let db = GraphDb::open_in_memory().unwrap();
        let fid = db
            .upsert_file("src/auth.ts", "typescript", "abc", 1000, 100)
            .unwrap();

        let symbols = vec![
            ("authenticateUser", SymbolKind::Function, 1, 10),
            ("rateLimitMiddleware", SymbolKind::Function, 12, 25),
            ("AuthService", SymbolKind::Class, 27, 50),
            ("verifyToken", SymbolKind::Function, 52, 65),
        ];

        for (name, kind, start, end) in symbols {
            let sym = SymbolBuilder::new(
                fid,
                name.into(),
                kind,
                format!("fn {}()", name),
                "typescript".into(),
            )
            .lines(start, end)
            .signature(format!("fn {}()", name))
            .build();
            db.insert_symbol(&sym).unwrap();
        }

        let cache = HotCache::with_defaults();
        (db, cache)
    }

    #[test]
    fn test_search_basic() {
        let (db, cache) = setup_engine();
        let engine = SearchEngine::new(&db, &cache);
        assert_eq!(engine.active_mode(), SearchMode::Fts);
        let result = engine.search(&SearchQuery::new("authenticateUser"));
        assert!(!result.results.is_empty());
        assert_eq!(result.results[0].symbol.name, "authenticateUser");
        assert!(!result.from_cache);
        assert_eq!(result.search_mode, SearchMode::Fts);
    }

    #[test]
    fn test_search_cache_hit() {
        let (db, cache) = setup_engine();
        let engine = SearchEngine::new(&db, &cache);

        let q = SearchQuery::new("authenticateUser");
        engine.search(&q);
        let result = engine.search(&q);
        assert!(result.from_cache);
    }

    #[test]
    fn test_search_decomposed() {
        let (db, cache) = setup_engine();
        let engine = SearchEngine::new(&db, &cache);
        let result = engine.search(&SearchQuery::new("rate limit"));
        assert!(!result.results.is_empty());
        assert!(result
            .results
            .iter()
            .any(|r| r.symbol.name == "rateLimitMiddleware"));
    }

    #[test]
    fn test_search_no_results() {
        let (db, cache) = setup_engine();
        let engine = SearchEngine::new(&db, &cache);
        let result = engine.search(&SearchQuery::new("xyzzyNothing"));
        assert!(result.results.is_empty());
    }

    #[test]
    fn test_search_with_debug() {
        let (db, cache) = setup_engine();
        let engine = SearchEngine::new(&db, &cache);
        let result = engine.search(&SearchQuery::new("auth").debug(true));
        assert!(!result.results.is_empty());
        assert!(result.results[0].breakdown.is_some());
    }

    #[test]
    fn test_search_file_filter() {
        let (db, cache) = setup_engine();
        let engine = SearchEngine::new(&db, &cache);
        let result = engine.search(&SearchQuery::new("auth").file_filter("auth"));
        assert!(!result.results.is_empty());

        let cache2 = HotCache::with_defaults();
        let engine2 = SearchEngine::new(&db, &cache2);
        let result2 = engine2.search(&SearchQuery::new("auth").file_filter("nonexistent"));
        assert!(result2.results.is_empty());
    }

    #[test]
    fn test_active_mode_fts_without_indexes() {
        let (db, cache) = setup_engine();
        let engine = SearchEngine::new(&db, &cache);
        assert_eq!(engine.active_mode(), SearchMode::Fts);
    }
}
