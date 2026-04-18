use std::collections::{HashMap, HashSet};

use crate::db::GraphDb;
use crate::lsa::extract_terms;

const HRR_DIM: usize = 1024;

pub struct HrrIndex {
    pub symbol_ids: Vec<i64>,
    pub symbol_names: Vec<String>,
    pub holograms: Vec<Vec<f64>>,
    pub identity_holograms: Vec<Vec<f64>>,
    boundary_freq_re: Vec<Vec<f64>>,
    boundary_freq_im: Vec<Vec<f64>>,
    boundary_areas: Vec<f64>,
    shared_holograms: Vec<Vec<f64>>,
    lang_centroids: HashMap<String, Vec<f64>>,
    symbol_langs: Vec<String>,
    term_vectors: HashMap<String, Vec<f64>>,
    term_idf: HashMap<String, f64>,
    dim: usize,
    adjacency: Vec<Vec<(usize, f64)>>,
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

fn complex_add_mul(
    a_re: &mut [f64],
    a_im: &mut [f64],
    b_re: &[f64],
    b_im: &[f64],
    c_re: &[f64],
    c_im: &[f64],
    scale: f64,
) {
    for i in 0..a_re.len() {
        a_re[i] += scale * (b_re[i] * c_re[i] - b_im[i] * c_im[i]);
        a_im[i] += scale * (b_re[i] * c_im[i] + b_im[i] * c_re[i]);
    }
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

pub fn hrr_dot(a: &[f64], b: &[f64]) -> f64 {
    dot(a, b)
}

fn edge_weight(kind: &str) -> f64 {
    match kind {
        "Calls" => 1.0,
        "Contains" => 0.8,
        "Imports" => 0.9,
        "Extends" => 0.7,
        "Implements" => 0.7,
        "References" => 0.5,
        _ => 0.3,
    }
}

pub fn compute_hrr(db: &GraphDb) -> Result<HrrIndex, String> {
    let conn = db.conn();
    let dim = HRR_DIM;

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
    eprintln!("  [hrr] {} symbols, dim={}", n, dim);

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
        "Calls",
        "Contains",
        "Imports",
        "Extends",
        "Implements",
        "References",
        "Unknown",
    ];
    let mut freq_relations: HashMap<String, (Vec<f64>, Vec<f64>)> = HashMap::new();
    for rel in &relation_types {
        for suffix in &["", "_inv"] {
            let key = format!("{}{}", rel, suffix);
            let seed = hash_to_seed(&format!("rel{}:{}", suffix, rel));
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
    let symbol_langs: Vec<String> = symbols
        .iter()
        .map(|(_, _, _, _, lang)| lang.clone())
        .collect();
    let id_to_idx: HashMap<i64, usize> = symbol_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i))
        .collect();

    let mut freq_identities: Vec<(Vec<f64>, Vec<f64>)> = Vec::with_capacity(n);
    let mut boundary_freq_re: Vec<Vec<f64>> = Vec::with_capacity(n);
    let mut boundary_freq_im: Vec<Vec<f64>> = Vec::with_capacity(n);
    let mut boundary_areas: Vec<f64> = Vec::with_capacity(n);
    let mut identity_holograms: Vec<Vec<f64>> = Vec::with_capacity(n);
    for term_set in &symbol_term_sets {
        let mut identity = vec![0.0; dim];
        for t in term_set {
            if let Some(tv) = term_vectors.get(t) {
                for j in 0..dim {
                    identity[j] += tv[j];
                }
            }
        }
        let raw_norm: f64 = identity.iter().map(|x| x * x).sum::<f64>().sqrt();
        identity_holograms.push(identity.clone());

        let norm = raw_norm;
        if norm > 1e-10 {
            for x in identity.iter_mut() {
                *x /= norm;
            }
        }
        let mut re = identity;
        let mut im = vec![0.0; dim];
        fft_inplace(&mut re, &mut im);
        freq_identities.push((re.clone(), im.clone()));

        let mut bnd_re = if raw_norm > 1e-10 {
            identity_holograms.last().unwrap().clone()
        } else {
            vec![0.0; dim]
        };
        let mut bnd_im = vec![0.0; dim];
        if raw_norm > 1e-10 {
            fft_inplace(&mut bnd_re, &mut bnd_im);
        }
        let area = bnd_re
            .iter()
            .zip(bnd_im.iter())
            .map(|(r, i)| r * r + i * i)
            .sum::<f64>()
            .sqrt();
        boundary_freq_re.push(bnd_re);
        boundary_freq_im.push(bnd_im);
        boundary_areas.push(area);
    }

    let mut outgoing: Vec<Vec<(usize, String, f64)>> = vec![Vec::new(); n];
    let mut incoming: Vec<Vec<(usize, String, f64)>> = vec![Vec::new(); n];
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
                outgoing[si].push((ti, kind.clone(), w));
                incoming[ti].push((si, kind.clone(), w));
            }
        }
    }

    let n_edges: usize = outgoing.iter().map(|e| e.len()).sum();
    eprintln!("  [hrr] {} edges", n_edges);

    let two_hop_types: HashSet<&str> = HashSet::from(["Calls", "Contains"]);
    let mut freq_two_hop: HashMap<(String, String), (Vec<f64>, Vec<f64>)> = HashMap::new();
    for r1 in &relation_types {
        for r2 in &relation_types {
            if !two_hop_types.contains(r1) && !two_hop_types.contains(r2) {
                continue;
            }
            let (r1_re, r1_im) = freq_relations.get(*r1).unwrap();
            let (r2_re, r2_im) = freq_relations.get(*r2).unwrap();
            let mut prod_re = vec![0.0; dim];
            let mut prod_im = vec![0.0; dim];
            for j in 0..dim {
                prod_re[j] = r1_re[j] * r2_re[j] - r1_im[j] * r2_im[j];
                prod_im[j] = r1_re[j] * r2_im[j] + r1_im[j] * r2_re[j];
            }
            freq_two_hop.insert((r1.to_string(), r2.to_string()), (prod_re, prod_im));

            let r1_inv = format!("{}_inv", r1);
            let r2_inv = format!("{}_inv", r2);
            let (r1i_re, r1i_im) = freq_relations.get(&r1_inv).unwrap();
            let (r2i_re, r2i_im) = freq_relations.get(&r2_inv).unwrap();
            let mut pi_re = vec![0.0; dim];
            let mut pi_im = vec![0.0; dim];
            for j in 0..dim {
                pi_re[j] = r1i_re[j] * r2i_re[j] - r1i_im[j] * r2i_im[j];
                pi_im[j] = r1i_re[j] * r2i_im[j] + r1i_im[j] * r2i_re[j];
            }
            freq_two_hop.insert((r1_inv, r2_inv), (pi_re, pi_im));
        }
    }

    let two_hop_decay = 0.3f64;
    let max_2hop_neighbors = 15usize;

    let mut holograms: Vec<Vec<f64>> = Vec::with_capacity(n);
    for i in 0..n {
        let mut h_re = freq_identities[i].0.clone();
        let mut h_im = freq_identities[i].1.clone();

        for (ni, kind, w) in &outgoing[i] {
            let (rr, ri) = freq_relations
                .get(kind.as_str())
                .unwrap_or_else(|| freq_relations.get("Unknown").unwrap());
            let (nr, ni_im) = &freq_identities[*ni];
            complex_add_mul(&mut h_re, &mut h_im, rr, ri, nr, ni_im, *w);

            if outgoing[*ni].len() <= max_2hop_neighbors {
                for (nj, kind2, w2) in &outgoing[*ni] {
                    if *nj == i {
                        continue;
                    }
                    if let Some((cr, ci)) = freq_two_hop.get(&(kind.clone(), kind2.clone())) {
                        let (njr, nji) = &freq_identities[*nj];
                        complex_add_mul(
                            &mut h_re,
                            &mut h_im,
                            cr,
                            ci,
                            njr,
                            nji,
                            two_hop_decay * w * w2,
                        );
                    }
                }
            }
        }

        for (ni, kind, w) in &incoming[i] {
            let inv_key = format!("{}_inv", kind);
            let (rr, ri) = freq_relations
                .get(&inv_key)
                .unwrap_or_else(|| freq_relations.get("Unknown_inv").unwrap());
            let (nr, ni_im) = &freq_identities[*ni];
            complex_add_mul(&mut h_re, &mut h_im, rr, ri, nr, ni_im, *w);

            if incoming[*ni].len() <= max_2hop_neighbors {
                for (nj, kind2, w2) in &incoming[*ni] {
                    if *nj == i {
                        continue;
                    }
                    let inv2 = format!("{}_inv", kind2);
                    if let Some((cr, ci)) = freq_two_hop.get(&(inv_key.clone(), inv2)) {
                        let (njr, nji) = &freq_identities[*nj];
                        complex_add_mul(
                            &mut h_re,
                            &mut h_im,
                            cr,
                            ci,
                            njr,
                            nji,
                            two_hop_decay * w * w2,
                        );
                    }
                }
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

    let norms: Vec<f64> = holograms
        .iter()
        .map(|h| h.iter().map(|x| x * x).sum::<f64>().sqrt())
        .collect();
    let avg_norm = norms.iter().copied().sum::<f64>() / n as f64;
    let max_norm = norms.iter().cloned().fold(0.0f64, f64::max);
    eprintln!(
        "  [hrr] hologram norms (post-normalize): avg={:.3} max={:.3}",
        avg_norm, max_norm
    );

    let term_idf: HashMap<String, f64> = all_terms
        .iter()
        .map(|t| {
            let df = *term_doc_count.get(t).unwrap_or(&1) as f64;
            let idf = (1.0 + (n as f64 / df).ln()).max(0.1);
            (t.clone(), idf)
        })
        .collect();

    let mut lang_groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, lang) in symbol_langs.iter().enumerate() {
        lang_groups.entry(lang.clone()).or_default().push(i);
    }

    let mut lang_centroids: HashMap<String, Vec<f64>> = HashMap::new();
    for (lang, indices) in &lang_groups {
        if indices.len() < 5 {
            continue;
        }
        let mut centroid = vec![0.0; dim];
        for &idx in indices {
            for j in 0..dim {
                centroid[j] += holograms[idx][j];
            }
        }
        let n_lang = indices.len() as f64;
        for j in 0..dim {
            centroid[j] /= n_lang;
        }
        let cn: f64 = centroid.iter().map(|x| x * x).sum::<f64>().sqrt();
        if cn > 1e-10 {
            for x in centroid.iter_mut() {
                *x /= cn;
            }
        }
        lang_centroids.insert(lang.clone(), centroid);
    }

    let main_langs: Vec<String> = lang_centroids.keys().cloned().collect();
    let n_main = main_langs.len();
    eprintln!(
        "  [hrr] fiber: {} languages with centroids ({:?})",
        n_main,
        main_langs
            .iter()
            .map(|l| {
                let cnt = lang_groups.get(l).map(|v| v.len()).unwrap_or(0);
                format!("{}:{}", l, cnt)
            })
            .collect::<Vec<_>>()
    );

    let mut shared_basis: Vec<Vec<f64>> = Vec::new();
    for lang in &main_langs {
        let centroid = lang_centroids.get(lang).unwrap();
        let mut v = centroid.clone();
        for b in &shared_basis {
            let proj = dot(&v, b);
            for j in 0..dim {
                v[j] -= proj * b[j];
            }
        }
        let vn: f64 = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        if vn > 1e-6 {
            for x in v.iter_mut() {
                *x /= vn;
            }
            shared_basis.push(v);
        }
    }
    eprintln!(
        "  [hrr] fiber: {} shared basis vectors (orthogonalized)",
        shared_basis.len()
    );

    let shared_holograms: Vec<Vec<f64>> = holograms
        .iter()
        .map(|h| {
            let mut proj = vec![0.0; dim];
            for b in &shared_basis {
                let d = dot(h, b);
                for j in 0..dim {
                    proj[j] += d * b[j];
                }
            }
            let pn: f64 = proj.iter().map(|x| x * x).sum::<f64>().sqrt();
            if pn > 1e-10 {
                for x in proj.iter_mut() {
                    *x /= pn;
                }
            }
            proj
        })
        .collect();

    let adjacency: Vec<Vec<(usize, f64)>> = outgoing
        .iter()
        .map(|edges| {
            let mut adj: Vec<(usize, f64)> =
                edges.iter().map(|(idx, _kind, w)| (*idx, *w)).collect();
            adj.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
            adj
        })
        .collect();

    Ok(HrrIndex {
        symbol_ids,
        symbol_names,
        holograms,
        identity_holograms,
        boundary_freq_re,
        boundary_freq_im,
        boundary_areas,
        shared_holograms,
        lang_centroids,
        symbol_langs,
        term_vectors,
        term_idf,
        dim,
        adjacency,
    })
}

impl HrrIndex {
    pub fn id_to_idx(&self, id: &i64) -> Option<usize> {
        self.symbol_ids.iter().position(|s| s == id)
    }

    pub fn hologram(&self, idx: usize) -> &[f64] {
        &self.holograms[idx]
    }

    pub fn shared_hologram(&self, idx: usize) -> &[f64] {
        &self.shared_holograms[idx]
    }

    pub fn symbol_lang(&self, idx: usize) -> &str {
        &self.symbol_langs[idx]
    }

    pub fn lang_centroid(&self, lang: &str) -> Option<&Vec<f64>> {
        self.lang_centroids.get(lang)
    }

    pub fn hologram_neighbors(&self, idx: usize) -> &[(usize, f64)] {
        self.adjacency.get(idx).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn boundary_freq(&self, idx: usize) -> (&[f64], &[f64]) {
        (&self.boundary_freq_re[idx], &self.boundary_freq_im[idx])
    }

    pub fn boundary_area(&self, idx: usize) -> f64 {
        self.boundary_areas[idx]
    }

    pub fn has_fibers(&self) -> bool {
        !self.lang_centroids.is_empty()
    }

    pub fn lang_count(&self) -> usize {
        self.lang_centroids.len()
    }

    pub fn query_vec(&self, query: &str) -> Option<Vec<f64>> {
        let q = build_query_vec(query, &self.term_vectors, &self.term_idf, self.dim);
        let norm: f64 = q.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm < 1e-10 {
            None
        } else {
            Some(q)
        }
    }
}

pub fn build_query_vec_public(query: &str, index: &HrrIndex) -> Option<Vec<f64>> {
    index.query_vec(query)
}

fn build_query_vec(
    query: &str,
    term_vectors: &HashMap<String, Vec<f64>>,
    _term_idf: &HashMap<String, f64>,
    dim: usize,
) -> Vec<f64> {
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

pub fn hrr_search(query: &str, index: &HrrIndex, top_k: usize) -> Vec<(i64, f64)> {
    let q = build_query_vec(query, &index.term_vectors, &index.term_idf, index.dim);
    if q.iter().all(|x| x.abs() < 1e-10) {
        return Vec::new();
    }

    let mut scored: Vec<(usize, f64)> = (0..index.symbol_ids.len())
        .map(|i| (i, dot(&q, &index.holograms[i])))
        .filter(|(_, s)| *s > 0.0)
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored.truncate(top_k);

    scored
        .into_iter()
        .map(|(i, s)| (index.symbol_ids[i], s))
        .collect()
}

pub fn hrr_rerank(
    query: &str,
    candidate_ids: &[i64],
    candidate_scores: &[f64],
    index: &HrrIndex,
) -> Vec<(i64, f64)> {
    let id_to_idx: HashMap<i64, usize> = index
        .symbol_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i))
        .collect();

    let q = build_query_vec(query, &index.term_vectors, &index.term_idf, index.dim);
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
                let hrr_sim = dot(&q, &index.holograms[idx]).max(0.0);
                let boost = 1.0 + 0.10 * hrr_sim;
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

pub fn hrr_fiber_rerank(
    query: &str,
    candidate_ids: &[i64],
    candidate_scores: &[f64],
    index: &HrrIndex,
) -> Vec<(i64, f64)> {
    let id_to_idx: HashMap<i64, usize> = index
        .symbol_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i))
        .collect();

    let q = build_query_vec(query, &index.term_vectors, &index.term_idf, index.dim);
    let q_empty = q.iter().all(|x| x.abs() < 1e-10);

    if !index.has_fibers() || q_empty {
        return hrr_rerank(query, candidate_ids, candidate_scores, index);
    }

    let n_langs = index.lang_centroids.len();
    if n_langs < 2 {
        return hrr_rerank(query, candidate_ids, candidate_scores, index);
    }

    let mut q_shared: Vec<f64> = vec![0.0; index.dim];
    for centroid in index.lang_centroids.values() {
        let proj = dot(&q, centroid);
        for j in 0..index.dim {
            q_shared[j] += proj * centroid[j];
        }
    }
    let qsn: f64 = q_shared.iter().map(|x| x * x).sum::<f64>().sqrt();
    if qsn > 1e-10 {
        for x in q_shared.iter_mut() {
            *x /= qsn;
        }
    }
    let shared_has_signal = qsn > 0.05;

    let alpha = 0.15;

    let mut scored: Vec<(i64, f64)> = candidate_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| {
            let base = candidate_scores.get(i).copied().unwrap_or(0.0);
            if base <= 0.0 {
                return (id, base);
            }
            if let Some(&idx) = id_to_idx.get(&id) {
                let hrr_sim = dot(&q, &index.holograms[idx]).max(0.0);

                if shared_has_signal {
                    let shared_sim = dot(&q_shared, &index.shared_holograms[idx]).max(0.0);
                    let combined = (1.0 - alpha) * hrr_sim + alpha * shared_sim;
                    let boost = 1.0 + 0.10 * combined;
                    (id, base * boost)
                } else {
                    let boost = 1.0 + 0.10 * hrr_sim;
                    (id, base * boost)
                }
            } else {
                (id, base)
            }
        })
        .filter(|(_, s)| *s > 0.0)
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored
}

pub fn hrr_fractal_attract(
    seed_ids: &[i64],
    index: &HrrIndex,
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
    let mut attractor: Vec<f64> = vec![0.0; index.dim];
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

    for _iter in 0..iterations {
        let mut scored: Vec<(usize, f64)> = (0..index.symbol_ids.len())
            .map(|i| (i, dot(&attractor, &index.holograms[i])))
            .filter(|(_, s)| *s > 0.01)
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        scored.truncate(expand_k);

        if scored.is_empty() {
            break;
        }

        let n_top = scored.len() as f64;
        let weights: Vec<f64> = scored.iter().map(|(_, s)| s.max(0.01)).collect();
        let w_sum: f64 = weights.iter().sum();

        let mut centroid: Vec<f64> = vec![0.0; index.dim];
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
        let new_norm: f64 = attractor.iter().map(|x| x * x).sum::<f64>().sqrt();
        if new_norm > 1e-10 {
            for x in attractor.iter_mut() {
                *x /= new_norm;
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

pub fn hrr_fiber_search(query: &str, index: &HrrIndex, top_k: usize) -> Vec<(i64, f64)> {
    if !index.has_fibers() || index.lang_centroids.len() < 2 {
        return Vec::new();
    }

    let q = build_query_vec(query, &index.term_vectors, &index.term_idf, index.dim);
    if q.iter().all(|x| x.abs() < 1e-10) {
        return Vec::new();
    }

    let mut q_shared: Vec<f64> = vec![0.0; index.dim];
    for centroid in index.lang_centroids.values() {
        let proj = dot(&q, centroid);
        for j in 0..index.dim {
            q_shared[j] += proj * centroid[j];
        }
    }
    let qsn: f64 = q_shared.iter().map(|x| x * x).sum::<f64>().sqrt();
    if qsn < 0.05 {
        return Vec::new();
    }
    for x in q_shared.iter_mut() {
        *x /= qsn;
    }

    let mut scored: Vec<(usize, f64)> = (0..index.symbol_ids.len())
        .map(|i| {
            let shared_sim = dot(&q_shared, &index.shared_holograms[i]);
            let full_sim = dot(&q, &index.holograms[i]).max(0.0);
            let blended = 0.6 * shared_sim + 0.4 * full_sim;
            (i, blended)
        })
        .filter(|(_, s)| *s > 0.0)
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored.truncate(top_k);

    scored
        .into_iter()
        .map(|(i, s)| (index.symbol_ids[i], s))
        .collect()
}

pub fn hrr_seed_expand(seed_ids: &[i64], index: &HrrIndex, top_k: usize) -> Vec<(i64, f64)> {
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
    let mut seed_holo = vec![0.0; index.dim];
    for &si in &seed_indices {
        for j in 0..index.dim {
            seed_holo[j] += index.holograms[si][j] / n_seeds;
        }
    }

    let seed_norm: f64 = seed_holo.iter().map(|x| x * x).sum::<f64>().sqrt();
    if seed_norm < 1e-10 {
        return Vec::new();
    }

    let mut scored: Vec<(usize, f64)> = (0..index.symbol_ids.len())
        .map(|i| {
            let s = dot(&seed_holo, &index.holograms[i]).max(0.0);
            (i, s)
        })
        .filter(|(_, s)| *s > 0.0)
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored.truncate(top_k);

    scored
        .into_iter()
        .map(|(i, s)| (index.symbol_ids[i], s))
        .collect()
}

pub fn hrr_expand_query(seed_ids: &[i64], index: &HrrIndex) -> String {
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
        return String::new();
    }

    let n_seeds = seed_indices.len() as f64;
    let mut seed_holo = vec![0.0; index.dim];
    for &si in &seed_indices {
        for j in 0..index.dim {
            seed_holo[j] += index.holograms[si][j] / n_seeds;
        }
    }

    let seed_norm: f64 = seed_holo.iter().map(|x| x * x).sum::<f64>().sqrt();
    if seed_norm < 1e-10 {
        return String::new();
    }

    let mut scored: Vec<(usize, f64)> = (0..index.symbol_ids.len())
        .map(|i| (i, dot(&seed_holo, &index.holograms[i]).max(0.0)))
        .filter(|(_, s)| *s > 0.0)
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    let mut expanded_terms: HashMap<String, f64> = HashMap::new();
    let max_expand = 10.min(scored.len());
    for &(si, score) in scored.iter().take(max_expand) {
        let terms = extract_terms(&index.symbol_names[si]);
        for t in terms {
            let entry = expanded_terms.entry(t).or_insert(0.0);
            *entry += score;
        }
    }

    let mut term_vec: Vec<(String, f64)> = expanded_terms.into_iter().collect();
    term_vec.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    term_vec
        .iter()
        .take(15)
        .map(|(t, _)| t.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn hrr_antivector_rerank(
    query: &str,
    candidate_ids: &[i64],
    candidate_scores: &[f64],
    index: &HrrIndex,
    beta: f64,
) -> Vec<(i64, f64)> {
    let id_to_idx: HashMap<i64, usize> = index
        .symbol_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i))
        .collect();

    let q = build_query_vec(query, &index.term_vectors, &index.term_idf, index.dim);
    if q.iter().all(|x| x.abs() < 1e-10) {
        return candidate_ids
            .iter()
            .zip(candidate_scores.iter())
            .map(|(&id, &s)| (id, s))
            .collect();
    }

    let q_norm_sq: f64 = q.iter().map(|x| x * x).sum();

    let seed_indices: Vec<usize> = candidate_ids
        .iter()
        .filter_map(|id| id_to_idx.get(id).copied())
        .collect();

    let n_seeds = seed_indices.len() as f64;
    let mut anti_dir = vec![0.0; index.dim];
    for &si in &seed_indices {
        let h = &index.holograms[si];
        let proj_scale = dot(&q, h) / q_norm_sq;
        for j in 0..index.dim {
            anti_dir[j] += (h[j] - proj_scale * q[j]) / n_seeds;
        }
    }

    let anti_norm: f64 = anti_dir.iter().map(|x| x * x).sum::<f64>().sqrt();
    if anti_norm < 1e-10 {
        return hrr_rerank(query, candidate_ids, candidate_scores, index);
    }
    for x in anti_dir.iter_mut() {
        *x /= anti_norm;
    }

    let mut scored: Vec<(i64, f64)> = candidate_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| {
            let base = candidate_scores.get(i).copied().unwrap_or(0.0);
            if base <= 0.0 {
                return (id, base);
            }
            if let Some(&idx) = id_to_idx.get(&id) {
                let hrr_sim = dot(&q, &index.holograms[idx]).max(0.0);
                let anti_sim = dot(&anti_dir, &index.holograms[idx]);
                let boost = 1.0 + 0.10 * hrr_sim + beta * anti_sim;
                (id, base * boost.max(0.0))
            } else {
                (id, base)
            }
        })
        .filter(|(_, s)| *s > 0.0)
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored
}

pub fn hrr_antivector_expand(
    query: &str,
    seed_ids: &[i64],
    index: &HrrIndex,
    top_k: usize,
    beta: f64,
) -> Vec<(i64, f64)> {
    let id_to_idx: HashMap<i64, usize> = index
        .symbol_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i))
        .collect();

    let q = build_query_vec(query, &index.term_vectors, &index.term_idf, index.dim);
    if q.iter().all(|x| x.abs() < 1e-10) {
        return Vec::new();
    }

    let q_norm_sq: f64 = q.iter().map(|x| x * x).sum();

    let seed_indices: Vec<usize> = seed_ids
        .iter()
        .filter_map(|id| id_to_idx.get(id).copied())
        .collect();

    let n_seeds = seed_indices.len() as f64;
    let mut anti_dir = vec![0.0; index.dim];
    for &si in &seed_indices {
        let h = &index.holograms[si];
        let proj_scale = dot(&q, h) / q_norm_sq;
        for j in 0..index.dim {
            anti_dir[j] += (h[j] - proj_scale * q[j]) / n_seeds;
        }
    }

    let anti_norm: f64 = anti_dir.iter().map(|x| x * x).sum::<f64>().sqrt();
    if anti_norm < 1e-10 {
        return Vec::new();
    }
    for x in anti_dir.iter_mut() {
        *x /= anti_norm;
    }

    let seed_set: HashSet<i64> = seed_ids.iter().copied().collect();

    let mut scored: Vec<(usize, f64)> = (0..index.symbol_ids.len())
        .filter(|i| !seed_set.contains(&index.symbol_ids[*i]))
        .map(|i| {
            let h = &index.holograms[i];
            let q_sim = dot(&q, h).max(0.0);
            let anti_sim = dot(&anti_dir, h);
            let score = q_sim + beta * anti_sim;
            (i, score)
        })
        .filter(|(_, s)| *s > 0.0)
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored.truncate(top_k);

    scored
        .into_iter()
        .map(|(i, s)| (index.symbol_ids[i], s))
        .collect()
}

pub fn hrr_bivector_expand(seed_ids: &[i64], index: &HrrIndex, top_k: usize) -> Vec<(i64, f64)> {
    let (expanded, _) = hrr_bivector_expand_scored(seed_ids, index, top_k);
    expanded
}

pub fn hrr_bivector_expand_scored(
    seed_ids: &[i64],
    index: &HrrIndex,
    top_k: usize,
) -> (Vec<(i64, f64)>, f64) {
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
        return (Vec::new(), 0.0);
    }

    let dim = index.dim;
    let mut bivec = vec![0.0; dim];
    let mut n_pairs = 0.0;

    let max_seeds = seed_indices.len().min(10);
    let mut pairwise_cos: Vec<f64> = Vec::new();
    for i in 0..max_seeds {
        for j in (i + 1)..max_seeds {
            let a = &index.holograms[seed_indices[i]];
            let b = &index.holograms[seed_indices[j]];
            let a_dot_b = dot(a, b);
            let a_norm: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
            let b_norm: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();
            if a_norm < 1e-10 || b_norm < 1e-10 {
                continue;
            }
            pairwise_cos.push(a_dot_b / (a_norm * b_norm));
            let b_norm_sq = b_norm * b_norm;
            let proj_scale = a_dot_b / b_norm_sq;
            for k in 0..dim {
                bivec[k] += (a[k] - proj_scale * b[k]) / dim as f64;
            }
            n_pairs += 1.0;
        }
    }

    let coherence = if pairwise_cos.is_empty() {
        0.0
    } else {
        pairwise_cos.iter().sum::<f64>() / pairwise_cos.len() as f64
    };

    if n_pairs < 1.0 {
        return (Vec::new(), coherence);
    }
    for x in bivec.iter_mut() {
        *x /= n_pairs;
    }

    let bv_norm: f64 = bivec.iter().map(|x| x * x).sum::<f64>().sqrt();
    if bv_norm < 1e-10 {
        return (Vec::new(), coherence);
    }
    for x in bivec.iter_mut() {
        *x /= bv_norm;
    }

    let seed_set: HashSet<i64> = seed_ids.iter().copied().collect();

    let mut scored: Vec<(usize, f64)> = (0..index.symbol_ids.len())
        .filter(|i| !seed_set.contains(&index.symbol_ids[*i]))
        .map(|i| {
            let score = dot(&bivec, &index.holograms[i]);
            (i, score)
        })
        .filter(|(_, s)| *s > 0.0)
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored.truncate(top_k);

    let result = scored
        .into_iter()
        .map(|(i, s)| (index.symbol_ids[i], s))
        .collect();
    (result, coherence)
}

pub fn hrr_quantum_rerank(
    query: &str,
    bm25_ids: &[i64],
    bm25_scores: &[f64],
    expanded_ids: &[(i64, f64)],
    coherence: f64,
    index: &HrrIndex,
) -> Vec<(i64, f64)> {
    let id_to_idx: HashMap<i64, usize> = index
        .symbol_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i))
        .collect();

    let q = build_query_vec(query, &index.term_vectors, &index.term_idf, index.dim);
    let q_norm_sq: f64 = q.iter().map(|x| x * x).sum();
    if q_norm_sq < 1e-10 {
        let mut result: Vec<(i64, f64)> = bm25_ids
            .iter()
            .zip(bm25_scores.iter())
            .map(|(&id, &s)| (id, s))
            .collect();
        for &(id, score) in expanded_ids {
            result.push((id, score));
        }
        result.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        return result;
    }

    let bm25_set: HashSet<i64> = bm25_ids.iter().copied().collect();

    let mut candidate_map: HashMap<i64, (f64, bool)> = HashMap::new();
    for (i, &id) in bm25_ids.iter().enumerate() {
        let score = bm25_scores.get(i).copied().unwrap_or(0.0);
        if score > 0.0 {
            candidate_map.insert(id, (score, true));
        }
    }
    for &(id, raw_score) in expanded_ids {
        let adj = raw_score * (0.3 + 0.7 * coherence.max(0.0).min(1.0));
        let entry = candidate_map.entry(id).or_insert((0.0, false));
        if !entry.1 {
            entry.0 = entry.0.max(adj);
        }
    }

    let scored: Vec<(i64, f64)> = candidate_map
        .into_iter()
        .map(|(id, (base_score, is_bm25))| {
            if base_score <= 0.0 {
                return (id, 0.0);
            }

            let idx = match id_to_idx.get(&id) {
                Some(&i) => i,
                None => return (id, base_score),
            };
            let h = &index.holograms[idx];

            let hrr_sim = dot(&q, h).max(0.0);
            let h_norm: f64 = h.iter().map(|x| x * x).sum::<f64>().sqrt();

            if is_bm25 {
                let theta = if h_norm > 1e-10 && hrr_sim > 0.0 {
                    (hrr_sim / h_norm).min(1.0).acos()
                } else {
                    std::f64::consts::FRAC_PI_2
                };

                let psi_re = base_score.sqrt() + hrr_sim.sqrt() * theta.cos();
                let psi_im = hrr_sim.sqrt() * theta.sin();
                let probability = psi_re * psi_re + psi_im * psi_im;

                let classical = base_score * (1.0 + 0.10 * hrr_sim);
                let blended = 0.5 * classical + 0.5 * probability;

                (id, blended)
            } else {
                let classical = base_score * (1.0 + 0.15 * hrr_sim);

                let q_proj = hrr_sim / q_norm_sq.sqrt().max(1e-10);
                let propagator = (q_proj * coherence).min(1.0);
                let virtual_boost = 1.0 + 0.3 * propagator;

                (id, classical * virtual_boost)
            }
        })
        .filter(|(_, s)| *s > 0.0)
        .collect();

    let mut result = scored;
    result.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    result
}

pub fn hrr_biv17_search(query: &str, index: &HrrIndex, top_k: usize) -> Vec<(i64, f64)> {
    hrr_search(query, index, top_k)
}

pub fn hrr_boundary_rerank(
    query: &str,
    candidate_ids: &[i64],
    candidate_scores: &[f64],
    index: &HrrIndex,
) -> Vec<(i64, f64)> {
    let id_to_idx: HashMap<i64, usize> = index
        .symbol_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i))
        .collect();

    let q = build_query_vec(query, &index.term_vectors, &index.term_idf, index.dim);
    let q_empty = q.iter().all(|x| x.abs() < 1e-10);

    let mut sims: Vec<(usize, f64)> = Vec::new();
    for (i, &id) in candidate_ids.iter().enumerate() {
        if q_empty {
            continue;
        }
        if let Some(&idx) = id_to_idx.get(&id) {
            let s = dot(&q, &index.holograms[idx]);
            if s > 0.0 {
                sims.push((i, s));
            }
        }
    }

    if sims.is_empty() {
        return candidate_ids
            .iter()
            .zip(candidate_scores.iter())
            .map(|(&id, &s)| (id, s))
            .filter(|(_, s)| *s > 0.0)
            .collect();
    }

    sims.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    let top_sim = sims[0].1;
    let threshold = top_sim * 0.3;

    let mut scored: Vec<(i64, f64)> = candidate_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| {
            let base = candidate_scores.get(i).copied().unwrap_or(0.0);
            if base <= 0.0 || q_empty {
                return (id, base);
            }
            if let Some(&idx) = id_to_idx.get(&id) {
                let hrr_sim = dot(&q, &index.holograms[idx]);

                if hrr_sim >= threshold {
                    let boost = 1.0 + 0.10 * hrr_sim;
                    (id, base * boost)
                } else {
                    (id, base)
                }
            } else {
                (id, base)
            }
        })
        .filter(|(_, s)| *s > 0.0)
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored
}

fn complex_dot(a_re: &[f64], a_im: &[f64], b_re: &[f64], b_im: &[f64]) -> f64 {
    a_re.iter()
        .zip(a_im.iter())
        .zip(b_re.iter())
        .zip(b_im.iter())
        .map(|(((ar, ai), br), bi)| ar * br + ai * bi)
        .sum()
}

pub fn hrr_holographic_rerank(
    query: &str,
    candidate_ids: &[i64],
    candidate_scores: &[f64],
    index: &HrrIndex,
) -> Vec<(i64, f64)> {
    let id_to_idx: HashMap<i64, usize> = index
        .symbol_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i))
        .collect();

    let q_time = build_query_vec(query, &index.term_vectors, &index.term_idf, index.dim);
    let q_empty = q_time.iter().all(|x| x.abs() < 1e-10);
    if q_empty {
        return candidate_ids
            .iter()
            .zip(candidate_scores.iter())
            .map(|(&id, &s)| (id, s))
            .filter(|(_, s)| *s > 0.0)
            .collect();
    }

    let mut q_freq_re = q_time.clone();
    let mut q_freq_im = vec![0.0; index.dim];
    fft_inplace(&mut q_freq_re, &mut q_freq_im);

    let mut scored: Vec<(i64, f64)> = candidate_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| {
            let base = candidate_scores.get(i).copied().unwrap_or(0.0);
            if base <= 0.0 {
                return (id, base);
            }
            if let Some(&idx) = id_to_idx.get(&id) {
                let (bnd_re, bnd_im) = index.boundary_freq(idx);
                let boundary_sim = complex_dot(&q_freq_re, &q_freq_im, bnd_re, bnd_im);

                let boundary_score = if boundary_sim > 0.0 {
                    boundary_sim
                } else {
                    0.0
                };

                let edge_tiebreak = {
                    let hrr_sim = dot(&q_time, &index.holograms[idx]).max(0.0);
                    hrr_sim * 0.1
                };

                let final_score = base * (1.0 + 0.15 * boundary_score + edge_tiebreak);
                (id, final_score)
            } else {
                (id, base)
            }
        })
        .filter(|(_, s)| *s > 0.0)
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored
}

pub fn hrr_holographic_search(query: &str, index: &HrrIndex, top_k: usize) -> Vec<(i64, f64)> {
    let q_time = build_query_vec(query, &index.term_vectors, &index.term_idf, index.dim);
    if q_time.iter().all(|x| x.abs() < 1e-10) {
        return Vec::new();
    }

    let mut q_freq_re = q_time.clone();
    let mut q_freq_im = vec![0.0; index.dim];
    fft_inplace(&mut q_freq_re, &mut q_freq_im);

    let mut scored: Vec<(usize, f64)> = (0..index.symbol_ids.len())
        .map(|i| {
            let (bnd_re, bnd_im) = index.boundary_freq(i);
            let boundary_sim = complex_dot(&q_freq_re, &q_freq_im, bnd_re, bnd_im);
            (i, boundary_sim)
        })
        .filter(|(_, s)| *s > 0.0)
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored.truncate(top_k);

    scored
        .into_iter()
        .map(|(i, s)| (index.symbol_ids[i], s))
        .collect()
}
