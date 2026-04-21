use std::collections::{HashMap, HashSet, VecDeque};

use crate::cruncher::{
    CruncherIndex, MAX_SEEDS,
    build_query_terms,
    term_match_score, name_coverage,
    per_term_match,
};
use crate::scoring::{Candidate, ScoreConfig, score_candidates, apply_bm25_lock};

pub struct PipelineConfig {
    pub top_k: usize,
    pub walk_weight: f64,
}

pub fn unified_search(
    query: &str,
    idx: &CruncherIndex,
    bm25_seeds: &[(i64, f64)],
    config: &PipelineConfig,
) -> Vec<(i64, f64)> {
    let query_terms = build_query_terms(query, &idx.global_idf);
    if query_terms.is_empty() {
        return bm25_seeds.to_vec();
    }

    let _n_qt = query_terms.len();
    let bm25_max = bm25_seeds.iter().map(|(_, s)| *s).fold(0.0f64, f64::max).max(1e-10);

    let mut candidates: HashMap<usize, Candidate> = HashMap::new();

    for &(id, score) in bm25_seeds.iter().take(MAX_SEEDS) {
        if let Some(&i) = idx.id_to_idx.get(&id) {
            let (cov_score, cov_count) = term_match_score(&query_terms, &idx.term_sets[i]);
            let (name_s, _) = name_coverage(&query_terms, &idx.term_sets[i].name_terms);

            let mut sp = HashSet::new();
            sp.insert(i);

            candidates.insert(i, Candidate {
                idx: i,
                bm25_score: score / bm25_max,
                coverage_score: cov_score,
                coverage_count: cov_count,
                name_score: name_s,
                is_seed: true,
                walk_evidence: 0.0,
                seed_paths: sp,
            });
        }
    }

    let mut idf_sorted: Vec<f64> = query_terms.iter().map(|qt| qt.idf).collect();
    idf_sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let idf_threshold = idf_sorted[idf_sorted.len() / 2];

    for qt in &query_terms {
        let ql = qt.text.to_lowercase();
        if let Some(indices) = idx.name_to_indices.get(&ql) {
            for &i in indices.iter().take(5) {
                if candidates.contains_key(&i) {
                    continue;
                }
                let (cov_score, cov_count) = term_match_score(&query_terms, &idx.term_sets[i]);
                let (name_s, _) = name_coverage(&query_terms, &idx.term_sets[i].name_terms);

                candidates.insert(i, Candidate {
                    idx: i,
                    bm25_score: 0.0,
                    coverage_score: cov_score,
                    coverage_count: cov_count,
                    name_score: name_s,
                    is_seed: true,
                    walk_evidence: 0.0,
                    seed_paths: {
                        let mut sp = HashSet::new();
                        sp.insert(i);
                        sp
                    },
                });
            }
        }
    }

    {
        let seed_indices: Vec<usize> = candidates.keys().cloned().collect();

        for &seed_i in seed_indices.iter().take(8) {
            let mut queue: VecDeque<(usize, f64, usize)> = VecDeque::new();
            let mut visited: HashSet<usize> = HashSet::new();
            visited.insert(seed_i);

            for edge in idx.outgoing[seed_i].iter().take(10) {
                if !visited.contains(&edge.target) {
                    queue.push_back((edge.target, edge.weight, 1));
                    visited.insert(edge.target);
                }
            }
            for edge in idx.incoming[seed_i].iter().take(10) {
                if !visited.contains(&edge.target) {
                    queue.push_back((edge.target, edge.weight, 1));
                    visited.insert(edge.target);
                }
            }

            let mut expanded = 0usize;
            while let Some((neighbor_i, edge_w, depth)) = queue.pop_front() {
                if depth > 2 || expanded >= 25 {
                    break;
                }

                let has_specific = query_terms
                    .iter()
                    .filter(|qt| qt.idf >= idf_threshold)
                    .any(|qt| per_term_match(&idx.term_sets[neighbor_i], qt) > 0.0);

                if !has_specific {
                    continue;
                }

                let (cov_score, cov_count) =
                    term_match_score(&query_terms, &idx.term_sets[neighbor_i]);
                if cov_count == 0 {
                    continue;
                }

                let proximity = 0.5_f64.powi(depth as i32);
                let evidence = cov_score * proximity * edge_w;

                let entry = candidates.entry(neighbor_i).or_insert_with(|| {
                    let (ns, _) =
                        name_coverage(&query_terms, &idx.term_sets[neighbor_i].name_terms);
                    Candidate {
                        idx: neighbor_i,
                        bm25_score: 0.0,
                        coverage_score: cov_score,
                        coverage_count: cov_count,
                        name_score: ns,
                        is_seed: false,
                        walk_evidence: 0.0,
                        seed_paths: HashSet::new(),
                    }
                });

                if !entry.is_seed {
                    entry.coverage_score = entry.coverage_score.max(cov_score);
                    entry.coverage_count = entry.coverage_count.max(cov_count);
                }
                entry.walk_evidence += evidence;
                entry.seed_paths.insert(seed_i);
                expanded += 1;

                if depth < 2 {
                    let next: Vec<(usize, f64)> = idx.outgoing[neighbor_i]
                        .iter()
                        .chain(idx.incoming[neighbor_i].iter())
                        .take(6)
                        .filter(|e| !visited.contains(&e.target))
                        .map(|e| (e.target, e.weight.min(edge_w)))
                        .collect();
                    for (next_i, next_w) in next {
                        visited.insert(next_i);
                        queue.push_back((next_i, next_w, depth + 1));
                    }
                }
            }
        }
    }

    let score_config = ScoreConfig {
        bm25_w: 4.0,
        cov_w: 1.0,
        name_w: 1.2,
        walk_weight: config.walk_weight,
    };

    let mut scored = score_candidates(&candidates, &query_terms, &score_config, idx);
    apply_bm25_lock(&mut scored, bm25_seeds, &query_terms, idx);

    let mut results: Vec<(i64, f64)> = Vec::with_capacity(config.top_k);
    let mut file_counts: HashMap<i64, usize> = HashMap::new();

    for (i, score) in scored {
        let fid = idx.symbol_file_ids[i];
        let fc = file_counts.entry(fid).or_insert(0);
        if *fc >= 3 {
            continue;
        }
        *fc += 1;
        results.push((idx.symbol_ids[i], score));
        if results.len() >= config.top_k {
            break;
        }
    }

    results
}
