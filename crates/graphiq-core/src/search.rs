use std::collections::{HashMap, HashSet};

use crate::blast;
use crate::cache::HotCache;
use crate::cruncher::{self, CruncherIndex, HoloIndex};
use crate::db::GraphDb;
use crate::edge::{BlastDirection, BlastRadius};
use crate::fts::FtsSearch;
use crate::graph::StructuralExpander;
use crate::rerank::{Reranker, ScoredSymbol};
use crate::spectral::{ChannelFingerprint, PredictiveModel, SpectralIndex};
use crate::query_family::{self, QueryFamily, RetrievalPolicy};
use crate::file_router;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    Fts,
    GooberV5,
    Geometric,
    Deformed,
}

impl std::fmt::Display for SearchMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SearchMode::Fts => write!(f, "FTS"),
            SearchMode::GooberV5 => write!(f, "GooberV5"),
            SearchMode::Geometric => write!(f, "Geometric"),
            SearchMode::Deformed => write!(f, "Deformed"),
        }
    }
}

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
    pub search_mode: SearchMode,
    pub query_family: QueryFamily,
}

pub struct SearchEngine<'a> {
    db: &'a GraphDb,
    cache: &'a HotCache,
    cruncher_index: Option<&'a CruncherIndex>,
    holo_index: Option<&'a HoloIndex>,
    spectral_index: Option<&'a SpectralIndex>,
    predictive_model: Option<&'a PredictiveModel>,
    fingerprints: Option<&'a [ChannelFingerprint]>,
    fp_id_to_idx: Option<&'a HashMap<i64, usize>>,
}

impl<'a> SearchEngine<'a> {
    pub fn new(db: &'a GraphDb, cache: &'a HotCache) -> Self {
        Self {
            db,
            cache,
            cruncher_index: None,
            holo_index: None,
            spectral_index: None,
            predictive_model: None,
            fingerprints: None,
            fp_id_to_idx: None,
        }
    }

    pub fn with_goober(mut self, ci: &'a CruncherIndex, hi: &'a HoloIndex) -> Self {
        self.cruncher_index = Some(ci);
        self.holo_index = Some(hi);
        self
    }

    pub fn with_spectral(mut self, si: &'a SpectralIndex) -> Self {
        self.spectral_index = Some(si);
        self
    }

    pub fn with_predictive(mut self, pm: &'a PredictiveModel) -> Self {
        self.predictive_model = Some(pm);
        self
    }

    pub fn with_fingerprints(
        mut self,
        fps: &'a [ChannelFingerprint],
        id_map: &'a HashMap<i64, usize>,
    ) -> Self {
        self.fingerprints = Some(fps);
        self.fp_id_to_idx = Some(id_map);
        self
    }

    pub fn active_mode(&self) -> SearchMode {
        if self.cruncher_index.is_some()
            && self.holo_index.is_some()
            && self.spectral_index.is_some()
            && self.predictive_model.is_some()
            && self.fingerprints.is_some()
            && self.fp_id_to_idx.is_some()
        {
            SearchMode::Deformed
        } else if self.cruncher_index.is_some()
            && self.holo_index.is_some()
            && self.spectral_index.is_some()
        {
            SearchMode::Geometric
        } else if self.cruncher_index.is_some() && self.holo_index.is_some() {
            SearchMode::GooberV5
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
            };
        }

        let policy = RetrievalPolicy::for_family(family);
        let effective_mode = self.effective_mode(&policy);

        if family == QueryFamily::FilePath {
            return self.search_file_path(query, query_hash, family);
        }

        match effective_mode {
            SearchMode::Deformed => {
                self.search_deformed(query, query_hash, &policy, family)
            }
            SearchMode::Geometric => {
                self.search_geometric(query, query_hash, &policy, family)
            }
            SearchMode::GooberV5 => {
                self.search_goober_v5(query, self.cruncher_index.unwrap(), self.holo_index.unwrap(), query_hash, family)
            }
            SearchMode::Fts => {
                self.search_fts_fallback(query, query_hash, family)
            }
        }
    }

    fn effective_mode(&self, policy: &RetrievalPolicy) -> SearchMode {
        let base = self.active_mode();
        if !policy.allow_spectral && base == SearchMode::Deformed {
            if self.cruncher_index.is_some() && self.holo_index.is_some() {
                SearchMode::GooberV5
            } else {
                SearchMode::Fts
            }
        } else if !policy.allow_spectral && base == SearchMode::Geometric {
            if self.cruncher_index.is_some() && self.holo_index.is_some() {
                SearchMode::GooberV5
            } else {
                SearchMode::Fts
            }
        } else if base == SearchMode::Deformed
            && (!policy.allow_predictive || !policy.allow_fingerprints)
        {
            if self.cruncher_index.is_some()
                && self.holo_index.is_some()
                && self.spectral_index.is_some()
            {
                SearchMode::Geometric
            } else if self.cruncher_index.is_some() && self.holo_index.is_some() {
                SearchMode::GooberV5
            } else {
                SearchMode::Fts
            }
        } else {
            base
        }
    }

    fn search_deformed(
        &self,
        query: &SearchQuery,
        query_hash: u64,
        policy: &RetrievalPolicy,
        family: QueryFamily,
    ) -> SearchResult {
        let ci = self.cruncher_index.unwrap();
        let hi = self.holo_index.unwrap();
        let spec = self.spectral_index.unwrap();
        let pm = match (policy.allow_predictive, self.predictive_model) {
            (true, Some(m)) => Some(m),
            _ => None,
        };
        let fps = match (policy.allow_fingerprints, self.fingerprints) {
            (true, Some(f)) => Some(f),
            _ => None,
        };
        let fp_map = match (policy.allow_fingerprints, self.fp_id_to_idx) {
            (true, Some(m)) => Some(m),
            _ => None,
        };

        let fts = FtsSearch::new(self.db);
        let fts_results = fts.search(&query.query, Some(200));
        let total_fts = fts_results.len();

        let bm25_seeds: Vec<(i64, f64)> = fts_results
            .iter()
            .map(|r| (r.symbol.id, r.bm25_score))
            .collect();

        let goober_results = cruncher::geometric_search(
            &query.query,
            ci,
            hi,
            &bm25_seeds,
            spec,
            query.top_k,
            policy.bm25_lock_strength,
            policy.spectral_expansion_seeds,
            policy.spectral_heat_scale,
            50,
            false,
            pm,
            fps,
            fp_map,
            policy.evidence_weight,
        );

        let file_paths = self.load_file_paths();
        let mut results: Vec<ScoredSymbol> = goober_results
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

        Self::apply_diversity_boost(&mut results, policy.diversity_boost);

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
            search_mode: SearchMode::Deformed,
            query_family: family,
        }
    }

    fn search_geometric(
        &self,
        query: &SearchQuery,
        query_hash: u64,
        policy: &RetrievalPolicy,
        family: QueryFamily,
    ) -> SearchResult {
        let ci = self.cruncher_index.unwrap();
        let hi = self.holo_index.unwrap();
        let spec = self.spectral_index.unwrap();

        let fts = FtsSearch::new(self.db);
        let fts_results = fts.search(&query.query, Some(200));
        let total_fts = fts_results.len();

        let bm25_seeds: Vec<(i64, f64)> = fts_results
            .iter()
            .map(|r| (r.symbol.id, r.bm25_score))
            .collect();

        let goober_results = cruncher::geometric_search(
            &query.query,
            ci,
            hi,
            &bm25_seeds,
            spec,
            query.top_k,
            policy.bm25_lock_strength,
            policy.spectral_expansion_seeds,
            policy.spectral_heat_scale,
            50,
            false,
            None,
            None,
            None,
            policy.evidence_weight,
        );

        let file_paths = self.load_file_paths();
        let mut results: Vec<ScoredSymbol> = goober_results
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

        Self::apply_diversity_boost(&mut results, policy.diversity_boost);

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
            search_mode: SearchMode::Geometric,
            query_family: family,
        }
    }

    fn search_file_path(
        &self,
        query: &SearchQuery,
        query_hash: u64,
        family: QueryFamily,
    ) -> SearchResult {
        let file_paths = self.load_file_paths();
        let result = file_router::file_route_query(self.db, &query.query, query.top_k);

        let results: Vec<ScoredSymbol> = result.ranked_symbols
            .into_iter()
            .filter_map(|(sym, score)| {
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
            total_fts_candidates: result.file_matches.len(),
            total_expanded: 0,
            from_cache: false,
            search_mode: SearchMode::Fts,
            query_family: family,
        }
    }

    fn search_goober_v5(
        &self,
        query: &SearchQuery,
        ci: &CruncherIndex,
        hi: &HoloIndex,
        query_hash: u64,
        family: QueryFamily,
    ) -> SearchResult {
        let fts = FtsSearch::new(self.db);
        let fts_results = fts.search(&query.query, Some(200));
        let total_fts = fts_results.len();

        let bm25_seeds: Vec<(i64, f64)> = fts_results
            .iter()
            .map(|r| (r.symbol.id, r.bm25_score))
            .collect();

        let goober_results = cruncher::goober_v5_search(
            &query.query,
            ci,
            hi,
            &bm25_seeds,
            query.top_k,
        );

        let file_paths = self.load_file_paths();

        let mut results: Vec<ScoredSymbol> = goober_results
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

        let policy = RetrievalPolicy::for_family(family);
        Self::apply_diversity_boost(&mut results, policy.diversity_boost);

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
            search_mode: SearchMode::GooberV5,
            query_family: family,
        }
    }

    fn search_fts_fallback(
        &self,
        query: &SearchQuery,
        query_hash: u64,
        family: QueryFamily,
    ) -> SearchResult {
        let mut results: Vec<ScoredSymbol>;
        let mut total_fts: usize;
        let mut total_expanded: usize;

        let fts = FtsSearch::new(self.db);
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

    fn apply_diversity_boost(results: &mut Vec<ScoredSymbol>, diversity_boost: f64) {
        if diversity_boost <= 0.0 || results.len() <= 1 {
            return;
        }
        let mut seen_files: HashSet<i64> = HashSet::new();
        for result in results.iter_mut() {
            let file_id = result.symbol.file_id;
            if seen_files.contains(&file_id) {
                let penalty = 1.0 / (1.0 + diversity_boost);
                result.score *= penalty;
            } else {
                seen_files.insert(file_id);
            }
        }
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
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
