use std::collections::HashMap;

use crate::blast;
use crate::cache::HotCache;
use crate::db::GraphDb;
use crate::directory_expand::DirectoryExpander;
use crate::edge::{BlastDirection, BlastRadius};
use crate::fts::FtsSearch;
use crate::graph::StructuralExpander;
use crate::rerank::{Reranker, ScoredSymbol};

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
        }
    }

    pub fn top_k(mut self, k: usize) -> Self {
        self.top_k = k;
        self
    }

    pub fn debug(mut self, d: bool) -> Self {
        self.debug = d;
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
}

pub struct SearchEngine<'a> {
    db: &'a GraphDb,
    cache: &'a HotCache,
}

impl<'a> SearchEngine<'a> {
    pub fn new(db: &'a GraphDb, cache: &'a HotCache) -> Self {
        Self { db, cache }
    }

    pub fn search(&self, query: &SearchQuery) -> SearchResult {
        let query_hash = HotCache::compute_query_hash(&query.query, query.top_k);

        if let Some(cached) = self.cache.get_results(query_hash) {
            return SearchResult {
                results: cached,
                blast_radius: None,
                total_fts_candidates: 0,
                total_expanded: 0,
                from_cache: true,
            };
        }

        let mut results: Vec<ScoredSymbol>;
        let mut total_fts: usize;
        let mut total_expanded: usize;

        if let Some(decomposed) =
            crate::decompose::decomposed_search(self.db, &query.query, query.top_k, query.debug)
        {
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
            total_fts = decomposed.subqueries.len();
            total_expanded = 0;
        } else {
            let fts = FtsSearch::new(self.db);
            let fts_results = fts.search(&query.query, Some(200));
            total_fts = fts_results.len();

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

            if is_cross_cutting_query(&query.query) {
                let existing_ids: std::collections::HashSet<i64> =
                    results.iter().map(|r| r.symbol.id).collect();
                let dir_expander = DirectoryExpander::new(self.db);

                let cross_pkg = dir_expander.expand_cross_package(
                    &fts_results,
                    &existing_ids,
                    20,
                    &query.query,
                );

                if !cross_pkg.is_empty() {
                    let best_fts_score = fts_results
                        .iter()
                        .map(|r| r.bm25_score)
                        .fold(0.0f64, f64::max);
                    for sib in &cross_pkg {
                        let fp = file_paths.get(&sib.symbol.file_id).cloned();
                        results.push(ScoredSymbol {
                            symbol: sib.symbol.clone(),
                            score: best_fts_score * sib.proximity,
                            breakdown: None,
                            is_fts_hit: false,
                            file_path: fp,
                        });
                    }
                    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
                    results.truncate(query.top_k);
                } else if let Some(decomposed) = crate::decompose::decomposed_search_cross_cutting(
                    self.db,
                    &query.query,
                    query.top_k,
                    query.debug,
                ) {
                    results = decomposed.results;
                    total_fts = decomposed.subqueries.len();
                    total_expanded = 0;
                }
            }
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

        let blast_result = if query.blast_radius {
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
        } else {
            None
        };

        self.cache.put_results(query_hash, results.clone());

        SearchResult {
            results,
            blast_radius: blast_result,
            total_fts_candidates: total_fts,
            total_expanded,
            from_cache: false,
        }
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
}

fn is_cross_cutting_query(query: &str) -> bool {
    let lower = query.to_lowercase();
    lower.starts_with("all ") || lower.starts_with("every ")
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
        let result = engine.search(&SearchQuery::new("authenticateUser"));
        assert!(!result.results.is_empty());
        assert_eq!(result.results[0].symbol.name, "authenticateUser");
        assert!(!result.from_cache);
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
}
