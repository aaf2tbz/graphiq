use std::collections::HashMap;

use rusqlite::params;

use crate::db::GraphDb;
use crate::tokenize::decompose_identifier;

const LSA_DIM: usize = 96;
const POWER_ITERS: usize = 3;
const OVERSAMPLING: usize = 16;

pub struct LsaIndex {
    pub term_basis: Vec<Vec<f64>>,
    pub term_index: HashMap<String, usize>,
    pub symbol_ids: Vec<i64>,
    pub symbol_vecs: Vec<Vec<f64>>,
    pub singular_values: Vec<f64>,
    pub term_idf: Vec<f64>,
    pub anisotropy_weights: Vec<f64>,
}

struct SparseMat {
    rows: usize,
    cols: usize,
    col_data: Vec<Vec<(usize, f64)>>,
}

impl SparseMat {
    fn right_multiply(&self, dense: &[Vec<f64>]) -> Vec<Vec<f64>> {
        let n = dense[0].len();
        let mut result = vec![vec![0.0f64; n]; self.rows];
        for (col_idx, col_entries) in self.col_data.iter().enumerate() {
            for &(row_idx, val) in col_entries {
                for j in 0..n {
                    result[row_idx][j] += val * dense[col_idx][j];
                }
            }
        }
        result
    }

    fn left_multiply(&self, dense: &[Vec<f64>]) -> Vec<Vec<f64>> {
        let k = if dense.is_empty() { 0 } else { dense[0].len() };
        let mut result = vec![vec![0.0f64; self.cols]; k];
        for (col_idx, col_entries) in self.col_data.iter().enumerate() {
            for &(row_idx, val) in col_entries {
                for i in 0..k {
                    result[i][col_idx] += dense[row_idx][i] * val;
                }
            }
        }
        result
    }
}

pub fn extract_terms(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let mut terms: Vec<String> = lower
        .split_whitespace()
        .filter(|t| t.len() >= 2)
        .filter(|t| !is_lsa_stop(t))
        .map(|t| t.to_string())
        .collect();

    let decomposed = decompose_identifier(text);
    for t in decomposed.split_whitespace() {
        let t = t.to_lowercase();
        if t.len() >= 2 && !is_lsa_stop(&t) {
            terms.push(t);
        }
    }

    terms
}

fn is_lsa_stop(t: &str) -> bool {
    matches!(
        t,
        "the"
            | "a"
            | "an"
            | "is"
            | "are"
            | "was"
            | "were"
            | "be"
            | "been"
            | "being"
            | "have"
            | "has"
            | "had"
            | "do"
            | "does"
            | "did"
            | "will"
            | "would"
            | "could"
            | "should"
            | "may"
            | "might"
            | "shall"
            | "can"
            | "need"
            | "to"
            | "of"
            | "in"
            | "for"
            | "on"
            | "with"
            | "at"
            | "by"
            | "from"
            | "as"
            | "into"
            | "through"
            | "during"
            | "before"
            | "after"
            | "above"
            | "below"
            | "between"
            | "out"
            | "off"
            | "over"
            | "under"
            | "again"
            | "further"
            | "then"
            | "once"
            | "here"
            | "there"
            | "when"
            | "where"
            | "why"
            | "how"
            | "all"
            | "each"
            | "every"
            | "both"
            | "few"
            | "more"
            | "most"
            | "other"
            | "some"
            | "such"
            | "no"
            | "nor"
            | "not"
            | "only"
            | "own"
            | "same"
            | "so"
            | "than"
            | "too"
            | "very"
            | "just"
            | "because"
            | "but"
            | "and"
            | "or"
            | "if"
            | "while"
            | "about"
            | "up"
            | "it"
            | "its"
            | "this"
            | "that"
            | "these"
            | "those"
            | "my"
            | "your"
            | "his"
            | "her"
            | "their"
            | "our"
            | "what"
            | "which"
            | "who"
            | "whom"
            | "self"
            | "pub"
            | "fn"
            | "let"
            | "mut"
            | "use"
            | "mod"
            | "impl"
            | "struct"
            | "enum"
            | "trait"
            | "type"
            | "const"
            | "static"
            | "return"
            | "new"
            | "true"
            | "false"
            | "none"
            | "some"
            | "ok"
            | "err"
            | "null"
            | "undefined"
            | "function"
            | "class"
            | "import"
            | "export"
            | "default"
            | "extends"
            | "implements"
    )
}

pub fn build_tfidf_matrix(db: &GraphDb) -> (SparseMat, HashMap<String, usize>, Vec<i64>, Vec<f64>) {
    let conn = db.conn();
    let mut stmt = conn
        .prepare(
            "SELECT id, name, name_decomposed, signature, doc_comment, source \
             FROM symbols WHERE visibility = 'public'",
        )
        .unwrap();

    let rows: Vec<(i64, String, String, String, String, String)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3).unwrap_or_default(),
                row.get::<_, String>(4).unwrap_or_default(),
                row.get::<_, String>(5).unwrap_or_default(),
            ))
        })
        .unwrap()
        .flatten()
        .collect();

    let n_symbols = rows.len();

    let mut term_counts: HashMap<String, usize> = HashMap::new();

    let mut term_freqs: Vec<HashMap<String, f64>> = Vec::with_capacity(n_symbols);

    for (_, name, decomp, sig, doc, src) in &rows {
        let combined = format!("{} {} {} {} {}", name, decomp, sig, doc, src);
        let terms = extract_terms(&combined);

        let mut tf: HashMap<String, f64> = HashMap::new();
        let total = terms.len() as f64;
        for t in &terms {
            *tf.entry(t.clone()).or_default() += 1.0;
        }
        for v in tf.values_mut() {
            *v /= total;
        }

        for t in tf.keys() {
            *term_counts.entry(t.clone()).or_insert(0) += 1;
        }

        term_freqs.push(tf);
    }

    let sym_id_to_col: HashMap<i64, usize> = rows
        .iter()
        .enumerate()
        .map(|(i, (id, _, _, _, _, _))| (*id, i))
        .collect();

    let structural_kinds = ["calls", "imports", "extends", "implements"];
    let mix_weight: f64 = 0.25;

    let mut aug_freqs: Vec<HashMap<String, f64>> = term_freqs.clone();

    for kind in &structural_kinds {
        let mut edge_stmt = conn
            .prepare("SELECT source_id, target_id FROM edges WHERE kind = ?1")
            .unwrap();
        let edge_rows: Vec<(i64, i64)> = edge_stmt
            .query_map([kind], |row: &rusqlite::Row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            })
            .unwrap()
            .flatten()
            .collect();

        for (source_id, target_id) in &edge_rows {
            if let (Some(&src_col), Some(&tgt_col)) =
                (sym_id_to_col.get(source_id), sym_id_to_col.get(target_id))
            {
                if src_col == tgt_col {
                    continue;
                }
                let neighbor_tf = &term_freqs[tgt_col];
                for (term, &freq) in neighbor_tf {
                    *aug_freqs[src_col].entry(term.clone()).or_default() += freq * mix_weight;
                }
                let source_tf = &term_freqs[src_col];
                for (term, &freq) in source_tf {
                    *aug_freqs[tgt_col].entry(term.clone()).or_default() += freq * mix_weight;
                }
            }
        }
    }

    let mut final_term_counts: HashMap<String, usize> = HashMap::new();
    for tf in &aug_freqs {
        for t in tf.keys() {
            *final_term_counts.entry(t.clone()).or_insert(0) += 1;
        }
    }

    let n_docs_f = n_symbols as f64;
    let idf: HashMap<String, f64> = final_term_counts
        .iter()
        .map(|(t, df)| {
            let idf_val = (1.0 + n_docs_f / (*df as f64 + 1.0)).ln();
            (t.clone(), idf_val)
        })
        .collect();

    let mut term_list: Vec<&String> = idf.keys().collect();
    term_list.sort();
    let term_index: HashMap<String, usize> = term_list
        .iter()
        .enumerate()
        .map(|(i, t)| ((*t).clone(), i))
        .collect();
    let n_terms = term_index.len();

    let mut col_data: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n_symbols];

    for (col, tf) in aug_freqs.iter().enumerate() {
        for (t, tf_val) in tf {
            if let Some(&row) = term_index.get(t) {
                let idf_val = idf[t];
                col_data[col].push((row, tf_val * idf_val));
            }
        }
    }

    let symbol_ids: Vec<i64> = rows.iter().map(|(id, _, _, _, _, _)| *id).collect();

    let mut idf_vec = vec![0.0f64; n_terms];
    for (t, &idx) in &term_index {
        if let Some(&val) = idf.get(t) {
            idf_vec[idx] = val;
        }
    }

    (
        SparseMat {
            rows: n_terms,
            cols: n_symbols,
            col_data,
        },
        term_index,
        symbol_ids,
        idf_vec,
    )
}

pub fn randomized_svd(matrix: &SparseMat, k: usize) -> (Vec<Vec<f64>>, Vec<f64>, Vec<Vec<f64>>) {
    let target = k + OVERSAMPLING;
    let cols = matrix.cols;

    let mut rng = SimpleRng::new(42);
    let mut omega: Vec<Vec<f64>> = (0..cols)
        .map(|_| (0..target).map(|_| rng.next()).collect())
        .collect();

    let mut y = matrix.right_multiply(&omega);

    let q = gram_schmidt(&y);

    let b = matrix.left_multiply(&q);

    let k_eff = q[0].len().min(target);
    let (u_hat, sigma, vt) = dense_svd(&b, k_eff.min(k));

    let u = mat_mul(&q, &u_hat, k);

    let term_basis: Vec<Vec<f64>> = (0..matrix.rows)
        .map(|i| (0..k).map(|j| u[i][j]).collect())
        .collect();

    let symbol_vecs: Vec<Vec<f64>> = (0..matrix.cols)
        .map(|j| {
            (0..k)
                .map(|i| {
                    if i < sigma.len() && sigma[i] > 1e-10 {
                        vt[i][j] * sigma[i]
                    } else {
                        0.0
                    }
                })
                .collect()
        })
        .collect();

    (term_basis, sigma, symbol_vecs)
}

pub fn normalize_to_sphere(vecs: &mut [Vec<f64>]) {
    for v in vecs.iter_mut() {
        let norm: f64 = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm > 1e-10 {
            for x in v.iter_mut() {
                *x /= norm;
            }
        }
    }
}

pub fn compute_anisotropy_weights(
    sigma: &[f64],
    symbol_vecs: &[Vec<f64>],
    alpha: f64,
    epsilon: f64,
) -> Vec<f64> {
    if sigma.is_empty() || symbol_vecs.is_empty() {
        return Vec::new();
    }

    let k = sigma.len();
    let n = symbol_vecs.len();

    let mut spec = vec![0.0f64; k];
    for dim in 0..k {
        let s_i = if dim < sigma.len() { sigma[dim] } else { 0.0 };

        let mut vals = Vec::with_capacity(n);
        for vec in symbol_vecs {
            if dim < vec.len() {
                vals.push(vec[dim]);
            }
        }
        if vals.len() < 2 {
            spec[dim] = s_i;
            continue;
        }

        let mean: f64 = vals.iter().sum::<f64>() / vals.len() as f64;
        let variance: f64 =
            vals.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / (vals.len() - 1) as f64;
        let std_dev = variance.sqrt();

        let disc = if std_dev > 1e-10 {
            1.0 - mean.abs() / std_dev
        } else {
            0.0
        };
        let disc = disc.max(0.0);

        spec[dim] = s_i * disc;
    }

    let max_spec = spec.iter().cloned().fold(0.0f64, f64::max);
    if max_spec < 1e-10 {
        return vec![epsilon; k];
    }

    let weights: Vec<f64> = spec
        .iter()
        .map(|&s| (s / max_spec).powf(alpha) + epsilon)
        .collect();

    weights
}

pub fn normalize_anisotropic(vecs: &mut [Vec<f64>], weights: &[f64]) {
    if weights.is_empty() {
        return;
    }
    for v in vecs.iter_mut() {
        for (i, x) in v.iter_mut().enumerate() {
            if i < weights.len() {
                *x *= weights[i];
            }
        }
        let norm: f64 = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm > 1e-10 {
            for x in v.iter_mut() {
                *x /= norm;
            }
        }
    }
}

pub fn angular_distance(a: &[f64], b: &[f64]) -> f64 {
    let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let dot = dot.clamp(-1.0, 1.0);
    dot.acos()
}

pub fn project_query(
    query: &str,
    term_index: &HashMap<String, usize>,
    term_basis: &[Vec<f64>],
    _n_terms: usize,
    anisotropy_weights: &[f64],
) -> Vec<f64> {
    let terms = extract_terms(query);
    let k = if term_basis.is_empty() {
        LSA_DIM
    } else {
        term_basis[0].len()
    };

    let mut query_vec = vec![0.0f64; k];
    let mut count = 0;
    for t in &terms {
        if let Some(&idx) = term_index.get(t) {
            if idx < term_basis.len() {
                for j in 0..k {
                    query_vec[j] += term_basis[idx][j];
                }
                count += 1;
            }
        }
    }

    if count > 0 {
        for x in query_vec.iter_mut() {
            *x /= count as f64;
        }
    }

    if !anisotropy_weights.is_empty() {
        for (i, x) in query_vec.iter_mut().enumerate() {
            if i < anisotropy_weights.len() {
                *x *= anisotropy_weights[i];
            }
        }
    }

    let norm: f64 = query_vec.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm > 1e-10 {
        for x in query_vec.iter_mut() {
            *x /= norm;
        }
    }

    query_vec
}

pub fn lsa_rerank(
    query: &str,
    candidates: &[(i64, f64)],
    db: &GraphDb,
    blend_weight: f64,
) -> Vec<(i64, f64)> {
    let (term_basis, term_index, _term_idf) = match load_lsa_basis(db) {
        Ok(b) if !b.0.is_empty() => b,
        _ => return candidates.to_vec(),
    };

    let symbol_map = match load_latent_vectors(db) {
        Ok(m) if !m.is_empty() => m,
        _ => return candidates.to_vec(),
    };

    let anisotropy_weights = load_anisotropy_weights(db).unwrap_or_default();

    let query_vec = project_query(query, &term_index, &term_basis, 0, &anisotropy_weights);
    let q_norm: f64 = query_vec.iter().map(|x| x * x).sum::<f64>().sqrt();
    if q_norm < 1e-10 {
        return candidates.to_vec();
    }

    let mut scored: Vec<(i64, f64, f64)> = Vec::with_capacity(candidates.len());
    for &(id, goober_score) in candidates {
        let sym_vec = match symbol_map.get(&id) {
            Some(v) => v,
            None => {
                scored.push((id, goober_score, 0.0));
                continue;
            }
        };

        let dot: f64 = query_vec
            .iter()
            .zip(sym_vec.iter())
            .map(|(a, b)| a * b)
            .sum();
        let s_norm: f64 = sym_vec.iter().map(|x| x * x).sum::<f64>().sqrt();
        let cosine = if s_norm > 1e-10 {
            (dot / (q_norm * s_norm)).clamp(0.0, 1.0)
        } else {
            0.0
        };

        scored.push((id, goober_score, cosine));
    }

    if scored.is_empty() {
        return candidates.to_vec();
    }

    let max_goober = scored
        .iter()
        .map(|&(_, g, _)| g)
        .fold(0.0f64, f64::max)
        .max(1e-10);

    let blended: Vec<(i64, f64)> = scored
        .into_iter()
        .map(|(id, g, c)| {
            let final_score = (1.0 - blend_weight) * g + blend_weight * c * max_goober;
            (id, final_score)
        })
        .collect();

    let mut result = blended;
    result.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    result
}

pub fn lsa_rerank_promote(
    query: &str,
    candidates: &[(i64, f64)],
    db: &GraphDb,
    boost_threshold: f64,
    boost_amount: f64,
) -> Vec<(i64, f64)> {
    let (term_basis, term_index, _term_idf) = match load_lsa_basis(db) {
        Ok(b) if !b.0.is_empty() => b,
        _ => return candidates.to_vec(),
    };

    let symbol_map = match load_latent_vectors(db) {
        Ok(m) if !m.is_empty() => m,
        _ => return candidates.to_vec(),
    };

    let anisotropy_weights = load_anisotropy_weights(db).unwrap_or_default();

    let query_vec = project_query(query, &term_index, &term_basis, 0, &anisotropy_weights);
    let q_norm: f64 = query_vec.iter().map(|x| x * x).sum::<f64>().sqrt();
    if q_norm < 1e-10 {
        return candidates.to_vec();
    }

    let max_goober = candidates
        .iter()
        .map(|&(_, g)| g)
        .fold(0.0f64, f64::max)
        .max(1e-10);

    let mut scored: Vec<(i64, f64)> = Vec::with_capacity(candidates.len());
    for &(id, goober_score) in candidates {
        let sym_vec = match symbol_map.get(&id) {
            Some(v) => v,
            None => {
                scored.push((id, goober_score));
                continue;
            }
        };

        let dot: f64 = query_vec
            .iter()
            .zip(sym_vec.iter())
            .map(|(a, b)| a * b)
            .sum();
        let s_norm: f64 = sym_vec.iter().map(|x| x * x).sum::<f64>().sqrt();
        let cosine = if s_norm > 1e-10 {
            (dot / (q_norm * s_norm)).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let boosted = if cosine > boost_threshold {
            goober_score + boost_amount * cosine * max_goober
        } else {
            goober_score
        };

        scored.push((id, boosted));
    }

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored
}

pub fn spherical_cap_search(
    query: &str,
    term_index: &HashMap<String, usize>,
    term_basis: &[Vec<f64>],
    term_idf: &[f64],
    symbol_vecs: &[Vec<f64>],
    symbol_ids: &[i64],
    top_k: usize,
) -> Vec<(i64, f64)> {
    let terms = extract_terms(query);
    let k = if term_basis.is_empty() {
        return Vec::new();
    } else {
        term_basis[0].len()
    };

    let mut term_projections: Vec<(usize, f64, Vec<f64>)> = Vec::new();
    for t in &terms {
        if let Some(&idx) = term_index.get(t) {
            if idx < term_basis.len() {
                let idf = term_idf.get(idx).copied().unwrap_or(1.0);
                let norm: f64 = term_basis[idx].iter().map(|x| x * x).sum::<f64>().sqrt();
                if norm > 1e-10 {
                    let mut v = term_basis[idx].clone();
                    for x in v.iter_mut() {
                        *x /= norm;
                    }
                    term_projections.push((idx, idf, v));
                }
            }
        }
    }

    if term_projections.is_empty() {
        return Vec::new();
    }

    let mut weighted_centroid = vec![0.0f64; k];
    let mut total_idf = 0.0f64;
    for (_term_idx, idf, ref term_vec) in &term_projections {
        for j in 0..k {
            weighted_centroid[j] += idf * term_vec[j];
        }
        total_idf += idf;
    }
    if total_idf > 0.0 {
        for x in weighted_centroid.iter_mut() {
            *x /= total_idf;
        }
    }
    let norm: f64 = weighted_centroid.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm > 1e-10 {
        for x in weighted_centroid.iter_mut() {
            *x /= norm;
        }
    }

    let n_symbols = symbol_vecs.len();
    let mut scores: Vec<f64> = vec![0.0; n_symbols];

    for i in 0..n_symbols {
        let centroid_angle = angular_distance(&weighted_centroid, &symbol_vecs[i]);
        let centroid_relevance = 1.0 - centroid_angle / std::f64::consts::PI;

        let mut cap_votes = 0usize;
        let theta = std::f64::consts::FRAC_PI_3;
        let mut min_angle = std::f64::consts::PI;
        for (_term_idx, _idf, ref term_vec) in &term_projections {
            let angle = angular_distance(term_vec, &symbol_vecs[i]);
            if angle < min_angle {
                min_angle = angle;
            }
            if angle < theta {
                cap_votes += 1;
            }
        }

        let vote_bonus = if term_projections.len() > 1 {
            (cap_votes as f64 / term_projections.len() as f64).sqrt()
        } else {
            1.0
        };

        scores[i] = centroid_relevance * vote_bonus;

        if cap_votes > 0 {
            let nearest_relevance = 1.0 - min_angle / std::f64::consts::PI;
            scores[i] = scores[i].max(nearest_relevance * vote_bonus);
        }
    }

    let mut indexed: Vec<(usize, f64)> = scores
        .iter()
        .enumerate()
        .filter(|(_, &s)| s > 0.0)
        .map(|(i, &s)| (i, s))
        .collect();
    indexed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    indexed.truncate(top_k);

    indexed
        .into_iter()
        .map(|(i, score)| (symbol_ids[i], score))
        .collect()
}

pub fn blade_search(
    query: &str,
    term_index: &HashMap<String, usize>,
    term_basis: &[Vec<f64>],
    term_idf: &[f64],
    symbol_vecs: &[Vec<f64>],
    symbol_ids: &[i64],
    top_k: usize,
) -> Vec<(i64, f64)> {
    let terms = extract_terms(query);
    let dim = if term_basis.is_empty() {
        return Vec::new();
    } else {
        term_basis[0].len()
    };

    let mut term_vecs: Vec<(f64, Vec<f64>)> = Vec::new();
    for t in &terms {
        if let Some(&idx) = term_index.get(t) {
            if idx < term_basis.len() {
                let idf = term_idf.get(idx).copied().unwrap_or(1.0);
                let norm: f64 = term_basis[idx].iter().map(|x| x * x).sum::<f64>().sqrt();
                if norm > 1e-10 {
                    let mut v = term_basis[idx].clone();
                    for x in v.iter_mut() {
                        *x /= norm;
                    }
                    term_vecs.push((idf, v));
                }
            }
        }
    }

    if term_vecs.is_empty() {
        return Vec::new();
    }

    let mut Q: Vec<Vec<f64>> = Vec::new();
    for (_, ref v) in &term_vecs {
        let mut u = v.clone();
        for q in &Q {
            let dot: f64 = u.iter().zip(q.iter()).map(|(a, b)| a * b).sum();
            for j in 0..dim {
                u[j] -= dot * q[j];
            }
        }
        let norm: f64 = u.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm > 1e-10 {
            for x in u.iter_mut() {
                *x /= norm;
            }
            Q.push(u);
        }
    }

    if Q.is_empty() {
        return Vec::new();
    }

    let grade = Q.len();
    let idf_sum: f64 = term_vecs.iter().map(|(idf, _)| *idf).sum();

    let n_symbols = symbol_vecs.len();
    let mut scored: Vec<(usize, f64)> = Vec::with_capacity(n_symbols);

    for i in 0..n_symbols {
        let s = &symbol_vecs[i];

        let mut proj = vec![0.0f64; dim];
        for q in &Q {
            let dot: f64 = s.iter().zip(q.iter()).map(|(a, b)| a * b).sum();
            for j in 0..dim {
                proj[j] += dot * q[j];
            }
        }

        let mut rejection_sq = 0.0f64;
        for j in 0..dim {
            let r = s[j] - proj[j];
            rejection_sq += r * r;
        }

        let inner_sum: f64 = s.iter().zip(proj.iter()).map(|(a, b)| a * b).sum();

        let blade_relevance = if grade == 1 {
            inner_sum.max(0.0)
        } else {
            let grade_bonus = 1.0 + 0.15 * (grade as f64 - 1.0);
            inner_sum.max(0.0) * grade_bonus / (1.0 + rejection_sq * 5.0)
        };

        let idf_boost = idf_sum / term_vecs.len() as f64;
        let score = blade_relevance * idf_boost;

        if score > 0.0 {
            scored.push((i, score));
        }
    }

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored.truncate(top_k);

    scored
        .into_iter()
        .map(|(i, score)| (symbol_ids[i], score))
        .collect()
}

pub fn compute_lsa(db: &GraphDb) -> Result<LsaIndex, String> {
    eprintln!("  building TF-IDF matrix...");
    let (matrix, term_index, symbol_ids, term_idf) = build_tfidf_matrix(db);
    eprintln!("  matrix: {} terms × {} symbols", matrix.rows, matrix.cols);

    eprintln!("  computing randomized SVD (k={})...", LSA_DIM);
    let (term_basis, singular_values, mut symbol_vecs) = randomized_svd(&matrix, LSA_DIM);

    let alpha = 1.0;
    let epsilon = 0.1;
    eprintln!(
        "  computing anisotropy weights (α={}, ε={})...",
        alpha, epsilon
    );
    let anisotropy_weights = compute_anisotropy_weights(
        &singular_values,
        &symbol_vecs,
        alpha,
        epsilon,
    );

    let k = if singular_values.is_empty() {
        0
    } else {
        singular_values.len().min(LSA_DIM)
    };
    for i in 0..k.min(anisotropy_weights.len()) {
        eprintln!(
            "    dim {:3}: σ={:10.4}  w={:.4}",
            i, singular_values[i], anisotropy_weights[i]
        );
    }

    eprintln!("  normalizing to anisotropic hypersphere...");
    normalize_anisotropic(&mut symbol_vecs, &anisotropy_weights);
    let mut term_basis_normed = term_basis;
    normalize_anisotropic(&mut term_basis_normed, &anisotropy_weights);

    eprintln!(
        "  LSA done: {} terms, {} symbols, top σ = {:.4}",
        term_index.len(),
        symbol_ids.len(),
        singular_values.first().copied().unwrap_or(0.0)
    );

    Ok(LsaIndex {
        term_basis: term_basis_normed,
        term_index,
        symbol_ids,
        symbol_vecs,
        singular_values,
        term_idf,
        anisotropy_weights,
    })
}

fn gram_schmidt(mat: &[Vec<f64>]) -> Vec<Vec<f64>> {
    if mat.is_empty() || mat[0].is_empty() {
        return mat.to_vec();
    }

    let rows = mat.len();
    let cols = mat[0].len();
    let mut q = mat.to_vec();

    for j in 0..cols {
        for i in 0..j {
            let dot: f64 = (0..rows).map(|r| q[r][i] * q[r][j]).sum();
            for r in 0..rows {
                q[r][j] -= dot * q[r][i];
            }
        }

        let norm: f64 = (0..rows).map(|r| q[r][j] * q[r][j]).sum::<f64>().sqrt();
        if norm > 1e-10 {
            for r in 0..rows {
                q[r][j] /= norm;
            }
        }
    }

    q
}

fn mat_mul(a: &[Vec<f64>], b: &[Vec<f64>], k: usize) -> Vec<Vec<f64>> {
    let rows = a.len();
    let mid = if a.is_empty() { 0 } else { a[0].len() };
    let mut result = vec![vec![0.0f64; k]; rows];
    for i in 0..rows {
        for j in 0..k {
            let mut sum = 0.0;
            for l in 0..mid {
                sum += a[i][l] * b[l][j];
            }
            result[i][j] = sum;
        }
    }
    result
}

fn dense_svd(mat: &[Vec<f64>], k: usize) -> (Vec<Vec<f64>>, Vec<f64>, Vec<Vec<f64>>) {
    let m = mat.len();
    let n = if mat.is_empty() { 0 } else { mat[0].len() };

    if m == 0 || n == 0 || k == 0 {
        return (Vec::new(), Vec::new(), Vec::new());
    }

    let k_eff = k.min(m).min(n);

    let mut bbt = vec![vec![0.0f64; m]; m];
    for i in 0..m {
        for j in 0..m {
            let mut sum = 0.0;
            for l in 0..n {
                sum += mat[i][l] * mat[j][l];
            }
            bbt[i][j] = sum;
        }
    }

    let (eigvals, eigvecs) = power_eigen(&bbt, k_eff);

    let mut sigma: Vec<f64> = eigvals.iter().map(|&v| v.max(0.0).sqrt()).collect();

    let mut sorted_indices: Vec<usize> = (0..eigvals.len()).collect();
    sorted_indices.sort_by(|&a, &b| eigvals[b].partial_cmp(&eigvals[a]).unwrap());

    let mut u_out = vec![vec![0.0f64; k_eff]; m];
    for i in 0..m {
        for j in 0..k_eff {
            let idx = sorted_indices[j];
            u_out[i][j] = eigvecs[idx][i];
        }
    }

    let mut sigma_out = vec![0.0f64; k_eff];
    for j in 0..k_eff {
        sigma_out[j] = sigma[sorted_indices[j]];
    }

    let mut vt = vec![vec![0.0f64; n]; k_eff];
    for j in 0..k_eff {
        let s = sigma_out[j].max(1e-10);
        for l in 0..n {
            let mut sum = 0.0;
            for i in 0..m {
                sum += u_out[i][j] * mat[i][l];
            }
            vt[j][l] = sum / s;
        }
    }

    (u_out, sigma_out, vt)
}

fn power_eigen(mat: &[Vec<f64>], k: usize) -> (Vec<f64>, Vec<Vec<f64>>) {
    let n = mat.len();
    if n == 0 || k == 0 {
        return (Vec::new(), Vec::new());
    }

    let mut rng = SimpleRng::new(123);
    let mut vectors: Vec<Vec<f64>> = (0..k)
        .map(|_| (0..n).map(|_| rng.next()).collect())
        .collect();

    for _ in 0..POWER_ITERS * 20 {
        let new = mat_vec_mul(mat, &vectors);
        let norm_per: Vec<f64> = new
            .iter()
            .map(|v| v.iter().map(|x| x * x).sum::<f64>().sqrt())
            .collect();

        for (i, v) in vectors.iter_mut().enumerate() {
            let n = norm_per[i].max(1e-10);
            for x in v.iter_mut() {
                *x /= n;
            }
        }

        for i in 1..k {
            for j in 0..i {
                let dot: f64 = vectors[i]
                    .iter()
                    .zip(vectors[j].iter())
                    .map(|(a, b)| a * b)
                    .sum();
                for l in 0..n {
                    vectors[i][l] -= dot * vectors[j][l];
                }
            }
            let norm: f64 = vectors[i].iter().map(|x| x * x).sum::<f64>().sqrt();
            if norm > 1e-10 {
                for x in vectors[i].iter_mut() {
                    *x /= norm;
                }
            }
        }
    }

    let mvm = mat_vec_mul(mat, &vectors);

    let eigenvalues: Vec<f64> = vectors
        .iter()
        .zip(mvm.iter())
        .map(|(v, mv)| v.iter().zip(mv.iter()).map(|(a, b)| a * b).sum())
        .collect();

    (eigenvalues, vectors)
}

fn mat_vec_mul(mat: &[Vec<f64>], vecs: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let n = mat.len();
    let k = vecs.len();
    (0..k)
        .map(|vi| {
            (0..n)
                .map(|i| {
                    let mut sum = 0.0;
                    for j in 0..n {
                        sum += mat[i][j] * vecs[vi][j];
                    }
                    sum
                })
                .collect()
        })
        .collect()
}

struct SimpleRng {
    state: u64,
}

impl SimpleRng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next(&mut self) -> f64 {
        self.state = self.state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let x = self.state;
        let val = ((x >> 33) as i64 as f64) / (1i64 << 31) as f64;
        val
    }
}

pub fn store_lsa_vectors(
    db: &GraphDb,
    symbol_ids: &[i64],
    vectors: &[Vec<f64>],
) -> Result<usize, String> {
    let conn = db.conn();
    conn.execute(
        "CREATE TABLE IF NOT EXISTS symbol_latent (
            symbol_id INTEGER NOT NULL PRIMARY KEY,
            latent BLOB NOT NULL,
            dim INTEGER NOT NULL
        )",
        [],
    )
    .map_err(|e| e.to_string())?;

    conn.execute("DELETE FROM symbol_latent", [])
        .map_err(|e| e.to_string())?;

    let dim = if vectors.is_empty() {
        0
    } else {
        vectors[0].len()
    };
    let mut count = 0;

    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    {
        let mut stmt = tx
            .prepare("INSERT INTO symbol_latent (symbol_id, latent, dim) VALUES (?1, ?2, ?3)")
            .map_err(|e| e.to_string())?;

        for (i, sym_id) in symbol_ids.iter().enumerate() {
            if i >= vectors.len() {
                break;
            }
            let bytes: Vec<u8> = vectors[i].iter().flat_map(|f| f.to_le_bytes()).collect();
            stmt.execute(params![sym_id, bytes, dim as i64])
                .map_err(|e| e.to_string())?;
            count += 1;
        }
    }
    tx.commit().map_err(|e| e.to_string())?;

    Ok(count)
}

pub fn store_lsa_basis(
    db: &GraphDb,
    term_basis: &[Vec<f64>],
    term_index: &HashMap<String, usize>,
    term_idf: &[f64],
) -> Result<(), String> {
    let conn = db.conn();
    conn.execute(
        "CREATE TABLE IF NOT EXISTS lsa_basis (
            id INTEGER PRIMARY KEY,
            term TEXT NOT NULL,
            term_idx INTEGER NOT NULL,
            basis_vec BLOB NOT NULL,
            dim INTEGER NOT NULL,
            idf REAL NOT NULL DEFAULT 0.0
        )",
        [],
    )
    .map_err(|e| e.to_string())?;

    conn.execute("DELETE FROM lsa_basis", [])
        .map_err(|e| e.to_string())?;

    let dim = if term_basis.is_empty() {
        0
    } else {
        term_basis[0].len()
    };

    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    {
        let mut stmt = tx
            .prepare("INSERT INTO lsa_basis (id, term, term_idx, basis_vec, dim, idf) VALUES (?1, ?2, ?3, ?4, ?5, ?6)")
            .map_err(|e| e.to_string())?;

        for (term, &idx) in term_index {
            if idx >= term_basis.len() {
                continue;
            }
            let bytes: Vec<u8> = term_basis[idx]
                .iter()
                .flat_map(|f| f.to_le_bytes())
                .collect();
            let idf_val = term_idf.get(idx).copied().unwrap_or(0.0);
            stmt.execute(params![
                idx as i64, term, idx as i64, bytes, dim as i64, idf_val
            ])
            .map_err(|e| e.to_string())?;
        }
    }
    tx.commit().map_err(|e| e.to_string())?;

    Ok(())
}

pub fn store_lsa_sigma(db: &GraphDb, sigma: &[f64]) -> Result<(), String> {
    let conn = db.conn();
    conn.execute(
        "CREATE TABLE IF NOT EXISTS lsa_sigma (
            id INTEGER PRIMARY KEY,
            sigma BLOB NOT NULL,
            dim INTEGER NOT NULL
        )",
        [],
    )
    .map_err(|e| e.to_string())?;

    conn.execute("DELETE FROM lsa_sigma", [])
        .map_err(|e| e.to_string())?;

    let bytes: Vec<u8> = sigma.iter().flat_map(|f| f.to_le_bytes()).collect();
    conn.execute(
        "INSERT INTO lsa_sigma (id, sigma, dim) VALUES (1, ?1, ?2)",
        params![bytes, sigma.len() as i64],
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub fn load_lsa_sigma(db: &GraphDb) -> Result<Vec<f64>, String> {
    let conn = db.conn();
    let row: Option<(Vec<u8>, i64)> = conn
        .query_row("SELECT sigma, dim FROM lsa_sigma WHERE id = 1", [], |row| {
            let bytes: Vec<u8> = row.get(0).unwrap_or_default();
            let dim: i64 = row.get(1).unwrap_or(0);
            Ok((bytes, dim))
        })
        .ok();

    match row {
        Some((bytes, dim)) if dim > 0 && bytes.len() == dim as usize * 8 => {
            let sigma: Vec<f64> = bytes
                .chunks_exact(8)
                .map(|c| f64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
                .collect();
            Ok(sigma)
        }
        _ => Err("no singular values stored".into()),
    }
}

pub fn load_latent_vectors(db: &GraphDb) -> Result<HashMap<i64, Vec<f64>>, String> {
    let conn = db.conn();
    let mut stmt = conn
        .prepare("SELECT symbol_id, latent, dim FROM symbol_latent")
        .map_err(|e| e.to_string())?;

    let rows: Vec<(i64, Vec<u8>, usize)> = stmt
        .query_map([], |row| {
            let bytes: Vec<u8> = row.get(1).unwrap_or_default();
            let dim: i64 = row.get(2).unwrap_or(0);
            Ok((row.get(0)?, bytes, dim as usize))
        })
        .map_err(|e| e.to_string())?
        .flatten()
        .collect();

    let mut result = HashMap::new();
    for (sym_id, bytes, dim) in rows {
        if dim == 0 || bytes.len() != dim * 8 {
            continue;
        }
        let vec: Vec<f64> = bytes
            .chunks_exact(8)
            .map(|chunk| {
                f64::from_le_bytes([
                    chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
                ])
            })
            .collect();
        result.insert(sym_id, vec);
    }

    Ok(result)
}

pub fn load_lsa_basis(
    db: &GraphDb,
) -> Result<(Vec<Vec<f64>>, HashMap<String, usize>, Vec<f64>), String> {
    let conn = db.conn();
    let mut stmt = conn
        .prepare("SELECT term, term_idx, basis_vec, dim, COALESCE(idf, 0.0) FROM lsa_basis ORDER BY term_idx")
        .map_err(|e| e.to_string())?;

    let rows: Vec<(String, usize, Vec<u8>, usize, f64)> = stmt
        .query_map([], |row| {
            let term: String = row.get(0)?;
            let idx: i64 = row.get(1)?;
            let bytes: Vec<u8> = row.get(2).unwrap_or_default();
            let dim: i64 = row.get(3).unwrap_or(0);
            let idf: f64 = row.get(4)?;
            Ok((term, idx as usize, bytes, dim as usize, idf))
        })
        .map_err(|e| e.to_string())?
        .flatten()
        .collect();

    if rows.is_empty() {
        return Ok((Vec::new(), HashMap::new(), Vec::new()));
    }

    let dim = rows[0].3;
    let max_idx = rows.iter().map(|(_, idx, _, _, _)| *idx).max().unwrap_or(0);
    let mut basis = vec![vec![0.0f64; dim]; max_idx + 1];
    let mut term_index = HashMap::new();
    let mut idf_vec = vec![0.0f64; max_idx + 1];

    for (term, idx, bytes, d, idf) in &rows {
        if *d != dim || bytes.len() != dim * 8 {
            continue;
        }
        let vec: Vec<f64> = bytes
            .chunks_exact(8)
            .map(|chunk| {
                f64::from_le_bytes([
                    chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
                ])
            })
            .collect();
        basis[*idx] = vec;
        idf_vec[*idx] = *idf;
        term_index.insert(term.clone(), *idx);
    }

    Ok((basis, term_index, idf_vec))
}

pub fn store_anisotropy_weights(db: &GraphDb, weights: &[f64]) -> Result<(), String> {
    let conn = db.conn();
    conn.execute(
        "CREATE TABLE IF NOT EXISTS lsa_anisotropy (
            id INTEGER PRIMARY KEY,
            weights BLOB NOT NULL,
            dim INTEGER NOT NULL
        )",
        [],
    )
    .map_err(|e| e.to_string())?;

    conn.execute("DELETE FROM lsa_anisotropy", [])
        .map_err(|e| e.to_string())?;

    let bytes: Vec<u8> = weights.iter().flat_map(|f| f.to_le_bytes()).collect();
    conn.execute(
        "INSERT INTO lsa_anisotropy (id, weights, dim) VALUES (1, ?1, ?2)",
        params![bytes, weights.len() as i64],
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub fn load_anisotropy_weights(db: &GraphDb) -> Result<Vec<f64>, String> {
    let conn = db.conn();
    let row: Option<(Vec<u8>, i64)> = conn
        .query_row(
            "SELECT weights, dim FROM lsa_anisotropy WHERE id = 1",
            [],
            |row| {
                let bytes: Vec<u8> = row.get(0).unwrap_or_default();
                let dim: i64 = row.get(1).unwrap_or(0);
                Ok((bytes, dim))
            },
        )
        .ok();

    match row {
        Some((bytes, dim)) if dim > 0 && bytes.len() == dim as usize * 8 => {
            let weights: Vec<f64> = bytes
                .chunks_exact(8)
                .map(|c| {
                    f64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]])
                })
                .collect();
            Ok(weights)
        }
        _ => Err("no anisotropy weights stored".into()),
    }
}

#[cfg(test)]
mod anisotropy_tests {
    use super::*;

    fn approx_eq(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-8
    }

    #[test]
    fn test_discriminativity_high_when_zero_mean_high_spread() {
        let sigma = vec![10.0, 5.0, 1.0];
        let symbol_vecs = vec![
            vec![10.0, 5.0, 1.0],
            vec![-10.0, -5.0, -1.0],
            vec![5.0, 0.0, 0.5],
            vec![-5.0, 0.0, -0.5],
        ];
        let weights = compute_anisotropy_weights(&sigma, &symbol_vecs, 1.0, 0.1);
        assert_eq!(weights.len(), 3);
        assert!(weights[0] > 0.1, "dim 0 should have weight > epsilon");
        assert!(weights[1] > 0.1, "dim 1 should have weight > epsilon");
        assert!(weights[2] > 0.1, "dim 2 should have weight > epsilon");
    }

    #[test]
    fn test_discriminativity_low_when_high_mean_low_spread() {
        let sigma = vec![10.0, 5.0];
        let symbol_vecs = vec![
            vec![100.0, 0.1],
            vec![100.0, -0.1],
            vec![100.0, 0.05],
        ];
        let weights = compute_anisotropy_weights(&sigma, &symbol_vecs, 1.0, 0.1);
        assert!(weights[1] > weights[0], "dim 1 (low mean/high disc) should outweigh dim 0 (high mean/low disc)");
    }

    #[test]
    fn test_alpha_zero_isotropic() {
        let sigma = vec![10.0, 1.0];
        let symbol_vecs = vec![
            vec![1.0, 0.0],
            vec![-1.0, 1.0],
        ];
        let weights = compute_anisotropy_weights(&sigma, &symbol_vecs, 0.0, 0.1);
        let first = weights[0];
        for w in &weights {
            assert!(approx_eq(*w, first), "alpha=0 should give uniform weights");
        }
    }

    #[test]
    fn test_epsilon_floor() {
        let sigma = vec![1.0];
        let symbol_vecs = vec![vec![0.0], vec![0.0]];
        let weights = compute_anisotropy_weights(&sigma, &symbol_vecs, 1.0, 0.1);
        assert!(approx_eq(weights[0], 0.1), "zero discriminativity should floor at epsilon");
    }

    #[test]
    fn test_anisotropic_normalization_preserves_unit_norm() {
        let weights = vec![2.0, 0.5, 1.0];
        let mut vecs = vec![
            vec![1.0, 0.0, 0.0],
            vec![0.0, 1.0, 0.0],
            vec![0.5, 0.5, 0.5],
        ];
        normalize_anisotropic(&mut vecs, &weights);
        for v in &vecs {
            let norm: f64 = v.iter().map(|x| x * x).sum::<f64>().sqrt();
            assert!(approx_eq(norm, 1.0), "all vectors should be unit norm after anisotropic normalization");
        }
    }

    #[test]
    fn test_anisotropic_stretches_high_weight_dims() {
        let weights = vec![10.0, 0.1];
        let mut vecs = vec![vec![1.0, 1.0]];
        normalize_anisotropic(&mut vecs, &weights);
        let v = &vecs[0];
        assert!(v[0].abs() > v[1].abs(), "high weight on dim 0 amplifies its contribution to unit sphere");
    }
}
