//! Candidate scoring — composite ranking from multiple signals.
//!
//! Combines BM25 score, term coverage, name matching, walk evidence, name
//! overlap, and neighbor fingerprints into a single score. Weights differ
//! between seed candidates and walk discoveries, and are configured per
//! query family via `ScoreConfig`.
//!
//! Key function: [`score_candidates`] — scores all candidates and applies
//! BM25 confidence lock, file diversity cap, and exact match promotion.

use std::collections::HashSet;

use crate::cruncher::{CruncherIndex, QueryTerm};
use crate::cruncher::{kind_boost, test_penalty};
use crate::query_family::QueryFamily;

pub struct Candidate {
    pub idx: usize,
    pub bm25_score: f64,
    pub coverage_score: f64,
    pub coverage_count: usize,
    pub name_score: f64,
    pub is_seed: bool,
    pub walk_evidence: f64,
    pub seed_paths: HashSet<usize>,
    pub name_overlap: f64,
    pub neighbor_score: f64,
    pub alias_score: f64,
}

pub struct ScoreConfig {
    pub bm25_w: f64,
    pub cov_w: f64,
    pub name_w: f64,
    pub walk_weight: f64,
    pub name_overlap_gate: f64,
    pub name_overlap_max_w: f64,
    pub walk_enabled: bool,
    pub name_overlap_enabled: bool,
    pub specificity_enabled: bool,
    pub diversity_max_per_file: usize,
}

impl ScoreConfig {
    pub fn for_family(family: QueryFamily) -> Self {
        match family {
            QueryFamily::SymbolExact => Self {
                bm25_w: 5.0,
                cov_w: 0.8,
                name_w: 1.0,
                walk_weight: 0.5,
                name_overlap_gate: 0.4,
                name_overlap_max_w: 1.5,
                walk_enabled: false,
                name_overlap_enabled: true,
                specificity_enabled: false,
                diversity_max_per_file: 3,
            },
            QueryFamily::SymbolPartial => Self {
                bm25_w: 4.5,
                cov_w: 1.0,
                name_w: 1.2,
                walk_weight: 0.8,
                name_overlap_gate: 0.3,
                name_overlap_max_w: 1.8,
                walk_enabled: true,
                name_overlap_enabled: true,
                specificity_enabled: false,
                diversity_max_per_file: 3,
            },
            QueryFamily::FilePath => Self {
                bm25_w: 3.0,
                cov_w: 1.5,
                name_w: 0.8,
                walk_weight: 0.3,
                name_overlap_gate: 0.5,
                name_overlap_max_w: 1.0,
                walk_enabled: false,
                name_overlap_enabled: false,
                specificity_enabled: false,
                diversity_max_per_file: 5,
            },
            QueryFamily::ErrorDebug => Self {
                bm25_w: 3.5,
                cov_w: 1.5,
                name_w: 1.5,
                walk_weight: 1.2,
                name_overlap_gate: 0.25,
                name_overlap_max_w: 2.0,
                walk_enabled: true,
                name_overlap_enabled: true,
                specificity_enabled: true,
                diversity_max_per_file: 3,
            },
            QueryFamily::NaturalDescriptive => Self {
                bm25_w: 3.0,
                cov_w: 1.5,
                name_w: 2.0,
                walk_weight: 1.0,
                name_overlap_gate: 0.25,
                name_overlap_max_w: 2.0,
                walk_enabled: true,
                name_overlap_enabled: true,
                specificity_enabled: true,
                diversity_max_per_file: 3,
            },
            QueryFamily::NaturalAbstract => Self {
                bm25_w: 2.5,
                cov_w: 2.0,
                name_w: 1.5,
                walk_weight: 1.5,
                name_overlap_gate: 0.2,
                name_overlap_max_w: 2.0,
                walk_enabled: true,
                name_overlap_enabled: true,
                specificity_enabled: true,
                diversity_max_per_file: 2,
            },
            QueryFamily::CrossCuttingSet => Self {
                bm25_w: 2.0,
                cov_w: 2.0,
                name_w: 1.0,
                walk_weight: 1.5,
                name_overlap_gate: 0.3,
                name_overlap_max_w: 1.5,
                walk_enabled: true,
                name_overlap_enabled: false,
                specificity_enabled: true,
                diversity_max_per_file: 1,
            },
            QueryFamily::Relationship => Self {
                bm25_w: 3.0,
                cov_w: 1.5,
                name_w: 1.0,
                walk_weight: 2.0,
                name_overlap_gate: 0.3,
                name_overlap_max_w: 1.5,
                walk_enabled: true,
                name_overlap_enabled: false,
                specificity_enabled: true,
                diversity_max_per_file: 3,
            },
        }
    }
}

impl Default for ScoreConfig {
    fn default() -> Self {
        Self::for_family(QueryFamily::NaturalDescriptive)
    }
}

pub fn score_candidates(
    candidates: &std::collections::BTreeMap<usize, Candidate>,
    query_terms: &[QueryTerm],
    config: &ScoreConfig,
    idx: &CruncherIndex,
) -> Vec<(usize, f64)> {
    let n_qt = query_terms.len();
    let idf_sum: f64 = query_terms.iter().map(|qt| qt.idf).sum();

    let mut cand_vec: Vec<&Candidate> = candidates.values().collect();
    cand_vec.sort_by_key(|c| c.idx);

    let mut scored: Vec<(usize, f64)> = cand_vec
        .into_iter()
        .filter_map(|c| {
            if !c.is_seed && c.seed_paths.is_empty() {
                return None;
            }

            let cov_norm = if idf_sum > 0.0 { c.coverage_score / idf_sum } else { 0.0 };
            let name_norm = if idf_sum > 0.0 { c.name_score / idf_sum } else { 0.0 };
            let walk_norm = if idf_sum > 0.0 { c.walk_evidence / idf_sum } else { 0.0 };

            let query_specificity = if !query_terms.is_empty() {
                query_terms.iter().filter(|qt| qt.idf > 1.0).count() as f64
                    / query_terms.len() as f64
            } else {
                0.0
            };

            let base = if c.is_seed {
                let bm25_w_adj = if config.specificity_enabled {
                    config.bm25_w * (1.0 - 0.3 * query_specificity)
                } else {
                    config.bm25_w
                };
                let cov_w_adj = if config.specificity_enabled {
                    config.cov_w * (1.0 + 0.5 * query_specificity)
                } else {
                    config.cov_w
                };
                let cov_cap = cov_norm.min(0.4);
                let name_cap = name_norm.min(0.5);
                bm25_w_adj * c.bm25_score + cov_w_adj * cov_cap + config.name_w * name_cap
            } else {
                1.5 * cov_norm + 2.0 * name_norm + config.walk_weight * walk_norm
            };

            let coverage_frac = if n_qt > 0 {
                c.coverage_count as f64 / n_qt as f64
            } else {
                0.0
            };

            let name_overlap_additive = if config.name_overlap_enabled && c.name_overlap > config.name_overlap_gate {
                let excess = (c.name_overlap - config.name_overlap_gate)
                    / (1.0 - config.name_overlap_gate);
                config.name_overlap_max_w * query_specificity * excess
            } else {
                0.0
            };

            let neighbor_boost = if c.neighbor_score > 0.0 && config.walk_enabled {
                let neighbor_norm = c.neighbor_score / idf_sum.max(1e-10);
                if neighbor_norm > 0.1 {
                    0.5 * neighbor_norm
                } else {
                    0.0
                }
            } else {
                0.0
            };

            let alias_boost = if c.alias_score > 0.0 {
                let alias_norm = c.alias_score / idf_sum.max(1e-10);
                if alias_norm > 0.15 {
                    1.0 * alias_norm
                } else {
                    0.0
                }
            } else {
                0.0
            };

            let seed_bonus = if c.is_seed { 1.15 } else { 1.0 };
            let kb = kind_boost(&idx.symbol_kinds[c.idx]);
            let tp = test_penalty(&idx.file_paths, idx.symbol_file_ids[c.idx]);

            let raw = (base + name_overlap_additive + neighbor_boost + alias_boost)
                * coverage_frac.powf(0.3)
                * seed_bonus
                * kb
                * tp;

            if raw > 0.0 { Some((c.idx, raw)) } else { None }
        })
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap().then(a.0.cmp(&b.0)));
    scored
}

pub fn apply_bm25_lock(
    scored: &mut Vec<(usize, f64)>,
    bm25_seeds: &[(i64, f64)],
    query_terms: &[QueryTerm],
    idx: &CruncherIndex,
) {
    let bm25_confident = bm25_seeds.len() >= 2
        && bm25_seeds[0].1 / bm25_seeds[1].1.max(1e-10) > 1.2;
    if bm25_confident {
        if let Some(&lock_i) = bm25_seeds
            .first()
            .and_then(|(id, _)| idx.id_to_idx.get(id))
        {
            let lock_name = idx.symbol_names[lock_i].to_lowercase();
            let has_name_match = query_terms.iter().any(|qt| lock_name.contains(&qt.text));
            if has_name_match {
                if let Some(pos) = scored.iter().position(|(i, _)| *i == lock_i) {
                    if pos > 0 {
                        let (li, ls) = scored.remove(pos);
                        scored.insert(0, (li, ls + 1e6));
                    }
                }
            }
        }
    }
}
