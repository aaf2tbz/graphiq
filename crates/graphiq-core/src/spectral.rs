use std::collections::{HashMap, HashSet};

use rusqlite::params;

use crate::db::GraphDb;
use crate::tokenize::decompose_identifier;

pub const SPECTRAL_DIM: usize = 50;

pub struct SpectralIndex {
    pub symbol_ids: Vec<i64>,
    pub symbol_coords: Vec<Vec<f64>>,
    pub sym_id_to_idx: HashMap<i64, usize>,
    pub eigenvalues: Vec<f64>,
    pub lambda_max: f64,
    pub graph: SparseGraph,
}

struct SparseSym {
    n: usize,
    entries: HashMap<(usize, usize), f64>,
}

impl SparseSym {
    fn new(n: usize) -> Self {
        Self {
            n,
            entries: HashMap::new(),
        }
    }

    fn add(&mut self, i: usize, j: usize, w: f64) {
        if i == j || w <= 0.0 {
            return;
        }
        let (a, b) = if i < j { (i, j) } else { (j, i) };
        *self.entries.entry((a, b)).or_insert(0.0) += w;
    }

    fn degree(&self) -> Vec<f64> {
        let mut deg = vec![0.0; self.n];
        for (&(i, j), &w) in &self.entries {
            deg[i] += w;
            deg[j] += w;
        }
        deg
    }

    fn matvec(&self, x: &[f64]) -> Vec<f64> {
        let mut y = vec![0.0; self.n];
        for (&(i, j), &w) in &self.entries {
            y[i] += w * x[j];
            y[j] += w * x[i];
        }
        y
    }
}

pub struct SparseGraph {
    pub n: usize,
    pub adj: SparseSym,
    pub inv_sqrt_d: Vec<f64>,
    pub edge_curvature: Option<Vec<f64>>,
    pub structural_edges: Vec<(usize, usize, f64)>,
}

impl SparseGraph {
    pub fn laplacian_matvec(&self, x: &[f64]) -> Vec<f64> {
        let wv = self.adj.matvec(x);
        let mut lv = vec![0.0; self.n];
        for i in 0..self.n {
            lv[i] = x[i] - self.inv_sqrt_d[i] * wv[i] * self.inv_sqrt_d[i];
        }
        lv
    }

    pub fn rescaled_laplacian_matvec(&self, x: &[f64]) -> Vec<f64> {
        let lx = self.laplacian_matvec(x);
        let scale = 2.0 / self.lambda_max();
        let mut y = vec![0.0; self.n];
        for i in 0..self.n {
            y[i] = scale * lx[i] - x[i];
        }
        y
    }

    pub fn lambda_max(&self) -> f64 {
        let deg = self.adj.degree();
        deg.into_iter().fold(0.0f64, f64::max).max(1.0)
    }

    pub fn curvature_weighted_matvec(&self, x: &[f64]) -> Vec<f64> {
        let mut y = vec![0.0; self.n];
        if let Some(ref kappa) = self.edge_curvature {
            for &(i, j, w) in &self.structural_edges {
                let k_raw = kappa[i * self.n + j];
                let k = (1.0 + k_raw).max(0.1);
                let wk = w * k;
                y[i] += wk * x[j];
                y[j] += wk * x[i];
            }
        } else {
            y = self.adj.matvec(x);
        }
        y
    }

    pub fn curvature_laplacian_matvec(&self, x: &[f64]) -> Vec<f64> {
        let wv = self.curvature_weighted_matvec(x);
        let degree = self.adj.degree();
        let mut lv = vec![0.0; self.n];
        for i in 0..self.n {
            if degree[i] > 1e-10 {
                lv[i] = x[i] - wv[i] / degree[i];
            }
        }
        lv
    }
}

pub fn build_adjacency(db: &GraphDb) -> (SparseSym, Vec<i64>, HashMap<i64, usize>, Vec<(usize, usize, f64)>) {
    let conn = db.conn();

    let mut sym_stmt = conn
        .prepare("SELECT id FROM symbols WHERE visibility = 'public' ORDER BY id")
        .unwrap();
    let symbol_ids: Vec<i64> = sym_stmt
        .query_map([], |row| row.get::<_, i64>(0))
        .unwrap()
        .flatten()
        .collect();
    drop(sym_stmt);

    let n = symbol_ids.len();
    let sym_id_to_idx: HashMap<i64, usize> = symbol_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i))
        .collect();

    let mut adj = SparseSym::new(n);

    let edge_weights = [
        ("calls", 1.0),
        ("imports", 0.8),
        ("extends", 1.2),
        ("implements", 1.1),
        ("references", 0.6),
        ("overrides", 0.9),
        ("tests", 0.4),
        ("re_exports", 0.7),
        ("contains", 0.5),
        ("shares_constant", 0.3),
        ("references_constant", 0.6),
        ("shares_type", 0.35),
        ("shares_error_type", 0.50),
        ("shares_data_shape", 0.30),
    ];

    let mut edge_stmt = conn
        .prepare("SELECT source_id, target_id, kind FROM edges")
        .unwrap();
    let edges: Vec<(i64, i64, String)> = edge_stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .unwrap()
        .flatten()
        .collect();
    drop(edge_stmt);

    let mut edge_count = 0usize;
    let mut struct_edges: Vec<(usize, usize, f64)> = Vec::new();
    for (src, tgt, kind) in &edges {
        if let (Some(&si), Some(&ti)) = (sym_id_to_idx.get(src), sym_id_to_idx.get(tgt)) {
            let w = edge_weights
                .iter()
                .find_map(|(k, w)| if *k == kind { Some(*w) } else { None })
                .unwrap_or(0.3);
            adj.add(si, ti, w);
            struct_edges.push((si, ti, w));
            edge_count += 1;
        }
    }

    eprintln!(
        "  adjacency: {} symbols, {} structural edges",
        n, edge_count
    );

    let mut text_stmt = conn
        .prepare(
            "SELECT id, name, name_decomposed, signature, doc_comment \
             FROM symbols WHERE visibility = 'public'",
        )
        .unwrap();
    let sym_texts: Vec<(i64, String)> = text_stmt
        .query_map([], |row| {
            let id: i64 = row.get(0)?;
            let name: String = row.get::<_, String>(1).unwrap_or_default();
            let decomp: String = row.get::<_, String>(2).unwrap_or_default();
            let sig: String = row.get::<_, String>(3).unwrap_or_default();
            let doc: String = row.get::<_, String>(4).unwrap_or_default();
            Ok((id, format!("{} {} {} {}", name, decomp, sig, doc)))
        })
        .unwrap()
        .flatten()
        .collect();
    drop(text_stmt);

    let sym_terms: Vec<std::collections::HashSet<String>> = sym_texts
        .iter()
        .map(|(_, text)| {
            text.split_whitespace()
                .filter(|t| t.len() >= 2)
                .map(|t| t.to_lowercase())
                .collect()
        })
        .collect();

    let mut term_edges = 0usize;
    for i in 0..n {
        if sym_terms[i].len() > 200 {
            continue;
        }
        for j in (i + 1)..n {
            if sym_terms[j].len() > 200 {
                continue;
            }
            let intersection = sym_terms[i]
                .iter()
                .filter(|t| sym_terms[j].contains(*t))
                .count();
            if intersection < 2 {
                continue;
            }
            let union = sym_terms[i].len() + sym_terms[j].len() - intersection;
            let jaccard = intersection as f64 / union as f64;
            let w = (jaccard * 0.3).min(0.3);
            if w > 0.01 {
                adj.add(i, j, w);
                term_edges += 1;
            }
        }
    }

    eprintln!("  added {} term-overlap edges", term_edges);

    (adj, symbol_ids, sym_id_to_idx, struct_edges)
}

pub fn compute_spectral(db: &GraphDb) -> Result<SpectralIndex, String> {
    let (adj, symbol_ids, sym_id_to_idx, struct_edges) = build_adjacency(db);
    let n = adj.n;
    let k = SPECTRAL_DIM.min(n.saturating_sub(1));

    let degree = adj.degree();
    let mut inv_sqrt_d = vec![0.0; n];
    for i in 0..n {
        if degree[i] > 1e-10 {
            inv_sqrt_d[i] = 1.0 / degree[i].sqrt();
        }
    }

    let graph = SparseGraph {
        n,
        adj,
        inv_sqrt_d,
        edge_curvature: None,
        structural_edges: struct_edges,
    };

    let graph_adj = &graph.adj;
    let graph_inv_sqrt_d = &graph.inv_sqrt_d;

    let lap_vec = |v: &[f64]| -> Vec<f64> {
        let wv = graph_adj.matvec(v);
        let mut lv = vec![0.0; n];
        for i in 0..n {
            lv[i] = v[i] - graph_inv_sqrt_d[i] * wv[i] * graph_inv_sqrt_d[i];
        }
        lv
    };

    let lanczos_m = (3 * k + 30).min(n);
    let mut rng = SimpleRng::new(42);
    let mut v = vec![0.0; n];
    for x in v.iter_mut() {
        *x = rng.next();
    }
    let norm: f64 = v.iter().map(|x| x * x).sum::<f64>().sqrt();
    for x in v.iter_mut() {
        *x /= norm;
    }

    let mut Q: Vec<Vec<f64>> = Vec::with_capacity(lanczos_m);
    let mut alpha: Vec<f64> = Vec::with_capacity(lanczos_m);
    let mut beta: Vec<f64> = Vec::new();

    Q.push(v.clone());
    let mut w = lap_vec(&v);

    let a0: f64 = w.iter().zip(v.iter()).map(|(wi, vi)| wi * vi).sum();
    alpha.push(a0);
    for i in 0..n {
        w[i] -= a0 * v[i];
    }

    for j in 1..lanczos_m {
        for prev in &Q {
            let dot: f64 = w.iter().zip(prev.iter()).map(|(a, b)| a * b).sum();
            for i in 0..n {
                w[i] -= dot * prev[i];
            }
        }

        let b: f64 = w.iter().map(|x| x * x).sum::<f64>().sqrt();
        if b < 1e-14 {
            break;
        }
        beta.push(b);

        let qj: Vec<f64> = w.iter().map(|wi| wi / b).collect();

        w = lap_vec(&qj);
        for prev in &Q {
            let dot: f64 = w.iter().zip(prev.iter()).map(|(a, b)| a * b).sum();
            for i in 0..n {
                w[i] -= dot * prev[i];
            }
        }

        let aj: f64 = w.iter().zip(qj.iter()).map(|(wi, qi)| wi * qi).sum();
        alpha.push(aj);
        for i in 0..n {
            w[i] -= aj * qj[i];
        }

        Q.push(qj);
    }

    let m = alpha.len();
    eprintln!("  Lanczos built {}x{} tridiagonal", m, m);

    let (eigvals, eigvecs_t) = tridiag_eig(&alpha, &beta, m);

    let mut indexed: Vec<(usize, f64)> = eigvals.iter().enumerate().map(|(i, &v)| (i, v)).collect();
    indexed.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

    let skip = if indexed.first().map_or(false, |(_, v)| *v < 0.01) {
        1
    } else {
        0
    };

    let mut eigenvectors: Vec<Vec<f64>> = Vec::new();
    let mut kept_eigenvalues: Vec<f64> = Vec::new();

    for (rank, &(idx, _lambda)) in indexed.iter().enumerate() {
        if rank < skip || eigenvectors.len() >= k {
            continue;
        }
        kept_eigenvalues.push(eigvals[idx]);

        let s: Vec<f64> = (0..m)
            .map(|i| eig_vec_entry(&eigvecs_t, m, i, idx))
            .collect();
        let mut ev = vec![0.0; n];
        for (qi, si) in Q.iter().zip(s.iter()) {
            for r in 0..n {
                ev[r] += si * qi[r];
            }
        }
        let ev_norm: f64 = ev.iter().map(|x| x * x).sum::<f64>().sqrt();
        if ev_norm > 1e-10 {
            for x in ev.iter_mut() {
                *x /= ev_norm;
            }
        }
        eigenvectors.push(ev);
    }

    eprintln!("  eigenvalues: {:?}", kept_eigenvalues);

    let actual_k = eigenvectors.len();
    let lambda_max = kept_eigenvalues.last().copied().unwrap_or(1.0);

    let mut symbol_coords: Vec<Vec<f64>> = vec![vec![0.0; actual_k]; n];
    for (ev_idx, ev) in eigenvectors.iter().enumerate() {
        for i in 0..n {
            symbol_coords[i][ev_idx] = ev[i];
        }
    }

    Ok(SpectralIndex {
        symbol_ids,
        symbol_coords,
        sym_id_to_idx,
        eigenvalues: kept_eigenvalues,
        lambda_max,
        graph,
    })
}

fn eig_vec_entry(data: &[Vec<f64>], _m: usize, i: usize, j: usize) -> f64 {
    data.get(i)
        .and_then(|row| row.get(j).copied())
        .unwrap_or(0.0)
}

fn tridiag_eig(alpha: &[f64], beta: &[f64], m: usize) -> (Vec<f64>, Vec<Vec<f64>>) {
    let mut d = alpha.to_vec();
    let mut e = vec![0.0; m];
    for i in 0..beta.len().min(m - 1) {
        e[i + 1] = beta[i];
    }

    let mut V: Vec<Vec<f64>> = (0..m)
        .map(|i| {
            let mut row = vec![0.0; m];
            row[i] = 1.0;
            row
        })
        .collect();

    for _ in 0..200 * m {
        let mut total_off = 0.0;
        for i in 0..m - 1 {
            total_off += e[i + 1] * e[i + 1];
        }
        if total_off < 1e-20 {
            break;
        }

        for i in 0..m - 1 {
            if e[i + 1].abs() < 1e-15 {
                continue;
            }

            let diff = d[i + 1] - d[i];
            let mu = 2.0 * e[i + 1];
            let t = if diff.abs() < 1e-15 {
                1.0
            } else {
                diff.signum()
                    / (0.5 * (diff / mu).abs() + (1.0 + (diff / (2.0 * mu)).powi(2)).sqrt())
                        .max(1e-15)
                    * mu.signum()
            };
            let c = 1.0 / (1.0 + t * t).sqrt();
            let s = t * c;

            let tmp_d = d[i];
            d[i] = c * c * tmp_d + s * s * d[i + 1] - 2.0 * s * c * e[i + 1];
            d[i + 1] = s * s * tmp_d + c * c * d[i + 1] + 2.0 * s * c * e[i + 1];
            e[i + 1] = 0.0;

            if i + 2 < m {
                let new_e = s * e[i + 2];
                e[i + 2] = c * e[i + 2];
                e[i + 1] = new_e;
            }

            for row in 0..m {
                let vi = V[row][i];
                let vi1 = V[row][i + 1];
                V[row][i] = c * vi - s * vi1;
                V[row][i + 1] = s * vi + c * vi1;
            }
        }
    }

    (d, V)
}

pub fn spectral_search(
    query: &str,
    index: &SpectralIndex,
    db: &GraphDb,
    top_k: usize,
) -> Vec<(i64, f64)> {
    let conn = db.conn();

    let fts = crate::fts::FtsSearch::new(db);
    let fts_results = fts.search(query, Some(20));

    let mut seed_indices: Vec<usize> = fts_results
        .iter()
        .filter_map(|r| index.sym_id_to_idx.get(&r.symbol.id).copied())
        .collect();

    if seed_indices.is_empty() {
        let query_terms: Vec<&str> = query.split_whitespace().filter(|t| t.len() >= 2).collect();
        for term in &query_terms {
            let lower = term.to_lowercase();
            let decomp = decompose_identifier(&lower);
            let first_word = decomp.split_whitespace().next().unwrap_or(term);
            let pat = format!("%{}%", if decomp.len() > 2 { first_word } else { term });
            let mut stmt = conn
                .prepare(
                    "SELECT id FROM symbols WHERE visibility = 'public' AND \
                     (name LIKE ?1 OR name_decomposed LIKE ?1) LIMIT 10",
                )
                .unwrap();
            let ids: Vec<i64> = stmt
                .query_map(params![pat], |row| row.get::<_, i64>(0))
                .unwrap()
                .flatten()
                .collect();
            for id in ids {
                if let Some(&idx) = index.sym_id_to_idx.get(&id) {
                    seed_indices.push(idx);
                }
            }
            if seed_indices.len() >= 20 {
                break;
            }
        }
    }

    if seed_indices.is_empty() {
        return Vec::new();
    }

    let k = index.symbol_coords[0].len();
    let mut centroid = vec![0.0f64; k];
    for &si in &seed_indices {
        for j in 0..k {
            centroid[j] += index.symbol_coords[si][j];
        }
    }
    for x in centroid.iter_mut() {
        *x /= seed_indices.len() as f64;
    }

    let mut scored: Vec<(usize, f64)> = index
        .symbol_coords
        .iter()
        .enumerate()
        .map(|(i, coords)| {
            let dist: f64 = coords
                .iter()
                .zip(centroid.iter())
                .map(|(a, b)| (a - b) * (a - b))
                .sum();
            (i, dist)
        })
        .collect();
    scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    scored.truncate(top_k);

    let max_dist = scored.last().map(|(_, d)| *d).unwrap_or(1.0).max(1e-10);
    scored
        .into_iter()
        .map(|(i, dist)| (index.symbol_ids[i], 1.0 - (dist / max_dist).sqrt()))
        .collect()
}

pub fn spectral_distance(index: &SpectralIndex, idx_a: usize, idx_b: usize) -> f64 {
    let k = index.symbol_coords[idx_a].len().min(index.symbol_coords[idx_b].len());
    let mut dist: f64 = 0.0;
    for j in 0..k {
        let d = index.symbol_coords[idx_a][j] - index.symbol_coords[idx_b][j];
        dist += d * d;
    }
    dist
}

pub fn heat_kernel(
    index: &SpectralIndex,
    seed_indices: &[usize],
    seed_weights: &[f64],
    t: f64,
    top_k: usize,
) -> Vec<(usize, f64)> {
    let n = index.symbol_ids.len();
    let k = index.symbol_coords[0].len();
    if k == 0 || seed_indices.is_empty() {
        return Vec::new();
    }

    let mut signal = vec![0.0; n];
    for (i, w) in seed_indices.iter().zip(seed_weights.iter()) {
        signal[*i] += w;
    }

    for ev_j in 0..k {
        let lambda_j = index.eigenvalues[ev_j];
        let decay = (-t * lambda_j / index.lambda_max).exp();

        let mut dot: f64 = 0.0;
        for (si, w) in seed_indices.iter().zip(seed_weights.iter()) {
            dot += *w * index.symbol_coords[*si][ev_j];
        }

        for i in 0..n {
            signal[i] += decay * index.symbol_coords[i][ev_j] * dot;
        }
    }

    let mut scored: Vec<(usize, f64)> = signal
        .iter()
        .enumerate()
        .filter(|(_, &s)| s > 0.0)
        .map(|(i, s)| (i, *s))
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored.truncate(top_k);
    scored
}

pub fn chebyshev_heat(
    graph: &SparseGraph,
    seed_indices: &[usize],
    seed_weights: &[f64],
    t: f64,
    order: usize,
    top_k: usize,
) -> Vec<(usize, f64)> {
    let n = graph.n;
    if seed_indices.is_empty() || order == 0 {
        return Vec::new();
    }

    let lmax = graph.lambda_max();
    if lmax < 1e-10 {
        return Vec::new();
    }

    let f0 = |x: f64| -> f64 { (-t * x).exp() };
    let x_max = lmax;

    let c = (0..=order)
        .map(|k| cheb_coeff(f0, k, x_max, 1024))
        .collect::<Vec<f64>>();

    let mut f = vec![0.0; n];
    for (i, w) in seed_indices.iter().zip(seed_weights.iter()) {
        f[*i] += *w;
    }

    let twf = graph.rescaled_laplacian_matvec(&f);
    let mut y2 = vec![0.0; n];
    for i in 0..n {
        y2[i] = 2.0 * twf[i] - f[i];
    }
    let mut y1 = twf;

    let mut result = vec![0.0; n];
    for i in 0..n {
        result[i] = c[0] * f[i] + c[1] * y1[i];
    }

    for j in 2..=order {
        let ty = graph.rescaled_laplacian_matvec(&y2);
        let mut y_new = vec![0.0; n];
        for i in 0..n {
            y_new[i] = 2.0 * ty[i] - y1[i];
        }
        if j < c.len() {
            for i in 0..n {
                result[i] += c[j] * y_new[i];
            }
        }
        y1 = y2;
        y2 = y_new;
    }

    let mut scored: Vec<(usize, f64)> = result
        .iter()
        .enumerate()
        .filter(|(_, &s)| s > 1e-12)
        .map(|(i, s)| (i, *s))
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored.truncate(top_k);
    scored
}

pub fn chebyshev_heat_curved(
    graph: &SparseGraph,
    seed_indices: &[usize],
    seed_weights: &[f64],
    t: f64,
    order: usize,
    top_k: usize,
) -> Vec<(usize, f64)> {
    let n = graph.n;
    if seed_indices.is_empty() || order == 0 {
        return Vec::new();
    }

    let lmax = graph.lambda_max();
    if lmax < 1e-10 {
        return Vec::new();
    }

    let f0 = |x: f64| -> f64 { (-t * x).exp() };
    let x_max = lmax;

    let c = (0..=order)
        .map(|k| cheb_coeff(f0, k, x_max, 1024))
        .collect::<Vec<f64>>();

    let mut f = vec![0.0; n];
    for (i, w) in seed_indices.iter().zip(seed_weights.iter()) {
        f[*i] += *w;
    }

    let clv = |v: &[f64]| -> Vec<f64> {
        let cl = graph.curvature_laplacian_matvec(v);
        let scale = 2.0 / lmax;
        let mut y = vec![0.0; n];
        for i in 0..n {
            y[i] = scale * cl[i] - v[i];
        }
        y
    };

    let twf = clv(&f);
    let mut y2 = vec![0.0; n];
    for i in 0..n {
        y2[i] = 2.0 * twf[i] - f[i];
    }
    let mut y1 = twf;

    let mut result = vec![0.0; n];
    for i in 0..n {
        result[i] = c[0] * f[i] + c[1] * y1[i];
    }

    for j in 2..=order {
        let ty = clv(&y2);
        let mut y_new = vec![0.0; n];
        for i in 0..n {
            y_new[i] = 2.0 * ty[i] - y1[i];
        }
        if j < c.len() {
            for i in 0..n {
                result[i] += c[j] * y_new[i];
            }
        }
        y1 = y2;
        y2 = y_new;
    }

    let mut scored: Vec<(usize, f64)> = result
        .iter()
        .enumerate()
        .filter(|(_, &s)| s > 1e-12)
        .map(|(i, s)| (i, *s))
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored.truncate(top_k);
    scored
}

fn cheb_coeff<F: Fn(f64) -> f64>(f: F, k: usize, b: f64, n_quad: usize) -> f64 {
    let a = 0.0;
    let mid = (b + a) / 2.0;
    let half = (b - a) / 2.0;
    let mut sum = 0.0;
    for j in 0..n_quad {
        let theta = std::f64::consts::PI * (j as f64 + 0.5) / n_quad as f64;
        let x = mid + half * (-theta.cos());
        let fx = f(x);
        let cheb = (k as f64 * theta).cos();
        sum += fx * cheb;
    }
    (2.0 / n_quad as f64) * sum
}

pub fn harmonic_extension(
    graph: &SparseGraph,
    seed_indices: &[usize],
    seed_values: &[f64],
    iterations: usize,
    top_k: usize,
) -> Vec<(usize, f64)> {
    let n = graph.n;
    if seed_indices.is_empty() {
        return Vec::new();
    }

    let degree = graph.adj.degree();
    let is_seed: std::collections::HashSet<usize> = seed_indices.iter().copied().collect();

    let mut x = vec![0.0; n];
    for (i, v) in seed_indices.iter().zip(seed_values.iter()) {
        x[*i] = *v;
    }

    let neighbors: Vec<Vec<(usize, f64)>> = {
        let mut nbrs: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
        for (&(i, j), &w) in &graph.adj.entries {
            nbrs[i].push((j, w));
            nbrs[j].push((i, w));
        }
        nbrs
    };

    for _ in 0..iterations {
        let mut x_new = x.clone();
        for i in 0..n {
            if is_seed.contains(&i) || degree[i] < 1e-10 {
                continue;
            }
            let mut weighted_sum = 0.0;
            for &(j, w) in &neighbors[i] {
                weighted_sum += w * x[j];
            }
            x_new[i] = weighted_sum / degree[i];
        }
        x = x_new;
    }

    let mut scored: Vec<(usize, f64)> = x
        .iter()
        .enumerate()
        .filter(|(i, _)| !is_seed.contains(i))
        .filter(|(_, &s)| s.abs() > 1e-12)
        .map(|(i, s)| (i, s.abs()))
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored.truncate(top_k);
    scored
}

pub fn spectral_centroid(index: &SpectralIndex, indices: &[usize]) -> Vec<f64> {
    let k = index.symbol_coords[0].len();
    if k == 0 || indices.is_empty() {
        return Vec::new();
    }
    let mut centroid = vec![0.0; k];
    for &si in indices {
        for j in 0..k {
            centroid[j] += index.symbol_coords[si][j];
        }
    }
    for x in centroid.iter_mut() {
        *x /= indices.len() as f64;
    }
    centroid
}

struct SimpleRng {
    state: u64,
}

impl SimpleRng {
    fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 1 } else { seed },
        }
    }

    fn next(&mut self) -> f64 {
        self.state = self.state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let x = self.state;
        let u = ((x >> 33) as f64) / (1u64 << 31) as f64;
        2.0 * u - 1.0
    }
}

pub fn store_spectral_coords(
    db: &GraphDb,
    symbol_ids: &[i64],
    coords: &[Vec<f64>],
    eigenvalues: &[f64],
    lambda_max: f64,
) -> Result<usize, String> {
    let conn = db.conn();
    conn.execute(
        "CREATE TABLE IF NOT EXISTS spectral_coords (
            symbol_id INTEGER PRIMARY KEY,
            coords BLOB NOT NULL,
            dim INTEGER NOT NULL
        )",
        [],
    )
    .map_err(|e| e.to_string())?;

    conn.execute("DELETE FROM spectral_coords", [])
        .map_err(|e| e.to_string())?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS spectral_meta (
            key TEXT PRIMARY KEY,
            value BLOB NOT NULL
        )",
        [],
    )
    .map_err(|e| e.to_string())?;

    let ev_bytes: Vec<u8> = eigenvalues
        .iter()
        .flat_map(|f| (*f as f64).to_le_bytes())
        .collect();
    conn.execute(
        "INSERT OR REPLACE INTO spectral_meta (key, value) VALUES ('eigenvalues', ?1)",
        params![ev_bytes],
    )
    .map_err(|e| e.to_string())?;

    let lm_bytes = lambda_max.to_le_bytes().to_vec();
    conn.execute(
        "INSERT OR REPLACE INTO spectral_meta (key, value) VALUES ('lambda_max', ?1)",
        params![lm_bytes],
    )
    .map_err(|e| e.to_string())?;

    let dim = coords.first().map(|v| v.len()).unwrap_or(0);
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let mut count = 0;
    {
        let mut stmt = tx
            .prepare("INSERT INTO spectral_coords (symbol_id, coords, dim) VALUES (?1, ?2, ?3)")
            .map_err(|e| e.to_string())?;

        for (i, sym_id) in symbol_ids.iter().enumerate() {
            if i >= coords.len() {
                break;
            }
            let bytes: Vec<u8> = coords[i]
                .iter()
                .flat_map(|f| (*f as f32).to_le_bytes())
                .collect();
            stmt.execute(params![*sym_id as i64, bytes, dim as i64])
                .map_err(|e| e.to_string())?;
            count += 1;
        }
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(count)
}

pub fn compute_ricci_curvature(graph: &SparseGraph) -> Vec<f64> {
    let n = graph.n;

    let mut nbrs: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
    for &(i, j, w) in &graph.structural_edges {
        nbrs[i].push((j, w));
        nbrs[j].push((i, w));
    }
    let degree: Vec<f64> = nbrs.iter().map(|v| v.iter().map(|(_, w)| w).sum()).collect();

    let mut kappa = vec![0.0f64; n * n];

    for i in 0..n {
        let di = degree[i];
        if di < 1e-10 {
            continue;
        }
        let ni: Vec<usize> = nbrs[i].iter().map(|(n, _)| *n).collect();
        let wi: Vec<f64> = nbrs[i].iter().map(|(_, w)| *w).collect();
        let ni_len = ni.len();

        for &(j, _) in &nbrs[i] {
            let dj = degree[j];
            if dj < 1e-10 {
                continue;
            }

            let nj: Vec<usize> = nbrs[j].iter().map(|(n, _)| *n).collect();
            let wj: Vec<f64> = nbrs[j].iter().map(|(_, w)| *w).collect();

            let mut l1 = 0.0f64;

            let mut mi: HashMap<usize, f64> = HashMap::new();
            mi.insert(i, 0.5);
            for k in 0..ni_len {
                let p = wi[k] / di;
                *mi.entry(ni[k]).or_insert(0.0) += 0.5 * p;
            }

            let mut mj: HashMap<usize, f64> = HashMap::new();
            mj.insert(j, 0.5);
            for k in 0..nj.len() {
                let p = wj[k] / dj;
                *mj.entry(nj[k]).or_insert(0.0) += 0.5 * p;
            }

            for (&node, &pi) in &mi {
                let pj = mj.get(&node).copied().unwrap_or(0.0);
                l1 += (pi - pj).abs();
            }
            for (&node, &pj) in &mj {
                if !mi.contains_key(&node) {
                    l1 += pj;
                }
            }

            kappa[i * n + j] = 1.0 - l1;
        }
    }

    kappa
}

pub fn store_ricci_curvature(
    db: &GraphDb,
    graph: &SparseGraph,
    symbol_ids: &[i64],
    kappa: &[f64],
) -> Result<usize, String> {
    let n = graph.n;
    let conn = db.conn();
    conn.execute(
        "CREATE TABLE IF NOT EXISTS ricci_edges (
            source_id INTEGER NOT NULL,
            target_id INTEGER NOT NULL,
            curvature REAL NOT NULL,
            PRIMARY KEY (source_id, target_id)
        )",
        [],
    )
    .map_err(|e| e.to_string())?;

    conn.execute("DELETE FROM ricci_edges", [])
        .map_err(|e| e.to_string())?;

    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let mut count = 0usize;
    {
        let mut stmt = tx
            .prepare("INSERT OR IGNORE INTO ricci_edges (source_id, target_id, curvature) VALUES (?1, ?2, ?3)")
            .map_err(|e| e.to_string())?;

        for &(i, j, _) in &graph.structural_edges {
            let k = kappa[i * n + j];
            if i < symbol_ids.len() && j < symbol_ids.len() {
                stmt.execute(params![symbol_ids[i], symbol_ids[j], k])
                    .map_err(|e| e.to_string())?;
                count += 1;
            }
        }
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(count)
}

#[derive(Clone)]
pub struct ChannelFingerprint {
    pub calls_out: f64,
    pub calls_in: f64,
    pub imports_out: f64,
    pub imports_in: f64,
    pub extends: f64,
    pub references: f64,
    pub tests: f64,
    pub entropy: f64,
    pub role: String,
}

pub struct PredictiveModel {
    pub symbol_ids: Vec<i64>,
    pub sym_id_to_idx: HashMap<i64, usize>,
    pub conditional_terms: Vec<HashMap<String, f64>>,
    pub background_terms: HashMap<String, f64>,
}

pub fn compute_predictive_model(db: &GraphDb) -> Result<PredictiveModel, String> {
    let conn = db.conn();

    let mut sym_stmt = conn
        .prepare("SELECT id FROM symbols ORDER BY id")
        .unwrap();
    let symbol_ids: Vec<i64> = sym_stmt
        .query_map([], |row| row.get::<_, i64>(0))
        .unwrap()
        .flatten()
        .collect();
    drop(sym_stmt);

    let id_to_idx: HashMap<i64, usize> = symbol_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i))
        .collect();

    let n = symbol_ids.len();

    let mut text_stmt = conn
        .prepare(
            "SELECT id, name, name_decomposed, signature, doc_comment, source \
             FROM symbols",
        )
        .unwrap();
    let sym_texts: Vec<(i64, Vec<String>)> = text_stmt
        .query_map([], |row| {
            let id: i64 = row.get(0)?;
            let name: String = row.get::<_, String>(1).unwrap_or_default();
            let decomp: String = row.get::<_, String>(2).unwrap_or_default();
            let sig: String = row.get::<_, String>(3).unwrap_or_default();
            let doc: String = row.get::<_, String>(4).unwrap_or_default();
            let src: String = row.get::<_, String>(5).unwrap_or_default();
            let raw = format!("{} {} {} {} {}", name, decomp, sig, doc, src);
            let terms: Vec<String> = raw
                .split_whitespace()
                .filter(|t| t.len() >= 2)
                .map(|t| t.to_lowercase())
                .collect();
            Ok((id, terms))
        })
        .unwrap()
        .flatten()
        .collect();
    drop(text_stmt);

    let sym_terms_map: HashMap<i64, Vec<String>> = sym_texts.into_iter().collect();

    let mut edge_stmt = conn
        .prepare("SELECT source_id, target_id, kind FROM edges")
        .unwrap();
    let edges: Vec<(i64, i64, String)> = edge_stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .unwrap()
        .flatten()
        .collect();
    drop(edge_stmt);

    let structural_kinds: HashSet<&str> = [
        "calls", "imports", "extends", "implements", "references", "tests",
    ].iter().copied().collect();

    let mut neighbors: Vec<HashSet<usize>> = vec![HashSet::new(); n];
    for (src, tgt, kind) in &edges {
        if !structural_kinds.contains(kind.as_str()) {
            continue;
        }
        if let (Some(&si), Some(&ti)) = (id_to_idx.get(src), id_to_idx.get(tgt)) {
            neighbors[si].insert(ti);
            neighbors[ti].insert(si);
        }
    }

    let vocab_size = 5000usize;
    let mut bg_tf: HashMap<String, f64> = HashMap::new();
    let mut bg_total = 0.0f64;

    for i in 0..n {
        let terms = sym_terms_map.get(&symbol_ids[i]).cloned().unwrap_or_default();
        for t in &terms {
            *bg_tf.entry(t.clone()).or_default() += 1.0;
            bg_total += 1.0;
        }
    }

    let mut bg_sorted: Vec<(String, f64)> = bg_tf.into_iter().collect();
    bg_sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    bg_sorted.truncate(vocab_size);

    let bg_sum: f64 = bg_sorted.iter().map(|(_, c)| c).sum();
    let background_terms: HashMap<String, f64> = bg_sorted
        .into_iter()
        .map(|(t, c)| (t, c / bg_sum))
        .collect();

    let conditional_terms: Vec<HashMap<String, f64>> = (0..n)
        .map(|i| {
            let own_terms = sym_terms_map.get(&symbol_ids[i]).cloned().unwrap_or_default();
            let mut local_tf: HashMap<String, f64> = HashMap::new();
            let mut local_total = 0.0f64;

            for t in &own_terms {
                *local_tf.entry(t.clone()).or_default() += 2.0;
                local_total += 2.0;
            }

            for &ni in &neighbors[i] {
                let ni_terms = sym_terms_map.get(&symbol_ids[ni]).cloned().unwrap_or_default();
                for t in &ni_terms {
                    *local_tf.entry(t.clone()).or_default() += 1.0;
                    local_total += 1.0;
                }
            }

            if local_total < 1e-10 {
                return HashMap::new();
            }

            let smoothed_alpha = 0.1;
            let vocab_n = background_terms.len() as f64;
            let mut model: HashMap<String, f64> = HashMap::new();
            for (t, &bg_p) in &background_terms {
                let raw = local_tf.get(t).copied().unwrap_or(0.0) / local_total;
                let smoothed = (1.0 - smoothed_alpha) * raw + smoothed_alpha * bg_p;
                model.insert(t.clone(), smoothed);
            }

            let residual = 1.0 - model.values().sum::<f64>();
            if residual > 0.0 {
                if let Some(bg_default) = background_terms.values().next() {
                    for v in model.values_mut() {
                        *v += residual / vocab_n;
                    }
                }
            }

            model
        })
        .collect();

    eprintln!("  predictive model: {} symbols, {} vocab terms", n, background_terms.len());

    Ok(PredictiveModel {
        symbol_ids,
        sym_id_to_idx: id_to_idx,
        conditional_terms,
        background_terms,
    })
}

pub fn predictive_surprise(
    model: &PredictiveModel,
    query_terms: &[String],
    sym_idx: usize,
) -> f64 {
    let cond = &model.conditional_terms[sym_idx];
    let bg = &model.background_terms;

    let mut kl = 0.0f64;
    for qt in query_terms {
        let q_lower = qt.to_lowercase();
        let q_prob = 1.0 / query_terms.len() as f64;

        let p_q = cond.get(&q_lower).copied().unwrap_or_else(|| {
            bg.get(&q_lower).copied().unwrap_or(1e-6)
        });

        if p_q > 1e-15 {
            kl += q_prob * (q_prob / p_q).ln();
        }
    }

    kl.max(0.0)
}

pub fn channel_capacity_weights(
    fingerprints: &[ChannelFingerprint],
    id_to_idx: &HashMap<i64, usize>,
    query_terms: &[String],
    top_seeds: &[(i64, f64)],
) -> [f64; 5] {
    let n = fingerprints.len();

    let mut seed_roles: Vec<(usize, f64)> = Vec::new();
    for &(id, score) in top_seeds.iter().take(10) {
        if let Some(&i) = id_to_idx.get(&id) {
            if i < n {
                seed_roles.push((i, score));
            }
        }
    }

    if seed_roles.is_empty() {
        return [0.0, 0.0, 0.0, 0.0, 0.0];
    }

    let mut role_counts: HashMap<String, (f64, f64)> = HashMap::new();
    for (i, score) in &seed_roles {
        let role = &fingerprints[*i].role;
        let entry = role_counts.entry(role.clone()).or_default();
        entry.0 += 1.0;
        entry.1 += score;
    }

    let total_weight: f64 = role_counts.values().map(|(c, s)| c * s).sum::<f64>().max(1e-10);

    let mut adjustment = [0.0f64; 5];
    for (role, (count, score_w)) in &role_counts {
        let influence = (count * score_w) / total_weight;
        match role.as_str() {
            "orchestrator" => {
                adjustment[2] += influence * 0.4;
                adjustment[3] += influence * 0.1;
            }
            "library" => {
                adjustment[0] += influence * 0.5;
                adjustment[3] += influence * 0.05;
            }
            "boundary" => {
                adjustment[2] += influence * 0.3;
                adjustment[4] += influence * 0.1;
            }
            "worker" => {
                adjustment[0] += influence * 0.3;
            }
            "isolate" => {
                adjustment[0] += influence * 0.4;
            }
            _ => {}
        }
    }

    let n_qt = query_terms.len() as f64;
    if n_qt > 1.0 {
        let breadth_factor = (3.0 / n_qt).min(1.0);
        adjustment[2] += 0.2 * breadth_factor;
    }

    adjustment
}

pub struct MdlExplanation {
    pub covered_terms: HashSet<String>,
    pub covered_frac: f64,
    pub marginal_gain: f64,
    pub cost: f64,
}

pub fn mdl_explanation_set(
    results: &[(i64, f64)],
    query_terms: &[String],
    term_lookup: &dyn Fn(i64) -> Option<(HashSet<String>, HashSet<String>, HashMap<String, f64>)>,
    fingerprints: &[ChannelFingerprint],
    id_to_idx: &HashMap<i64, usize>,
) -> MdlExplanation {
    if results.is_empty() || query_terms.is_empty() {
        return MdlExplanation {
            covered_terms: HashSet::new(),
            covered_frac: 0.0,
            marginal_gain: 0.0,
            cost: 0.0,
        };
    }

    let n_terms = query_terms.len();
    let term_set: HashSet<String> = query_terms.iter().map(|t| t.to_lowercase()).collect();
    let mut covered: HashSet<String> = HashSet::new();

    let mut explanation_cost = 0.0f64;
    let mut info_gain = 0.0f64;
    let mut prev_gain = 0.0f64;

    for (rank, &(sym_id, score)) in results.iter().enumerate() {
        if covered.len() >= n_terms {
            break;
        }

        let (name_terms, sig_terms, terms_map) = match term_lookup(sym_id) {
            Some(t) => t,
            None => continue,
        };

        let mut new_covered = 0usize;
        for qt in query_terms {
            let q_lower = qt.to_lowercase();
            if covered.contains(&q_lower) {
                continue;
            }
            if name_terms.contains(&q_lower)
                || sig_terms.contains(&q_lower)
                || terms_map.contains_key(&q_lower)
            {
                covered.insert(q_lower);
                new_covered += 1;
            }
        }

        if new_covered == 0 {
            continue;
        }

        let symbol_cost = 1.0 + (rank as f64).log2().max(0.0) * 0.5;
        explanation_cost += symbol_cost;

        let marginal = new_covered as f64 / n_terms as f64;
        let gain = marginal - prev_gain;
        info_gain += gain;

        if rank >= 3 {
            let efficiency = gain / symbol_cost;
            if efficiency < 0.05 {
                break;
            }
        }

        prev_gain = marginal;
    }

    let covered_frac = covered.len() as f64 / n_terms as f64;

    let diversity_bonus = {
        let roles_seen: HashSet<String> = results
            .iter()
            .take(covered.len().max(3))
            .filter_map(|(id, _)| {
                id_to_idx.get(id).map(|&i| fingerprints[i].role.clone())
            })
            .collect();
        (roles_seen.len() as f64 / 5.0).min(1.0) * 0.15
    };

    let mdl_score = (info_gain + diversity_bonus) / (explanation_cost + 1.0).log2().max(1.0);

    MdlExplanation {
        covered_terms: covered,
        covered_frac,
        marginal_gain: mdl_score,
        cost: explanation_cost,
    }
}

pub fn compute_channel_fingerprints(db: &GraphDb) -> (Vec<ChannelFingerprint>, HashMap<i64, usize>) {
    let conn = db.conn();

    let mut sym_stmt = conn
        .prepare("SELECT id FROM symbols WHERE visibility = 'public' ORDER BY id")
        .unwrap();
    let symbol_ids: Vec<i64> = sym_stmt
        .query_map([], |row| row.get::<_, i64>(0))
        .unwrap()
        .flatten()
        .collect();
    drop(sym_stmt);

    let id_to_idx: HashMap<i64, usize> = symbol_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i))
        .collect();

    let n = symbol_ids.len();
    let edge_kinds = ["calls", "imports", "extends", "implements", "references", "tests"];

    let mut counts: Vec<[f64; 6]> = vec![[0.0; 6]; n];
    let mut total: Vec<f64> = vec![0.0; n];

    let mut edge_stmt = conn
        .prepare("SELECT source_id, target_id, kind FROM edges")
        .unwrap();
    let edges: Vec<(i64, i64, String)> = edge_stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .unwrap()
        .flatten()
        .collect();
    drop(edge_stmt);

    for (src, tgt, kind) in &edges {
        if let (Some(&si), Some(_ti)) = (id_to_idx.get(src), id_to_idx.get(tgt)) {
            if let Some(ch) = edge_kinds.iter().position(|k| *k == kind) {
                counts[si][ch] += 1.0;
                total[si] += 1.0;
            }
        }
    }

    let mut fingerprints: Vec<ChannelFingerprint> = Vec::with_capacity(n);
    for i in 0..n {
        let t = total[i].max(1.0);
        let dist: [f64; 6] = std::array::from_fn(|j| counts[i][j] / t);

        let mut entropy = 0.0f64;
        for &p in &dist {
            if p > 0.0 {
                entropy -= p * p.log2();
            }
        }

        let role = if dist[0] > 0.4 {
            "orchestrator"
        } else if dist[1] > 0.4 {
            "library"
        } else if entropy > 1.5 {
            "boundary"
        } else if total[i] < 2.0 {
            "isolate"
        } else {
            "worker"
        };

        fingerprints.push(ChannelFingerprint {
            calls_out: dist[0],
            calls_in: dist[1],
            imports_out: dist[2],
            imports_in: dist[3],
            extends: dist[4],
            references: dist[5],
            tests: 0.0,
            entropy,
            role: role.to_string(),
        });
    }

    (fingerprints, id_to_idx)
}
