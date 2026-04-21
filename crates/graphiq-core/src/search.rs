use std::collections::{HashMap, HashSet};

use crate::blast;
use crate::cache::HotCache;
use crate::cruncher::CruncherIndex;
use crate::holo_name::HoloIndex;
use crate::db::GraphDb;
use crate::edge::{BlastDirection, BlastRadius};
use crate::fts::{FtsConfig, FtsSearch};
use crate::graph::StructuralExpander;
use crate::rerank::{Reranker, ScoredSymbol};
use crate::spectral::{ChannelFingerprint, PredictiveModel, SpectralIndex};
use crate::query_family::{self, QueryFamily, RetrievalPolicy};
use crate::trace::RetrievalTrace;
use crate::self_model::RepoSelfModel;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    Fts,
    GooberV5,
    Geometric,
    Deformed,
    CARE,
}

impl std::fmt::Display for SearchMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SearchMode::Fts => write!(f, "FTS"),
            SearchMode::GooberV5 => write!(f, "GooberV5"),
            SearchMode::Geometric => write!(f, "Geometric"),
            SearchMode::Deformed => write!(f, "Deformed"),
            SearchMode::CARE => write!(f, "CARE"),
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
    holo_index: Option<&'a HoloIndex>,
    spectral_index: Option<&'a SpectralIndex>,
    predictive_model: Option<&'a PredictiveModel>,
    fingerprints: Option<&'a [ChannelFingerprint]>,
    fp_id_to_idx: Option<&'a HashMap<i64, usize>>,
    self_model: Option<&'a RepoSelfModel>,
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
            self_model: None,
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

    pub fn with_self_model(mut self, model: &'a RepoSelfModel) -> Self {
        self.self_model = Some(model);
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

    fn route_mode(&self, family: QueryFamily) -> SearchMode {
        let has_spectral = self.spectral_index.is_some()
            && self.predictive_model.is_some()
            && self.fingerprints.is_some();
        let has_geometric = self.spectral_index.is_some();
        let has_goober = self.cruncher_index.is_some() && self.holo_index.is_some();

        match family {
            QueryFamily::SymbolExact | QueryFamily::SymbolPartial => {
                if has_goober { SearchMode::GooberV5 }
                else { SearchMode::Fts }
            }
            QueryFamily::ErrorDebug => {
                if has_spectral { SearchMode::Deformed }
                else if has_geometric { SearchMode::Geometric }
                else if has_goober { SearchMode::GooberV5 }
                else { SearchMode::Fts }
            }
            QueryFamily::NaturalAbstract | QueryFamily::CrossCuttingSet => {
                if has_spectral { SearchMode::Deformed }
                else if has_geometric { SearchMode::Geometric }
                else if has_goober { SearchMode::GooberV5 }
                else { SearchMode::Fts }
            }
            QueryFamily::NaturalDescriptive | QueryFamily::Relationship => {
                if has_spectral { SearchMode::Geometric }
                else if has_goober { SearchMode::GooberV5 }
                else { SearchMode::Fts }
            }
            QueryFamily::FilePath => {
                if has_spectral { SearchMode::Geometric }
                else if has_goober { SearchMode::GooberV5 }
                else { SearchMode::Fts }
            }
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
                search_mode: self.route_mode(family),
                query_family: family,
                traces: HashMap::new(),
            };
        }

        let policy = RetrievalPolicy::for_family(family);
        let mode = self.route_mode(family);

        let mut result = match mode {
            SearchMode::Deformed | SearchMode::Geometric | SearchMode::GooberV5 => {
                self.search_unified(query, query_hash, &policy, family)
            }
            SearchMode::Fts => self.search_fts_fallback(query, query_hash, family),
            SearchMode::CARE => self.search_unified(query, query_hash, &policy, family),
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
        policy: &RetrievalPolicy,
        family: QueryFamily,
    ) -> SearchResult {
        let ci = self.cruncher_index.unwrap();
        let hi = self.holo_index.unwrap();

        let seed_config = crate::seeds::SeedConfig::for_family(family);
        let (seeds, total_fts, _bm25_original, source_scan_start) = crate::seeds::generate_seeds(
            self.db, &query.query, &seed_config, self.self_model,
        );

        let has_spectral = self.spectral_index.is_some()
            && self.predictive_model.is_some()
            && self.fingerprints.is_some();

        let pipeline_config = crate::pipeline::PipelineConfig {
            top_k: query.top_k,
            use_heat_diffusion: self.spectral_index.is_some(),
            heat_t: policy.spectral_heat_scale,
            cheb_order: policy.spectral_expansion_seeds,
            walk_weight: policy.evidence_weight,
            heat_top_k: 50,
            predictive: if policy.allow_predictive { self.predictive_model } else { None },
            fingerprints: if policy.allow_fingerprints { self.fingerprints } else { None },
            fp_id_to_idx: if policy.allow_fingerprints { self.fp_id_to_idx } else { None },
            evidence_weight: policy.evidence_weight,
        };

        let raw_results = crate::pipeline::unified_search(
            &query.query,
            ci,
            hi,
            &seeds,
            self.spectral_index,
            &pipeline_config,
            source_scan_start,
        );

        let file_paths = self.load_file_paths();
        let mut results: Vec<ScoredSymbol> = raw_results
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

        let mode = if has_spectral {
            SearchMode::Deformed
        } else {
            SearchMode::GooberV5
        };

        SearchResult {
            results,
            blast_radius: blast_result,
            total_fts_candidates: total_fts,
            total_expanded: 0,
            from_cache: false,
            search_mode: mode,
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
        result.results.truncate(result.results.len().max(10).min(result.results.len()));
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
