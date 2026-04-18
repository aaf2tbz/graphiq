use std::collections::{HashMap, HashSet};

use crate::db::GraphDb;
use crate::lsa::extract_terms;

const H_DIM: usize = 1024;

pub struct HoloIndex {
    pub symbol_ids: Vec<i64>,
    pub symbol_names: Vec<String>,
    pub holograms: Vec<Vec<f64>>,
    term_vectors: HashMap<String, Vec<f64>>,
    term_idf: HashMap<String, f64>,
    pub dim: usize,
}

fn hash_to_seed(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in s.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn random_unit_vec(dim: usize, seed: u64) -> Vec<f64> {
    let mut state = seed;
    let mut v = Vec::with_capacity(dim);
    for _ in 0..dim / 2 {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let u1 = ((state >> 11) as f64) / (1u64 << 53) as f64;
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let u2 = ((state >> 11) as f64) / (1u64 << 53) as f64;
        let r = (-2.0 * u1.max(1e-15).ln()).sqrt();
        v.push(r * (2.0 * std::f64::consts::PI * u2).cos());
        v.push(r * (2.0 * std::f64::consts::PI * u2).sin());
    }
    let norm: f64 = v.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm > 1e-10 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
    v
}

fn fft_inplace(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let log_n = n.trailing_zeros() as usize;
    for i in 0..n {
        let j = i.reverse_bits() >> (usize::BITS as usize - log_n);
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut size = 2usize;
    while size <= n {
        let half = size / 2;
        let step = 2.0 * std::f64::consts::PI / size as f64;
        for block in (0..n).step_by(size) {
            for j in 0..half {
                let angle = -step * j as f64;
                let wr = angle.cos();
                let wi = angle.sin();
                let ei = block + j;
                let oi = block + j + half;
                let er = re[ei];
                let eim = im[ei];
                let tr = wr * re[oi] - wi * im[oi];
                let ti = wr * im[oi] + wi * re[oi];
                re[ei] = er + tr;
                im[ei] = eim + ti;
                re[oi] = er - tr;
                im[oi] = eim - ti;
            }
        }
        size *= 2;
    }
}

fn ifft_inplace(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    for i in 0..n {
        im[i] = -im[i];
    }
    fft_inplace(re, im);
    let inv_n = 1.0 / n as f64;
    for i in 0..n {
        re[i] *= inv_n;
        im[i] = -im[i] * inv_n;
    }
}

fn freq_mul_add(
    h_re: &mut [f64],
    h_im: &mut [f64],
    a_re: &[f64],
    a_im: &[f64],
    b_re: &[f64],
    b_im: &[f64],
    scale: f64,
) {
    for i in 0..h_re.len() {
        h_re[i] += scale * (a_re[i] * b_re[i] - a_im[i] * b_im[i]);
        h_im[i] += scale * (a_re[i] * b_im[i] + a_im[i] * b_re[i]);
    }
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

fn edge_weight(kind: &str) -> f64 {
    match kind {
        "calls" => 1.0,
        "contains" => 0.8,
        "imports" => 0.9,
        "extends" => 0.7,
        "implements" => 0.7,
        "references" => 0.5,
        _ => 0.3,
    }
}

fn edge_rank_key(kind: &str, neighbor_id: i64) -> (u8, i64) {
    let priority = match kind {
        "contains" => 0,
        "calls" => 1,
        "implements" => 2,
        "extends" => 3,
        "imports" => 4,
        "references" => 5,
        _ => 6,
    };
    (priority, neighbor_id)
}

pub fn compute_holo(db: &GraphDb) -> Result<HoloIndex, String> {
    let conn = db.conn();
    let dim = H_DIM;

    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.name, s.qualified_name, f.path, s.language \
             FROM symbols s LEFT JOIN files f ON s.file_id = f.id \
             ORDER BY s.id",
        )
        .map_err(|e| e.to_string())?;
    let symbols: Vec<(i64, String, Option<String>, Option<String>, String)> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .flatten()
        .collect();

    let n = symbols.len();
    eprintln!("  [holo] {} symbols, dim={}", n, dim);

    let mut all_terms: HashSet<String> = HashSet::new();
    let mut term_doc_count: HashMap<String, usize> = HashMap::new();
    let mut symbol_term_sets: Vec<HashSet<String>> = Vec::with_capacity(n);

    for (_, name, _, _, _) in &symbols {
        let mut terms_for_symbol = HashSet::new();
        for t in extract_terms(name) {
            all_terms.insert(t.clone());
            terms_for_symbol.insert(t);
        }
        for t in &terms_for_symbol {
            *term_doc_count.entry(t.clone()).or_insert(0) += 1;
        }
        symbol_term_sets.push(terms_for_symbol);
    }

    let mut term_vectors: HashMap<String, Vec<f64>> = HashMap::new();
    for term in &all_terms {
        let seed = hash_to_seed(&format!("term:{}", term));
        term_vectors.insert(term.clone(), random_unit_vec(dim, seed));
    }

    let relation_types = [
        "calls",
        "contains",
        "imports",
        "extends",
        "implements",
        "references",
        "unknown",
    ];
    let mut freq_relations: HashMap<String, (Vec<f64>, Vec<f64>)> = HashMap::new();
    for rel in &relation_types {
        for suffix in &["", "_inv"] {
            let key = format!("{}{}", rel, suffix);
            let seed = hash_to_seed(&format!("holo_rel{}:{}", suffix, rel));
            let vec = random_unit_vec(dim, seed);
            let mut re = vec;
            let mut im = vec![0.0; dim];
            fft_inplace(&mut re, &mut im);
            freq_relations.insert(key, (re, im));
        }
    }

    let symbol_ids: Vec<i64> = symbols.iter().map(|(id, _, _, _, _)| *id).collect();
    let symbol_names: Vec<String> = symbols
        .iter()
        .map(|(_, name, _, _, _)| name.clone())
        .collect();
    let id_to_idx: HashMap<i64, usize> = symbol_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i))
        .collect();

    let mut freq_identities: Vec<(Vec<f64>, Vec<f64>)> = Vec::with_capacity(n);
    for term_set in &symbol_term_sets {
        let mut identity = vec![0.0; dim];
        for t in term_set {
            if let Some(tv) = term_vectors.get(t) {
                for j in 0..dim {
                    identity[j] += tv[j];
                }
            }
        }
        let norm: f64 = identity.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm > 1e-10 {
            for x in identity.iter_mut() {
                *x /= norm;
            }
        }
        let mut re = identity;
        let mut im = vec![0.0; dim];
        fft_inplace(&mut re, &mut im);
        freq_identities.push((re, im));
    }

    let mut outgoing: Vec<Vec<(usize, String, f64, i64)>> = vec![Vec::new(); n];
    let mut incoming: Vec<Vec<(usize, String, f64, i64)>> = vec![Vec::new(); n];
    if let Ok(mut stmt) = conn.prepare("SELECT source_id, target_id, kind FROM edges") {
        let rows: Vec<(i64, i64, String)> = stmt
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get::<_, String>(2)?))
            })
            .unwrap_or_else(|_| panic!("edge query failed"))
            .flatten()
            .collect();

        for (src, tgt, kind) in &rows {
            if let (Some(&si), Some(&ti)) = (id_to_idx.get(src), id_to_idx.get(tgt)) {
                let w = edge_weight(kind);
                outgoing[si].push((ti, kind.clone(), w, *tgt));
                incoming[ti].push((si, kind.clone(), w, *src));
            }
        }
    }

    let n_edges: usize = outgoing.iter().map(|e| e.len()).sum();
    eprintln!("  [holo] {} edges", n_edges);

    let mut holograms: Vec<Vec<f64>> = Vec::with_capacity(n);
    let max_boundary = 20usize;

    for i in 0..n {
        let mut h_re = freq_identities[i].0.clone();
        let mut h_im = freq_identities[i].1.clone();

        let mut boundary: Vec<(usize, String, f64)> = Vec::new();

        for (ni, kind, w, nid) in &outgoing[i] {
            boundary.push((*ni, kind.clone(), *w));
        }
        for (ni, kind, w, nid) in &incoming[i] {
            let inv_key = format!("{}_inv", kind);
            boundary.push((*ni, inv_key, *w));
        }

        boundary.sort_by(|a, b| {
            let ka = edge_rank_key(&a.1, symbol_ids[a.0]);
            let kb = edge_rank_key(&b.1, symbol_ids[b.0]);
            ka.cmp(&kb)
        });
        boundary.truncate(max_boundary);

        if boundary.len() >= 2 {
            let mut contour_re = freq_identities[boundary[0].0].0.clone();
            let mut contour_im = freq_identities[boundary[0].0].1.clone();

            if let Some((rr, ri)) = freq_relations.get(boundary[0].1.as_str()) {
                let mut new_re = vec![0.0; dim];
                let mut new_im = vec![0.0; dim];
                for j in 0..dim {
                    new_re[j] = rr[j] * contour_re[j] - ri[j] * contour_im[j];
                    new_im[j] = rr[j] * contour_im[j] + ri[j] * contour_re[j];
                }
                contour_re = new_re;
                contour_im = new_im;
            }

            for k in 1..boundary.len() {
                let (ni, ref kind, w) = boundary[k];
                let (nr, ni_im) = &freq_identities[ni];

                if let Some((rr, ri)) = freq_relations.get(kind.as_str()) {
                    let bound_re: Vec<f64> =
                        (0..dim).map(|j| rr[j] * nr[j] - ri[j] * ni_im[j]).collect();
                    let bound_im: Vec<f64> =
                        (0..dim).map(|j| rr[j] * ni_im[j] + ri[j] * nr[j]).collect();

                    let mut new_re = vec![0.0; dim];
                    let mut new_im = vec![0.0; dim];
                    for j in 0..dim {
                        new_re[j] = contour_re[j] * bound_re[j] - contour_im[j] * bound_im[j];
                        new_im[j] = contour_re[j] * bound_im[j] + contour_im[j] * bound_re[j];
                    }
                    contour_re = new_re;
                    contour_im = new_im;
                }
            }

            let decay = 0.4f64 / (boundary.len() as f64).sqrt().max(1.0);
            freq_mul_add(
                &mut h_re,
                &mut h_im,
                &contour_re,
                &contour_im,
                &freq_identities[i].0,
                &freq_identities[i].1,
                decay,
            );
        } else if boundary.len() == 1 {
            let (ni, ref kind, w) = boundary[0];
            if let Some((rr, ri)) = freq_relations.get(kind.as_str()) {
                let (nr, ni_im) = &freq_identities[ni];
                freq_mul_add(&mut h_re, &mut h_im, rr, ri, nr, ni_im, 0.5 * w);
            }
        }

        ifft_inplace(&mut h_re, &mut h_im);
        let norm: f64 = h_re.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm > 1e-10 {
            for x in h_re.iter_mut() {
                *x /= norm;
            }
        }
        holograms.push(h_re);
    }

    let avg_norm = holograms
        .iter()
        .map(|h| h.iter().map(|x| x * x).sum::<f64>().sqrt())
        .sum::<f64>()
        / n as f64;
    eprintln!("  [holo] hologram norms: avg={:.3}", avg_norm);

    let term_idf: HashMap<String, f64> = all_terms
        .iter()
        .map(|t| {
            let df = *term_doc_count.get(t).unwrap_or(&1) as f64;
            (t.clone(), (1.0 + (n as f64 / df).ln()).max(0.1))
        })
        .collect();

    Ok(HoloIndex {
        symbol_ids,
        symbol_names,
        holograms,
        term_vectors,
        term_idf,
        dim,
    })
}

impl HoloIndex {
    pub fn id_to_idx(&self, id: &i64) -> Option<usize> {
        self.symbol_ids.iter().position(|s| s == id)
    }
}

fn build_query_vec(query: &str, term_vectors: &HashMap<String, Vec<f64>>, dim: usize) -> Vec<f64> {
    let terms = extract_terms(query);
    let mut q = vec![0.0; dim];
    for t in &terms {
        if let Some(tv) = term_vectors.get(t) {
            for j in 0..dim {
                q[j] += tv[j];
            }
        }
    }
    let norm: f64 = q.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm > 1e-10 {
        for x in q.iter_mut() {
            *x /= norm;
        }
    }
    q
}

pub fn holo_rerank(
    query: &str,
    candidate_ids: &[i64],
    candidate_scores: &[f64],
    index: &HoloIndex,
) -> Vec<(i64, f64)> {
    let id_to_idx: HashMap<i64, usize> = index
        .symbol_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i))
        .collect();

    let q = build_query_vec(query, &index.term_vectors, index.dim);
    let q_empty = q.iter().all(|x| x.abs() < 1e-10);

    let mut scored: Vec<(i64, f64)> = candidate_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| {
            let base = candidate_scores.get(i).copied().unwrap_or(0.0);
            if base <= 0.0 || q_empty {
                return (id, base);
            }
            if let Some(&idx) = id_to_idx.get(&id) {
                let sim = dot(&q, &index.holograms[idx]).max(0.0);
                let boost = 1.0 + 0.10 * sim;
                (id, base * boost)
            } else {
                (id, base)
            }
        })
        .filter(|(_, s)| *s > 0.0)
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored
}

pub fn holo_bivector_expand(seed_ids: &[i64], index: &HoloIndex, top_k: usize) -> Vec<(i64, f64)> {
    let id_to_idx: HashMap<i64, usize> = index
        .symbol_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i))
        .collect();

    let seed_indices: Vec<usize> = seed_ids
        .iter()
        .filter_map(|id| id_to_idx.get(id).copied())
        .collect();

    if seed_indices.len() < 2 {
        return Vec::new();
    }

    let dim = index.dim;
    let mut bivec = vec![0.0; dim];
    let mut n_pairs = 0.0;

    let max_seeds = seed_indices.len().min(10);
    for i in 0..max_seeds {
        for j in (i + 1)..max_seeds {
            let a = &index.holograms[seed_indices[i]];
            let b = &index.holograms[seed_indices[j]];
            let ad = dot(a, b);
            let b_norm_sq: f64 = b.iter().map(|x| x * x).sum();
            if b_norm_sq < 1e-10 {
                continue;
            }
            let proj = ad / b_norm_sq;
            for k in 0..dim {
                bivec[k] += (a[k] - proj * b[k]) / dim as f64;
            }
            n_pairs += 1.0;
        }
    }

    if n_pairs < 1.0 {
        return Vec::new();
    }
    for x in bivec.iter_mut() {
        *x /= n_pairs;
    }
    let bn: f64 = bivec.iter().map(|x| x * x).sum::<f64>().sqrt();
    if bn < 1e-10 {
        return Vec::new();
    }
    for x in bivec.iter_mut() {
        *x /= bn;
    }

    let seed_set: HashSet<i64> = seed_ids.iter().copied().collect();
    let mut scored: Vec<(usize, f64)> = (0..index.symbol_ids.len())
        .filter(|i| !seed_set.contains(&index.symbol_ids[*i]))
        .map(|i| (i, dot(&bivec, &index.holograms[i])))
        .filter(|(_, s)| *s > 0.0)
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored.truncate(top_k);
    scored
        .into_iter()
        .map(|(i, s)| (index.symbol_ids[i], s))
        .collect()
}

pub fn holo_fractal_attract(
    seed_ids: &[i64],
    index: &HoloIndex,
    iterations: usize,
    top_k: usize,
) -> Vec<(i64, f64)> {
    let id_to_idx: HashMap<i64, usize> = index
        .symbol_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i))
        .collect();

    let seed_indices: Vec<usize> = seed_ids
        .iter()
        .filter_map(|id| id_to_idx.get(id).copied())
        .collect();
    if seed_indices.is_empty() {
        return Vec::new();
    }

    let n_seeds = seed_indices.len() as f64;
    let mut attractor = vec![0.0; index.dim];
    for &si in &seed_indices {
        for j in 0..index.dim {
            attractor[j] += index.holograms[si][j] / n_seeds;
        }
    }
    let an: f64 = attractor.iter().map(|x| x * x).sum::<f64>().sqrt();
    if an < 1e-10 {
        return Vec::new();
    }
    for x in attractor.iter_mut() {
        *x /= an;
    }

    let momentum = 0.6;
    let expand_k = 15;

    for _ in 0..iterations {
        let mut scored: Vec<(usize, f64)> = (0..index.symbol_ids.len())
            .map(|i| (i, dot(&attractor, &index.holograms[i])))
            .filter(|(_, s)| *s > 0.01)
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        scored.truncate(expand_k);
        if scored.is_empty() {
            break;
        }

        let weights: Vec<f64> = scored.iter().map(|(_, s)| s.max(0.01)).collect();
        let w_sum: f64 = weights.iter().sum();
        let mut centroid = vec![0.0; index.dim];
        for (k, &(idx, _)) in scored.iter().enumerate() {
            let w = weights[k] / w_sum;
            for j in 0..index.dim {
                centroid[j] += w * index.holograms[idx][j];
            }
        }
        let cn: f64 = centroid.iter().map(|x| x * x).sum::<f64>().sqrt();
        if cn > 1e-10 {
            for x in centroid.iter_mut() {
                *x /= cn;
            }
        }

        for j in 0..index.dim {
            attractor[j] = momentum * attractor[j] + (1.0 - momentum) * centroid[j];
        }
        let nn: f64 = attractor.iter().map(|x| x * x).sum::<f64>().sqrt();
        if nn > 1e-10 {
            for x in attractor.iter_mut() {
                *x /= nn;
            }
        }
    }

    let mut final_scored: Vec<(usize, f64)> = (0..index.symbol_ids.len())
        .map(|i| (i, dot(&attractor, &index.holograms[i])))
        .filter(|(_, s)| *s > 0.01)
        .collect();
    final_scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    final_scored.truncate(top_k);
    final_scored
        .into_iter()
        .map(|(i, s)| (index.symbol_ids[i], s))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::GraphDb;
    use crate::edge::EdgeKind;
    use crate::symbol::{SymbolBuilder, SymbolKind};

    #[test]
    fn test_holo_basic() {
        let db = GraphDb::open_in_memory().unwrap();
        let fid = db.upsert_file("t.rs", "rust", "abc", 100, 10).unwrap();
        let s1 = SymbolBuilder::new(
            fid,
            "search".into(),
            SymbolKind::Function,
            "fn search()".into(),
            "rust".into(),
        )
        .lines(1, 5)
        .build();
        let s2 = SymbolBuilder::new(
            fid,
            "find".into(),
            SymbolKind::Function,
            "fn find()".into(),
            "rust".into(),
        )
        .lines(6, 10)
        .build();
        let s3 = SymbolBuilder::new(
            fid,
            "delete".into(),
            SymbolKind::Function,
            "fn delete()".into(),
            "rust".into(),
        )
        .lines(11, 15)
        .build();
        let id1 = db.insert_symbol(&s1).unwrap();
        let id2 = db.insert_symbol(&s2).unwrap();
        let id3 = db.insert_symbol(&s3).unwrap();
        db.insert_edge(id1, id2, EdgeKind::Calls, 1.0, serde_json::Value::Null)
            .unwrap();
        db.insert_edge(id2, id3, EdgeKind::Calls, 1.0, serde_json::Value::Null)
            .unwrap();

        let idx = compute_holo(&db).unwrap();
        assert_eq!(idx.symbol_ids.len(), 3);

        let res = holo_rerank("search", &[id1, id2, id3], &[1.0, 0.8, 0.5], &idx);
        assert!(!res.is_empty());
        assert_eq!(res[0].0, id1);
    }

    #[test]
    fn test_holo_boundary_trace() {
        let db = GraphDb::open_in_memory().unwrap();
        let fid = db.upsert_file("t.rs", "rust", "abc", 100, 10).unwrap();
        let sa = SymbolBuilder::new(
            fid,
            "alpha".into(),
            SymbolKind::Function,
            "fn alpha()".into(),
            "rust".into(),
        )
        .lines(1, 5)
        .build();
        let sb = SymbolBuilder::new(
            fid,
            "beta".into(),
            SymbolKind::Function,
            "fn beta()".into(),
            "rust".into(),
        )
        .lines(6, 10)
        .build();
        let sc = SymbolBuilder::new(
            fid,
            "gamma".into(),
            SymbolKind::Function,
            "fn gamma()".into(),
            "rust".into(),
        )
        .lines(11, 15)
        .build();
        let sd = SymbolBuilder::new(
            fid,
            "delta".into(),
            SymbolKind::Function,
            "fn delta()".into(),
            "rust".into(),
        )
        .lines(16, 20)
        .build();
        let ia = db.insert_symbol(&sa).unwrap();
        let ib = db.insert_symbol(&sb).unwrap();
        let ic = db.insert_symbol(&sc).unwrap();
        let id_ = db.insert_symbol(&sd).unwrap();

        db.insert_edge(ia, ib, EdgeKind::Calls, 1.0, serde_json::Value::Null)
            .unwrap();
        db.insert_edge(ia, ic, EdgeKind::Calls, 1.0, serde_json::Value::Null)
            .unwrap();
        db.insert_edge(ia, id_, EdgeKind::Contains, 1.0, serde_json::Value::Null)
            .unwrap();

        let idx = compute_holo(&db).unwrap();
        assert_eq!(idx.symbol_ids.len(), 4);

        let res = holo_rerank("alpha", &[ia, ib, ic, id_], &[1.0, 0.5, 0.5, 0.3], &idx);
        assert!(!res.is_empty());
    }

    #[test]
    fn test_holo_fractal() {
        let db = GraphDb::open_in_memory().unwrap();
        let fid = db.upsert_file("t.rs", "rust", "abc", 100, 10).unwrap();
        let s1 = SymbolBuilder::new(
            fid,
            "search".into(),
            SymbolKind::Function,
            "fn search()".into(),
            "rust".into(),
        )
        .lines(1, 5)
        .build();
        let s2 = SymbolBuilder::new(
            fid,
            "find".into(),
            SymbolKind::Function,
            "fn find()".into(),
            "rust".into(),
        )
        .lines(6, 10)
        .build();
        let id1 = db.insert_symbol(&s1).unwrap();
        let id2 = db.insert_symbol(&s2).unwrap();
        db.insert_edge(id1, id2, EdgeKind::Calls, 1.0, serde_json::Value::Null)
            .unwrap();

        let idx = compute_holo(&db).unwrap();
        let res = holo_fractal_attract(&[id1], &idx, 3, 10);
        assert!(res.len() <= 10);
    }
}
