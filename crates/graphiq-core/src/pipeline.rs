//! Unified search pipeline — seed→walk→score on the CruncherIndex.
//!
//! Orchestrates the in-process search path: builds query terms from the
//! CruncherIndex's IDF dictionary, performs graph walk expansion from BM25
//! seeds, computes per-candidate scores with coverage, name overlap, and
//! neighbor fingerprint signals, and returns ranked symbol IDs.
//!
//! This is the fast path used by the MCP server (~18μs per query).
//! The CLI uses the full `SearchEngine` in `search.rs` which adds DB
//! lookups and post-processing.

use std::collections::{BTreeMap, HashSet, VecDeque};

use crate::cruncher::{
    alias_match_score, build_query_terms, compute_name_overlap, name_coverage,
    neighbor_match_score, per_term_match, term_match_score, CruncherIndex,
};
use crate::query_family::QueryFamily;
use crate::scoring::{
    apply_bm25_lock, score_candidates_with_coverage_overrides, Candidate, ScoreConfig,
};

pub struct PipelineConfig {
    pub top_k: usize,
    pub seed_limit: usize,
}

pub fn unified_search(
    query: &str,
    idx: &CruncherIndex,
    bm25_seeds: &[(i64, f64)],
    config: &PipelineConfig,
    family: QueryFamily,
) -> Vec<(i64, f64)> {
    let query_terms = build_query_terms(query, &idx.global_idf);
    if query_terms.is_empty() {
        return bm25_seeds.to_vec();
    }

    let _n_qt = query_terms.len();
    let bm25_max = bm25_seeds
        .iter()
        .map(|(_, s)| *s)
        .fold(0.0f64, f64::max)
        .max(1e-10);

    let mut candidates: BTreeMap<usize, Candidate> = BTreeMap::new();

    for &(id, score) in bm25_seeds.iter().take(config.seed_limit) {
        if let Some(&i) = idx.id_to_idx.get(&id) {
            // Coverage is computed once after candidate generation. This
            // avoids doing the same work on CPU before a GPU batch can take
            // over for a large candidate set.
            let (cov_score, cov_count) = (0.0, 0);
            let (name_s, _) = name_coverage(&query_terms, &idx.term_sets[i].name_terms);
            let no = compute_name_overlap(&query_terms, &idx.term_sets[i].name_terms);
            let ns = neighbor_match_score(&query_terms, &idx.neighbor_terms[i]);
            let sym_name_lower = idx.symbol_names[i].to_lowercase();
            let is_col = idx.collision_names.contains(&sym_name_lower);
            let as_ = alias_match_score(&query_terms, &idx.alias_terms[i], is_col);

            let mut sp = HashSet::new();
            sp.insert(i);

            candidates.insert(
                i,
                Candidate {
                    idx: i,
                    bm25_score: score / bm25_max,
                    coverage_score: cov_score,
                    coverage_count: cov_count,
                    name_score: name_s,
                    is_seed: true,
                    walk_evidence: 0.0,
                    seed_paths: sp,
                    name_overlap: no,
                    neighbor_score: ns,
                    alias_score: as_,
                },
            );
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
                let (cov_score, cov_count) = (0.0, 0);
                let (name_s, _) = name_coverage(&query_terms, &idx.term_sets[i].name_terms);
                let no = compute_name_overlap(&query_terms, &idx.term_sets[i].name_terms);
                let ns = neighbor_match_score(&query_terms, &idx.neighbor_terms[i]);
                let is_col = idx.collision_names.contains(&ql);
                let as_ = alias_match_score(&query_terms, &idx.alias_terms[i], is_col);

                candidates.insert(
                    i,
                    Candidate {
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
                        name_overlap: no,
                        neighbor_score: ns,
                        alias_score: as_,
                    },
                );
            }
        }
    }

    let score_config = ScoreConfig::for_family(family);

    if score_config.walk_enabled {
        let mut seed_indices: Vec<usize> = candidates.keys().cloned().collect();
        seed_indices.sort_unstable();

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
                    let (ns_name, _) =
                        name_coverage(&query_terms, &idx.term_sets[neighbor_i].name_terms);
                    let no =
                        compute_name_overlap(&query_terms, &idx.term_sets[neighbor_i].name_terms);
                    let ns_nbr =
                        neighbor_match_score(&query_terms, &idx.neighbor_terms[neighbor_i]);
                    let sym_name_lower = idx.symbol_names[neighbor_i].to_lowercase();
                    let is_col = idx.collision_names.contains(&sym_name_lower);
                    let as_ = alias_match_score(&query_terms, &idx.alias_terms[neighbor_i], is_col);
                    Candidate {
                        idx: neighbor_i,
                        bm25_score: 0.0,
                        coverage_score: cov_score,
                        coverage_count: cov_count,
                        name_score: ns_name,
                        is_seed: false,
                        walk_evidence: 0.0,
                        seed_paths: HashSet::new(),
                        name_overlap: no,
                        neighbor_score: ns_nbr,
                        alias_score: as_,
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

    let candidate_indices: Vec<usize> = candidates.keys().copied().collect();
    let gpu_coverage_overrides = idx.gpu_data.as_ref().and_then(|data| {
        let result =
            crate::gpu_compute::try_gpu_term_match(data, &query_terms, &candidate_indices)?;
        if crate::gpu_compute::gpu_trace_enabled() {
            eprintln!(
                "  Search: GPU term scoring on {} candidates via {} ({}ms)",
                result.candidate_indices.len(),
                result.device,
                result.elapsed_ms,
            );
        }
        let mut overrides = std::collections::HashMap::with_capacity(result.scores.len());
        for ((&candidate, &score), &matched) in result
            .candidate_indices
            .iter()
            .zip(result.scores.iter())
            .zip(result.matched.iter())
        {
            overrides.insert(candidate, (score as f64, matched as usize));
        }
        Some(overrides)
    });

    let cpu_coverage_overrides = if gpu_coverage_overrides.is_none() {
        let mut overrides = std::collections::HashMap::with_capacity(candidates.len());
        for (&candidate, entry) in &candidates {
            let (score, matched) = term_match_score(&query_terms, &idx.term_sets[entry.idx]);
            overrides.insert(candidate, (score, matched));
        }
        Some(overrides)
    } else {
        None
    };

    let mut scored = score_candidates_with_coverage_overrides(
        &candidates,
        &query_terms,
        &score_config,
        idx,
        gpu_coverage_overrides
            .as_ref()
            .or(cpu_coverage_overrides.as_ref()),
    );
    apply_bm25_lock(&mut scored, bm25_seeds, &query_terms, idx);

    let mut results: Vec<(i64, f64)> = Vec::with_capacity(config.top_k);
    let mut file_counts: std::collections::HashMap<i64, usize> = std::collections::HashMap::new();

    for (i, score) in scored {
        let fid = idx.symbol_file_ids[i];
        let fc = file_counts.entry(fid).or_insert(0);
        if *fc >= score_config.diversity_max_per_file {
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
