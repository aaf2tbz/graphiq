use std::collections::{HashMap, HashSet, VecDeque};

use crate::db::GraphDb;
use crate::lsa::{extract_terms, load_latent_vectors, load_lsa_basis, load_lsa_sigma};

pub struct AfmoIndex {
    pub symbol_ids: Vec<i64>,
    pub poincare_coords: Vec<Vec<f64>>,
    pub hierarchy_depth: Vec<f64>,
    pub term_index: HashMap<String, usize>,
    pub term_basis: Vec<Vec<f64>>,
    pub term_idf: Vec<f64>,
    pub sigma: Vec<f64>,
    pub gravity: Vec<Vec<(usize, f64)>>,
    pub centrality: Vec<f64>,
    pub hyperbolic_dim: usize,
    pub symbol_generality: Vec<f64>,
    pub subtree_ids: Vec<u64>,
}

fn poincare_distance(x: &[f64], y: &[f64]) -> f64 {
    let x_sq: f64 = x.iter().map(|xi| xi * xi).sum();
    let y_sq: f64 = y.iter().map(|yi| yi * yi).sum();
    let diff_sq: f64 = x
        .iter()
        .zip(y.iter())
        .map(|(xi, yi)| (xi - yi) * (xi - yi))
        .sum();

    let denom = (1.0 - x_sq) * (1.0 - y_sq);
    if denom <= 1e-10 {
        return 100.0;
    }

    let alpha = 1.0 + 2.0 * diff_sq / denom;
    if alpha <= 1.0 {
        return 0.0;
    }

    alpha.acosh()
}

fn poincare_similarity(x: &[f64], y: &[f64]) -> f64 {
    let d = poincare_distance(x, y);
    1.0 / (1.0 + d)
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

fn compute_hierarchy(db: &GraphDb, symbol_ids: &[i64]) -> (Vec<f64>, Vec<u64>) {
    let n = symbol_ids.len();
    let id_to_idx: HashMap<i64, usize> = symbol_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i))
        .collect();

    let conn = db.conn();

    let mut kind_map: HashMap<i64, String> = HashMap::new();
    let mut file_map: HashMap<i64, i64> = HashMap::new();
    let mut qualified_map: HashMap<i64, String> = HashMap::new();
    {
        let mut stmt = conn
            .prepare("SELECT id, kind, file_id, qualified_name FROM symbols")
            .unwrap();
        let rows: Vec<(i64, String, i64, Option<String>)> = stmt
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .unwrap()
            .flatten()
            .collect();
        for (id, kind, file_id, qn) in &rows {
            kind_map.insert(*id, kind.clone());
            file_map.insert(*id, *file_id);
            if let Some(q) = qn {
                qualified_map.insert(*id, q.clone());
            }
        }
    }

    let mut children: Vec<HashSet<usize>> = vec![HashSet::new(); n];
    let mut parents: Vec<HashSet<usize>> = vec![HashSet::new(); n];

    {
        if let Ok(mut stmt) = conn.prepare("SELECT source_id, target_id, kind FROM edges") {
            let rows: Vec<(i64, i64, String)> = stmt
                .query_map([], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get::<_, String>(2)?))
                })
                .unwrap()
                .flatten()
                .collect();

            for (src, tgt, kind) in &rows {
                if let (Some(&si), Some(&ti)) = (id_to_idx.get(src), id_to_idx.get(tgt)) {
                    if kind == "Contains" {
                        children[si].insert(ti);
                        parents[ti].insert(si);
                    }
                }
            }
        }
    }

    let mut file_groups: HashMap<i64, Vec<usize>> = HashMap::new();
    for (i, &id) in symbol_ids.iter().enumerate() {
        if let Some(&fid) = file_map.get(&id) {
            file_groups.entry(fid).or_default().push(i);
        }
    }

    let mut file_module_idx: HashMap<i64, usize> = HashMap::new();
    for (i, &id) in symbol_ids.iter().enumerate() {
        if let Some(kind) = kind_map.get(&id) {
            if kind == "module" {
                if let Some(&fid) = file_map.get(&id) {
                    file_module_idx.insert(fid, i);
                }
            }
        }
    }

    for (&fid, members) in &file_groups {
        if let Some(&mod_idx) = file_module_idx.get(&fid) {
            for &member_idx in members {
                if member_idx != mod_idx && !parents[member_idx].contains(&mod_idx) {
                    children[mod_idx].insert(member_idx);
                    parents[member_idx].insert(mod_idx);
                }
            }
        }
    }

    let mut qualified_name_map: HashMap<String, usize> = HashMap::new();
    for (i, &id) in symbol_ids.iter().enumerate() {
        if let Some(qn) = qualified_map.get(&id) {
            qualified_name_map.insert(qn.clone(), i);
        }
    }

    for (i, &id) in symbol_ids.iter().enumerate() {
        if let Some(qn) = qualified_map.get(&id) {
            if let Some(kind) = kind_map.get(&id) {
                if kind == "method" || kind == "field" || kind == "constant" {
                    if let Some(dot_pos) = qn.rfind('.') {
                        let parent_name = &qn[..dot_pos];
                        if let Some(&parent_idx) = qualified_name_map.get(parent_name) {
                            if parent_idx != i && !parents[i].contains(&parent_idx) {
                                children[parent_idx].insert(i);
                                parents[i].insert(parent_idx);
                            }
                        }
                    }
                }
            }
        }
    }

    let mut depth: Vec<f64> = vec![-1.0; n];

    let mut roots: Vec<usize> = Vec::new();
    for i in 0..n {
        if parents[i].is_empty() && !children[i].is_empty() {
            roots.push(i);
        }
    }
    if roots.is_empty() {
        for i in 0..n {
            if let Some(kind) = kind_map.get(&symbol_ids[i]) {
                if kind == "module" {
                    roots.push(i);
                }
            }
        }
    }

    let mut queue: VecDeque<usize> = VecDeque::new();
    for &r in &roots {
        depth[r] = 0.0;
        queue.push_back(r);
    }

    while let Some(node) = queue.pop_front() {
        for &child in &children[node] {
            if depth[child] < 0.0 {
                depth[child] = depth[node] + 1.0;
                queue.push_back(child);
            }
        }
    }

    let fallback = infer_depth_from_kind(&kind_map, symbol_ids, n);
    for i in 0..n {
        if depth[i] < 0.0 {
            depth[i] = fallback[i];
        }
    }

    let subtree_ids = compute_subtree_hashes(&children, &depth, n);

    (depth, subtree_ids)
}

fn infer_depth_from_kind(
    kind_map: &HashMap<i64, String>,
    symbol_ids: &[i64],
    n: usize,
) -> Vec<f64> {
    (0..n)
        .map(|i| match kind_map.get(&symbol_ids[i]).map(|s| s.as_str()) {
            Some("module") | Some("file") => 0.0,
            Some("struct") | Some("enum") | Some("trait") | Some("impl") | Some("section") => 1.0,
            Some("method") | Some("function") | Some("constant") | Some("import") => 2.0,
            _ => 1.5,
        })
        .collect()
}

fn compute_subtree_hashes(children: &[HashSet<usize>], depth: &[f64], n: usize) -> Vec<u64> {
    let mut subtree_ids: Vec<u64> = (0..n)
        .map(|i| {
            let mut h: u64 = 0xa5a5_a5a5_a5a5_a5a5u64;
            h ^= (i as u64).wrapping_mul(0x9e3779b97f4a7c15);
            h = h.wrapping_mul(0x85ebca6b);
            h ^= h >> 13;
            h = h.wrapping_mul(0xc2b2ae35);
            h ^= h >> 16;
            h
        })
        .collect();

    let max_depth = depth.iter().copied().fold(0.0f64, f64::max);
    for d in (0..=max_depth as usize).rev() {
        for i in 0..n {
            if depth[i] as usize == d {
                let mut h = subtree_ids[i];
                let mut child_hashes: Vec<u64> =
                    children[i].iter().map(|&c| subtree_ids[c]).collect();
                child_hashes.sort();
                for ch in child_hashes {
                    h ^= ch.wrapping_mul(0x9e3779b97f4a7c15);
                    h = h.wrapping_add(h << 6);
                    h = h.wrapping_sub(h >> 2);
                }
                subtree_ids[i] = h;
            }
        }
    }

    subtree_ids
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

pub fn compute_afmo(db: &GraphDb) -> Result<AfmoIndex, String> {
    eprintln!("  [afmo] loading LSA foundation...");
    let (term_basis, term_index, term_idf) = load_lsa_basis(db)?;
    let symbol_map = load_latent_vectors(db)?;

    if term_basis.is_empty() || symbol_map.is_empty() {
        return Err("no LSA data available".into());
    }

    let mut symbol_ids: Vec<i64> = symbol_map.keys().copied().collect();
    symbol_ids.sort();
    let symbol_vecs: Vec<Vec<f64>> = symbol_ids.iter().map(|id| symbol_map[id].clone()).collect();

    let svd_dim = symbol_vecs[0].len();
    let hyperbolic_dim = 16.min(svd_dim);

    eprintln!(
        "  [afmo] {} symbols, svd_dim={}, hyperbolic_dim={}",
        symbol_ids.len(),
        svd_dim,
        hyperbolic_dim
    );

    let sigma = load_lsa_sigma(db).unwrap_or_else(|_| {
        eprintln!("  [afmo] WARNING: no stored sigma");
        vec![1.0; svd_dim]
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

    let (hierarchy_depth, subtree_ids) = compute_hierarchy(db, &symbol_ids);

    let mut generality: Vec<f64> = vec![0.0; symbol_ids.len()];
    for i in 0..symbol_ids.len() {
        let n_children = gravity[i]
            .iter()
            .filter(|(ni, _)| hierarchy_depth[*ni] > hierarchy_depth[i])
            .count();
        let depth_factor = 1.0 / (1.0 + hierarchy_depth[i]);
        let child_factor = (n_children as f64).ln_1p();
        let call_factor = gravity[i]
            .iter()
            .filter(|(ni, _)| hierarchy_depth[*ni] == hierarchy_depth[i])
            .count() as f64
            * 0.1;
        generality[i] = depth_factor * (1.0 + child_factor) + call_factor;
    }
    let max_gen = generality.iter().copied().fold(0.0f64, f64::max);
    if max_gen > 0.0 {
        for g in generality.iter_mut() {
            *g /= max_gen;
        }
    }

    let sigma_top: Vec<f64> = sigma.iter().take(hyperbolic_dim).copied().collect();
    let sigma_norm: f64 = sigma_top.iter().map(|s| s * s).sum::<f64>().sqrt();

    let poincare_coords: Vec<Vec<f64>> = symbol_vecs
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let mut dir = Vec::with_capacity(hyperbolic_dim);
            for j in 0..hyperbolic_dim {
                dir.push(v[j] * sigma[j] / sigma_norm);
            }
            let dir_norm: f64 = dir.iter().map(|x| x * x).sum::<f64>().sqrt();
            if dir_norm > 1e-10 {
                for x in dir.iter_mut() {
                    *x /= dir_norm;
                }
            }

            let max_radius = 0.95;
            let depth_clamped = hierarchy_depth[i].max(0.0).min(4.0);
            let gen_factor = 1.0 - 0.3 * generality[i];
            let r = max_radius * (1.0 - (-depth_clamped * 1.0).exp()) * gen_factor;

            dir.iter().map(|x| r * x).collect()
        })
        .collect();

    let depth_hist: Vec<usize> = {
        let max_d = hierarchy_depth.iter().copied().fold(0.0f64, f64::max) as usize;
        let mut h = vec![0usize; max_d + 1];
        for &d in &hierarchy_depth {
            h[d as usize] += 1;
        }
        h
    };
    eprintln!(
        "  [afmo] hierarchy: depths [{}], {} gravity edges",
        depth_hist
            .iter()
            .enumerate()
            .map(|(d, c)| format!("{}:{}", d, c))
            .collect::<Vec<_>>()
            .join(" "),
        n_edges
    );

    let mut radius_stats = vec![0.0f64, f64::MAX, 0.0f64, 0.0f64];
    for coord in &poincare_coords {
        let r: f64 = coord.iter().map(|x| x * x).sum::<f64>().sqrt();
        radius_stats[0] += r;
        radius_stats[1] = radius_stats[1].min(r);
        radius_stats[2] = radius_stats[2].max(r);
    }
    radius_stats[3] = radius_stats[0] / symbol_ids.len() as f64;
    eprintln!(
        "  [afmo] radii: min={:.3} max={:.3} mean={:.3}",
        radius_stats[1], radius_stats[2], radius_stats[3]
    );

    Ok(AfmoIndex {
        symbol_ids,
        poincare_coords,
        hierarchy_depth,
        term_index,
        term_basis,
        term_idf,
        sigma,
        gravity,
        centrality,
        hyperbolic_dim,
        symbol_generality: generality,
        subtree_ids,
    })
}

pub fn afmo_search(query: &str, index: &AfmoIndex, top_k: usize) -> Vec<(i64, f64)> {
    let dim = index.sigma.len();
    if dim == 0 {
        return Vec::new();
    }

    let query_dir = project_query(
        query,
        &index.term_index,
        &index.term_basis,
        &index.term_idf,
        dim,
    );

    let mut query_hyper = Vec::with_capacity(index.hyperbolic_dim);
    let sigma_norm: f64 = index
        .sigma
        .iter()
        .take(index.hyperbolic_dim)
        .map(|s| s * s)
        .sum::<f64>()
        .sqrt();
    for j in 0..index.hyperbolic_dim {
        query_hyper.push(query_dir[j] * index.sigma[j] / sigma_norm);
    }
    let qn: f64 = query_hyper.iter().map(|x| x * x).sum::<f64>().sqrt();
    if qn > 1e-10 {
        for x in query_hyper.iter_mut() {
            *x /= qn;
        }
    }

    let query_radius = 0.6;
    let query_point: Vec<f64> = query_hyper.iter().map(|x| query_radius * x).collect();

    let n = index.poincare_coords.len();
    let mut scored: Vec<(usize, f64)> = (0..n)
        .map(|i| {
            let hyp_sim = poincare_similarity(&query_point, &index.poincare_coords[i]);

            let depth_diff = (index.hierarchy_depth[i] - 1.5).abs();
            let depth_prior = 1.0 / (1.0 + depth_diff * 0.3);

            let score = hyp_sim * depth_prior;

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

pub fn afmo_rerank(
    query: &str,
    candidate_ids: &[i64],
    candidate_scores: &[f64],
    index: &AfmoIndex,
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

    let query_dir = project_query(
        query,
        &index.term_index,
        &index.term_basis,
        &index.term_idf,
        dim,
    );

    let mut query_hyper = Vec::with_capacity(index.hyperbolic_dim);
    let sigma_norm: f64 = index
        .sigma
        .iter()
        .take(index.hyperbolic_dim)
        .map(|s| s * s)
        .sum::<f64>()
        .sqrt();
    for j in 0..index.hyperbolic_dim {
        query_hyper.push(query_dir[j] * index.sigma[j] / sigma_norm);
    }
    let qn: f64 = query_hyper.iter().map(|x| x * x).sum::<f64>().sqrt();
    if qn > 1e-10 {
        for x in query_hyper.iter_mut() {
            *x /= qn;
        }
    }

    let top_fts_indices: Vec<usize> = candidate_ids
        .iter()
        .take(10)
        .filter_map(|id| id_to_idx.get(id).copied())
        .collect();

    let avg_depth = if top_fts_indices.is_empty() {
        1.5
    } else {
        let sum: f64 = top_fts_indices
            .iter()
            .map(|&idx| index.hierarchy_depth[idx])
            .sum();
        sum / top_fts_indices.len() as f64
    };

    let max_radius = 0.95;
    let query_radius = max_radius * (1.0 - (-(avg_depth.max(0.0).min(4.0)) * 1.0).exp());
    let query_point: Vec<f64> = query_hyper.iter().map(|x| query_radius * x).collect();

    let seed_set: HashSet<usize> = top_fts_indices.iter().copied().collect();

    let mut hyp_sim_hist = vec![0usize; 10];
    let mut scored: Vec<(i64, f64)> = Vec::with_capacity(candidate_ids.len());

    for (i, &id) in candidate_ids.iter().enumerate() {
        let base_score = candidate_scores.get(i).copied().unwrap_or(0.0);
        if base_score <= 0.0 {
            continue;
        }

        if let Some(&idx) = id_to_idx.get(&id) {
            let hyp_sim = poincare_similarity(&query_point, &index.poincare_coords[idx]);
            let bin = (hyp_sim * 10.0).min(9.0) as usize;
            hyp_sim_hist[bin] += 1;

            let hyp_boost = if hyp_sim > 0.7 {
                1.0 + 0.35 * hyp_sim
            } else if hyp_sim > 0.5 {
                1.0 + 0.20 * hyp_sim
            } else if hyp_sim > 0.3 {
                1.0 + 0.08 * hyp_sim
            } else {
                1.0
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
                let avg_w = graph_proximity / graph_count as f64;
                1.0 + 0.10 * avg_w.min(1.0)
            } else {
                1.0
            };

            let depth_agreement =
                1.0 / (1.0 + (index.hierarchy_depth[idx] - avg_depth).abs() * 0.2);
            let depth_boost = 1.0 + 0.08 * (depth_agreement - 0.5).max(0.0);

            let mut same_tree_count = 0usize;
            let mut neighbor_count = 0usize;
            for &(ni, _) in &index.gravity[idx] {
                if index.subtree_ids[ni] == index.subtree_ids[idx] {
                    same_tree_count += 1;
                }
                neighbor_count += 1;
            }
            let subtree_cohesion = if neighbor_count > 0 {
                same_tree_count as f64 / neighbor_count as f64
            } else {
                0.0
            };
            let subtree_boost = if subtree_cohesion > 0.5 && hyp_sim > 0.4 {
                1.0 + 0.06 * subtree_cohesion
            } else {
                1.0
            };

            let score = base_score * hyp_boost * graph_boost * depth_boost * subtree_boost;
            scored.push((id, score));
        } else {
            scored.push((id, base_score));
        }
    }

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    eprintln!(
        "  [afmo-rerank] q='{}' hyp=[{}] n={}",
        &query[..query.len().min(25)],
        hyp_sim_hist
            .iter()
            .enumerate()
            .map(|(b, &c)| format!("{:.1}:{}", b as f64 / 10.0, c))
            .collect::<Vec<_>>()
            .join(" "),
        scored.len()
    );

    scored
}
