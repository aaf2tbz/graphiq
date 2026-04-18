use std::collections::HashMap;

use crate::blast;
use crate::cache::HotCache;
use crate::db::GraphDb;
use crate::directory_expand::DirectoryExpander;
use crate::edge::{BlastDirection, BlastRadius};
use crate::evidence::{self, EvidenceIndex};
use crate::fts::FtsSearch;
use crate::graph::StructuralExpander;
use crate::hrr::HrrIndex;
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
    hrr_index: Option<&'a HrrIndex>,
    evidence_index: Option<&'a EvidenceIndex>,
}

impl<'a> SearchEngine<'a> {
    pub fn new(db: &'a GraphDb, cache: &'a HotCache) -> Self {
        Self {
            db,
            cache,
            hrr_index: None,
            evidence_index: None,
        }
    }

    pub fn with_hrr(mut self, hrr: &'a HrrIndex) -> Self {
        self.hrr_index = Some(hrr);
        self
    }

    pub fn with_evidence(mut self, ev: &'a EvidenceIndex) -> Self {
        self.evidence_index = Some(ev);
        self
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

        let q_tokens: Vec<String> = query
            .query
            .split_whitespace()
            .map(|t| t.to_lowercase())
            .filter(|t| t.len() >= 2)
            .collect();
        let is_nl = crate::rerank::is_nl_query(&q_tokens)
            && !query.query.to_lowercase().starts_with("all ")
            && !query.query.to_lowercase().starts_with("every ");

        if is_nl && self.evidence_index.is_some() {
            let ev_idx = self.evidence_index.unwrap();
            let file_paths = self.load_file_paths();
            let evidence_hits =
                crate::evidence::evidence_search(&query.query, ev_idx, query.top_k * 3);

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

            let reranker = Reranker::new(self.db, query.debug).for_query(&query.query);
            results = reranker.rerank(&fts_results, &expanded, &[], &file_paths, query.top_k);

            let bm25_ids: Vec<i64> = results.iter().take(10).map(|r| r.symbol.id).collect();
            let bm25_scores: Vec<f64> = results.iter().take(10).map(|r| r.score).collect();

            let evidence_reranked =
                crate::evidence::evidence_rerank(&query.query, &bm25_ids, &bm25_scores, ev_idx);

            let ev_raw_max = evidence_hits.iter().map(|(_, s)| *s).fold(0.0f64, f64::max);
            let ev_has_good_hits = evidence_hits.iter().take(3).any(|(_, s)| *s > 1.0);

            let ev_weight = if ev_has_good_hits { 3.0 } else { 1.5 };
            let ev_rerank_weight = if ev_has_good_hits { 2.0 } else { 1.0 };
            let bm25_weight = if ev_has_good_hits { 1.0 } else { 2.0 };

            let mut fused: std::collections::HashMap<i64, f64> = std::collections::HashMap::new();

            let ev_max = ev_raw_max.max(1e-10);
            for (rank, (id, score)) in evidence_hits.iter().enumerate() {
                let normalized = score / ev_max;
                let rank_decay = 1.0 / (1.0 + rank as f64 * 0.3);
                *fused.entry(*id).or_insert(0.0) += ev_weight * normalized * rank_decay;
            }

            let ev_rerank_max = evidence_reranked
                .iter()
                .map(|(_, s)| *s)
                .fold(0.0f64, f64::max)
                .max(1e-10);
            for (rank, (id, score)) in evidence_reranked.iter().enumerate() {
                let normalized = score / ev_rerank_max;
                let rank_decay = 1.0 / (1.0 + rank as f64 * 0.3);
                *fused.entry(*id).or_insert(0.0) += ev_rerank_weight * normalized * rank_decay;
            }

            let bm25_max = bm25_scores
                .iter()
                .cloned()
                .fold(0.0f64, f64::max)
                .max(1e-10);
            for (rank, (&id, &score)) in bm25_ids.iter().zip(bm25_scores.iter()).enumerate() {
                let normalized = score / bm25_max;
                let rank_decay = 1.0 / (1.0 + rank as f64 * 0.3);
                *fused.entry(id).or_insert(0.0) += bm25_weight * normalized * rank_decay;
            }

            let mut merged: Vec<(i64, f64)> = fused.into_iter().collect();
            merged.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

            results = merged
                .into_iter()
                .take(query.top_k)
                .filter_map(|(id, score)| {
                    let sym = self.db.get_symbol(id).ok()??;
                    let fp = file_paths.get(&sym.file_id).cloned();
                    Some(ScoredSymbol {
                        symbol: sym,
                        score,
                        breakdown: None,
                        is_fts_hit: false,
                        file_path: fp,
                    })
                })
                .collect();
        } else if let Some(decomposed) = crate::decompose::decomposed_search(
            self.db,
            &query.query,
            query.top_k,
            query.debug,
            self.hrr_index,
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

            if let Some(ev_idx) = self.evidence_index {
                let q_tokens: Vec<String> = query
                    .query
                    .split_whitespace()
                    .map(|t| t.to_lowercase())
                    .filter(|t| t.len() >= 2)
                    .collect();
                let is_nl = crate::rerank::is_nl_query(&q_tokens)
                    && !query.query.to_lowercase().starts_with("all ")
                    && !query.query.to_lowercase().starts_with("every ");

                if is_nl {
                    let file_paths = self.load_file_paths();
                    let evidence_hits =
                        crate::evidence::evidence_search(&query.query, ev_idx, query.top_k * 3);

                    let bm25_ids: Vec<i64> = results.iter().take(10).map(|r| r.symbol.id).collect();
                    let bm25_scores: Vec<f64> = results.iter().take(10).map(|r| r.score).collect();

                    let evidence_reranked = crate::evidence::evidence_rerank(
                        &query.query,
                        &bm25_ids,
                        &bm25_scores,
                        ev_idx,
                    );

                    let mut fused: std::collections::HashMap<i64, f64> =
                        std::collections::HashMap::new();

                    let ev_max = evidence_hits
                        .iter()
                        .map(|(_, s)| *s)
                        .fold(0.0f64, f64::max)
                        .max(1e-10);
                    for (rank, (id, score)) in evidence_hits.iter().enumerate() {
                        let normalized = score / ev_max;
                        let rank_decay = 1.0 / (1.0 + rank as f64 * 0.3);
                        *fused.entry(*id).or_insert(0.0) += 3.0 * normalized * rank_decay;
                    }

                    let ev_rerank_max = evidence_reranked
                        .iter()
                        .map(|(_, s)| *s)
                        .fold(0.0f64, f64::max)
                        .max(1e-10);
                    for (rank, (id, score)) in evidence_reranked.iter().enumerate() {
                        let normalized = score / ev_rerank_max;
                        let rank_decay = 1.0 / (1.0 + rank as f64 * 0.3);
                        *fused.entry(*id).or_insert(0.0) += 2.0 * normalized * rank_decay;
                    }

                    let bm25_max = bm25_scores
                        .iter()
                        .cloned()
                        .fold(0.0f64, f64::max)
                        .max(1e-10);
                    for (rank, (&id, &score)) in bm25_ids.iter().zip(bm25_scores.iter()).enumerate()
                    {
                        let normalized = score / bm25_max;
                        let rank_decay = 1.0 / (1.0 + rank as f64 * 0.3);
                        *fused.entry(id).or_insert(0.0) += 1.0 * normalized * rank_decay;
                    }

                    let mut merged: Vec<(i64, f64)> = fused.into_iter().collect();
                    merged.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

                    results = merged
                        .into_iter()
                        .take(query.top_k)
                        .filter_map(|(id, score)| {
                            let sym = self.db.get_symbol(id).ok()??;
                            let fp = file_paths.get(&sym.file_id).cloned();
                            Some(ScoredSymbol {
                                symbol: sym,
                                score,
                                breakdown: None,
                                is_fts_hit: false,
                                file_path: fp,
                            })
                        })
                        .collect();
                } else if let Some(hrr_idx) = self.hrr_index {
                    let file_paths = self.load_file_paths();
                    let bm25_ids: Vec<i64> = results.iter().take(5).map(|r| r.symbol.id).collect();

                    if bm25_ids.len() >= 2 {
                        let (biv_expanded, _) =
                            crate::hrr::hrr_bivector_expand_scored(&bm25_ids, hrr_idx, 50);

                        let mut rrf: std::collections::HashMap<i64, f64> =
                            std::collections::HashMap::new();
                        let k_rrf = 60.0;
                        for (rank, &id) in bm25_ids.iter().enumerate() {
                            *rrf.entry(id).or_insert(0.0) += 1.0 / (k_rrf + rank as f64 + 1.0);
                        }
                        for (rank, (id, _)) in biv_expanded.iter().enumerate() {
                            *rrf.entry(*id).or_insert(0.0) += 1.0 / (k_rrf + rank as f64 + 1.0);
                        }

                        let mut merged: Vec<(i64, f64)> = rrf.into_iter().collect();
                        merged.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
                        let merged_ids: Vec<i64> =
                            merged.iter().take(50).map(|(id, _)| *id).collect();
                        let merged_scores: Vec<f64> =
                            merged.iter().take(50).map(|(_, s)| *s).collect();

                        let reranked = crate::hrr::hrr_rerank(
                            &query.query,
                            &merged_ids,
                            &merged_scores,
                            hrr_idx,
                        );
                        let top_n: Vec<(i64, f64)> =
                            reranked.into_iter().take(query.top_k).collect();

                        results = top_n
                            .into_iter()
                            .filter_map(|(id, score)| {
                                let sym = self.db.get_symbol(id).ok()??;
                                let fp = file_paths.get(&sym.file_id).cloned();
                                Some(ScoredSymbol {
                                    symbol: sym,
                                    score,
                                    breakdown: None,
                                    is_fts_hit: false,
                                    file_path: fp,
                                })
                            })
                            .collect();
                    }
                }
            } else if let Some(hrr_idx) = self.hrr_index {
                let file_paths = self.load_file_paths();
                let bm25_ids: Vec<i64> = results.iter().take(5).map(|r| r.symbol.id).collect();

                if bm25_ids.len() >= 2 {
                    let q_tokens: Vec<String> = query
                        .query
                        .split_whitespace()
                        .map(|t| t.to_lowercase())
                        .filter(|t| t.len() >= 2)
                        .collect();
                    let is_nl = crate::rerank::is_nl_query(&q_tokens)
                        && !query.query.to_lowercase().starts_with("all ")
                        && !query.query.to_lowercase().starts_with("every ");

                    if is_nl {
                        let concrete_terms = crate::decompose::extract_concrete_terms(&query.query);
                        let holo_hits = if !concrete_terms.is_empty() {
                            let hrr_query = concrete_terms.join(" ");
                            crate::hrr::hrr_holographic_search(&hrr_query, hrr_idx, 50)
                        } else {
                            crate::hrr::hrr_holographic_search(&query.query, hrr_idx, 50)
                        };

                        let all_seed_ids: Vec<i64> =
                            results.iter().take(20).map(|r| r.symbol.id).collect();
                        let (biv_expanded, coherence) =
                            crate::hrr::hrr_bivector_expand_scored(&all_seed_ids, hrr_idx, 50);

                        let fractal_results =
                            crate::hrr::hrr_fractal_attract(&all_seed_ids, hrr_idx, 3, 30);

                        let mut rrf: std::collections::HashMap<i64, f64> =
                            std::collections::HashMap::new();
                        let k = 60.0;
                        for (rank, &id) in bm25_ids.iter().enumerate() {
                            *rrf.entry(id).or_insert(0.0) += 2.0 / (k + rank as f64 + 1.0);
                        }
                        for (rank, (id, _)) in holo_hits.iter().enumerate() {
                            *rrf.entry(*id).or_insert(0.0) += 1.5 / (k + rank as f64 + 1.0);
                        }
                        for (rank, (id, _)) in biv_expanded.iter().enumerate() {
                            let weight = 0.3 + 0.7 * coherence.max(0.0).min(1.0);
                            *rrf.entry(*id).or_insert(0.0) += weight / (k + rank as f64 + 1.0);
                        }
                        for (rank, (id, _)) in fractal_results.iter().enumerate() {
                            *rrf.entry(*id).or_insert(0.0) += 0.3 / (k + rank as f64 + 1.0);
                        }

                        let mut merged: Vec<(i64, f64)> = rrf.into_iter().collect();
                        merged.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
                        let merged_ids: Vec<i64> =
                            merged.iter().take(50).map(|(id, _)| *id).collect();
                        let merged_scores: Vec<f64> =
                            merged.iter().take(50).map(|(_, s)| *s).collect();

                        let reranked = crate::hrr::hrr_holographic_rerank(
                            &query.query,
                            &merged_ids,
                            &merged_scores,
                            hrr_idx,
                        );
                        let top_n: Vec<(i64, f64)> =
                            reranked.into_iter().take(query.top_k).collect();

                        results = top_n
                            .into_iter()
                            .filter_map(|(id, score)| {
                                let sym = self.db.get_symbol(id).ok()??;
                                let fp = file_paths.get(&sym.file_id).cloned();
                                Some(ScoredSymbol {
                                    symbol: sym,
                                    score,
                                    breakdown: None,
                                    is_fts_hit: false,
                                    file_path: fp,
                                })
                            })
                            .collect();
                        total_expanded += biv_expanded.len() + holo_hits.len();
                    } else {
                        let (biv_expanded, _) =
                            crate::hrr::hrr_bivector_expand_scored(&bm25_ids, hrr_idx, 50);

                        let mut rrf: std::collections::HashMap<i64, f64> =
                            std::collections::HashMap::new();
                        let k = 60.0;
                        for (rank, &id) in bm25_ids.iter().enumerate() {
                            *rrf.entry(id).or_insert(0.0) += 1.0 / (k + rank as f64 + 1.0);
                        }
                        for (rank, (id, _)) in biv_expanded.iter().enumerate() {
                            *rrf.entry(*id).or_insert(0.0) += 1.0 / (k + rank as f64 + 1.0);
                        }

                        let mut merged: Vec<(i64, f64)> = rrf.into_iter().collect();
                        merged.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
                        let merged_ids: Vec<i64> =
                            merged.iter().take(50).map(|(id, _)| *id).collect();
                        let merged_scores: Vec<f64> =
                            merged.iter().take(50).map(|(_, s)| *s).collect();

                        let reranked = crate::hrr::hrr_rerank(
                            &query.query,
                            &merged_ids,
                            &merged_scores,
                            hrr_idx,
                        );
                        let top_n: Vec<(i64, f64)> =
                            reranked.into_iter().take(query.top_k).collect();

                        results = top_n
                            .into_iter()
                            .filter_map(|(id, score)| {
                                let sym = self.db.get_symbol(id).ok()??;
                                let fp = file_paths.get(&sym.file_id).cloned();
                                Some(ScoredSymbol {
                                    symbol: sym,
                                    score,
                                    breakdown: None,
                                    is_fts_hit: false,
                                    file_path: fp,
                                })
                            })
                            .collect();
                        total_expanded += biv_expanded.len();
                    }
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

    fn structural_prf(&self, seed_ids: &[i64], _query: &str, top_k: usize) -> Vec<(i64, f64)> {
        if seed_ids.is_empty() {
            return Vec::new();
        }

        let seed_set: std::collections::HashSet<i64> = seed_ids.iter().copied().collect();
        let mut vote_count: std::collections::HashMap<i64, f64> = std::collections::HashMap::new();

        for &seed_id in seed_ids {
            if let Ok(edges) = self.db.edges_from(seed_id) {
                for edge in &edges {
                    if seed_set.contains(&edge.target_id) {
                        continue;
                    }
                    let weight = match edge.kind.as_str() {
                        "Calls" => 2.0,
                        "Contains" => 1.5,
                        "References" => 1.0,
                        "Imports" => 1.0,
                        _ => 0.5,
                    };
                    *vote_count.entry(edge.target_id).or_insert(0.0) += weight;
                }
            }
            if let Ok(edges) = self.db.edges_to(seed_id) {
                for edge in &edges {
                    if seed_set.contains(&edge.source_id) {
                        continue;
                    }
                    let weight = match edge.kind.as_str() {
                        "Calls" => 2.0,
                        "Contains" => 1.5,
                        "References" => 1.0,
                        "Imports" => 1.0,
                        _ => 0.5,
                    };
                    *vote_count.entry(edge.source_id).or_insert(0.0) += weight;
                }
            }
        }

        let max_votes = vote_count.values().cloned().fold(0.0f64, f64::max).max(1.0);

        let mut scored: Vec<(i64, f64)> = vote_count
            .into_iter()
            .filter(|(_, v)| *v >= 2.0)
            .map(|(id, v)| (id, v / max_votes))
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        scored.truncate(top_k);
        scored
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
