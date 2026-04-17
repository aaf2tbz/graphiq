use std::collections::HashMap;

use crate::db::GraphDb;
use crate::lsa::{extract_terms, load_latent_vectors, load_lsa_basis, load_lsa_sigma};

pub struct Cell {
    pub members: Vec<usize>,
    pub centroid: Vec<f64>,
}

pub struct Af26Index {
    pub symbol_ids: Vec<i64>,
    pub symbol_coords: Vec<Vec<f64>>,
    pub term_index: HashMap<String, usize>,
    pub term_basis: Vec<Vec<f64>>,
    pub term_idf: Vec<f64>,
    pub sigma: Vec<f64>,
    pub gravity: Vec<Vec<(usize, f64)>>,
    pub centrality: Vec<f64>,
    pub cells: Vec<Cell>,
    pub symbol_cell: Vec<usize>,
    pub coarse_dim: usize,
}

pub fn compute_af26(db: &GraphDb) -> Result<Af26Index, String> {
    eprintln!("  [af26] loading LSA foundation...");
    let (term_basis, term_index, term_idf) = load_lsa_basis(db)?;
    let symbol_map = load_latent_vectors(db)?;

    if term_basis.is_empty() || symbol_map.is_empty() {
        return Err("no LSA data available".into());
    }

    let mut symbol_ids: Vec<i64> = symbol_map.keys().copied().collect();
    symbol_ids.sort();
    let symbol_vecs: Vec<Vec<f64>> = symbol_ids.iter().map(|id| symbol_map[id].clone()).collect();

    let dim = symbol_vecs[0].len();
    eprintln!(
        "  [af26] {} symbols, dim={}, {} terms",
        symbol_ids.len(),
        dim,
        term_index.len()
    );

    let sigma = load_lsa_sigma(db).unwrap_or_else(|_| {
        eprintln!("  [af26] WARNING: no stored sigma");
        vec![1.0; dim]
    });

    let gravity = build_gravity(db, &symbol_ids);
    let n_edges: usize = gravity.iter().map(|e| e.len()).sum::<usize>() / 2;

    let centrality: Vec<f64> = gravity
        .iter()
        .map(|neighbors| {
            let deg = neighbors.len() as f64;
            let w_sum: f64 = neighbors.iter().map(|(_, w)| w).sum();
            (1.0 + (1.0 + deg).ln()) * (1.0 + w_sum * 0.1)
        })
        .collect();

    let max_c = centrality.iter().copied().fold(0.0f64, f64::max);
    let centrality: Vec<f64> = centrality.iter().map(|c| c / max_c).collect();

    eprintln!("  [af26] {} gravity edges", n_edges);

    let coarse_dim = 8usize.min(dim);
    let sigma_top: Vec<f64> = sigma.iter().take(coarse_dim).copied().collect();
    let sigma_norm: f64 = sigma_top.iter().map(|s| s * s).sum::<f64>().sqrt();

    let coarse_vecs: Vec<Vec<f64>> = symbol_vecs
        .iter()
        .map(|v| {
            let mut cv = Vec::with_capacity(coarse_dim);
            for j in 0..coarse_dim {
                cv.push(v[j] * sigma[j] / sigma_norm);
            }
            let norm: f64 = cv.iter().map(|x| x * x).sum::<f64>().sqrt();
            if norm > 1e-10 {
                for x in cv.iter_mut() {
                    *x /= norm;
                }
            }
            cv
        })
        .collect();

    let n_symbols = symbol_ids.len();
    let k = (n_symbols as f64).sqrt().ceil() as usize;
    let k = k.max(8).min(256);

    let (cells, symbol_cell) = spherical_kmeans(&coarse_vecs, k, 30);

    eprintln!(
        "  [af26] {} cells (coarse_dim={}, top_σ=[{}])",
        cells.len(),
        coarse_dim,
        sigma
            .iter()
            .take(4)
            .map(|s| format!("{:.2}", s))
            .collect::<Vec<_>>()
            .join(",")
    );

    Ok(Af26Index {
        symbol_ids,
        symbol_coords: symbol_vecs,
        term_index,
        term_basis,
        term_idf,
        sigma,
        gravity,
        centrality,
        cells,
        symbol_cell,
        coarse_dim,
    })
}

fn build_gravity(db: &GraphDb, symbol_ids: &[i64]) -> Vec<Vec<(usize, f64)>> {
    let conn = db.conn();
    let id_to_idx: HashMap<i64, usize> = symbol_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i))
        .collect();

    let mut edges: Vec<Vec<(usize, f64)>> = vec![Vec::new(); symbol_ids.len()];

    let mut stmt = match conn.prepare("SELECT source_id, target_id, kind FROM edges") {
        Ok(s) => s,
        Err(_) => return edges,
    };

    let rows: Vec<(i64, i64, String)> = stmt
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get::<_, String>(2)?))
        })
        .unwrap_or_else(|_| panic!("failed"))
        .flatten()
        .collect();

    let edge_w = |kind: &str| -> f64 {
        match kind {
            "Calls" => 1.0,
            "Contains" => 0.8,
            "Imports" => 0.9,
            "Extends" => 0.7,
            "Implements" => 0.7,
            "References" => 0.5,
            _ => 0.3,
        }
    };

    for (src, tgt, kind) in &rows {
        let w = edge_w(kind);
        if let (Some(&si), Some(&ti)) = (id_to_idx.get(src), id_to_idx.get(tgt)) {
            edges[si].push((ti, w));
            edges[ti].push((si, w));
        }
    }

    edges
}

fn id_to_idx_map(index: &Af26Index) -> HashMap<i64, usize> {
    index
        .symbol_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i))
        .collect()
}

fn cosine(a: &[f64], b: &[f64]) -> f64 {
    let dot: f64 = a.iter().zip(b.iter()).map(|(ai, bi)| ai * bi).sum();
    let na: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let nb: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();
    if na < 1e-10 || nb < 1e-10 {
        return 0.0;
    }
    dot / (na * nb)
}

fn angular_sim(a: &[f64], b: &[f64]) -> f64 {
    1.0 - cosine(a, b).acos() / std::f64::consts::PI
}

fn project_query(
    query: &str,
    term_index: &HashMap<String, usize>,
    term_basis: &[Vec<f64>],
    term_idf: &[f64],
    dim: usize,
) -> Vec<f64> {
    let terms = extract_terms(query);
    let mut q = vec![0.0f64; dim];
    let mut total_idf = 0.0f64;

    for t in &terms {
        if let Some(&idx) = term_index.get(t) {
            if idx < term_basis.len() {
                let idf = term_idf.get(idx).copied().unwrap_or(1.0);
                for j in 0..dim {
                    q[j] += idf * term_basis[idx][j];
                }
                total_idf += idf;
            }
        }
    }

    if total_idf > 0.0 {
        let norm = q.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm > 1e-10 {
            for x in q.iter_mut() {
                *x /= norm;
            }
        }
    }
    q
}

fn per_term_similarities(
    query: &str,
    term_index: &HashMap<String, usize>,
    term_basis: &[Vec<f64>],
    symbol: &[f64],
    dim: usize,
) -> Vec<f64> {
    let terms = extract_terms(query);
    let mut sims = Vec::new();

    for t in &terms {
        if let Some(&idx) = term_index.get(t) {
            if idx < term_basis.len() {
                let norm: f64 = term_basis[idx].iter().map(|x| x * x).sum::<f64>().sqrt();
                if norm > 1e-10 {
                    let term_vec: Vec<f64> = term_basis[idx].iter().map(|x| x / norm).collect();
                    sims.push(angular_sim(symbol, &term_vec));
                }
            }
        }
    }
    sims
}

fn geometric_mean(sims: &[f64]) -> f64 {
    if sims.is_empty() {
        return 0.0;
    }
    let log_sum: f64 = sims.iter().map(|&s| (s.max(0.01)).ln()).sum();
    (log_sum / sims.len() as f64).exp()
}

pub fn af26_search(query: &str, index: &Af26Index, top_k: usize) -> Vec<(i64, f64)> {
    let dim = index.sigma.len();
    if dim == 0 {
        return Vec::new();
    }

    let query_vec = project_query(
        query,
        &index.term_index,
        &index.term_basis,
        &index.term_idf,
        dim,
    );

    let n = index.symbol_coords.len();
    let mut scored: Vec<(usize, f64)> = Vec::with_capacity(n);

    for i in 0..n {
        let s = &index.symbol_coords[i];

        let centroid_sim = angular_sim(&query_vec, s);

        let per_term = per_term_similarities(query, &index.term_index, &index.term_basis, s, dim);

        let geo_mean = geometric_mean(&per_term);

        let grade = per_term.len().max(1);
        let combined = if grade > 1 {
            centroid_sim * 0.4 + geo_mean * 0.6
        } else {
            centroid_sim
        };

        let centrality_boost = 1.0 + 0.1 * index.centrality[i];

        let score = combined * centrality_boost;

        if score > 0.0 {
            scored.push((i, score));
        }
    }

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored.truncate(top_k);

    scored
        .into_iter()
        .map(|(i, score)| (index.symbol_ids[i], score))
        .collect()
}

pub fn af26_pipeline_boost(
    query: &str,
    fts_symbol_ids: &[i64],
    expanded_symbol_ids: &[i64],
    index: &Af26Index,
    top_k: usize,
) -> Vec<(i64, f64)> {
    let id_to_idx = id_to_idx_map(index);
    let dim = index.sigma.len();
    if dim == 0 {
        return Vec::new();
    }

    let query_vec = project_query(
        query,
        &index.term_index,
        &index.term_basis,
        &index.term_idf,
        dim,
    );

    let fts_set: std::collections::HashSet<i64> = fts_symbol_ids.iter().copied().collect();

    let mut fts_scores: HashMap<i64, f64> = HashMap::new();
    for &id in fts_symbol_ids {
        if let Some(&idx) = id_to_idx.get(&id) {
            let sim = angular_sim(&query_vec, &index.symbol_coords[idx]);
            fts_scores.insert(id, sim);
        }
    }

    let seed_indices: Vec<usize> = fts_symbol_ids
        .iter()
        .take(20)
        .filter_map(|id| id_to_idx.get(id).copied())
        .collect();

    let mut all_candidates: std::collections::HashSet<i64> = std::collections::HashSet::new();
    for &id in fts_symbol_ids {
        all_candidates.insert(id);
    }
    for &id in expanded_symbol_ids {
        all_candidates.insert(id);
    }
    for &si in &seed_indices {
        for &(ni, _) in &index.gravity[si] {
            all_candidates.insert(index.symbol_ids[ni]);
        }
    }

    let seed_set: std::collections::HashSet<usize> = seed_indices.iter().copied().collect();

    let mut scored: Vec<(i64, f64)> = Vec::new();

    for &id in &all_candidates {
        if let Some(&idx) = id_to_idx.get(&id) {
            let s = &index.symbol_coords[idx];

            let semantic_sim = angular_sim(&query_vec, s);

            let per_term =
                per_term_similarities(query, &index.term_index, &index.term_basis, s, dim);
            let geo_mean = geometric_mean(&per_term);

            let multi_concept = if per_term.len() > 1 {
                let min_sim = per_term.iter().cloned().fold(f64::INFINITY, f64::min);
                let coverage =
                    per_term.iter().filter(|&&s| s > 0.3).count() as f64 / per_term.len() as f64;
                min_sim * coverage
            } else {
                semantic_sim
            };

            let mut graph_proximity = 0.0f64;
            let mut graph_weight = 0.0f64;
            for &(ni, w) in &index.gravity[idx] {
                if seed_set.contains(&ni) {
                    graph_proximity += w;
                    graph_weight += 1.0;
                }
            }
            let graph_score = if graph_weight > 0.0 {
                graph_proximity / graph_weight
            } else {
                0.0
            };

            let is_fts = fts_set.contains(&id);
            let base_mult = if is_fts { 1.0 } else { 0.7 };

            let semantic_boost = 1.0 + 0.15 * multi_concept;
            let graph_boost = 1.0 + 0.2 * graph_score;
            let centrality_boost = 1.0 + 0.05 * index.centrality[idx];

            let score = base_mult * semantic_boost * graph_boost * centrality_boost;

            scored.push((id, score));
        }
    }

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored.truncate(top_k);

    scored
}

pub fn af26_manifold_search(query: &str, index: &Af26Index, top_k: usize) -> Vec<(i64, f64)> {
    let dim = index.sigma.len();
    if dim == 0 {
        return Vec::new();
    }

    let query_vec = project_query(
        query,
        &index.term_index,
        &index.term_basis,
        &index.term_idf,
        dim,
    );

    let n = index.symbol_coords.len();

    let mut scored: Vec<(usize, f64)> = (0..n)
        .map(|i| {
            let s = &index.symbol_coords[i];

            let flat_dist_sq: f64 = s
                .iter()
                .zip(query_vec.iter())
                .map(|(si, qi)| (si - qi) * (si - qi))
                .sum();

            let mut curvature_factor = 0.0f64;
            for &(ni, w) in &index.gravity[i] {
                let ns = &index.symbol_coords[ni];
                let mid: Vec<f64> = s
                    .iter()
                    .zip(ns.iter())
                    .map(|(a, b)| (a + b) / 2.0)
                    .collect();
                let mid_q: f64 = mid
                    .iter()
                    .zip(query_vec.iter())
                    .map(|(m, q)| (m - q) * (m - q))
                    .sum();
                let neighbor_sim = cosine(&query_vec, ns);
                if neighbor_sim > 0.3 {
                    curvature_factor += w * neighbor_sim;
                }
            }

            let effective_dist = flat_dist_sq / (1.0 + 0.3 * curvature_factor);

            let per_term =
                per_term_similarities(query, &index.term_index, &index.term_basis, s, dim);
            let geo_mean = geometric_mean(&per_term);

            let grade = per_term.len().max(1);
            let combined = if grade > 1 {
                let flat_sim = 1.0 / (1.0 + effective_dist.sqrt());
                flat_sim * 0.5 + geo_mean * 0.5
            } else {
                1.0 / (1.0 + effective_dist.sqrt())
            };

            let score = combined * (1.0 + 0.1 * index.centrality[i]);
            (i, score)
        })
        .filter(|(_, s)| *s > 0.0)
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored.truncate(top_k);

    scored
        .into_iter()
        .map(|(i, score)| (index.symbol_ids[i], score))
        .collect()
}

pub fn af26_combined_boost(
    query: &str,
    candidate_ids: &[i64],
    candidate_scores: &[f64],
    index: &Af26Index,
) -> Vec<(i64, f64)> {
    let id_to_idx: HashMap<i64, usize> = index
        .symbol_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i))
        .collect();

    let dim = index.sigma.len();
    if dim == 0 || candidate_ids.is_empty() {
        return candidate_ids
            .iter()
            .zip(candidate_scores.iter())
            .map(|(id, s)| (*id, *s))
            .collect();
    }

    let query_vec = project_query(
        query,
        &index.term_index,
        &index.term_basis,
        &index.term_idf,
        dim,
    );

    let seed_set: std::collections::HashSet<usize> = candidate_ids
        .iter()
        .take(20)
        .filter_map(|id| id_to_idx.get(id).copied())
        .collect();

    let mut scored: Vec<(i64, f64)> = Vec::with_capacity(candidate_ids.len());

    for (i, &id) in candidate_ids.iter().enumerate() {
        let base_score = candidate_scores.get(i).copied().unwrap_or(0.0);
        if base_score <= 0.0 {
            continue;
        }

        if let Some(&idx) = id_to_idx.get(&id) {
            let s = &index.symbol_coords[idx];

            let per_term =
                per_term_similarities(query, &index.term_index, &index.term_basis, s, dim);
            let geo_mean = geometric_mean(&per_term);
            let min_term = per_term.iter().cloned().fold(1.0f64, f64::min);
            let coverage = if per_term.is_empty() {
                0.0
            } else {
                per_term.iter().filter(|&&s| s > 0.3).count() as f64 / per_term.len() as f64
            };

            let af26_multiplier = {
                let multi_concept_boost = if per_term.len() > 1 {
                    1.0 + 0.1 * geo_mean * coverage
                } else {
                    1.0 + 0.05 * geo_mean
                };

                let mut graph_proximity = 0.0f64;
                let mut graph_count = 0usize;
                for &(ni, w) in &index.gravity[idx] {
                    if seed_set.contains(&ni) {
                        graph_proximity += w;
                        graph_count += 1;
                    }
                }
                let graph_boost = if graph_count > 0 {
                    1.0 + 0.08 * (graph_proximity / graph_count as f64).min(1.0)
                } else {
                    1.0
                };

                let mut curvature_factor = 0.0f64;
                for &(ni, w) in &index.gravity[idx] {
                    let ns = &index.symbol_coords[ni];
                    let neighbor_sim = cosine(&query_vec, ns);
                    if neighbor_sim > 0.3 {
                        curvature_factor += w * neighbor_sim;
                    }
                }
                let manifold_boost = 1.0 + 0.03 * curvature_factor.min(2.0);

                let centrality_boost = 1.0 + 0.015 * index.centrality[idx];

                let semantic_agreement = if min_term > 0.5 && coverage > 0.8 {
                    1.05
                } else {
                    1.0
                };

                multi_concept_boost
                    * graph_boost
                    * manifold_boost
                    * centrality_boost
                    * semantic_agreement
            };

            scored.push((id, base_score * af26_multiplier));
        } else {
            scored.push((id, base_score));
        }
    }

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored
}

fn spherical_kmeans(vecs: &[Vec<f64>], k: usize, max_iter: usize) -> (Vec<Cell>, Vec<usize>) {
    let n = vecs.len();
    let d = vecs[0].len();

    let mut centroids: Vec<Vec<f64>> = Vec::with_capacity(k);
    centroids.push(vecs[0].clone());

    for seed in 1..k {
        let min_cos: Vec<f64> = vecs
            .iter()
            .map(|v| {
                centroids
                    .iter()
                    .map(|c| {
                        let dot: f64 = v.iter().zip(c.iter()).map(|(a, b)| a * b).sum();
                        dot
                    })
                    .fold(f64::NEG_INFINITY, f64::max)
            })
            .collect();
        let gap: Vec<f64> = min_cos.iter().map(|&c| (1.0 - c).max(0.0)).collect();
        let total: f64 = gap.iter().sum();
        if total < 1e-10 {
            centroids.push(vecs[seed % n].clone());
            continue;
        }
        let threshold = (seed as f64 * 0.618033988749895 * total) % total;
        let mut acc = 0.0f64;
        let mut chosen = n - 1;
        for (i, &g) in gap.iter().enumerate() {
            acc += g;
            if acc >= threshold {
                chosen = i;
                break;
            }
        }
        centroids.push(vecs[chosen].clone());
    }

    let mut assignments: Vec<usize> = vec![0; n];

    for _ in 0..max_iter {
        let mut converged = true;

        for (i, v) in vecs.iter().enumerate() {
            let (best, best_sim) = centroids
                .iter()
                .enumerate()
                .map(|(ci, c)| {
                    let dot: f64 = v.iter().zip(c.iter()).map(|(a, b)| a * b).sum();
                    (ci, dot)
                })
                .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
                .unwrap();
            if assignments[i] != best {
                converged = false;
                assignments[i] = best;
            }
        }

        let mut new_centroids: Vec<Vec<f64>> = (0..k).map(|_| vec![0.0; d]).collect();
        let mut counts: Vec<usize> = vec![0; k];
        for (i, &ci) in assignments.iter().enumerate() {
            counts[ci] += 1;
            for j in 0..d {
                new_centroids[ci][j] += vecs[i][j];
            }
        }
        for ci in 0..k {
            if counts[ci] > 0 {
                let norm: f64 = new_centroids[ci].iter().map(|x| x * x).sum::<f64>().sqrt();
                if norm > 1e-10 {
                    for x in new_centroids[ci].iter_mut() {
                        *x /= norm;
                    }
                }
            } else {
                let mut farthest = 0usize;
                let mut min_sim = f64::INFINITY;
                for (i, v) in vecs.iter().enumerate() {
                    let sim: f64 = centroids
                        .iter()
                        .map(|c| v.iter().zip(c.iter()).map(|(a, b)| a * b).sum::<f64>())
                        .fold(f64::NEG_INFINITY, f64::max);
                    if sim < min_sim {
                        min_sim = sim;
                        farthest = i;
                    }
                }
                new_centroids[ci] = vecs[farthest].clone();
            }
        }
        centroids = new_centroids;

        if converged {
            break;
        }
    }

    let mut cells: Vec<Cell> = (0..k)
        .map(|_| Cell {
            members: Vec::new(),
            centroid: centroids[0].clone(),
        })
        .collect();
    for ci in 0..k {
        cells[ci].centroid = centroids[ci].clone();
        cells[ci].members = Vec::new();
    }
    for (i, &ci) in assignments.iter().enumerate() {
        cells[ci].members.push(i);
    }
    cells.retain(|c| !c.members.is_empty());

    let mut remap: HashMap<usize, usize> = HashMap::new();
    for (new_ci, cell) in cells.iter().enumerate() {
        for &si in &cell.members {
            remap.insert(si, new_ci);
        }
    }
    let final_assignments: Vec<usize> = (0..n).map(|i| *remap.get(&i).unwrap_or(&0)).collect();

    (cells, final_assignments)
}

pub fn af27_hybrid_search(
    query: &str,
    candidate_ids: &[i64],
    candidate_scores: &[f64],
    index: &Af26Index,
) -> Vec<(i64, f64)> {
    let id_to_idx: HashMap<i64, usize> = index
        .symbol_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i))
        .collect();

    let dim = index.sigma.len();
    if dim == 0 || candidate_ids.is_empty() || index.cells.is_empty() {
        return candidate_ids
            .iter()
            .zip(candidate_scores.iter())
            .map(|(id, s)| (*id, *s))
            .collect();
    }

    let sigma_norm: f64 = index
        .sigma
        .iter()
        .take(index.coarse_dim)
        .map(|s| s * s)
        .sum::<f64>()
        .sqrt();
    let query_vec = project_query(
        query,
        &index.term_index,
        &index.term_basis,
        &index.term_idf,
        dim,
    );

    let mut query_coarse = Vec::with_capacity(index.coarse_dim);
    for j in 0..index.coarse_dim {
        query_coarse.push(query_vec[j] * index.sigma[j] / sigma_norm);
    }
    let qcn: f64 = query_coarse.iter().map(|x| x * x).sum::<f64>().sqrt();
    if qcn > 1e-10 {
        for x in query_coarse.iter_mut() {
            *x /= qcn;
        }
    }

    let cell_scores: Vec<(usize, f64)> = index
        .cells
        .iter()
        .enumerate()
        .map(|(ci, cell)| {
            let dot: f64 = query_coarse
                .iter()
                .zip(cell.centroid.iter())
                .map(|(a, b)| a * b)
                .sum();
            (ci, dot.max(0.0))
        })
        .filter(|(_, s)| *s > 0.0)
        .collect();

    let max_cell_sim = cell_scores.iter().map(|(_, s)| *s).fold(0.0f64, f64::max);
    if max_cell_sim <= 0.0 {
        return candidate_ids
            .iter()
            .zip(candidate_scores.iter())
            .map(|(id, s)| (*id, *s))
            .collect();
    }

    let n_top_cells = 3.max(index.cells.len() / 3);
    let mut ranked_cells: Vec<(usize, f64)> = cell_scores;
    ranked_cells.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    let top_cells: HashMap<usize, f64> = ranked_cells
        .into_iter()
        .take(n_top_cells)
        .map(|(ci, sim)| (ci, sim / max_cell_sim))
        .collect();

    let mut scored: Vec<(i64, f64)> = Vec::with_capacity(candidate_ids.len());

    for (i, &id) in candidate_ids.iter().enumerate() {
        let base_score = candidate_scores.get(i).copied().unwrap_or(0.0);
        if base_score <= 0.0 {
            continue;
        }

        if let Some(&idx) = id_to_idx.get(&id) {
            let sym_cell = index.symbol_cell[idx];

            let cell_strength = top_cells.get(&sym_cell).copied().unwrap_or(0.0);

            let gate_boost = if cell_strength > 0.8 {
                1.0 + 0.12 * cell_strength
            } else if cell_strength > 0.5 {
                1.0 + 0.06 * cell_strength
            } else if cell_strength > 0.0 {
                1.0 + 0.02 * cell_strength
            } else {
                1.0
            };

            let s = &index.symbol_coords[idx];
            let per_term =
                per_term_similarities(query, &index.term_index, &index.term_basis, s, dim);
            let geo_mean = geometric_mean(&per_term);

            let semantic_gate = if per_term.len() > 1 && geo_mean > 0.55 {
                1.0 + 0.04 * (geo_mean - 0.55)
            } else {
                1.0
            };

            let mut neighbor_cell_agreement = 0usize;
            let mut neighbor_total = 0usize;
            for &(ni, _) in &index.gravity[idx] {
                let n_cell = index.symbol_cell[ni];
                if top_cells.contains_key(&n_cell) {
                    neighbor_cell_agreement += 1;
                }
                neighbor_total += 1;
            }
            let neighbor_gate = if neighbor_total > 0 {
                let ratio = neighbor_cell_agreement as f64 / neighbor_total as f64;
                if ratio > 0.5 {
                    1.0 + 0.03 * ratio
                } else {
                    1.0
                }
            } else {
                1.0
            };

            scored.push((id, base_score * gate_boost * semantic_gate * neighbor_gate));
        } else {
            scored.push((id, base_score));
        }
    }

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored
}
