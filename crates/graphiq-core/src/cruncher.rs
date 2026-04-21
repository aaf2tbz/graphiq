use std::collections::{HashMap, HashSet};

use crate::db::GraphDb;
use crate::tokenize::decompose_identifier;
use crate::tokenize::extract_terms;

const TOP_K_TERMS: usize = 30;
pub const MAX_SEEDS: usize = 30;

pub const STOP: &[&str] = &[
    "the", "a", "an", "is", "are", "was", "were", "be", "been", "being", "have", "has", "had",
    "do", "does", "did", "will", "would", "could", "should", "may", "might", "can", "shall", "to",
    "of", "in", "for", "on", "with", "at", "by", "from", "as", "into", "through", "and", "or",
    "but", "not", "that", "this", "these", "those", "it", "its", "if", "then", "than", "so", "up",
    "out", "new", "all", "every", "how", "what", "where", "when", "why", "which", "who", "whom",
    "there", "here", "no", "nor", "just", "very", "also", "some", "any", "each", "both", "few",
    "more", "most", "other", "such", "only", "own", "same", "about",
];

const EDGE_WEIGHT_CALLS: f64 = 1.0;
const EDGE_WEIGHT_REFS: f64 = 0.8;
const EDGE_WEIGHT_IMPORTS: f64 = 0.6;
const EDGE_WEIGHT_CONTAINS: f64 = 0.7;
const EDGE_WEIGHT_EXTENDS: f64 = 0.9;
const EDGE_WEIGHT_IMPLEMENTS: f64 = 0.9;
const EDGE_WEIGHT_TESTS: f64 = 0.3;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Edge {
    pub target: usize,
    pub weight: f64,
    pub kind_weight: f64,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct CruncherIndex {
    pub n: usize,
    pub symbol_ids: Vec<i64>,
    pub symbol_names: Vec<String>,
    pub symbol_kinds: Vec<String>,
    pub symbol_file_ids: Vec<i64>,
    pub file_paths: HashMap<i64, String>,

    pub outgoing: Vec<Vec<Edge>>,
    pub incoming: Vec<Vec<Edge>>,

    pub term_sets: Vec<TermSet>,
    pub global_idf: HashMap<String, f64>,

    pub bridging: Vec<f64>,
    pub id_to_idx: HashMap<i64, usize>,
    pub name_to_indices: HashMap<String, Vec<usize>>,
    pub structural_degree: Vec<f64>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct TermSet {
    pub terms: HashMap<String, f64>,
    pub name_terms: HashSet<String>,
    pub sig_terms: HashSet<String>,
}

pub struct QueryTerm {
    pub text: String,
    pub variants: Vec<String>,
    pub idf: f64,
}

fn cr_tokenize(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let mut terms: Vec<String> = lower
        .split_whitespace()
        .filter(|t| t.len() >= 2)
        .filter(|t| !STOP.contains(t))
        .map(|t| t.to_string())
        .collect();

    let decomp = decompose_identifier(text);
    for t in decomp.split_whitespace() {
        let t = t.to_lowercase();
        if t.len() >= 2 && !STOP.contains(&t.as_str()) {
            terms.push(t);
        }
    }

    terms.sort_unstable();
    terms.dedup();
    terms
}

fn expand_variants(term: &str) -> Vec<String> {
    let mut variants = vec![term.to_string()];
    let w = term.to_lowercase();
    if w.ends_with("ies") && w.len() > 4 {
        variants.push(format!("{}y", &w[..w.len() - 3]));
    } else if w.ends_with("es") && w.len() > 3 {
        variants.push(format!("{}", &w[..w.len() - 2]));
    } else if w.ends_with("s") && w.len() > 3 {
        variants.push(format!("{}", &w[..w.len() - 1]));
    }
    if w.ends_with("ing") && w.len() > 5 {
        variants.push(format!("{}", &w[..w.len() - 3]));
        variants.push(format!("{}e", &w[..w.len() - 3]));
    }
    if w.ends_with("ed") && w.len() > 4 {
        variants.push(format!("{}", &w[..w.len() - 2]));
    }
    if w.ends_with("tion") {
        variants.push(format!("{}te", &w[..w.len() - 4]));
    }
    if w.ends_with("ment") && w.len() > 5 {
        variants.push(format!("{}", &w[..w.len() - 4]));
    }
    variants.sort_unstable();
    variants.dedup();
    variants
}

fn edge_kind_weight(kind: &str) -> f64 {
    match kind {
        "calls" => EDGE_WEIGHT_CALLS,
        "references" => EDGE_WEIGHT_REFS,
        "imports" => EDGE_WEIGHT_IMPORTS,
        "contains" => EDGE_WEIGHT_CONTAINS,
        "extends" => EDGE_WEIGHT_EXTENDS,
        "implements" => EDGE_WEIGHT_IMPLEMENTS,
        "tests" => EDGE_WEIGHT_TESTS,
        _ => 0.5,
    }
}

pub fn kind_boost(kind: &str) -> f64 {
    match kind {
        "function" | "method" | "constructor" => 1.3,
        "class" | "struct" | "interface" | "trait" => 1.2,
        "enum" | "type_alias" => 1.1,
        "constant" | "field" | "property" => 0.9,
        "module" | "section" => 0.7,
        _ => 1.0,
    }
}

pub fn test_penalty(file_paths: &HashMap<i64, String>, file_id: i64) -> f64 {
    let path_lower = file_paths
        .get(&file_id)
        .map(|p| p.to_lowercase())
        .unwrap_or_default();
    if path_lower.contains("/test")
        || path_lower.contains("_test.")
        || path_lower.contains("/benches/")
        || path_lower.contains("/spec/")
    {
        0.3
    } else {
        1.0
    }
}

pub fn build_cruncher_index(db: &GraphDb) -> Result<CruncherIndex, String> {
    let conn = db.conn();

    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.name, s.kind, s.signature, s.file_id, s.source, s.doc_comment, s.search_hints \
             FROM symbols s ORDER BY s.id",
        )
        .map_err(|e| e.to_string())?;
    let rows: Vec<(i64, String, String, Option<String>, i64, String, Option<String>, String)> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get(4)?,
                row.get::<_, String>(5).unwrap_or_default(),
                row.get::<_, Option<String>>(6)?,
                row.get::<_, String>(7).unwrap_or_default(),
            ))
        })
        .map_err(|e| e.to_string())?
        .flatten()
        .collect();

    let n = rows.len();
    let symbol_ids: Vec<i64> = rows.iter().map(|r| r.0).collect();
    let symbol_names: Vec<String> = rows.iter().map(|r| r.1.clone()).collect();
    let symbol_kinds: Vec<String> = rows.iter().map(|r| r.2.clone()).collect();
    let symbol_sigs: Vec<Option<String>> = rows.iter().map(|r| r.3.clone()).collect();
    let symbol_file_ids: Vec<i64> = rows.iter().map(|r| r.4).collect();
    let symbol_sources: Vec<String> = rows.iter().map(|r| r.5.clone()).collect();
    let symbol_docs: Vec<Option<String>> = rows.iter().map(|r| r.6.clone()).collect();
    let symbol_hints: Vec<String> = rows.iter().map(|r| r.7.clone()).collect();

    let id_to_idx: HashMap<i64, usize> = symbol_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i))
        .collect();

    let mut name_to_indices: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, name) in symbol_names.iter().enumerate() {
        name_to_indices
            .entry(name.to_lowercase())
            .or_default()
            .push(i);
    }

    let mut file_stmt = conn
        .prepare("SELECT id, path FROM files")
        .map_err(|e| e.to_string())?;
    let file_paths: HashMap<i64, String> = file_stmt
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))
        .map_err(|e| e.to_string())?
        .flatten()
        .collect();

    eprintln!("  Cruncher: building adjacency lists...");
    let mut outgoing: Vec<Vec<Edge>> = vec![Vec::new(); n];
    let mut incoming: Vec<Vec<Edge>> = vec![Vec::new(); n];

    let mut edge_stmt = conn
        .prepare("SELECT source_id, target_id, kind FROM edges")
        .map_err(|e| e.to_string())?;
    let edges: Vec<(i64, i64, String)> = edge_stmt
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get::<_, String>(2)?))
        })
        .map_err(|e| e.to_string())?
        .flatten()
        .collect();

    for (src, tgt, kind) in &edges {
        if let (Some(&si), Some(&ti)) = (id_to_idx.get(src), id_to_idx.get(tgt)) {
            let kw = edge_kind_weight(kind);
            outgoing[si].push(Edge {
                target: ti,
                weight: kw,
                kind_weight: kw,
            });
            incoming[ti].push(Edge {
                target: si,
                weight: kw,
                kind_weight: kw,
            });
        }
    }

    for adj in &mut outgoing {
        adj.sort_by(|a, b| a.target.cmp(&b.target));
        adj.dedup_by(|a, b| a.target == b.target);
    }
    for adj in &mut incoming {
        adj.sort_by(|a, b| a.target.cmp(&b.target));
        adj.dedup_by(|a, b| b.target == a.target);
    }

    eprintln!("  Cruncher: building per-symbol term sets...");
    let term_sets: Vec<TermSet> = (0..n)
        .map(|i| {
            let name_terms = cr_tokenize(&symbol_names[i]);
            let name_set: HashSet<String> = name_terms.iter().cloned().collect();

            let sig_text = symbol_sigs[i].as_deref().unwrap_or("");
            let sig_terms_vec = cr_tokenize(sig_text);
            let sig_set: HashSet<String> = sig_terms_vec.iter().cloned().collect();

            let hint_terms = cr_tokenize(&symbol_hints[i]);
            let doc_terms = match &symbol_docs[i] {
                Some(doc) => cr_tokenize(doc),
                None => Vec::new(),
            };

            let src = &symbol_sources[i];
            let src_terms = if src.len() > 4000 {
                let end = src.floor_char_boundary(4000);
                cr_tokenize(&src[..end])
            } else {
                cr_tokenize(src)
            };

            let mut all_terms: Vec<String> = Vec::new();
            all_terms.extend(name_terms);
            all_terms.extend(sig_terms_vec);
            all_terms.extend(hint_terms);
            all_terms.extend(doc_terms);
            all_terms.extend(src_terms);

            let mut tf: HashMap<String, f64> = HashMap::new();
            let total = all_terms.len() as f64;
            for t in &all_terms {
                *tf.entry(t.clone()).or_default() += 1.0;
            }
            for v in tf.values_mut() {
                *v /= total;
            }

            TermSet {
                terms: tf,
                name_terms: name_set,
                sig_terms: sig_set,
            }
        })
        .collect();

    eprintln!("  Cruncher: computing global IDF...");
    let mut doc_freq: HashMap<String, usize> = HashMap::new();
    for ts in &term_sets {
        for term in ts.terms.keys() {
            *doc_freq.entry(term.clone()).or_default() += 1;
        }
    }
    let n_f = n as f64;
    let global_idf: HashMap<String, f64> = doc_freq
        .iter()
        .map(|(term, &df)| {
            let idf = (1.0 + n_f / (df as f64 + 1.0)).ln();
            (term.clone(), idf)
        })
        .collect();

    let term_sets: Vec<TermSet> = term_sets
        .into_iter()
        .map(|mut ts| {
            let mut scored: Vec<(String, f64)> = ts
                .terms
                .iter()
                .map(|(t, &tf)| {
                    let idf = global_idf.get(t).copied().unwrap_or(1.0);
                    (t.clone(), tf * idf)
                })
                .collect();
            scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
            scored.truncate(TOP_K_TERMS);
            let mut filtered = HashMap::new();
            for (t, _score) in scored {
                let tf = ts.terms.get(&t).copied().unwrap_or(0.0);
                filtered.insert(t, tf);
            }
            ts.terms = filtered;
            ts
        })
        .collect();

    eprintln!("  Cruncher: computing bridging potential...");
    let bridging: Vec<f64> = (0..n)
        .map(|i| {
            if outgoing[i].is_empty() {
                return 0.0;
            }
            let self_terms: HashSet<&str> =
                term_sets[i].terms.keys().map(|s| s.as_str()).collect();
            let mut novel_count = 0usize;
            let mut total_terms = 0usize;
            for edge in outgoing[i].iter().take(20) {
                for term in term_sets[edge.target].terms.keys() {
                    total_terms += 1;
                    if !self_terms.contains(term.as_str()) {
                        novel_count += 1;
                    }
                }
            }
            if total_terms == 0 {
                return 0.0;
            }
            let novel_ratio = novel_count as f64 / total_terms as f64;
            let degree_boost = (1.0 + outgoing[i].len() as f64).ln_1p() * 0.3;
            novel_ratio * (1.0 + degree_boost)
        })
        .collect();

    eprintln!("  Cruncher: done ({} symbols)", n);

    let structural_degree: Vec<f64> = (0..n)
        .map(|i| {
            let out_deg = outgoing[i].len() as f64;
            let in_deg = incoming[i].len() as f64;
            (out_deg + in_deg).ln_1p()
        })
        .collect();

    Ok(CruncherIndex {
        n,
        symbol_ids,
        symbol_names,
        symbol_kinds,
        symbol_file_ids,
        file_paths,
        outgoing,
        incoming,
        term_sets,
        global_idf,
        bridging,
        id_to_idx,
        name_to_indices,
        structural_degree,
    })
}

pub fn build_query_terms(query: &str, idf: &HashMap<String, f64>) -> Vec<QueryTerm> {
    let raw_terms = extract_terms(query);
    raw_terms
        .into_iter()
        .map(|t| {
            let variants = expand_variants(&t);
            let idf_val = idf.get(&t).copied().unwrap_or(1.0);
            QueryTerm {
                text: t,
                variants,
                idf: idf_val,
            }
        })
        .collect()
}

pub fn term_match_score(query_terms: &[QueryTerm], term_set: &TermSet) -> (f64, usize) {
    let mut score = 0.0f64;
    let mut matched = 0usize;

    for qt in query_terms {
        let mut best = 0.0f64;
        for variant in &qt.variants {
            if let Some(&w) = term_set.terms.get(variant) {
                best = best.max(w);
            }
            for (term, &w) in &term_set.terms {
                if term == variant {
                    continue;
                }
                if term.contains(variant) || variant.contains(term.as_str()) {
                    let ratio = variant.len().min(term.len()) as f64
                        / variant.len().max(term.len()) as f64;
                    best = best.max(w * ratio);
                }
            }
        }
        if best > 0.0 {
            matched += 1;
            score += qt.idf * best;
        }
    }

    (score, matched)
}

pub fn name_coverage(query_terms: &[QueryTerm], name_terms: &HashSet<String>) -> (f64, usize) {
    let mut score = 0.0f64;
    let mut matched = 0usize;
    for qt in query_terms {
        for variant in &qt.variants {
            if name_terms.contains(variant) {
                score += qt.idf * 3.0;
                matched += 1;
                break;
            }
            for nt in name_terms {
                if nt.contains(variant) || variant.contains(nt.as_str()) {
                    let ratio = variant.len().min(nt.len()) as f64
                        / variant.len().max(nt.len()) as f64;
                    score += qt.idf * 2.0 * ratio;
                    matched += 1;
                    break;
                }
            }
        }
    }
    (score, matched)
}

pub fn per_term_match(term_set: &TermSet, qt: &QueryTerm) -> f64 {
    let mut best = 0.0f64;
    for variant in &qt.variants {
        if let Some(&w) = term_set.terms.get(variant) {
            best = best.max(w);
        }
        for (term, &w) in &term_set.terms {
            if term == variant {
                continue;
            }
            if term.contains(variant) || variant.contains(term.as_str()) {
                let ratio = variant.len().min(term.len()) as f64
                    / variant.len().max(term.len()) as f64;
                best = best.max(w * ratio);
            }
        }
    }
    best * qt.idf
}

#[derive(Clone)]
pub struct SecChannelVec {
    pub ch_self: f64,
    pub ch_name: f64,
    pub ch_sig: f64,
    pub ch_out_1hop: f64,
    pub ch_in_1hop: f64,
    pub ch_out_2hop: f64,
    pub ch_in_2hop: f64,
}

pub fn compute_sec_channels(
    query_terms: &[QueryTerm],
    idx: &CruncherIndex,
    sym_i: usize,
) -> SecChannelVec {
    let ts = &idx.term_sets[sym_i];

    let (ch_self, _) = term_match_score(query_terms, ts);
    let (ch_name, _) = name_coverage(query_terms, &ts.name_terms);

    let sig_terms_ref: HashSet<String> = ts.sig_terms.iter().cloned().collect();
    let (ch_sig, _) = name_coverage(query_terms, &sig_terms_ref);

    let mut ch_out_1hop = 0.0f64;
    let mut ch_in_1hop = 0.0f64;
    let mut out_2hop_set: HashSet<usize> = HashSet::new();
    let mut in_2hop_set: HashSet<usize> = HashSet::new();

    for edge in idx.outgoing[sym_i].iter().take(30) {
        let (s, _) = term_match_score(query_terms, &idx.term_sets[edge.target]);
        ch_out_1hop = ch_out_1hop.max(s);
        for e2 in idx.outgoing[edge.target].iter().take(15) {
            if e2.target != sym_i {
                out_2hop_set.insert(e2.target);
            }
        }
    }

    for edge in idx.incoming[sym_i].iter().take(30) {
        let (s, _) = term_match_score(query_terms, &idx.term_sets[edge.target]);
        ch_in_1hop = ch_in_1hop.max(s);
        for e2 in idx.incoming[edge.target].iter().take(15) {
            if e2.target != sym_i {
                in_2hop_set.insert(e2.target);
            }
        }
    }

    let mut ch_out_2hop = 0.0f64;
    for &ni in out_2hop_set.iter().take(50) {
        let (s, _) = term_match_score(query_terms, &idx.term_sets[ni]);
        ch_out_2hop = ch_out_2hop.max(s);
    }

    let mut ch_in_2hop = 0.0f64;
    for &ni in in_2hop_set.iter().take(50) {
        let (s, _) = term_match_score(query_terms, &idx.term_sets[ni]);
        ch_in_2hop = ch_in_2hop.max(s);
    }

    SecChannelVec {
        ch_self,
        ch_name,
        ch_sig,
        ch_out_1hop: ch_out_1hop * 0.6,
        ch_in_1hop: ch_in_1hop * 0.6,
        ch_out_2hop: ch_out_2hop * 0.3,
        ch_in_2hop: ch_in_2hop * 0.3,
    }
}

pub fn negentropy(channels: &SecChannelVec) -> f64 {
    let scores = [
        channels.ch_self,
        channels.ch_name,
        channels.ch_sig,
        channels.ch_out_1hop,
        channels.ch_in_1hop,
        channels.ch_out_2hop,
        channels.ch_in_2hop,
    ];

    let sum: f64 = scores.iter().sum::<f64>().max(1e-15);
    let n = scores.len() as f64;
    let uniform_entropy = n.ln();

    let mut entropy = 0.0f64;
    for &s in &scores {
        let p = s / sum;
        if p > 1e-15 {
            entropy -= p * p.ln();
        }
    }

    let ng = uniform_entropy - entropy;

    let mean = sum / n;
    let variance = scores.iter().map(|s| (s - mean).powi(2)).sum::<f64>() / n;
    let kurtosis = if variance > 1e-15 {
        scores.iter().map(|s| ((s - mean) / variance.sqrt()).powi(4)).sum::<f64>() / n - 3.0
    } else {
        0.0
    };

    ng + kurtosis.max(0.0) * 0.3
}

pub fn channel_coherence(
    query_terms: &[QueryTerm],
    idx: &CruncherIndex,
    sym_i: usize,
) -> f64 {
    if query_terms.len() < 2 {
        return 0.0;
    }

    let ts = &idx.term_sets[sym_i];
    let mut term_self_match: Vec<bool> = Vec::with_capacity(query_terms.len());
    let mut term_name_match: Vec<bool> = Vec::with_capacity(query_terms.len());
    let mut term_sig_match: Vec<bool> = Vec::with_capacity(query_terms.len());
    let mut term_out_match: Vec<bool> = Vec::with_capacity(query_terms.len());
    let mut term_in_match: Vec<bool> = Vec::with_capacity(query_terms.len());

    for qt in query_terms {
        term_self_match.push(per_term_match(ts, qt) > 0.0);
        term_name_match.push(qt.variants.iter().any(|v| ts.name_terms.contains(v)));
        term_sig_match.push(qt.variants.iter().any(|v| ts.sig_terms.contains(v)));

        let mut out_hit = false;
        for edge in idx.outgoing[sym_i].iter().take(20) {
            if per_term_match(&idx.term_sets[edge.target], qt) > 0.0 {
                out_hit = true;
                break;
            }
        }
        term_out_match.push(out_hit);

        let mut in_hit = false;
        for edge in idx.incoming[sym_i].iter().take(20) {
            if per_term_match(&idx.term_sets[edge.target], qt) > 0.0 {
                in_hit = true;
                break;
            }
        }
        term_in_match.push(in_hit);
    }

    let channels: [&[bool]; 5] = [
        &term_self_match,
        &term_name_match,
        &term_sig_match,
        &term_out_match,
        &term_in_match,
    ];

    let mut coherent_pairs = 0usize;
    let mut total_pairs = 0usize;

    for ci in 0..channels.len() {
        for cj in (ci + 1)..channels.len() {
            for k in 0..query_terms.len() {
                total_pairs += 1;
                if channels[ci][k] && channels[cj][k] {
                    coherent_pairs += 1;
                }
            }
        }
    }

    if total_pairs == 0 {
        return 0.0;
    }

    let ratio = coherent_pairs as f64 / total_pairs as f64;
    ratio * ratio.ln_1p()
}

#[cfg(test)]
mod fuzz_tests {
    use super::*;
    use crate::db::GraphDb;
    use crate::fts::FtsSearch;
    use crate::symbol::{SymbolBuilder, SymbolKind};

    fn build_tiny_db() -> GraphDb {
        let db = GraphDb::open_in_memory().unwrap();
        let fid = db
            .upsert_file("src/app.rs", "rust", "abc", 500, 50)
            .unwrap();
        for (name, kind, start, end) in [
            ("parse_config", SymbolKind::Function, 1, 10),
            ("build_index", SymbolKind::Function, 11, 20),
            ("handle_request", SymbolKind::Function, 21, 30),
            ("validate_token", SymbolKind::Function, 31, 40),
            ("rate_limit", SymbolKind::Function, 41, 50),
        ] {
            let sym = SymbolBuilder::new(fid, name.into(), kind, format!("fn {}()", name), "rust".into())
                .lines(start, end)
                .signature(format!("fn {}(x: i32) -> bool", name))
                .build();
            db.insert_symbol(&sym).unwrap();
        }
        db
    }

    fn run_fuzz_query(db: &GraphDb, query: &str) {
        let ci = build_cruncher_index(db).unwrap();
        let fts = FtsSearch::new(db);
        let bm25: Vec<(i64, f64)> = fts
            .search(query, Some(30))
            .into_iter()
            .map(|r| (r.symbol.id, r.bm25_score))
            .collect();

        let query_terms = build_query_terms(query, &ci.global_idf);
        for &(id, _score) in bm25.iter().take(MAX_SEEDS) {
            if let Some(&i) = ci.id_to_idx.get(&id) {
                let _ = term_match_score(&query_terms, &ci.term_sets[i]);
                let _ = name_coverage(&query_terms, &ci.term_sets[i].name_terms);
                let _ = per_term_match(&ci.term_sets[i], query_terms.first().unwrap_or(&QueryTerm {
                    text: "x".into(),
                    variants: vec!["x".into()],
                    idf: 1.0,
                }));
                let channels = compute_sec_channels(&query_terms, &ci, i);
                let _ = negentropy(&channels);
                let _ = channel_coherence(&query_terms, &ci, i);
            }
        }
    }

    #[test]
    fn fuzz_empty_query() {
        let db = build_tiny_db();
        run_fuzz_query(&db, "");
    }

    #[test]
    fn fuzz_single_char() {
        let db = build_tiny_db();
        for c in &["a", "z", "0", " ", ".", "-", "_", "日本"] {
            run_fuzz_query(&db, c);
        }
    }

    #[test]
    fn fuzz_whitespace_variants() {
        let db = build_tiny_db();
        for q in &[" ", "  ", "\t", "\n", "  parse  config  "] {
            run_fuzz_query(&db, q);
        }
    }

    #[test]
    fn fuzz_special_chars() {
        let db = build_tiny_db();
        for q in &[
            "parse(config)",
            "a && b || c",
            "foo.bar.baz",
            "rate-limit",
            "parse+config",
            "parse*config",
            "parse[0]",
            "{json: true}",
            "<html>",
            "a->b",
            "a=>b",
            "a::b",
            "a;b",
            "a,b",
            "\"quoted\"",
            "'single'",
            "\\escaped\\",
            "null",
            "undefined",
            "NaN",
        ] {
            run_fuzz_query(&db, q);
        }
    }

    #[test]
    fn fuzz_unicode() {
        let db = build_tiny_db();
        for q in &[
            "парсить",
            "解析設定",
            "구성분석",
            "🦀 rust",
            "parse_конфиг",
        ] {
            run_fuzz_query(&db, q);
        }
    }

    #[test]
    fn fuzz_very_long_query() {
        let db = build_tiny_db();
        let long = (0..1000).map(|i| format!("term{}", i)).collect::<Vec<_>>().join(" ");
        run_fuzz_query(&db, &long);
    }

    #[test]
    fn fuzz_repeated_terms() {
        let db = build_tiny_db();
        run_fuzz_query(&db, "parse parse parse parse");
        run_fuzz_query(&db, "a a a a a a a a a a");
    }

    #[test]
    fn fuzz_only_stopwords() {
        let db = build_tiny_db();
        for q in &["the", "a an the", "is are was were"] {
            run_fuzz_query(&db, q);
        }
    }

    #[test]
    fn fuzz_camel_snake_kebab() {
        let db = build_tiny_db();
        for q in &[
            "parseConfig",
            "parse_config",
            "parse-config",
            "PascalCase",
            "UPPER_CASE",
            "miXeD_CaSe_Name",
        ] {
            run_fuzz_query(&db, q);
        }
    }

    #[test]
    fn fuzz_numeric_queries() {
        let db = build_tiny_db();
        for q in &["123", "3.14", "0x1F", "1e10", "v2.0", "h264"] {
            run_fuzz_query(&db, q);
        }
    }
}
