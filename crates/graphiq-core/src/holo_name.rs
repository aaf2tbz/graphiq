use std::collections::{HashMap, HashSet};

use crate::cruncher::{CruncherIndex, STOP};
use crate::db::GraphDb;
use crate::tokenize::decompose_identifier;

const HOLO_DIM: usize = 1024;

fn holo_hash_seed(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn holo_random_unit(seed: u64) -> Vec<f64> {
    let mut state = seed;
    let mut v = Vec::with_capacity(HOLO_DIM);
    for _ in 0..HOLO_DIM / 2 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let s = state as u32;
        let u1 = (1.0 - s as f64 / u32::MAX as f64).max(1e-10);
        let u2 = 2.0 * std::f64::consts::PI * (s.wrapping_add(1) as f64 / u32::MAX as f64);
        let r = (-2.0 * u1.ln()).sqrt();
        v.push(r * u2.cos());
        v.push(r * u2.sin());
    }
    let norm: f64 = v.iter().map(|x| x * x).sum::<f64>().sqrt().max(1e-10);
    v.iter_mut().for_each(|x| *x /= norm);
    v
}

fn holo_fft_inplace(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    if n <= 1 { return; }
    let mut j: usize = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 { j &= !bit; bit >>= 1; }
        j ^= bit;
        if i < j { re.swap(i, j); im.swap(i, j); }
    }
    let mut len = 2usize;
    while len <= n {
        let ang = -2.0 * std::f64::consts::PI / len as f64;
        let (wre, wim) = (ang.cos(), ang.sin());
        for i in (0..n).step_by(len) {
            let (mut cre, mut cim) = (1.0, 0.0);
            for jj in 0..len / 2 {
                let (are, aim) = (re[i + jj], im[i + jj]);
                let (bre, bim) = (re[i + jj + len / 2], im[i + jj + len / 2]);
                let (tre, tim) = (bre * cre - bim * cim, bre * cim + bim * cre);
                re[i + jj] = are + tre; im[i + jj] = aim + tim;
                re[i + jj + len / 2] = are - tre; im[i + jj + len / 2] = aim - tim;
                let nre = cre * wre - cim * wim;
                let nim = cre * wim + cim * wre;
                cre = nre; cim = nim;
            }
        }
        len *= 2;
    }
}

fn holo_ifft_inplace(re: &mut [f64], im: &mut [f64]) {
    for v in im.iter_mut() { *v = -*v; }
    holo_fft_inplace(re, im);
    let n = re.len() as f64;
    for v in re.iter_mut() { *v /= n; }
    for v in im.iter_mut() { *v = -*v / n; }
}

fn holo_to_freq(time: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let mut re = time.to_vec();
    let mut im = vec![0.0; time.len()];
    holo_fft_inplace(&mut re, &mut im);
    (re, im)
}

fn holo_from_freq(re: &[f64], im: &[f64]) -> Vec<f64> {
    let mut r = re.to_vec();
    let mut i = im.to_vec();
    holo_ifft_inplace(&mut r, &mut i);
    r
}

fn holo_normalize(v: &mut [f64]) {
    let norm: f64 = v.iter().map(|x| x * x).sum::<f64>().sqrt().max(1e-10);
    for x in v.iter_mut() { *x /= norm; }
}

fn holo_cosine(a: &[f64], b: &[f64]) -> f64 {
    let na: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt().max(1e-10);
    let nb: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt().max(1e-10);
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum::<f64>() / (na * nb)
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct HoloIndex {
    pub name_holos: Vec<Vec<f64>>,
    pub term_freq: HashMap<String, (Vec<f64>, Vec<f64>)>,
}

pub fn build_holo_index(_db: &GraphDb, idx: &CruncherIndex) -> HoloIndex {
    let mut all_terms: HashSet<String> = HashSet::new();
    for ts in &idx.term_sets {
        for t in ts.name_terms.iter() { all_terms.insert(t.clone()); }
        for t in ts.terms.keys() { all_terms.insert(t.clone()); }
    }
    for qt_term in idx.global_idf.keys() { all_terms.insert(qt_term.clone()); }

    let mut term_time: HashMap<String, Vec<f64>> = HashMap::new();
    let mut term_freq: HashMap<String, (Vec<f64>, Vec<f64>)> = HashMap::new();
    for t in &all_terms {
        let v = holo_random_unit(holo_hash_seed(t));
        term_freq.insert(t.clone(), holo_to_freq(&v));
        term_time.insert(t.clone(), v);
    }

    let mut name_holos: Vec<Vec<f64>> = Vec::with_capacity(idx.n);
    for i in 0..idx.n {
        let nt = &idx.term_sets[i].name_terms;
        if nt.is_empty() {
            name_holos.push(vec![0.0; HOLO_DIM]);
            continue;
        }
        let mut holo = vec![0.0; HOLO_DIM];
        for t in nt {
            if let Some(v) = term_time.get(t) {
                for j in 0..HOLO_DIM { holo[j] += v[j]; }
            }
        }
        holo_normalize(&mut holo);
        name_holos.push(holo);
    }

    HoloIndex { name_holos, term_freq }
}

pub fn holo_query_name_cosine(query: &str, hi: &HoloIndex, symbol_i: usize) -> f64 {
    let terms: Vec<String> = query.to_lowercase()
        .split_whitespace()
        .filter(|t| t.len() >= 2)
        .filter(|t| !STOP.contains(t))
        .map(|t| t.to_string())
        .collect();
    let decomp = decompose_identifier(query);
    let mut all_terms: Vec<String> = terms;
    for t in decomp.split_whitespace() {
        let t = t.to_lowercase();
        if t.len() >= 2 && !STOP.contains(&&*t) && !all_terms.contains(&t) {
            all_terms.push(t);
        }
    }
    if all_terms.is_empty() { return 0.0; }

    let mut q_holo = vec![0.0; HOLO_DIM];
    for t in &all_terms {
        if let Some((re, im)) = hi.term_freq.get(t) {
            let tv = holo_from_freq(re, im);
            for j in 0..HOLO_DIM { q_holo[j] += tv[j]; }
        } else {
            let tv = holo_random_unit(holo_hash_seed(t));
            for j in 0..HOLO_DIM { q_holo[j] += tv[j]; }
        }
    }
    holo_normalize(&mut q_holo);

    holo_cosine(&q_holo, &hi.name_holos[symbol_i])
}
