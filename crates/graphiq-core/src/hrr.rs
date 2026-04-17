use std::collections::{HashMap, HashSet};

use crate::db::GraphDb;
use crate::lsa::extract_terms;

const HRR_DIM: usize = 1024;

pub struct HrrIndex {
    pub symbol_ids: Vec<i64>,
    pub symbol_names: Vec<String>,
    pub holograms: Vec<Vec<f64>>,
    term_vectors: HashMap<String, Vec<f64>>,
    term_idf: HashMap<String, f64>,
    dim: usize,
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
            "SELECT s.id, s.name, s.qualified_name, f.path \
             FROM symbols s LEFT JOIN files f ON s.file_id = f.id \
             ORDER BY s.id",
        )
        .map_err(|e| e.to_string())?;
    let symbols: Vec<(i64, String, Option<String>, Option<String>)> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
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

    for (_, name, _, _) in &symbols {
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

    let symbol_ids: Vec<i64> = symbols.iter().map(|(id, _, _, _)| *id).collect();
    let symbol_names: Vec<String> = symbols.iter().map(|(_, name, _, _)| name.clone()).collect();
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
        holograms.push(h_re);
    }

    let norms: Vec<f64> = holograms
        .iter()
        .map(|h| h.iter().map(|x| x * x).sum::<f64>().sqrt())
        .collect();
    let avg_norm = norms.iter().sum::<f64>() / n as f64;
    let max_norm = norms.iter().cloned().fold(0.0f64, f64::max);
    eprintln!(
        "  [hrr] hologram norms: avg={:.3} max={:.3}",
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

    Ok(HrrIndex {
        symbol_ids,
        symbol_names,
        holograms,
        term_vectors,
        term_idf,
        dim,
    })
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
