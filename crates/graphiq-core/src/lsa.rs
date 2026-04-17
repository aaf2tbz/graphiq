use std::collections::HashMap;

use rusqlite::params;

use crate::db::GraphDb;
use crate::tokenize::decompose_identifier;

const LSA_DIM: usize = 128;
const POWER_ITERS: usize = 3;
const OVERSAMPLING: usize = 16;

pub struct LsaIndex {
    pub term_basis: Vec<Vec<f64>>,
    pub term_index: HashMap<String, usize>,
    pub symbol_ids: Vec<i64>,
    pub symbol_vecs: Vec<Vec<f64>>,
    pub singular_values: Vec<f64>,
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

fn extract_terms(text: &str) -> Vec<String> {
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

pub fn build_tfidf_matrix(db: &GraphDb) -> (SparseMat, HashMap<String, usize>, Vec<i64>) {
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
    let mut symbol_terms: Vec<Vec<(usize, f64)>> = Vec::with_capacity(n_symbols);

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

    let n_docs_f = n_symbols as f64;
    let idf: HashMap<String, f64> = term_counts
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

    for (col, tf) in term_freqs.iter().enumerate() {
        for (t, tf_val) in tf {
            if let Some(&row) = term_index.get(t) {
                let idf_val = idf[t];
                col_data[col].push((row, tf_val * idf_val));
            }
        }
    }

    let symbol_ids: Vec<i64> = rows.iter().map(|(id, _, _, _, _, _)| *id).collect();

    (
        SparseMat {
            rows: n_terms,
            cols: n_symbols,
            col_data,
        },
        term_index,
        symbol_ids,
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

pub fn angular_distance(a: &[f64], b: &[f64]) -> f64 {
    let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let dot = dot.clamp(-1.0, 1.0);
    dot.acos()
}

pub fn project_query(
    query: &str,
    term_index: &HashMap<String, usize>,
    term_basis: &[Vec<f64>],
    n_terms: usize,
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

    let norm: f64 = query_vec.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm > 1e-10 {
        for x in query_vec.iter_mut() {
            *x /= norm;
        }
    }

    query_vec
}

pub fn compute_lsa(db: &GraphDb) -> Result<LsaIndex, String> {
    eprintln!("  building TF-IDF matrix...");
    let (matrix, term_index, symbol_ids) = build_tfidf_matrix(db);
    eprintln!("  matrix: {} terms × {} symbols", matrix.rows, matrix.cols);

    eprintln!("  computing randomized SVD (k={})...", LSA_DIM);
    let (term_basis, singular_values, mut symbol_vecs) = randomized_svd(&matrix, LSA_DIM);

    eprintln!("  normalizing to unit hypersphere...");
    normalize_to_sphere(&mut symbol_vecs);
    let mut term_basis_normed = term_basis;
    normalize_to_sphere(&mut term_basis_normed);

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
) -> Result<(), String> {
    let conn = db.conn();
    conn.execute(
        "CREATE TABLE IF NOT EXISTS lsa_basis (
            id INTEGER PRIMARY KEY,
            term TEXT NOT NULL,
            term_idx INTEGER NOT NULL,
            basis_vec BLOB NOT NULL,
            dim INTEGER NOT NULL
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
            .prepare("INSERT INTO lsa_basis (id, term, term_idx, basis_vec, dim) VALUES (?1, ?2, ?3, ?4, ?5)")
            .map_err(|e| e.to_string())?;

        for (term, &idx) in term_index {
            if idx >= term_basis.len() {
                continue;
            }
            let bytes: Vec<u8> = term_basis[idx]
                .iter()
                .flat_map(|f| f.to_le_bytes())
                .collect();
            stmt.execute(params![idx as i64, term, idx as i64, bytes, dim as i64])
                .map_err(|e| e.to_string())?;
        }
    }
    tx.commit().map_err(|e| e.to_string())?;

    Ok(())
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
        if dim == 0 || bytes.len() != dim * 4 {
            continue;
        }
        let vec: Vec<f64> = bytes
            .chunks_exact(4)
            .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as f64)
            .collect();
        result.insert(sym_id, vec);
    }

    Ok(result)
}

pub fn load_lsa_basis(db: &GraphDb) -> Result<(Vec<Vec<f64>>, HashMap<String, usize>), String> {
    let conn = db.conn();
    let mut stmt = conn
        .prepare("SELECT term, term_idx, basis_vec, dim FROM lsa_basis ORDER BY term_idx")
        .map_err(|e| e.to_string())?;

    let rows: Vec<(String, usize, Vec<u8>, usize)> = stmt
        .query_map([], |row| {
            let term: String = row.get(0)?;
            let idx: i64 = row.get(1)?;
            let bytes: Vec<u8> = row.get(2).unwrap_or_default();
            let dim: i64 = row.get(3).unwrap_or(0);
            Ok((term, idx as usize, bytes, dim as usize))
        })
        .map_err(|e| e.to_string())?
        .flatten()
        .collect();

    if rows.is_empty() {
        return Ok((Vec::new(), HashMap::new()));
    }

    let dim = rows[0].3;
    let max_idx = rows.iter().map(|(_, idx, _, _)| *idx).max().unwrap_or(0);
    let mut basis = vec![vec![0.0f64; dim]; max_idx + 1];
    let mut term_index = HashMap::new();

    for (term, idx, bytes, d) in &rows {
        if *d != dim || bytes.len() != dim * 4 {
            continue;
        }
        let vec: Vec<f64> = bytes
            .chunks_exact(4)
            .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as f64)
            .collect();
        basis[*idx] = vec;
        term_index.insert(term.clone(), *idx);
    }

    Ok((basis, term_index))
}
