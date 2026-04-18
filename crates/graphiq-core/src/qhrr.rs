use std::collections::{HashMap, HashSet};

use crate::db::GraphDb;
use crate::lsa::extract_terms;

const QHRR_DIM: usize = 1024;

pub struct QHrrIndex {
    pub symbol_ids: Vec<i64>,
    pub symbol_names: Vec<String>,
    freq_holograms_w: Vec<Vec<f64>>,
    freq_holograms_x: Vec<Vec<f64>>,
    freq_holograms_y: Vec<Vec<f64>>,
    freq_holograms_z: Vec<Vec<f64>>,
    time_holograms: Vec<Vec<f64>>,
    term_vecs_w: HashMap<String, Vec<f64>>,
    term_vecs_x: HashMap<String, Vec<f64>>,
    term_vecs_y: HashMap<String, Vec<f64>>,
    term_vecs_z: HashMap<String, Vec<f64>>,
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

#[inline]
fn quat_mul(
    aw: f64,
    ax: f64,
    ay: f64,
    az: f64,
    bw: f64,
    bx: f64,
    by: f64,
    bz: f64,
) -> (f64, f64, f64, f64) {
    (
        aw * bw - ax * bx - ay * by - az * bz,
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
    )
}

fn quat_add_scaled(
    hw: &mut [f64],
    hx: &mut [f64],
    hy: &mut [f64],
    hz: &mut [f64],
    rw: &[f64],
    rx: &[f64],
    ry: &[f64],
    rz: &[f64],
    nw: &[f64],
    nx: &[f64],
    ny: &[f64],
    nz: &[f64],
    scale: f64,
) {
    for i in 0..hw.len() {
        let (pw, px, py, pz) = quat_mul(rw[i], rx[i], ry[i], rz[i], nw[i], nx[i], ny[i], nz[i]);
        hw[i] += scale * pw;
        hx[i] += scale * px;
        hy[i] += scale * py;
        hz[i] += scale * pz;
    }
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

pub fn compute_qhrr(db: &GraphDb) -> Result<QHrrIndex, String> {
    let conn = db.conn();
    let dim = QHRR_DIM;

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
    eprintln!("  [qhrr] {} symbols, dim={}", n, dim);

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

    let mut term_vecs_w: HashMap<String, Vec<f64>> = HashMap::new();
    let mut term_vecs_x: HashMap<String, Vec<f64>> = HashMap::new();
    let mut term_vecs_y: HashMap<String, Vec<f64>> = HashMap::new();
    let mut term_vecs_z: HashMap<String, Vec<f64>> = HashMap::new();
    for term in &all_terms {
        let base_seed = hash_to_seed(&format!("qt:{}", term));
        term_vecs_w.insert(term.clone(), random_unit_vec(dim, base_seed));
        term_vecs_x.insert(
            term.clone(),
            random_unit_vec(dim, base_seed.wrapping_add(1)),
        );
        term_vecs_y.insert(
            term.clone(),
            random_unit_vec(dim, base_seed.wrapping_add(2)),
        );
        term_vecs_z.insert(
            term.clone(),
            random_unit_vec(dim, base_seed.wrapping_add(3)),
        );
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
    let mut freq_relations: HashMap<String, (Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>)> =
        HashMap::new();
    for rel in &relation_types {
        for suffix in &["", "_inv"] {
            let key = format!("{}{}", rel, suffix);
            let base_seed = hash_to_seed(&format!("qr{}:{}", suffix, rel));
            let vw = random_unit_vec(dim, base_seed);
            let vx = random_unit_vec(dim, base_seed.wrapping_add(1));
            let vy = random_unit_vec(dim, base_seed.wrapping_add(2));
            let vz = random_unit_vec(dim, base_seed.wrapping_add(3));

            let mut fw = vw;
            let mut iw = vec![0.0; dim];
            fft_inplace(&mut fw, &mut iw);
            let mut fx = vx;
            let mut ix = vec![0.0; dim];
            fft_inplace(&mut fx, &mut ix);
            let mut fy = vy;
            let mut iy = vec![0.0; dim];
            fft_inplace(&mut fy, &mut iy);
            let mut fz = vz;
            let mut iz = vec![0.0; dim];
            fft_inplace(&mut fz, &mut iz);

            freq_relations.insert(key, (fw, fx, fy, fz));
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

    let mut freq_identities: Vec<(Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>)> = Vec::with_capacity(n);
    for (si, term_set) in symbol_term_sets.iter().enumerate() {
        let mut idw = vec![0.0; dim];
        let mut idx_v = vec![0.0; dim];
        let mut idy = vec![0.0; dim];
        let mut idz = vec![0.0; dim];
        for t in term_set {
            if let (Some(tw), Some(tx), Some(ty), Some(tz)) = (
                term_vecs_w.get(t),
                term_vecs_x.get(t),
                term_vecs_y.get(t),
                term_vecs_z.get(t),
            ) {
                for j in 0..dim {
                    idw[j] += tw[j];
                    idx_v[j] += tx[j];
                    idy[j] += ty[j];
                    idz[j] += tz[j];
                }
            }
        }

        let norm_w: f64 = idw.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm_w > 1e-10 {
            for x in idw.iter_mut() {
                *x /= norm_w;
            }
        }
        let norm_x: f64 = idx_v.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm_x > 1e-10 {
            for x in idx_v.iter_mut() {
                *x /= norm_x;
            }
        }
        let norm_y: f64 = idy.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm_y > 1e-10 {
            for x in idy.iter_mut() {
                *x /= norm_y;
            }
        }
        let norm_z: f64 = idz.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm_z > 1e-10 {
            for x in idz.iter_mut() {
                *x /= norm_z;
            }
        }

        let mut fw = idw;
        let mut iw = vec![0.0; dim];
        fft_inplace(&mut fw, &mut iw);
        let mut fx = idx_v;
        let mut ix = vec![0.0; dim];
        fft_inplace(&mut fx, &mut ix);
        let mut fy = idy;
        let mut iy = vec![0.0; dim];
        fft_inplace(&mut fy, &mut iy);
        let mut fz = idz;
        let mut iz = vec![0.0; dim];
        fft_inplace(&mut fz, &mut iz);

        let _ = (si, iw, ix, iy, iz);
        freq_identities.push((fw, fx, fy, fz));
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
    eprintln!("  [qhrr] {} edges", n_edges);

    let two_hop_decay = 0.3f64;
    let max_2hop = 15usize;

    let mut freq_holograms_w: Vec<Vec<f64>> = Vec::with_capacity(n);
    let mut freq_holograms_x: Vec<Vec<f64>> = Vec::with_capacity(n);
    let mut freq_holograms_y: Vec<Vec<f64>> = Vec::with_capacity(n);
    let mut freq_holograms_z: Vec<Vec<f64>> = Vec::with_capacity(n);
    let mut time_holograms: Vec<Vec<f64>> = Vec::with_capacity(n);

    for i in 0..n {
        let mut hw = freq_identities[i].0.clone();
        let mut hx = freq_identities[i].1.clone();
        let mut hy = freq_identities[i].2.clone();
        let mut hz = freq_identities[i].3.clone();

        for (ni, kind, w) in &outgoing[i] {
            if let Some((rw, rx, ry, rz)) = freq_relations.get(kind.as_str()) {
                let (nw, nx, ny, nz) = &freq_identities[*ni];
                quat_add_scaled(
                    &mut hw, &mut hx, &mut hy, &mut hz, rw, rx, ry, rz, nw, nx, ny, nz, *w,
                );
            }

            if outgoing[*ni].len() <= max_2hop {
                for (nj, kind2, w2) in &outgoing[*ni] {
                    if *nj == i {
                        continue;
                    }
                    if let (Some(r1), Some(r2)) = (
                        freq_relations.get(kind.as_str()),
                        freq_relations.get(kind2.as_str()),
                    ) {
                        let mut c12w = vec![0.0; dim];
                        let mut c12x = vec![0.0; dim];
                        let mut c12y = vec![0.0; dim];
                        let mut c12z = vec![0.0; dim];
                        for j in 0..dim {
                            let (pw, px, py, pz) = quat_mul(
                                r1.0[j], r1.1[j], r1.2[j], r1.3[j], r2.0[j], r2.1[j], r2.2[j],
                                r2.3[j],
                            );
                            c12w[j] = pw;
                            c12x[j] = px;
                            c12y[j] = py;
                            c12z[j] = pz;
                        }
                        quat_add_scaled(
                            &mut hw,
                            &mut hx,
                            &mut hy,
                            &mut hz,
                            &c12w,
                            &c12x,
                            &c12y,
                            &c12z,
                            &freq_identities[*nj].0,
                            &freq_identities[*nj].1,
                            &freq_identities[*nj].2,
                            &freq_identities[*nj].3,
                            two_hop_decay * w * w2,
                        );
                    }
                }
            }
        }

        for (ni, kind, w) in &incoming[i] {
            let inv_key = format!("{}_inv", kind);
            if let Some((rw, rx, ry, rz)) = freq_relations.get(&inv_key) {
                let (nw, nx, ny, nz) = &freq_identities[*ni];
                quat_add_scaled(
                    &mut hw, &mut hx, &mut hy, &mut hz, rw, rx, ry, rz, nw, nx, ny, nz, *w,
                );
            }

            if incoming[*ni].len() <= max_2hop {
                for (nj, kind2, w2) in &incoming[*ni] {
                    if *nj == i {
                        continue;
                    }
                    let inv2 = format!("{}_inv", kind2);
                    if let (Some(r1), Some(r2)) =
                        (freq_relations.get(&inv_key), freq_relations.get(&inv2))
                    {
                        let mut c12w = vec![0.0; dim];
                        let mut c12x = vec![0.0; dim];
                        let mut c12y = vec![0.0; dim];
                        let mut c12z = vec![0.0; dim];
                        for j in 0..dim {
                            let (pw, px, py, pz) = quat_mul(
                                r1.0[j], r1.1[j], r1.2[j], r1.3[j], r2.0[j], r2.1[j], r2.2[j],
                                r2.3[j],
                            );
                            c12w[j] = pw;
                            c12x[j] = px;
                            c12y[j] = py;
                            c12z[j] = pz;
                        }
                        quat_add_scaled(
                            &mut hw,
                            &mut hx,
                            &mut hy,
                            &mut hz,
                            &c12w,
                            &c12x,
                            &c12y,
                            &c12z,
                            &freq_identities[*nj].0,
                            &freq_identities[*nj].1,
                            &freq_identities[*nj].2,
                            &freq_identities[*nj].3,
                            two_hop_decay * w * w2,
                        );
                    }
                }
            }
        }

        let mut tw = hw.clone();
        let mut tx = hx.clone();
        let mut ty = hy.clone();
        let mut tz = hz.clone();
        ifft_inplace(&mut tw, &mut tx);
        ifft_inplace(&mut ty, &mut tz);

        let mut time_h = vec![0.0; dim];
        for j in 0..dim {
            time_h[j] = tw[j];
        }
        let norm: f64 = time_h.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm > 1e-10 {
            for x in time_h.iter_mut() {
                *x /= norm;
            }
        }

        freq_holograms_w.push(hw);
        freq_holograms_x.push(hx);
        freq_holograms_y.push(hy);
        freq_holograms_z.push(hz);
        time_holograms.push(time_h);
    }

    let avg_norm = time_holograms
        .iter()
        .map(|h| h.iter().map(|x| x * x).sum::<f64>().sqrt())
        .sum::<f64>()
        / n as f64;
    eprintln!("  [qhrr] time hologram norms: avg={:.3}", avg_norm);

    let term_idf: HashMap<String, f64> = all_terms
        .iter()
        .map(|t| {
            let df = *term_doc_count.get(t).unwrap_or(&1) as f64;
            (t.clone(), (1.0 + (n as f64 / df).ln()).max(0.1))
        })
        .collect();

    Ok(QHrrIndex {
        symbol_ids,
        symbol_names,
        freq_holograms_w,
        freq_holograms_x,
        freq_holograms_y,
        freq_holograms_z,
        time_holograms,
        term_vecs_w,
        term_vecs_x,
        term_vecs_y,
        term_vecs_z,
        term_idf,
        dim,
    })
}

fn build_query_quat(
    query: &str,
    term_vecs_w: &HashMap<String, Vec<f64>>,
    term_vecs_x: &HashMap<String, Vec<f64>>,
    term_vecs_y: &HashMap<String, Vec<f64>>,
    term_vecs_z: &HashMap<String, Vec<f64>>,
    dim: usize,
) -> (Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>) {
    let terms = extract_terms(query);
    let mut qw = vec![0.0; dim];
    let mut qx = vec![0.0; dim];
    let mut qy = vec![0.0; dim];
    let mut qz = vec![0.0; dim];
    for t in &terms {
        if let (Some(tw), Some(tx), Some(ty), Some(tz)) = (
            term_vecs_w.get(t),
            term_vecs_x.get(t),
            term_vecs_y.get(t),
            term_vecs_z.get(t),
        ) {
            for j in 0..dim {
                qw[j] += tw[j];
                qx[j] += tx[j];
                qy[j] += ty[j];
                qz[j] += tz[j];
            }
        }
    }
    let nw: f64 = qw.iter().map(|x| x * x).sum::<f64>().sqrt();
    if nw > 1e-10 {
        for x in qw.iter_mut() {
            *x /= nw;
        }
    }
    let nx: f64 = qx.iter().map(|x| x * x).sum::<f64>().sqrt();
    if nx > 1e-10 {
        for x in qx.iter_mut() {
            *x /= nx;
        }
    }
    let ny: f64 = qy.iter().map(|x| x * x).sum::<f64>().sqrt();
    if ny > 1e-10 {
        for x in qy.iter_mut() {
            *x /= ny;
        }
    }
    let nz: f64 = qz.iter().map(|x| x * x).sum::<f64>().sqrt();
    if nz > 1e-10 {
        for x in qz.iter_mut() {
            *x /= nz;
        }
    }

    let mut fw = qw;
    let mut iw = vec![0.0; dim];
    fft_inplace(&mut fw, &mut iw);
    let mut fx = qx;
    let mut ix = vec![0.0; dim];
    fft_inplace(&mut fx, &mut ix);
    let mut fy = qy;
    let mut iy = vec![0.0; dim];
    fft_inplace(&mut fy, &mut iy);
    let mut fz = qz;
    let mut iz = vec![0.0; dim];
    fft_inplace(&mut fz, &mut iz);

    let _ = (iw, ix, iy, iz);
    (fw, fx, fy, fz)
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

fn hypercone_similarity(
    qw: &[f64],
    qx: &[f64],
    qy: &[f64],
    qz: &[f64],
    cw: &[f64],
    cx: &[f64],
    cy: &[f64],
    cz: &[f64],
) -> f64 {
    let dim = qw.len();
    let mut total_w = 0.0f64;
    let mut count = 0.0f64;

    for j in 0..dim {
        let (pw, _px, _py, _pz) =
            quat_mul(qw[j], qx[j], qy[j], qz[j], cw[j], -cx[j], -cy[j], -cz[j]);
        let pn = (pw * pw + _px * _px + _py * _py + _pz * _pz).sqrt();
        if pn > 1e-12 {
            total_w += pw / pn;
            count += 1.0;
        }
    }

    if count > 0.0 {
        total_w / count
    } else {
        0.0
    }
}

pub fn qhrr_rerank(
    query: &str,
    candidate_ids: &[i64],
    candidate_scores: &[f64],
    index: &QHrrIndex,
) -> Vec<(i64, f64)> {
    let id_to_idx: HashMap<i64, usize> = index
        .symbol_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i))
        .collect();

    let (qw, qx, qy, qz) = build_query_quat(
        query,
        &index.term_vecs_w,
        &index.term_vecs_x,
        &index.term_vecs_y,
        &index.term_vecs_z,
        index.dim,
    );
    let q_empty = qw.iter().all(|x| x.abs() < 1e-10);

    let q_time = {
        let terms = extract_terms(query);
        let mut q = vec![0.0; index.dim];
        for t in &terms {
            if let Some(tv) = index.term_vecs_w.get(t) {
                for j in 0..index.dim {
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
    };

    let mut scored: Vec<(i64, f64)> = candidate_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| {
            let base = candidate_scores.get(i).copied().unwrap_or(0.0);
            if base <= 0.0 || q_empty {
                return (id, base);
            }
            if let Some(&idx) = id_to_idx.get(&id) {
                let time_sim = dot(&q_time, &index.time_holograms[idx]).max(0.0);
                let boost = 1.0 + 0.10 * time_sim;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quat_mul_non_commutative() {
        let (r1w, r1x, r1y, r1z) = quat_mul(1.0, 0.5, 0.3, 0.2, 0.8, 0.1, 0.4, 0.6);
        let (r2w, r2x, r2y, r2z) = quat_mul(0.8, 0.1, 0.4, 0.6, 1.0, 0.5, 0.3, 0.2);
        assert!((r1w - r2w).abs() > 0.01 || (r1x - r2x).abs() > 0.01);
    }

    #[test]
    fn test_hypercone_self() {
        let dim = 64;
        let vw = random_unit_vec(dim, 42);
        let vx = random_unit_vec(dim, 43);
        let vy = random_unit_vec(dim, 44);
        let vz = random_unit_vec(dim, 45);
        let mut fw = vw;
        let mut iw = vec![0.0; dim];
        fft_inplace(&mut fw, &mut iw);
        let mut fx = vx;
        let mut ix = vec![0.0; dim];
        fft_inplace(&mut fx, &mut ix);
        let mut fy = vy;
        let mut iy = vec![0.0; dim];
        fft_inplace(&mut fy, &mut iy);
        let mut fz = vz;
        let mut iz = vec![0.0; dim];
        fft_inplace(&mut fz, &mut iz);
        let _ = (iw, ix, iy, iz);
        let sim = hypercone_similarity(&fw, &fx, &fy, &fz, &fw, &fx, &fy, &fz);
        assert!(sim > 0.9, "self sim should be > 0.9, got {}", sim);
    }

    #[test]
    fn test_qhrr_rerank() {
        use crate::db::GraphDb;
        use crate::edge::EdgeKind;
        use crate::symbol::{SymbolBuilder, SymbolKind};

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

        let idx = compute_qhrr(&db).unwrap();
        let res = qhrr_rerank("search", &[id1, id2], &[1.0, 0.8], &idx);
        assert!(!res.is_empty());
        assert_eq!(res[0].0, id1);
    }
}
