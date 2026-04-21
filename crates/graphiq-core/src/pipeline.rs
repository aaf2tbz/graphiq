use std::collections::{HashMap, HashSet, VecDeque};

use crate::cruncher::{
    CruncherIndex, MAX_SEEDS,
    build_query_terms,
    term_match_score, name_coverage, compute_sec_channels,
    negentropy, channel_coherence,
    per_term_match,
};
use crate::holo_name::{HoloIndex, holo_query_name_cosine};
use crate::spectral::{SpectralIndex, PredictiveModel, ChannelFingerprint};
use crate::scoring::{Candidate, ScoreConfig, score_candidates, apply_bm25_lock};
use crate::structural_fallback::{
    SnpConfig, seed_diffusion_score, infer_query_role,
    structural_neighborhood_expansion,
};

pub struct PipelineConfig<'a> {
    pub top_k: usize,
    pub use_heat_diffusion: bool,
    pub heat_t: f64,
    pub cheb_order: usize,
    pub walk_weight: f64,
    pub heat_top_k: usize,
    pub predictive: Option<&'a PredictiveModel>,
    pub fingerprints: Option<&'a [ChannelFingerprint]>,
    pub fp_id_to_idx: Option<&'a HashMap<i64, usize>>,
    pub evidence_weight: f64,
}

pub fn unified_search(
    query: &str,
    idx: &CruncherIndex,
    hi: &HoloIndex,
    bm25_seeds: &[(i64, f64)],
    spectral: Option<&SpectralIndex>,
    config: &PipelineConfig<'_>,
    source_scan_start: usize,
) -> Vec<(i64, f64)> {
    let query_terms = build_query_terms(query, &idx.global_idf);
    if query_terms.is_empty() {
        return bm25_seeds.to_vec();
    }

    let n_qt = query_terms.len();
    let _idf_sum: f64 = query_terms.iter().map(|qt| qt.idf).sum();
    let bm25_max = bm25_seeds.iter().map(|(_, s)| *s).fold(0.0f64, f64::max).max(1e-10);

    let _query_specificity = if n_qt > 0 {
        query_terms.iter().filter(|qt| qt.idf > 1.0).count() as f64 / n_qt as f64
    } else {
        0.0
    };

    let intent_weights: [f64; 5] = [4.0, 1.0, 1.2, 0.15, 0.08];

    let channel_adj: [f64; 5] = if let (Some(fps), Some(fp_map)) = (config.fingerprints, config.fp_id_to_idx) {
        crate::spectral::channel_capacity_weights(
            fps, fp_map,
            &query_terms.iter().map(|qt| qt.text.clone()).collect::<Vec<_>>(),
            bm25_seeds,
        )
    } else {
        [0.0; 5]
    };

    let (bm25_w, cov_w, name_w, ng_w, coh_w) = (
        intent_weights[0] + channel_adj[0],
        intent_weights[1] + channel_adj[1],
        intent_weights[2] + channel_adj[2],
        intent_weights[3] + channel_adj[3],
        intent_weights[4] + channel_adj[4],
    );

    let surprise_map: HashMap<usize, f64> = if let Some(pm) = config.predictive {
        let mut map = HashMap::new();
        for &(id, _) in bm25_seeds.iter().take(MAX_SEEDS) {
            if let Some(&ci) = idx.id_to_idx.get(&id) {
                if let Some(&si) = pm.sym_id_to_idx.get(&id) {
                    let qt_strings: Vec<String> = query_terms.iter().map(|qt| qt.text.clone()).collect();
                    let surprise = crate::spectral::predictive_surprise(pm, &qt_strings, si);
                    map.insert(ci, surprise);
                }
            }
        }
        if !map.is_empty() {
            let max_surprise = map.values().cloned().fold(0.0f64, f64::max).max(1e-10);
            map.iter_mut().for_each(|(_, v)| *v /= max_surprise);
        }
        map
    } else {
        HashMap::new()
    };

    let mut candidates: HashMap<usize, Candidate> = HashMap::new();

    for &(id, score) in bm25_seeds.iter().take(MAX_SEEDS) {
        if let Some(&i) = idx.id_to_idx.get(&id) {
            let (cov_score, cov_count) = term_match_score(&query_terms, &idx.term_sets[i]);
            let (name_s, _) = name_coverage(&query_terms, &idx.term_sets[i].name_terms);
            let channels = compute_sec_channels(&query_terms, idx, i);
            let ng = negentropy(&channels);
            let coherence = channel_coherence(&query_terms, idx, i);
            let holo_name = holo_query_name_cosine(query, hi, i);
            let surprise_boost = surprise_map.get(&i).copied().unwrap_or(0.0);

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
                ng_score: ng,
                coherence_score: coherence,
                holo_name_sim: holo_name,
                structural_recall: false,
                surprise_boost,
                source_scan_hit: false,
            });
        }
    }

    // Secondary pass: source scan seeds not yet in candidates
    if source_scan_start > 0 {
        for (seed_idx, &(id, score)) in bm25_seeds.iter().enumerate() {
            if seed_idx < source_scan_start {
                continue;
            }
            if let Some(&i) = idx.id_to_idx.get(&id) {
                if candidates.contains_key(&i) {
                    continue;
                }
                let (cov_score, cov_count) = term_match_score(&query_terms, &idx.term_sets[i]);
                if cov_count == 0 {
                    continue;
                }
                let (name_s, _) = name_coverage(&query_terms, &idx.term_sets[i].name_terms);
                let channels = compute_sec_channels(&query_terms, idx, i);
                let ng = negentropy(&channels);
                let coherence = channel_coherence(&query_terms, idx, i);
                let holo_name = holo_query_name_cosine(query, hi, i);

                candidates.insert(i, Candidate {
                    idx: i,
                    bm25_score: score / bm25_max,
                    coverage_score: cov_score,
                    coverage_count: cov_count,
                    name_score: name_s,
                    is_seed: false,
                    walk_evidence: 0.0,
                    seed_paths: {
                        let mut sp = HashSet::new();
                        sp.insert(i);
                        sp
                    },
                    ng_score: ng,
                    coherence_score: coherence,
                    holo_name_sim: holo_name,
                    structural_recall: false,
                    surprise_boost: 0.0,
                    source_scan_hit: true,
                });
            }
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
                let channels = compute_sec_channels(&query_terms, idx, i);
                let ng = negentropy(&channels);
                let coherence = channel_coherence(&query_terms, idx, i);
                let holo_name = holo_query_name_cosine(query, hi, i);
                let surprise_boost = surprise_map.get(&i).copied().unwrap_or(0.0);

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
                    ng_score: ng,
                    coherence_score: coherence,
                    holo_name_sim: holo_name,
                    structural_recall: true,
                    surprise_boost,
                    source_scan_hit: false,
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

                let channels = compute_sec_channels(&query_terms, idx, neighbor_i);
                let ng = negentropy(&channels);
                let coherence = channel_coherence(&query_terms, idx, neighbor_i);
                let holo_name = holo_query_name_cosine(query, hi, neighbor_i);

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
                        ng_score: ng,
                        coherence_score: coherence,
                        holo_name_sim: holo_name,
                        structural_recall: false,
                        surprise_boost: 0.0,
                        source_scan_hit: false,
                    }
                });

                if !entry.is_seed {
                    entry.coverage_score = entry.coverage_score.max(cov_score);
                    entry.coverage_count = entry.coverage_count.max(cov_count);
                    entry.ng_score = entry.ng_score.max(ng);
                    entry.coherence_score = entry.coherence_score.max(coherence);
                    entry.holo_name_sim = entry.holo_name_sim.max(holo_name);
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

        if config.use_heat_diffusion {
            if let Some(spec) = spectral {
                let spectral_seeds: Vec<usize> = seed_indices
                    .iter()
                    .filter_map(|&ci| {
                        let sym_id = idx.symbol_ids[ci];
                        spec.sym_id_to_idx.get(&sym_id).copied()
                    })
                    .collect();

                if !spectral_seeds.is_empty() {
                    let seed_weights: Vec<f64> = spectral_seeds
                        .iter()
                        .map(|_| 1.0 / spectral_seeds.len() as f64)
                        .collect();

                    let heat_results = crate::spectral::chebyshev_heat(
                        &spec.graph,
                        &spectral_seeds,
                        &seed_weights,
                        config.heat_t,
                        config.cheb_order,
                        config.heat_top_k,
                    );

                    let heat_max = heat_results.first().map(|(_, s)| *s).unwrap_or(1.0).max(1e-10);

                    for (spec_i, heat_score) in &heat_results {
                        let sym_id = spec.symbol_ids[*spec_i];
                        if let Some(&ci) = idx.id_to_idx.get(&sym_id) {
                            if candidates.contains_key(&ci) {
                                continue;
                            }

                            let normalized_heat = heat_score / heat_max;

                            let has_specific = query_terms
                                .iter()
                                .filter(|qt| qt.idf >= idf_threshold)
                                .any(|qt| per_term_match(&idx.term_sets[ci], qt) > 0.0);

                            if !has_specific {
                                continue;
                            }

                            let (cov_score, cov_count) = term_match_score(&query_terms, &idx.term_sets[ci]);
                            if cov_count == 0 {
                                continue;
                            }

                            let channels = compute_sec_channels(&query_terms, idx, ci);
                            let ng = negentropy(&channels);
                            let coherence = channel_coherence(&query_terms, idx, ci);
                            let holo_name = holo_query_name_cosine(query, hi, ci);

                            let surprise_boost = if let Some(pm) = config.predictive {
                                if let Some(&si) = pm.sym_id_to_idx.get(&sym_id) {
                                    let qt_strings: Vec<String> = query_terms.iter().map(|qt| qt.text.clone()).collect();
                                    let surprise = crate::spectral::predictive_surprise(pm, &qt_strings, si);
                                    let max_s = surprise_map.values().cloned().fold(0.0f64, f64::max).max(1e-10);
                                    (surprise / max_s).min(1.0)
                                } else {
                                    0.0
                                }
                            } else {
                                0.0
                            };

                            let entry = candidates.entry(ci).or_insert_with(|| {
                                let (ns, _) = name_coverage(&query_terms, &idx.term_sets[ci].name_terms);
                                Candidate {
                                    idx: ci,
                                    bm25_score: 0.0,
                                    coverage_score: cov_score,
                                    coverage_count: cov_count,
                                    name_score: ns,
                                    is_seed: false,
                                    walk_evidence: 0.0,
                                    seed_paths: HashSet::new(),
                                    ng_score: ng,
                                    coherence_score: coherence,
                                    holo_name_sim: holo_name,
                                    structural_recall: false,
                                    surprise_boost: 0.0,
                                    source_scan_hit: false,
                                }
                            });

                            entry.walk_evidence = entry.walk_evidence.max(cov_score * normalized_heat * config.evidence_weight);
                            entry.surprise_boost = entry.surprise_boost.max(surprise_boost);
                            entry.seed_paths.insert(seed_indices[0]);
                        }
                    }
                }
            }
        }
    }

    // --- Structural Neighborhood Profiling (SNP) fallback ---
    // Activates when BM25 seeds are diffuse (generic names, no clear winner)
    // and structural fingerprints are available.
    if config.fingerprints.is_some() && config.fp_id_to_idx.is_some() {
        let diffusion = seed_diffusion_score(bm25_seeds);
        if diffusion > 0.5 {
            let query_role = infer_query_role(query);
            let existing_indices: HashSet<usize> = candidates.keys().cloned().collect();
            let seed_idx_list: Vec<usize> = bm25_seeds
                .iter()
                .take(MAX_SEEDS)
                .filter_map(|(id, _)| idx.id_to_idx.get(id).copied())
                .collect();

            let snp_results = structural_neighborhood_expansion(
                &seed_idx_list,
                idx,
                config.fingerprints,
                config.fp_id_to_idx,
                query_role,
                &SnpConfig::default(),
                &existing_indices,
            );

            let snp_max = snp_results.first().map(|c| c.snp_score).unwrap_or(1.0).max(1e-10);
            for snp_c in snp_results {
                let (cov_score, cov_count) = term_match_score(&query_terms, &idx.term_sets[snp_c.idx]);
                let (name_s, _) = name_coverage(&query_terms, &idx.term_sets[snp_c.idx].name_terms);
                let channels = compute_sec_channels(&query_terms, idx, snp_c.idx);
                let ng = negentropy(&channels);
                let coherence = channel_coherence(&query_terms, idx, snp_c.idx);
                let holo_name = holo_query_name_cosine(query, hi, snp_c.idx);

                let snp_norm = snp_c.snp_score / snp_max;

                candidates.insert(snp_c.idx, Candidate {
                    idx: snp_c.idx,
                    bm25_score: 0.0,
                    coverage_score: cov_score,
                    coverage_count: cov_count,
                    name_score: name_s,
                    is_seed: false,
                    walk_evidence: snp_norm * 0.5,
                    seed_paths: snp_c.source_seeds,
                    ng_score: ng,
                    coherence_score: coherence,
                    holo_name_sim: holo_name,
                    structural_recall: true,
                    surprise_boost: 0.0,
                    source_scan_hit: false,
                });
            }
        }
    }

    let seed_paths_threshold = if config.use_heat_diffusion { 1 } else { 2 };
    let score_config = ScoreConfig {
        bm25_w, cov_w, name_w, ng_w, coh_w,
        walk_weight: config.walk_weight,
        holo_gate: 0.25,
        holo_max_w: 2.0,
        seed_paths_threshold,
        use_surprise: config.predictive.is_some(),
        use_mdl: false,
        mdl_penalty: 1.0,
        use_idf_coverage_frac: config.use_heat_diffusion,
    };

    let mut scored = score_candidates(&candidates, &query_terms, &score_config, idx);

    apply_bm25_lock(&mut scored, bm25_seeds, &query_terms, idx);

    let scored_for_mdl: Vec<(i64, f64)> = scored
        .iter()
        .map(|(i, s)| (idx.symbol_ids[*i], *s))
        .collect();

    let mdl = if let (Some(fps), Some(fp_map)) = (config.fingerprints, config.fp_id_to_idx) {
        let idx_ref = idx;
        Some(crate::spectral::mdl_explanation_set(
            &scored_for_mdl,
            &query_terms.iter().map(|qt| qt.text.clone()).collect::<Vec<_>>(),
            &|sym_id: i64| -> Option<(HashSet<String>, HashSet<String>, HashMap<String, f64>)> {
                let ci = *idx_ref.id_to_idx.get(&sym_id)?;
                let ts = &idx_ref.term_sets[ci];
                Some((
                    ts.name_terms.clone(),
                    ts.sig_terms.clone(),
                    ts.terms.clone(),
                ))
            },
            fps,
            fp_map,
        ))
    } else {
        None
    };

    let mdl_penalty = if mdl.as_ref().map_or(false, |m| m.covered_frac > 0.5) {
        1.0 + 0.1 * mdl.as_ref().unwrap().marginal_gain
    } else {
        1.0
    };

    let mut results: Vec<(i64, f64)> = Vec::with_capacity(config.top_k);
    let mut file_counts: HashMap<i64, usize> = HashMap::new();

    for (i, score) in scored {
        let fid = idx.symbol_file_ids[i];
        let fc = file_counts.entry(fid).or_insert(0);
        if *fc >= 3 {
            continue;
        }
        *fc += 1;
        results.push((idx.symbol_ids[i], score * mdl_penalty));
        if results.len() >= config.top_k {
            break;
        }
    }

    results
}
