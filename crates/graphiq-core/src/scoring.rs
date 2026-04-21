use std::collections::HashSet;

use crate::cruncher::{CruncherIndex, QueryTerm};
use crate::cruncher::{kind_boost, test_penalty};

pub struct Candidate {
    pub idx: usize,
    pub bm25_score: f64,
    pub coverage_score: f64,
    pub coverage_count: usize,
    pub name_score: f64,
    pub is_seed: bool,
    pub walk_evidence: f64,
    pub seed_paths: HashSet<usize>,
    pub ng_score: f64,
    pub coherence_score: f64,
    pub holo_name_sim: f64,
    pub structural_recall: bool,
    pub surprise_boost: f64,
    pub source_scan_hit: bool,
}

pub struct ScoreConfig {
    pub bm25_w: f64,
    pub cov_w: f64,
    pub name_w: f64,
    pub ng_w: f64,
    pub coh_w: f64,
    pub walk_weight: f64,
    pub holo_gate: f64,
    pub holo_max_w: f64,
    pub seed_paths_threshold: usize,
    pub use_surprise: bool,
    pub use_mdl: bool,
    pub mdl_penalty: f64,
    pub use_idf_coverage_frac: bool,
}

impl ScoreConfig {
    pub fn for_goober_v5(bm25_w: f64, cov_w: f64, name_w: f64, ng_w: f64, coh_w: f64) -> Self {
        Self {
            bm25_w, cov_w, name_w, ng_w, coh_w,
            walk_weight: 1.0,
            holo_gate: 0.25,
            holo_max_w: 2.0,
            seed_paths_threshold: 2,
            use_surprise: false,
            use_mdl: false,
            mdl_penalty: 1.0,
            use_idf_coverage_frac: false,
        }
    }

    pub fn for_geometric(bm25_w: f64, cov_w: f64, name_w: f64, ng_w: f64, coh_w: f64, walk_weight: f64, mdl_penalty: f64) -> Self {
        Self {
            bm25_w, cov_w, name_w, ng_w, coh_w,
            walk_weight,
            holo_gate: 0.25,
            holo_max_w: 2.0,
            seed_paths_threshold: 1,
            use_surprise: true,
            use_mdl: mdl_penalty != 1.0,
            mdl_penalty,
            use_idf_coverage_frac: true,
        }
    }
}

pub fn score_candidates(
    candidates: &std::collections::HashMap<usize, Candidate>,
    query_terms: &[QueryTerm],
    config: &ScoreConfig,
    idx: &CruncherIndex,
) -> Vec<(usize, f64)> {
    let n_qt = query_terms.len();
    let idf_sum: f64 = query_terms.iter().map(|qt| qt.idf).sum();
    let query_specificity = if n_qt > 0 {
        query_terms.iter().filter(|qt| qt.idf > 1.0).count() as f64 / n_qt as f64
    } else {
        0.0
    };
    let max_ng = candidates.values().map(|c| c.ng_score).fold(0.0f64, f64::max).max(1e-10);
    let max_coherence = candidates.values().map(|c| c.coherence_score).fold(0.0f64, f64::max).max(1e-10);
    let structural_max_deg = idx.structural_degree.iter().cloned().fold(0.0f64, f64::max).max(1e-10);

    let mut scored: Vec<(usize, f64)> = candidates
        .values()
        .filter_map(|c| {
            if !c.is_seed && c.seed_paths.len() < config.seed_paths_threshold {
                return None;
            }

            let cov_norm = if idf_sum > 0.0 { c.coverage_score / idf_sum } else { 0.0 };
            let name_norm = if idf_sum > 0.0 { c.name_score / idf_sum } else { 0.0 };
            let walk_norm = if idf_sum > 0.0 { c.walk_evidence / idf_sum } else { 0.0 };

            let base = if c.is_seed {
                let (cov_cap, name_cap) = if config.use_idf_coverage_frac {
                    (cov_norm.min(0.5), name_norm.min(0.5))
                } else {
                    (cov_norm.min(0.3), name_norm.min(0.4))
                };
                config.bm25_w * c.bm25_score + config.cov_w * cov_cap + config.name_w * name_cap
            } else {
                1.5 * cov_norm + 2.0 * name_norm + config.walk_weight * walk_norm
            };

            let coverage_frac = if config.use_idf_coverage_frac {
                if idf_sum > 0.0 {
                    cov_norm.max(c.coverage_count as f64 / n_qt as f64)
                } else if n_qt > 0 {
                    c.coverage_count as f64 / n_qt as f64
                } else {
                    0.0
                }
            } else {
                if n_qt > 0 { c.coverage_count as f64 / n_qt as f64 } else { 0.0 }
            };

            let ng_norm = c.ng_score / max_ng;
            let coh_norm = c.coherence_score / max_coherence;
            let ng_boost = 1.0 + config.ng_w * ng_norm + config.coh_w * coh_norm;

            let holo_additive = if c.holo_name_sim > config.holo_gate {
                let excess = (c.holo_name_sim - config.holo_gate) / (1.0 - config.holo_gate);
                config.holo_max_w * query_specificity * excess
            } else {
                0.0
            };

            let seed_bonus = if c.is_seed { 1.15 } else { 1.0 };
            let kb = kind_boost(&idx.symbol_kinds[c.idx]);
            let tp = test_penalty(&idx.file_paths, idx.symbol_file_ids[c.idx]);

            let structural_bonus = if c.structural_recall && c.name_score > 0.0 {
                let deg_norm = idx.structural_degree[c.idx] / structural_max_deg;
                2.0 + deg_norm * 3.0
            } else {
                0.0
            };

            let surprise_bonus = if config.use_surprise {
                1.0 + 0.08 * c.surprise_boost
            } else {
                1.0
            };

            let _source_scan_mult = if c.source_scan_hit && c.coverage_count >= 1 {
                3.0
            } else {
                1.0
            };

            let source_scan_floor = if c.source_scan_hit {
                12.0 * (c.coverage_count.max(1) as f64) / n_qt.max(1) as f64
            } else {
                0.0
            };

            let source_scan_mult = if c.source_scan_hit {
                3.0
            } else {
                1.0
            };

            let raw = (base + holo_additive + structural_bonus + source_scan_floor)
                * coverage_frac.powf(0.3)
                * ng_boost
                * seed_bonus
                * kb
                * tp
                * surprise_bonus
                * config.mdl_penalty
                * source_scan_mult;

            if raw > 0.0 { Some((c.idx, raw)) } else { None }
        })
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
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
