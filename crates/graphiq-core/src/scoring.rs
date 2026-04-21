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
}

pub struct ScoreConfig {
    pub bm25_w: f64,
    pub cov_w: f64,
    pub name_w: f64,
    pub walk_weight: f64,
}

impl Default for ScoreConfig {
    fn default() -> Self {
        Self {
            bm25_w: 4.0,
            cov_w: 1.0,
            name_w: 1.2,
            walk_weight: 1.0,
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

    let mut scored: Vec<(usize, f64)> = candidates
        .values()
        .filter_map(|c| {
            if !c.is_seed && c.seed_paths.is_empty() {
                return None;
            }

            let cov_norm = if idf_sum > 0.0 { c.coverage_score / idf_sum } else { 0.0 };
            let name_norm = if idf_sum > 0.0 { c.name_score / idf_sum } else { 0.0 };
            let walk_norm = if idf_sum > 0.0 { c.walk_evidence / idf_sum } else { 0.0 };

            let base = if c.is_seed {
                let cov_cap = cov_norm.min(0.4);
                let name_cap = name_norm.min(0.5);
                config.bm25_w * c.bm25_score + config.cov_w * cov_cap + config.name_w * name_cap
            } else {
                1.5 * cov_norm + 2.0 * name_norm + config.walk_weight * walk_norm
            };

            let coverage_frac = if n_qt > 0 {
                c.coverage_count as f64 / n_qt as f64
            } else {
                0.0
            };

            let seed_bonus = if c.is_seed { 1.15 } else { 1.0 };
            let kb = kind_boost(&idx.symbol_kinds[c.idx]);
            let tp = test_penalty(&idx.file_paths, idx.symbol_file_ids[c.idx]);

            let raw = base
                * coverage_frac.powf(0.3)
                * seed_bonus
                * kb
                * tp;

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
