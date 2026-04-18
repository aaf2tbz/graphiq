use std::collections::{HashMap, HashSet};

use crate::db::GraphDb;
use crate::lsa::extract_terms;

const HRR_V2_DIM: usize = 4096;

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

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

fn normalize(v: &mut [f64]) {
    let norm: f64 = v.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm > 1e-10 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    Name,
    Kind,
    CallsOut,
    CallsIn,
    TypeRet,
    FilePath,
}

impl Role {
    fn seed(&self) -> u64 {
        hash_to_seed(match self {
            Role::Name => "role:name",
            Role::Kind => "role:kind",
            Role::CallsOut => "role:calls_out",
            Role::CallsIn => "role:calls_in",
            Role::TypeRet => "role:type_ret",
            Role::FilePath => "role:file_path",
        })
    }

    pub fn label(&self) -> &'static str {
        match self {
            Role::Name => "name",
            Role::Kind => "kind",
            Role::CallsOut => "calls_out",
            Role::CallsIn => "calls_in",
            Role::TypeRet => "type_ret",
            Role::FilePath => "file_path",
        }
    }
}

const ALL_ROLES: [Role; 6] = [
    Role::Name,
    Role::Kind,
    Role::CallsOut,
    Role::CallsIn,
    Role::TypeRet,
    Role::FilePath,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MotifV2 {
    Combiner,
    Facade,
    Adapter,
    Dispatcher,
    Fallback,
    Collector,
    Guard,
}

impl MotifV2 {
    fn seed(&self) -> u64 {
        hash_to_seed(match self {
            MotifV2::Combiner => "motif:combiner",
            MotifV2::Facade => "motif:facade",
            MotifV2::Adapter => "motif:adapter",
            MotifV2::Dispatcher => "motif:dispatcher",
            MotifV2::Fallback => "motif:fallback",
            MotifV2::Collector => "motif:collector",
            MotifV2::Guard => "motif:guard",
        })
    }

    pub fn label(&self) -> &'static str {
        match self {
            MotifV2::Combiner => "COMBINER",
            MotifV2::Facade => "FACADE",
            MotifV2::Adapter => "ADAPTER",
            MotifV2::Dispatcher => "DISPATCHER",
            MotifV2::Fallback => "FALLBACK",
            MotifV2::Collector => "COLLECTOR",
            MotifV2::Guard => "GUARD",
        }
    }

    fn trigger_terms(&self) -> &[&str] {
        match self {
            MotifV2::Combiner => &[
                "combine",
                "merge",
                "mix",
                "blend",
                "join",
                "unify",
                "integrate",
                "hybrid",
                "fuse",
            ],
            MotifV2::Facade => &["facade", "wrapper", "interface", "abstraction", "delegate"],
            MotifV2::Adapter => &[
                "adapt",
                "convert",
                "transform",
                "translate",
                "cast",
                "parse",
                "serialize",
            ],
            MotifV2::Dispatcher => &[
                "dispatch",
                "route",
                "router",
                "direct",
                "switch",
                "fan",
                "distribute",
            ],
            MotifV2::Fallback => &[
                "fallback",
                "backup",
                "recovery",
                "retry",
                "secondary",
                "failover",
                "degrade",
            ],
            MotifV2::Collector => &[
                "collect",
                "gather",
                "accumulate",
                "harvest",
                "batch",
                "pool",
            ],
            MotifV2::Guard => &[
                "guard",
                "validate",
                "verify",
                "protect",
                "authenticate",
                "authorize",
                "gate",
                "filter",
            ],
        }
    }
}

const ALL_MOTIFS: [MotifV2; 7] = [
    MotifV2::Combiner,
    MotifV2::Facade,
    MotifV2::Adapter,
    MotifV2::Dispatcher,
    MotifV2::Fallback,
    MotifV2::Collector,
    MotifV2::Guard,
];

fn make_permutation(seed: u64, dim: usize) -> Vec<usize> {
    let mut perm: Vec<usize> = (0..dim).collect();
    let mut state = seed;
    for i in (1..dim).rev() {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let j = (state as usize) % (i + 1);
        perm.swap(i, j);
    }
    perm
}

fn apply_perm(vec: &[f64], perm: &[usize]) -> Vec<f64> {
    let mut out = vec![0.0f64; vec.len()];
    for (i, &p) in perm.iter().enumerate() {
        out[i] = vec[p];
    }
    out
}

fn complex_mul_add(
    acc_re: &mut [f64],
    acc_im: &mut [f64],
    a_re: &[f64],
    a_im: &[f64],
    b_re: &[f64],
    b_im: &[f64],
    weight: f64,
) {
    for i in 0..acc_re.len() {
        acc_re[i] += weight * (a_re[i] * b_re[i] - a_im[i] * b_im[i]);
        acc_im[i] += weight * (a_re[i] * b_im[i] + a_im[i] * b_re[i]);
    }
}

fn build_channel(
    terms: &[(String, f64)],
    role_freq: &(Vec<f64>, Vec<f64>),
    role_perm: &[usize],
    term_vectors: &HashMap<String, Vec<f64>>,
    dim: usize,
) -> Vec<f64> {
    let mut ch_re = vec![0.0; dim];
    let mut ch_im = vec![0.0; dim];

    for (term, weight) in terms {
        if let Some(tv) = term_vectors.get(term) {
            let permuted = apply_perm(tv, role_perm);
            let mut f_re = permuted;
            let mut f_im = vec![0.0; dim];
            fft_inplace(&mut f_re, &mut f_im);
            complex_mul_add(
                &mut ch_re,
                &mut ch_im,
                &role_freq.0,
                &role_freq.1,
                &f_re,
                &f_im,
                *weight,
            );
        }
    }

    ifft_inplace(&mut ch_re, &mut ch_im);
    normalize(&mut ch_re);
    ch_re
}

#[derive(Clone)]
pub struct ChannelHolograms {
    pub name: Vec<f64>,
    pub calls_out: Vec<f64>,
    pub calls_in: Vec<f64>,
    pub type_ret: Vec<f64>,
    pub file_path: Vec<f64>,
    pub motif: Vec<f64>,
}

impl ChannelHolograms {
    #[allow(dead_code)]
    fn zero(dim: usize) -> Self {
        ChannelHolograms {
            name: vec![0.0; dim],
            calls_out: vec![0.0; dim],
            calls_in: vec![0.0; dim],
            type_ret: vec![0.0; dim],
            file_path: vec![0.0; dim],
            motif: vec![0.0; dim],
        }
    }

    pub fn get(&self, role: Role) -> &[f64] {
        match role {
            Role::Name | Role::Kind => &self.name,
            Role::CallsOut => &self.calls_out,
            Role::CallsIn => &self.calls_in,
            Role::TypeRet => &self.type_ret,
            Role::FilePath => &self.file_path,
        }
    }

    #[allow(dead_code)]
    fn get_mut(&mut self, role: Role) -> &mut Vec<f64> {
        match role {
            Role::Name | Role::Kind => &mut self.name,
            Role::CallsOut => &mut self.calls_out,
            Role::CallsIn => &mut self.calls_in,
            Role::TypeRet => &mut self.type_ret,
            Role::FilePath => &mut self.file_path,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ChannelScores {
    pub name: f64,
    pub calls_out: f64,
    pub calls_in: f64,
    pub type_ret: f64,
    pub file_path: f64,
    pub motif: f64,
}

impl ChannelScores {
    fn zero() -> Self {
        ChannelScores {
            name: 0.0,
            calls_out: 0.0,
            calls_in: 0.0,
            type_ret: 0.0,
            file_path: 0.0,
            motif: 0.0,
        }
    }
}

fn extract_sig_types(sig: Option<&str>) -> Vec<String> {
    let sig = match sig {
        Some(s) if !s.is_empty() => s,
        _ => return Vec::new(),
    };

    let mut terms = Vec::new();

    let after_arrow = if let Some(pos) = sig.find("->") {
        &sig[pos + 2..]
    } else if let Some(pos) = sig.find("→") {
        &sig[pos + "→".len()..]
    } else {
        ""
    };

    for chunk in after_arrow.split(|c: char| !c.is_alphanumeric() && c != '_') {
        let c = chunk.trim();
        if c.len() >= 2 {
            terms.push(c.to_lowercase());
        }
    }

    for chunk in sig.split(|c: char| !c.is_alphanumeric() && c != '_') {
        let c = chunk.trim().to_lowercase();
        if c.len() >= 3
            && ![
                "fn", "func", "function", "let", "mut", "pub", "self", "const", "static", "the",
                "and", "for", "return", "async", "await", "where",
            ]
            .contains(&c.as_str())
        {
            if !terms.contains(&c) {
                terms.push(c);
            }
        }
    }

    terms
}

fn extract_path_terms(path: &str) -> Vec<String> {
    let skip = [
        "src",
        "lib",
        "pkg",
        "mod",
        "index",
        "main",
        "test",
        "tests",
        "spec",
        "bench",
        "internal",
        "internal",
        "target",
        "dist",
        "build",
        "out",
        "node_modules",
    ];

    let mut terms = Vec::new();
    for segment in path.split(|c: char| c == '/' || c == '\\') {
        let name = segment.rsplit('.').last().unwrap_or(segment);
        for t in extract_terms(name) {
            if !skip.contains(&t.as_str()) && !terms.contains(&t) {
                terms.push(t);
            }
        }
    }
    terms
}

fn detect_symbol_motifs(
    symbol_name: &str,
    kind: &str,
    sig: Option<&str>,
    outgoing_names: &[String],
    _incoming_names: &[String],
    out_degree: usize,
    in_degree: usize,
) -> HashMap<MotifV2, f64> {
    let mut activations = HashMap::new();

    let all_out_terms: HashSet<String> = outgoing_names
        .iter()
        .flat_map(|n| extract_terms(n))
        .collect();
    let all_name_terms: HashSet<String> = extract_terms(symbol_name).into_iter().collect();

    if out_degree >= 2 {
        let has_result = all_out_terms.iter().any(|t| {
            [
                "result",
                "results",
                "collection",
                "vec",
                "list",
                "map",
                "array",
                "output",
                "response",
                "item",
            ]
            .contains(&t.as_str())
        }) || extract_sig_types(sig).iter().any(|t| {
            [
                "result",
                "results",
                "vec",
                "list",
                "map",
                "array",
                "collection",
            ]
            .contains(&t.as_str())
        });

        let strength = if has_result {
            (out_degree as f64 / 4.0).min(1.0) * 0.9
        } else if out_degree >= 3 {
            0.5
        } else {
            0.0
        };
        if strength > 0.0 {
            activations.insert(MotifV2::Combiner, strength);
        }
    }

    if in_degree >= 2 && out_degree >= 1 {
        let strength = ((in_degree as f64).ln_1p() / 3.0).min(1.0) * 0.7;
        activations.insert(MotifV2::Facade, strength);
    }

    {
        let adapter_terms = [
            "adapt",
            "convert",
            "transform",
            "translate",
            "cast",
            "parse",
            "serialize",
            "deserialize",
            "encode",
            "decode",
            "from",
            "into",
        ];
        let has_adapter = all_name_terms
            .iter()
            .any(|t| adapter_terms.iter().any(|at| t == at || t.contains(at)))
            || all_out_terms
                .iter()
                .any(|t| adapter_terms.iter().any(|at| t == at || t.contains(at)));
        if has_adapter {
            activations.insert(MotifV2::Adapter, 0.7);
        }
    }

    if out_degree >= 3 && in_degree >= 1 {
        let strength = (out_degree as f64 / 6.0).min(1.0) * 0.8;
        activations.insert(MotifV2::Dispatcher, strength);
    }

    {
        let fallback_terms = [
            "fallback",
            "backup",
            "retry",
            "secondary",
            "default",
            "recover",
            "error",
            "fail",
            "degrade",
            "catch",
        ];
        let has_fallback = all_name_terms
            .iter()
            .any(|t| fallback_terms.iter().any(|ft| t.contains(ft)))
            || all_out_terms
                .iter()
                .any(|t| fallback_terms.iter().any(|ft| t.contains(ft)));
        if has_fallback && out_degree >= 2 {
            activations.insert(MotifV2::Fallback, 0.8);
        }
    }

    if out_degree >= 4 && in_degree <= 2 {
        let strength = (out_degree as f64 / 8.0).min(1.0) * 0.7;
        activations.insert(MotifV2::Collector, strength);
    }

    {
        let guard_terms = [
            "check", "validate", "verify", "guard", "protect", "auth", "permit", "gate", "filter",
            "ensure", "assert", "require",
        ];
        let has_guard = all_name_terms
            .iter()
            .any(|t| guard_terms.iter().any(|gt| t.contains(gt)))
            || all_out_terms
                .iter()
                .any(|t| guard_terms.iter().any(|gt| t.contains(gt)));
        if has_guard {
            activations.insert(MotifV2::Guard, 0.7);
        }
    }

    if kind == "function" || kind == "method" {
        activations.retain(|_, &mut v| v > 0.0);
    }

    activations
}

#[derive(Clone)]
pub struct HrrV2Index {
    pub symbol_ids: Vec<i64>,
    pub symbol_names: Vec<String>,
    pub channels: Vec<ChannelHolograms>,
    pub term_vectors: HashMap<String, Vec<f64>>,
    pub term_idf: HashMap<String, f64>,
    pub role_freq: HashMap<Role, (Vec<f64>, Vec<f64>)>,
    pub role_perms: HashMap<Role, Vec<usize>>,
    pub motif_vecs: HashMap<MotifV2, Vec<f64>>,
    pub channel_means: ChannelHolograms,
    pub dim: usize,
    pub id_to_idx: HashMap<i64, usize>,
    pub motif_activations: Vec<HashMap<MotifV2, f64>>,
}

pub fn build_hrr_v2(db: &GraphDb) -> Result<HrrV2Index, String> {
    let conn = db.conn();
    let dim = HRR_V2_DIM;

    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.name, s.kind, s.signature, f.path, s.language \
             FROM symbols s LEFT JOIN files f ON s.file_id = f.id \
             ORDER BY s.id",
        )
        .map_err(|e| e.to_string())?;
    let symbols: Vec<(i64, String, String, Option<String>, Option<String>, String)> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .flatten()
        .collect();

    let n = symbols.len();
    eprintln!("  [hrr_v2] {} symbols, dim={}", n, dim);

    let symbol_ids: Vec<i64> = symbols.iter().map(|(id, _, _, _, _, _)| *id).collect();
    let symbol_names: Vec<String> = symbols
        .iter()
        .map(|(_, name, _, _, _, _)| name.clone())
        .collect();
    let id_to_idx: HashMap<i64, usize> = symbol_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i))
        .collect();

    let mut all_terms: HashSet<String> = HashSet::new();
    let mut term_doc_count: HashMap<String, usize> = HashMap::new();

    for (_, name, kind, sig, path, _) in &symbols {
        let mut terms_for_symbol = HashSet::new();
        for t in extract_terms(name) {
            all_terms.insert(t.clone());
            terms_for_symbol.insert(t);
        }
        for t in extract_terms(kind) {
            if t.len() >= 2 {
                all_terms.insert(t.clone());
                terms_for_symbol.insert(t);
            }
        }
        for t in extract_sig_types(sig.as_deref()) {
            all_terms.insert(t.clone());
            terms_for_symbol.insert(t);
        }
        if let Some(p) = path {
            for t in extract_path_terms(p) {
                all_terms.insert(t.clone());
                terms_for_symbol.insert(t);
            }
        }
        for t in &terms_for_symbol {
            *term_doc_count.entry(t.clone()).or_insert(0) += 1;
        }
    }

    let mut term_vectors: HashMap<String, Vec<f64>> = HashMap::new();
    for term in &all_terms {
        let seed = hash_to_seed(&format!("hrrv2:term:{}", term));
        term_vectors.insert(term.clone(), random_unit_vec(dim, seed));
    }

    let term_idf: HashMap<String, f64> = all_terms
        .iter()
        .map(|t| {
            let df = *term_doc_count.get(t).unwrap_or(&1) as f64;
            let idf = (1.0 + (n as f64 / df).ln()).max(0.1);
            (t.clone(), idf)
        })
        .collect();

    eprintln!("  [hrr_v2] {} unique terms, IDF computed", all_terms.len());

    let mut role_freq: HashMap<Role, (Vec<f64>, Vec<f64>)> = HashMap::new();
    let mut role_perms: HashMap<Role, Vec<usize>> = HashMap::new();
    for role in ALL_ROLES {
        let rv = random_unit_vec(dim, role.seed());
        let mut r_re = rv;
        let mut r_im = vec![0.0; dim];
        fft_inplace(&mut r_re, &mut r_im);
        role_freq.insert(role, (r_re, r_im));
        role_perms.insert(
            role,
            make_permutation(role.seed().wrapping_add(0xDEADBEEF), dim),
        );
    }

    let mut motif_vecs: HashMap<MotifV2, Vec<f64>> = HashMap::new();
    for motif in ALL_MOTIFS {
        motif_vecs.insert(motif, random_unit_vec(dim, motif.seed()));
    }

    let mut outgoing: Vec<Vec<(usize, String, f64)>> = vec![Vec::new(); n];
    let mut incoming: Vec<Vec<(usize, String, f64)>> = vec![Vec::new(); n];

    if let Ok(mut estmt) = conn.prepare("SELECT source_id, target_id, kind FROM edges") {
        let edges: Vec<(i64, i64, String)> = estmt
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get::<_, String>(2)?))
            })
            .map_err(|e| e.to_string())?
            .flatten()
            .collect();

        let edge_weight = |kind: &str| -> f64 {
            match kind {
                "Calls" => 1.0,
                "References" => 0.5,
                "Contains" => 0.3,
                "Imports" => 0.6,
                "Extends" => 0.7,
                "Implements" => 0.7,
                _ => 0.3,
            }
        };

        for (src, tgt, kind) in &edges {
            if let (Some(&si), Some(&ti)) = (id_to_idx.get(src), id_to_idx.get(tgt)) {
                let w = edge_weight(kind);
                outgoing[si].push((ti, symbol_names[ti].clone(), w));
                incoming[ti].push((si, symbol_names[si].clone(), w));
            }
        }
    }

    let n_edges: usize = outgoing.iter().map(|e| e.len()).sum();
    eprintln!("  [hrr_v2] {} edges loaded", n_edges);

    let mut channels: Vec<ChannelHolograms> = Vec::with_capacity(n);
    let mut motif_activations: Vec<HashMap<MotifV2, f64>> = Vec::with_capacity(n);

    for i in 0..n {
        let (_, name, kind, sig, path, _) = &symbols[i];
        let out_degree = outgoing[i].len();
        let in_degree = incoming[i].len();

        let out_hubness = if out_degree > 0 {
            1.0 / (out_degree as f64).sqrt()
        } else {
            1.0
        };
        let in_hubness = if in_degree > 0 {
            1.0 / (in_degree as f64).sqrt()
        } else {
            1.0
        };

        let name_terms: Vec<(String, f64)> = extract_terms(name)
            .into_iter()
            .filter_map(|t| {
                let idf = term_idf.get(&t).copied().unwrap_or(0.1);
                Some((t, idf))
            })
            .collect();

        let kind_terms: Vec<(String, f64)> = extract_terms(kind)
            .into_iter()
            .filter(|t| t.len() >= 2)
            .filter_map(|t| {
                let idf = term_idf.get(&t).copied().unwrap_or(0.1);
                Some((t, idf * 0.5))
            })
            .collect();

        let mut all_name_terms = name_terms;
        all_name_terms.extend(kind_terms);

        let calls_out_terms: Vec<(String, f64)> = outgoing[i]
            .iter()
            .flat_map(|(ni, nname, w)| {
                let ni = *ni;
                let w = *w;
                extract_terms(nname).into_iter().map(move |t| (ni, t, w))
            })
            .map(|(_, t, w)| {
                let idf = term_idf.get(&t).copied().unwrap_or(0.1);
                (t, idf * w * out_hubness)
            })
            .collect();

        let calls_in_terms: Vec<(String, f64)> = incoming[i]
            .iter()
            .flat_map(|(ni, nname, w)| {
                let ni = *ni;
                let w = *w;
                extract_terms(nname).into_iter().map(move |t| (ni, t, w))
            })
            .map(|(_, t, w)| {
                let idf = term_idf.get(&t).copied().unwrap_or(0.1);
                (t, idf * w * in_hubness)
            })
            .collect();

        let type_terms: Vec<(String, f64)> = extract_sig_types(sig.as_deref())
            .into_iter()
            .filter_map(|t| {
                let idf = term_idf.get(&t).copied().unwrap_or(0.1);
                Some((t, idf))
            })
            .collect();

        let path_terms: Vec<(String, f64)> = path
            .as_ref()
            .map(|p| {
                extract_path_terms(p)
                    .into_iter()
                    .filter_map(|t| {
                        let idf = term_idf.get(&t).copied().unwrap_or(0.1);
                        Some((t, idf * 0.5))
                    })
                    .collect()
            })
            .unwrap_or_default();

        let out_names: Vec<String> = outgoing[i].iter().map(|(_, n, _)| n.clone()).collect();
        let in_names: Vec<String> = incoming[i].iter().map(|(_, n, _)| n.clone()).collect();
        let motifs = detect_symbol_motifs(
            name,
            kind,
            sig.as_deref(),
            &out_names,
            &in_names,
            out_degree,
            in_degree,
        );
        motif_activations.push(motifs.clone());

        let motif_terms: Vec<(String, f64)> = motifs
            .iter()
            .flat_map(|(motif, &strength)| {
                motif
                    .trigger_terms()
                    .iter()
                    .map(move |&t| (t.to_string(), strength))
            })
            .map(|(t, strength)| {
                let idf = term_idf.get(&t).copied().unwrap_or(0.1);
                (t, idf * strength)
            })
            .collect();

        let name_ch = build_channel(
            &all_name_terms,
            role_freq.get(&Role::Name).unwrap(),
            role_perms.get(&Role::Name).unwrap(),
            &term_vectors,
            dim,
        );
        let calls_out_ch = build_channel(
            &calls_out_terms,
            role_freq.get(&Role::CallsOut).unwrap(),
            role_perms.get(&Role::CallsOut).unwrap(),
            &term_vectors,
            dim,
        );
        let calls_in_ch = build_channel(
            &calls_in_terms,
            role_freq.get(&Role::CallsIn).unwrap(),
            role_perms.get(&Role::CallsIn).unwrap(),
            &term_vectors,
            dim,
        );
        let type_ch = build_channel(
            &type_terms,
            role_freq.get(&Role::TypeRet).unwrap(),
            role_perms.get(&Role::TypeRet).unwrap(),
            &term_vectors,
            dim,
        );
        let path_ch = build_channel(
            &path_terms,
            role_freq.get(&Role::FilePath).unwrap(),
            role_perms.get(&Role::FilePath).unwrap(),
            &term_vectors,
            dim,
        );
        let motif_ch = build_channel(
            &motif_terms,
            role_freq.get(&Role::Name).unwrap(),
            role_perms.get(&Role::Name).unwrap(),
            &term_vectors,
            dim,
        );

        channels.push(ChannelHolograms {
            name: name_ch,
            calls_out: calls_out_ch,
            calls_in: calls_in_ch,
            type_ret: type_ch,
            file_path: path_ch,
            motif: motif_ch,
        });
    }

    eprintln!("  [hrr_v2] channel holograms built, computing means...");

    let mut mean_name = vec![0.0; dim];
    let mut mean_calls_out = vec![0.0; dim];
    let mut mean_calls_in = vec![0.0; dim];
    let mut mean_type_ret = vec![0.0; dim];
    let mut mean_file_path = vec![0.0; dim];
    let mut mean_motif = vec![0.0; dim];

    for ch in &channels {
        for j in 0..dim {
            mean_name[j] += ch.name[j];
            mean_calls_out[j] += ch.calls_out[j];
            mean_calls_in[j] += ch.calls_in[j];
            mean_type_ret[j] += ch.type_ret[j];
            mean_file_path[j] += ch.file_path[j];
            mean_motif[j] += ch.motif[j];
        }
    }

    let inv_n = 1.0 / n as f64;
    for v in [
        &mut mean_name,
        &mut mean_calls_out,
        &mut mean_calls_in,
        &mut mean_type_ret,
        &mut mean_file_path,
        &mut mean_motif,
    ] {
        for x in v.iter_mut() {
            *x *= inv_n;
        }
    }

    for ch in channels.iter_mut() {
        for j in 0..dim {
            ch.name[j] -= mean_name[j];
            ch.calls_out[j] -= mean_calls_out[j];
            ch.calls_in[j] -= mean_calls_in[j];
            ch.type_ret[j] -= mean_type_ret[j];
            ch.file_path[j] -= mean_file_path[j];
            ch.motif[j] -= mean_motif[j];
        }
        normalize(&mut ch.name);
        normalize(&mut ch.calls_out);
        normalize(&mut ch.calls_in);
        normalize(&mut ch.type_ret);
        normalize(&mut ch.file_path);
        normalize(&mut ch.motif);
    }

    let channel_means = ChannelHolograms {
        name: mean_name,
        calls_out: mean_calls_out,
        calls_in: mean_calls_in,
        type_ret: mean_type_ret,
        file_path: mean_file_path,
        motif: mean_motif,
    };

    eprintln!("  [hrr_v2] done: {} symbols, {} dim, centered", n, dim);

    Ok(HrrV2Index {
        symbol_ids,
        symbol_names,
        channels,
        term_vectors,
        term_idf,
        role_freq,
        role_perms,
        motif_vecs,
        channel_means,
        dim,
        id_to_idx,
        motif_activations,
    })
}

pub struct QueryPredicates {
    pub name_terms: Vec<String>,
    pub call_out_terms: Vec<String>,
    pub call_in_terms: Vec<String>,
    pub return_terms: Vec<String>,
    pub motif_hint: Option<MotifV2>,
    pub all_anchors: Vec<String>,
}

fn is_query_stop(t: &str) -> bool {
    matches!(
        t,
        "how"
            | "what"
            | "where"
            | "when"
            | "why"
            | "which"
            | "who"
            | "does"
            | "do"
            | "did"
            | "is"
            | "are"
            | "was"
            | "were"
            | "be"
            | "the"
            | "a"
            | "an"
            | "of"
            | "in"
            | "to"
            | "for"
            | "on"
            | "at"
            | "by"
            | "with"
            | "from"
            | "as"
            | "into"
            | "through"
            | "and"
            | "or"
            | "but"
            | "not"
            | "that"
            | "this"
            | "it"
            | "its"
            | "if"
            | "then"
            | "than"
            | "so"
            | "up"
            | "out"
            | "all"
            | "every"
            | "has"
            | "having"
            | "can"
            | "will"
            | "would"
            | "could"
            | "should"
            | "may"
            | "might"
            | "some"
            | "any"
            | "much"
            | "many"
            | "way"
            | "thing"
            | "work"
            | "make"
            | "get"
            | "find"
            | "see"
            | "know"
            | "tell"
            | "look"
            | "use"
            | "used"
            | "call"
            | "called"
    )
}

pub fn parse_query_predicates(query: &str) -> QueryPredicates {
    let lowered = query.to_lowercase();
    let words: Vec<&str> = lowered.split_whitespace().collect();

    let mut motif_hint: Option<MotifV2> = None;
    let mut motif_trigger_set: HashSet<String> = HashSet::new();

    for motif in ALL_MOTIFS {
        for trigger in motif.trigger_terms() {
            if words.iter().any(|w| w == trigger || w.starts_with(trigger)) {
                motif_hint = Some(motif);
                motif_trigger_set.insert(trigger.to_string());
                break;
            }
        }
        if motif_hint.is_some() {
            break;
        }
    }

    let type_suffixes = [
        "result", "error", "option", "config", "builder", "handler", "manager", "provider",
        "service", "factory", "response", "request", "context", "info", "data", "state",
        "settings", "params", "options",
    ];

    let mut name_terms = Vec::new();
    let mut return_terms = Vec::new();

    for w in &words {
        let w = w.trim();
        if w.len() < 2 || is_query_stop(w) {
            continue;
        }
        if motif_trigger_set.contains(w) {
            continue;
        }
        if type_suffixes.iter().any(|s| w.ends_with(s) || w == *s) {
            return_terms.push(w.to_string());
        }
        name_terms.push(w.to_string());
    }

    let all_anchors = name_terms.clone();

    QueryPredicates {
        name_terms,
        call_out_terms: all_anchors.clone(),
        call_in_terms: all_anchors.clone(),
        return_terms,
        motif_hint,
        all_anchors,
    }
}

fn build_query_channel(terms: &[String], role: Role, index: &HrrV2Index) -> Option<Vec<f64>> {
    if terms.is_empty() {
        return None;
    }

    let weighted: Vec<(String, f64)> = terms
        .iter()
        .filter_map(|t| {
            if index.term_vectors.contains_key(t) {
                let idf = index.term_idf.get(t).copied().unwrap_or(0.1);
                Some((t.clone(), idf))
            } else {
                None
            }
        })
        .collect();

    if weighted.is_empty() {
        return None;
    }

    let mut ch = build_channel(
        &weighted,
        index.role_freq.get(&role).unwrap(),
        index.role_perms.get(&role).unwrap(),
        &index.term_vectors,
        index.dim,
    );

    let mean = index.channel_means.get(role);
    for j in 0..index.dim {
        ch[j] -= mean[j];
    }
    normalize(&mut ch);

    let norm: f64 = ch.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm < 1e-10 {
        None
    } else {
        Some(ch)
    }
}

fn build_motif_query_channel(motif: MotifV2, index: &HrrV2Index) -> Option<Vec<f64>> {
    let vec = index.motif_vecs.get(&motif)?;
    let mut ch = vec.clone();

    let mean = &index.channel_means.motif;
    for j in 0..index.dim {
        ch[j] -= mean[j];
    }
    normalize(&mut ch);

    let norm: f64 = ch.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm < 1e-10 {
        None
    } else {
        Some(ch)
    }
}

fn geometric_mean(scores: &[f64]) -> f64 {
    if scores.is_empty() {
        return 0.0;
    }
    let log_sum: f64 = scores
        .iter()
        .map(|&s| if s > 1e-10 { s.ln() } else { -23.0 })
        .sum();
    (log_sum / scores.len() as f64).exp()
}

pub fn hrr_v2_rerank(
    query: &str,
    candidate_ids: &[i64],
    candidate_scores: &[f64],
    index: &HrrV2Index,
    top_k: usize,
) -> Vec<(i64, f64)> {
    let preds = parse_query_predicates(query);

    let q_name = build_query_channel(&preds.name_terms, Role::Name, index);
    let q_calls_out = build_query_channel(&preds.call_out_terms, Role::CallsOut, index);
    let q_calls_in = build_query_channel(&preds.call_in_terms, Role::CallsIn, index);
    let q_type = if !preds.return_terms.is_empty() {
        build_query_channel(&preds.return_terms, Role::TypeRet, index)
    } else {
        None
    };
    let q_motif = preds
        .motif_hint
        .and_then(|m| build_motif_query_channel(m, index));

    let active_channels: Vec<(f64, Option<&Vec<f64>>)> = vec![
        (1.0, q_name.as_ref()),
        (0.8, q_calls_out.as_ref()),
        (0.6, q_calls_in.as_ref()),
        (0.7, q_type.as_ref()),
        (0.9, q_motif.as_ref()),
    ];

    let has_any = active_channels.iter().any(|(_, q)| q.is_some());
    if !has_any {
        let mut result: Vec<(i64, f64)> = candidate_ids
            .iter()
            .zip(candidate_scores.iter())
            .map(|(&id, &s)| (id, s))
            .filter(|(_, s)| *s > 0.0)
            .collect();
        result.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        result.truncate(top_k);
        return result;
    }

    let mut scored: Vec<(i64, f64)> = candidate_ids
        .iter()
        .zip(candidate_scores.iter())
        .map(|(&id, &base)| {
            if base <= 0.0 {
                return (id, 0.0);
            }

            let idx = match index.id_to_idx.get(&id) {
                Some(&i) => i,
                None => return (id, base),
            };

            let ch = &index.channels[idx];
            let mut channel_scores: Vec<f64> = Vec::new();

            for (weight, query_opt) in &active_channels {
                if let Some(query_vec) = query_opt {
                    let role = match weight {
                        1.0 => Role::Name,
                        0.8 => Role::CallsOut,
                        0.6 => Role::CallsIn,
                        0.7 => Role::TypeRet,
                        0.9 => Role::Name, // motif uses name channel
                        _ => Role::Name,
                    };
                    let sym_vec = if *weight == 0.9 {
                        &ch.motif
                    } else {
                        ch.get(role)
                    };
                    let sim = dot(query_vec, sym_vec).max(0.0);
                    if sim > 0.01 {
                        channel_scores.push(sim * weight);
                    }
                }
            }

            if channel_scores.is_empty() {
                return (id, base);
            }

            let hrr_score = geometric_mean(&channel_scores);
            let boost = 1.0 + 0.60 * hrr_score;
            (id, base * boost)
        })
        .filter(|(_, s)| *s > 0.0)
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored.truncate(top_k);
    scored
}

pub fn hrr_v2_rerank_debug(
    query: &str,
    candidate_ids: &[i64],
    candidate_scores: &[f64],
    index: &HrrV2Index,
    top_k: usize,
) -> Vec<(i64, f64, ChannelScores)> {
    let preds = parse_query_predicates(query);

    let q_name = build_query_channel(&preds.name_terms, Role::Name, index);
    let q_calls_out = build_query_channel(&preds.call_out_terms, Role::CallsOut, index);
    let q_calls_in = build_query_channel(&preds.call_in_terms, Role::CallsIn, index);
    let q_type = if !preds.return_terms.is_empty() {
        build_query_channel(&preds.return_terms, Role::TypeRet, index)
    } else {
        None
    };
    let q_motif = preds
        .motif_hint
        .and_then(|m| build_motif_query_channel(m, index));

    let mut scored: Vec<(i64, f64, ChannelScores)> = candidate_ids
        .iter()
        .zip(candidate_scores.iter())
        .map(|(&id, &base)| {
            let mut cs = ChannelScores::zero();
            if base <= 0.0 {
                return (id, 0.0, cs);
            }

            let idx = match index.id_to_idx.get(&id) {
                Some(&i) => i,
                None => return (id, base, cs),
            };

            let ch = &index.channels[idx];
            let mut channel_scores: Vec<f64> = Vec::new();

            if let Some(ref qv) = q_name {
                let sim = dot(qv, &ch.name).max(0.0);
                cs.name = sim;
                if sim > 0.01 {
                    channel_scores.push(sim);
                }
            }
            if let Some(ref qv) = q_calls_out {
                let sim = dot(qv, &ch.calls_out).max(0.0);
                cs.calls_out = sim;
                if sim > 0.01 {
                    channel_scores.push(sim * 0.8);
                }
            }
            if let Some(ref qv) = q_calls_in {
                let sim = dot(qv, &ch.calls_in).max(0.0);
                cs.calls_in = sim;
                if sim > 0.01 {
                    channel_scores.push(sim * 0.6);
                }
            }
            if let Some(ref qv) = q_type {
                let sim = dot(qv, &ch.type_ret).max(0.0);
                cs.type_ret = sim;
                if sim > 0.01 {
                    channel_scores.push(sim * 0.7);
                }
            }
            if let Some(ref qv) = q_motif {
                let sim = dot(qv, &ch.motif).max(0.0);
                cs.motif = sim;
                if sim > 0.01 {
                    channel_scores.push(sim * 0.9);
                }
            }

            if channel_scores.is_empty() {
                return (id, base, cs);
            }

            let hrr_score = geometric_mean(&channel_scores);
            let boost = 1.0 + 0.60 * hrr_score;
            (id, base * boost, cs)
        })
        .filter(|(_, s, _)| *s > 0.0)
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored.truncate(top_k);
    scored
}

pub fn decode_match(symbol_id: i64, query: &str, index: &HrrV2Index) -> Option<Vec<(String, f64)>> {
    let idx = *index.id_to_idx.get(&symbol_id)?;
    let _ch = &index.channels[idx];
    let preds = parse_query_predicates(query);

    let q_name = build_query_channel(&preds.name_terms, Role::Name, index)?;

    let role_freq = index.role_freq.get(&Role::Name)?;
    let role_perm = index.role_perms.get(&Role::Name)?;

    let mut q_re = q_name.clone();
    let mut q_im = vec![0.0; index.dim];
    fft_inplace(&mut q_re, &mut q_im);
    let r_re = &role_freq.0;
    let r_im: Vec<f64> = role_freq.1.iter().map(|x| -x).collect();

    let mut unbound_re = vec![0.0; index.dim];
    let mut unbound_im = vec![0.0; index.dim];
    for j in 0..index.dim {
        unbound_re[j] = r_re[j] * q_re[j] - r_im[j] * q_im[j];
        unbound_im[j] = r_re[j] * q_im[j] + r_im[j] * q_re[j];
    }
    ifft_inplace(&mut unbound_re, &mut unbound_im);

    let mut unperm = vec![0.0; index.dim];
    for i in 0..index.dim {
        unperm[role_perm[i]] = unbound_re[i];
    }

    let mut matched: Vec<(String, f64)> = index
        .term_vectors
        .iter()
        .map(|(term, vec)| (term.clone(), dot(&unperm, vec)))
        .filter(|(_, s)| *s > 0.05)
        .collect();
    matched.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    matched.truncate(10);

    Some(matched)
}
