use std::collections::{HashMap, HashSet};

use crate::db::GraphDb;
use crate::lsa::extract_terms;
use crate::tokenize::decompose_identifier;

pub struct SecIndex {
    pub symbol_ids: Vec<i64>,
    pub symbol_names: Vec<String>,
    pub symbol_kinds: Vec<String>,
    pub symbol_sigs: Vec<Option<String>>,
    pub symbol_file_ids: Vec<i64>,
    pub file_paths: HashMap<i64, String>,
    pub id_to_idx: HashMap<i64, usize>,

    pub ch_self: Vec<HashMap<String, f64>>,
    pub ch_calls_out: Vec<HashMap<String, f64>>,
    pub ch_calls_in: Vec<HashMap<String, f64>>,
    pub ch_calls_out_2hop: Vec<HashMap<String, f64>>,
    pub ch_calls_in_2hop: Vec<HashMap<String, f64>>,
    pub ch_type_ret: Vec<HashMap<String, f64>>,
    pub ch_file_path: Vec<HashMap<String, f64>>,

    pub outgoing: Vec<Vec<usize>>,
    pub incoming: Vec<Vec<usize>>,

    pub global_idf: HashMap<String, f64>,
}

#[derive(Debug, Clone, Default)]
pub struct ChannelScores {
    pub ch_self: f64,
    pub ch_calls_out: f64,
    pub ch_calls_in: f64,
    pub ch_calls_out_2hop: f64,
    pub ch_calls_in_2hop: f64,
    pub ch_type_ret: f64,
    pub ch_file_path: f64,
}

const STOP_WORDS: &[&str] = &[
    "the", "a", "an", "is", "are", "was", "were", "be", "been", "being", "have", "has", "had",
    "do", "does", "did", "will", "would", "could", "should", "may", "might", "can", "shall", "to",
    "of", "in", "for", "on", "with", "at", "by", "from", "as", "into", "through", "and", "or",
    "but", "not", "that", "this", "these", "those", "it", "its", "if", "then", "than", "so", "up",
    "out", "new", "all", "every", "how", "what", "where", "when", "why", "which", "who", "whom",
    "there", "here", "no", "nor", "just", "very", "also", "some", "any", "each", "both", "few",
    "more", "most", "other", "such", "only", "own", "same",
];

fn sec_tokenize(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let mut terms: Vec<String> = lower
        .split_whitespace()
        .filter(|t| t.len() >= 2)
        .filter(|t| !STOP_WORDS.contains(t))
        .map(|t| t.to_string())
        .collect();

    let decomp = decompose_identifier(text);
    for t in decomp.split_whitespace() {
        let t = t.to_lowercase();
        if t.len() >= 2 && !STOP_WORDS.contains(&t.as_str()) {
            terms.push(t);
        }
    }

    terms.sort_unstable();
    terms.dedup();
    terms
}

fn term_bag(terms: &[String]) -> HashMap<String, f64> {
    let total = terms.len() as f64;
    if total == 0.0 {
        return HashMap::new();
    }
    let mut counts: HashMap<String, f64> = HashMap::new();
    for t in terms {
        *counts.entry(t.clone()).or_default() += 1.0;
    }
    for v in counts.values_mut() {
        *v /= total;
    }
    counts
}

fn merge_bags(bags: &[&HashMap<String, f64>], decay: f64) -> HashMap<String, f64> {
    let mut merged: HashMap<String, f64> = HashMap::new();
    for bag in bags {
        for (term, &weight) in *bag {
            *merged.entry(term.clone()).or_default() += weight * decay;
        }
    }
    merged
}

pub fn build_sec_index(db: &GraphDb) -> Result<SecIndex, String> {
    let conn = db.conn();

    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.name, s.kind, s.signature, s.file_id \
             FROM symbols s ORDER BY s.id",
        )
        .map_err(|e| e.to_string())?;
    let rows: Vec<(i64, String, String, Option<String>, i64)> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get(4)?,
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

    let id_to_idx: HashMap<i64, usize> = symbol_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i))
        .collect();

    let mut file_stmt = conn
        .prepare("SELECT id, path FROM files")
        .map_err(|e| e.to_string())?;
    let file_paths: HashMap<i64, String> = file_stmt
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| e.to_string())?
        .flatten()
        .collect();

    let mut outgoing: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut incoming: Vec<Vec<usize>> = vec![Vec::new(); n];

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
            if kind == "calls" || kind == "references" || kind == "imports" {
                outgoing[si].push(ti);
                incoming[ti].push(si);
            }
        }
    }

    for adj in &mut outgoing {
        adj.sort_unstable();
        adj.dedup();
    }
    for adj in &mut incoming {
        adj.sort_unstable();
        adj.dedup();
    }

    eprintln!("  SEC: building self channels...");
    let ch_self: Vec<HashMap<String, f64>> = (0..n)
        .map(|i| {
            let name_terms = sec_tokenize(&symbol_names[i]);
            let kind_terms = sec_tokenize(&symbol_kinds[i]);
            let mut all = name_terms;
            all.extend(kind_terms);
            if let Some(ref sig) = symbol_sigs[i] {
                all.extend(sec_tokenize(sig));
            }
            term_bag(&all)
        })
        .collect();

    eprintln!("  SEC: propagating 1-hop outgoing terms...");
    let ch_calls_out: Vec<HashMap<String, f64>> = (0..n)
        .map(|i| {
            if outgoing[i].is_empty() {
                return HashMap::new();
            }
            let neighbor_bags: Vec<&HashMap<String, f64>> = outgoing[i]
                .iter()
                .take(50)
                .map(|&ni| &ch_self[ni])
                .collect();
            merge_bags(&neighbor_bags, 0.6)
        })
        .collect();

    eprintln!("  SEC: propagating 1-hop incoming terms...");
    let ch_calls_in: Vec<HashMap<String, f64>> = (0..n)
        .map(|i| {
            if incoming[i].is_empty() {
                return HashMap::new();
            }
            let neighbor_bags: Vec<&HashMap<String, f64>> = incoming[i]
                .iter()
                .take(50)
                .map(|&ni| &ch_self[ni])
                .collect();
            merge_bags(&neighbor_bags, 0.6)
        })
        .collect();

    eprintln!("  SEC: propagating 2-hop outgoing terms...");
    let ch_calls_out_2hop: Vec<HashMap<String, f64>> = (0..n)
        .map(|i| {
            let mut two_hop: HashSet<usize> = HashSet::new();
            for &n1 in outgoing[i].iter().take(30) {
                for &n2 in outgoing[n1].iter().take(20) {
                    if n2 != i {
                        two_hop.insert(n2);
                    }
                }
            }
            if two_hop.is_empty() {
                return HashMap::new();
            }
            let bags: Vec<&HashMap<String, f64>> =
                two_hop.iter().take(100).map(|&ni| &ch_self[ni]).collect();
            merge_bags(&bags, 0.3)
        })
        .collect();

    eprintln!("  SEC: propagating 2-hop incoming terms...");
    let ch_calls_in_2hop: Vec<HashMap<String, f64>> = (0..n)
        .map(|i| {
            let mut two_hop: HashSet<usize> = HashSet::new();
            for &n1 in incoming[i].iter().take(30) {
                for &n2 in incoming[n1].iter().take(20) {
                    if n2 != i {
                        two_hop.insert(n2);
                    }
                }
            }
            if two_hop.is_empty() {
                return HashMap::new();
            }
            let bags: Vec<&HashMap<String, f64>> =
                two_hop.iter().take(100).map(|&ni| &ch_self[ni]).collect();
            merge_bags(&bags, 0.3)
        })
        .collect();

    eprintln!("  SEC: extracting type_ret channels...");
    let ch_type_ret: Vec<HashMap<String, f64>> = (0..n)
        .map(|i| {
            if let Some(ref sig) = symbol_sigs[i] {
                let sig_lower = sig.to_lowercase();
                let mut type_terms: Vec<String> = Vec::new();

                for chunk in sig_lower.split(
                    &[
                        ':', '-', '<', '>', '(', ')', '[', ']', '{', '}', ',', '=', '+', '|', '&',
                    ][..],
                ) {
                    let tokens = sec_tokenize(chunk);
                    type_terms.extend(tokens);
                }

                type_terms.sort_unstable();
                type_terms.dedup();
                term_bag(&type_terms)
            } else {
                HashMap::new()
            }
        })
        .collect();

    eprintln!("  SEC: building file_path channels...");
    let ch_file_path: Vec<HashMap<String, f64>> = (0..n)
        .map(|i| {
            let path = file_paths
                .get(&symbol_file_ids[i])
                .cloned()
                .unwrap_or_default();
            let path_lower = path.to_lowercase();
            let mut path_terms: Vec<String> = Vec::new();
            for component in path_lower.split(&['/', '\\', '.', '-', '_'][..]) {
                if component.len() >= 2 && !STOP_WORDS.contains(&component) {
                    path_terms.push(component.to_string());
                    let decomp = decompose_identifier(component);
                    for t in decomp.split_whitespace() {
                        let t = t.to_lowercase();
                        if t.len() >= 2 && !STOP_WORDS.contains(&t.as_str()) {
                            path_terms.push(t);
                        }
                    }
                }
            }
            path_terms.sort_unstable();
            path_terms.dedup();
            term_bag(&path_terms)
        })
        .collect();

    eprintln!("  SEC: computing global IDF...");
    let mut doc_freq: HashMap<String, usize> = HashMap::new();
    for bag in &ch_self {
        for term in bag.keys() {
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

    eprintln!(
        "  SEC: done ({} symbols, {} unique terms)",
        n,
        global_idf.len()
    );

    Ok(SecIndex {
        symbol_ids,
        symbol_names,
        symbol_kinds,
        symbol_sigs,
        symbol_file_ids,
        file_paths,
        id_to_idx,
        ch_self,
        ch_calls_out,
        ch_calls_in,
        ch_calls_out_2hop,
        ch_calls_in_2hop,
        ch_type_ret,
        ch_file_path,
        outgoing,
        incoming,
        global_idf,
    })
}

#[derive(Debug, Clone)]
struct QueryTerm {
    text: String,
    variants: Vec<String>,
    idf: f64,
}

fn expand_query_variants(term: &str) -> Vec<String> {
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

fn build_query_terms(query: &str, index: &SecIndex) -> Vec<QueryTerm> {
    let raw_terms = extract_terms(query);
    raw_terms
        .into_iter()
        .map(|t| {
            let variants = expand_query_variants(&t);
            let idf = index.global_idf.get(&t).copied().unwrap_or(1.0);
            QueryTerm {
                text: t,
                variants,
                idf,
            }
        })
        .collect()
}

fn channel_overlap(query_terms: &[QueryTerm], channel: &HashMap<String, f64>) -> f64 {
    let mut score = 0.0f64;
    for qt in query_terms {
        let mut best_match = 0.0f64;
        for variant in &qt.variants {
            if let Some(&w) = channel.get(variant) {
                best_match = best_match.max(w);
            }
            for (term, &w) in channel {
                if term.contains(variant) || variant.contains(term.as_str()) {
                    let ratio =
                        variant.len().min(term.len()) as f64 / variant.len().max(term.len()) as f64;
                    best_match = best_match.max(w * ratio);
                }
            }
        }
        score += best_match * qt.idf;
    }
    score
}

fn channel_has_hit(query_terms: &[QueryTerm], channel: &HashMap<String, f64>) -> bool {
    for qt in query_terms {
        for variant in &qt.variants {
            if channel.contains_key(variant) {
                return true;
            }
            for term in channel.keys() {
                if term.contains(variant) || variant.contains(term.as_str()) {
                    return true;
                }
            }
        }
    }
    false
}

const W_SELF: f64 = 3.0;
const W_CALLS_OUT: f64 = 1.5;
const W_CALLS_IN: f64 = 1.5;
const W_CALLS_OUT_2HOP: f64 = 0.7;
const W_CALLS_IN_2HOP: f64 = 0.7;
const W_TYPE_RET: f64 = 1.0;
const W_FILE_PATH: f64 = 0.5;
const DIVERSITY_BONUS: f64 = 2.5;

pub fn sec_rerank(
    query: &str,
    candidate_ids: &[i64],
    candidate_scores: &[f64],
    index: &SecIndex,
    top_k: usize,
) -> Vec<(i64, f64)> {
    let query_terms = build_query_terms(query, index);
    if query_terms.is_empty() {
        return candidate_ids
            .iter()
            .zip(candidate_scores.iter())
            .filter(|(_, &s)| s > 0.0)
            .take(top_k)
            .map(|(&id, &s)| (id, s))
            .collect();
    }

    let mut scored: Vec<(i64, f64, ChannelScores)> = Vec::with_capacity(candidate_ids.len());

    for (i, &id) in candidate_ids.iter().enumerate() {
        let base = candidate_scores.get(i).copied().unwrap_or(0.0);
        if base <= 0.0 {
            continue;
        }

        if let Some(&idx) = index.id_to_idx.get(&id) {
            let cs = ChannelScores {
                ch_self: channel_overlap(&query_terms, &index.ch_self[idx]),
                ch_calls_out: channel_overlap(&query_terms, &index.ch_calls_out[idx]),
                ch_calls_in: channel_overlap(&query_terms, &index.ch_calls_in[idx]),
                ch_calls_out_2hop: channel_overlap(&query_terms, &index.ch_calls_out_2hop[idx]),
                ch_calls_in_2hop: channel_overlap(&query_terms, &index.ch_calls_in_2hop[idx]),
                ch_type_ret: channel_overlap(&query_terms, &index.ch_type_ret[idx]),
                ch_file_path: channel_overlap(&query_terms, &index.ch_file_path[idx]),
            };

            let mut channels_hitting = 0usize;
            if cs.ch_self > 0.01 {
                channels_hitting += 1;
            }
            if cs.ch_calls_out > 0.01 {
                channels_hitting += 1;
            }
            if cs.ch_calls_in > 0.01 {
                channels_hitting += 1;
            }
            if cs.ch_calls_out_2hop > 0.01 {
                channels_hitting += 1;
            }
            if cs.ch_calls_in_2hop > 0.01 {
                channels_hitting += 1;
            }
            if cs.ch_type_ret > 0.01 {
                channels_hitting += 1;
            }
            if cs.ch_file_path > 0.01 {
                channels_hitting += 1;
            }

            let weighted_sum = W_SELF * cs.ch_self
                + W_CALLS_OUT * cs.ch_calls_out
                + W_CALLS_IN * cs.ch_calls_in
                + W_CALLS_OUT_2HOP * cs.ch_calls_out_2hop
                + W_CALLS_IN_2HOP * cs.ch_calls_in_2hop
                + W_TYPE_RET * cs.ch_type_ret
                + W_FILE_PATH * cs.ch_file_path;

            let diversity_mult = if channels_hitting >= 3 {
                1.0 + DIVERSITY_BONUS * (channels_hitting as f64 - 2.0).ln_1p()
            } else if channels_hitting >= 2 {
                1.0 + DIVERSITY_BONUS * 0.3
            } else {
                1.0
            };

            let sec_score = weighted_sum * diversity_mult;

            let kind_boost = match index.symbol_kinds[idx].as_str() {
                "function" | "method" | "constructor" => 1.3,
                "class" | "struct" | "interface" | "trait" => 1.2,
                "enum" | "type_alias" => 1.1,
                "constant" | "field" | "property" => 0.9,
                "module" | "section" => 0.7,
                _ => 1.0,
            };

            let path_lower = index
                .file_paths
                .get(&index.symbol_file_ids[idx])
                .map(|p| p.to_lowercase())
                .unwrap_or_default();
            let test_penalty = if path_lower.contains("/test")
                || path_lower.contains("_test.")
                || path_lower.contains("/benches/")
                || path_lower.contains("/spec/")
            {
                0.3
            } else {
                1.0
            };

            let has_self_hit = cs.ch_self > 0.1;

            let final_score = if has_self_hit {
                base * (1.0 + sec_score * kind_boost * test_penalty)
            } else {
                base * (1.0 + sec_score * 0.5 * kind_boost * test_penalty)
            };

            scored.push((id, final_score, cs));
        } else {
            scored.push((id, base, ChannelScores::default()));
        }
    }

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored.truncate(top_k);
    scored
        .into_iter()
        .map(|(id, score, _)| (id, score))
        .collect()
}

pub fn sec_rerank_debug(
    query: &str,
    candidate_ids: &[i64],
    candidate_scores: &[f64],
    index: &SecIndex,
    top_k: usize,
) -> Vec<(i64, f64, ChannelScores)> {
    let query_terms = build_query_terms(query, index);
    if query_terms.is_empty() {
        return candidate_ids
            .iter()
            .zip(candidate_scores.iter())
            .filter(|(_, &s)| s > 0.0)
            .take(top_k)
            .map(|(&id, &s)| (id, s, ChannelScores::default()))
            .collect();
    }

    let mut scored: Vec<(i64, f64, ChannelScores)> = Vec::with_capacity(candidate_ids.len());

    for (i, &id) in candidate_ids.iter().enumerate() {
        let base = candidate_scores.get(i).copied().unwrap_or(0.0);
        if base <= 0.0 {
            continue;
        }

        if let Some(&idx) = index.id_to_idx.get(&id) {
            let cs = ChannelScores {
                ch_self: channel_overlap(&query_terms, &index.ch_self[idx]),
                ch_calls_out: channel_overlap(&query_terms, &index.ch_calls_out[idx]),
                ch_calls_in: channel_overlap(&query_terms, &index.ch_calls_in[idx]),
                ch_calls_out_2hop: channel_overlap(&query_terms, &index.ch_calls_out_2hop[idx]),
                ch_calls_in_2hop: channel_overlap(&query_terms, &index.ch_calls_in_2hop[idx]),
                ch_type_ret: channel_overlap(&query_terms, &index.ch_type_ret[idx]),
                ch_file_path: channel_overlap(&query_terms, &index.ch_file_path[idx]),
            };

            let mut channels_hitting = 0usize;
            if cs.ch_self > 0.01 {
                channels_hitting += 1;
            }
            if cs.ch_calls_out > 0.01 {
                channels_hitting += 1;
            }
            if cs.ch_calls_in > 0.01 {
                channels_hitting += 1;
            }
            if cs.ch_calls_out_2hop > 0.01 {
                channels_hitting += 1;
            }
            if cs.ch_calls_in_2hop > 0.01 {
                channels_hitting += 1;
            }
            if cs.ch_type_ret > 0.01 {
                channels_hitting += 1;
            }
            if cs.ch_file_path > 0.01 {
                channels_hitting += 1;
            }

            let weighted_sum = W_SELF * cs.ch_self
                + W_CALLS_OUT * cs.ch_calls_out
                + W_CALLS_IN * cs.ch_calls_in
                + W_CALLS_OUT_2HOP * cs.ch_calls_out_2hop
                + W_CALLS_IN_2HOP * cs.ch_calls_in_2hop
                + W_TYPE_RET * cs.ch_type_ret
                + W_FILE_PATH * cs.ch_file_path;

            let diversity_mult = if channels_hitting >= 3 {
                1.0 + DIVERSITY_BONUS * (channels_hitting as f64 - 2.0).ln_1p()
            } else if channels_hitting >= 2 {
                1.0 + DIVERSITY_BONUS * 0.3
            } else {
                1.0
            };

            let sec_score = weighted_sum * diversity_mult;

            let kind_boost = match index.symbol_kinds[idx].as_str() {
                "function" | "method" | "constructor" => 1.3,
                "class" | "struct" | "interface" | "trait" => 1.2,
                "enum" | "type_alias" => 1.1,
                "constant" | "field" | "property" => 0.9,
                "module" | "section" => 0.7,
                _ => 1.0,
            };

            let path_lower = index
                .file_paths
                .get(&index.symbol_file_ids[idx])
                .map(|p| p.to_lowercase())
                .unwrap_or_default();
            let test_penalty = if path_lower.contains("/test")
                || path_lower.contains("_test.")
                || path_lower.contains("/benches/")
                || path_lower.contains("/spec/")
            {
                0.3
            } else {
                1.0
            };

            let has_self_hit = cs.ch_self > 0.1;

            let final_score = if has_self_hit {
                base * (1.0 + sec_score * kind_boost * test_penalty)
            } else {
                base * (1.0 + sec_score * 0.5 * kind_boost * test_penalty)
            };

            scored.push((id, final_score, cs));
        } else {
            scored.push((id, base, ChannelScores::default()));
        }
    }

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored.truncate(top_k);
    scored
}

pub struct SecInvertedIndex {
    postings: HashMap<String, Vec<Posting>>,
}

#[derive(Clone)]
struct Posting {
    symbol_idx: usize,
    channel_mask: u8,
    max_weight: f64,
}

const CH_SELF: u8 = 0x01;
const CH_OUT: u8 = 0x02;
const CH_IN: u8 = 0x04;
const CH_OUT2: u8 = 0x08;
const CH_IN2: u8 = 0x10;
const CH_TYPE: u8 = 0x20;
const CH_PATH: u8 = 0x40;

pub fn build_sec_inverted_index(sec: &SecIndex) -> SecInvertedIndex {
    let n = sec.symbol_ids.len();
    let mut term_postings: HashMap<String, Vec<Posting>> = HashMap::new();

    let channels: [(&Vec<HashMap<String, f64>>, u8); 7] = [
        (&sec.ch_self, CH_SELF),
        (&sec.ch_calls_out, CH_OUT),
        (&sec.ch_calls_in, CH_IN),
        (&sec.ch_calls_out_2hop, CH_OUT2),
        (&sec.ch_calls_in_2hop, CH_IN2),
        (&sec.ch_type_ret, CH_TYPE),
        (&sec.ch_file_path, CH_PATH),
    ];

    for i in 0..n {
        let mut term_channels: HashMap<&str, (u8, f64)> = HashMap::new();
        for (channel, mask) in &channels {
            for (term, &weight) in channel[i].iter() {
                let entry = term_channels.entry(term.as_str()).or_insert((0, 0.0));
                entry.0 |= mask;
                entry.1 = entry.1.max(weight);
            }
        }
        for (term, (mask, weight)) in term_channels {
            term_postings
                .entry(term.to_string())
                .or_default()
                .push(Posting {
                    symbol_idx: i,
                    channel_mask: mask,
                    max_weight: weight,
                });
        }
    }

    for postings in term_postings.values_mut() {
        postings.sort_by_key(|p| p.symbol_idx);
    }

    eprintln!(
        "  SEC inverted index: {} terms, {} total postings",
        term_postings.len(),
        term_postings.values().map(|v| v.len()).sum::<usize>()
    );

    SecInvertedIndex {
        postings: term_postings,
    }
}

pub fn sec_standalone_search(
    query: &str,
    sec: &SecIndex,
    inv: &SecInvertedIndex,
    top_k: usize,
) -> Vec<(i64, f64)> {
    let query_terms = build_query_terms(query, sec);
    if query_terms.is_empty() {
        return Vec::new();
    }

    let mut candidate_set: HashSet<usize> = HashSet::new();
    for qt in &query_terms {
        for variant in &qt.variants {
            if let Some(postings) = inv.postings.get(variant) {
                for p in postings {
                    candidate_set.insert(p.symbol_idx);
                }
            }
            for term in inv.postings.keys() {
                if term.contains(variant.as_str()) || variant.contains(term.as_str()) {
                    if let Some(postings) = inv.postings.get(term) {
                        for p in postings {
                            candidate_set.insert(p.symbol_idx);
                        }
                    }
                }
            }
        }
    }

    if candidate_set.is_empty() {
        return Vec::new();
    }

    let mut scored: Vec<(usize, f64)> = Vec::with_capacity(candidate_set.len());

    for &i in &candidate_set {
        let cs = ChannelScores {
            ch_self: channel_overlap(&query_terms, &sec.ch_self[i]),
            ch_calls_out: channel_overlap(&query_terms, &sec.ch_calls_out[i]),
            ch_calls_in: channel_overlap(&query_terms, &sec.ch_calls_in[i]),
            ch_calls_out_2hop: channel_overlap(&query_terms, &sec.ch_calls_out_2hop[i]),
            ch_calls_in_2hop: channel_overlap(&query_terms, &sec.ch_calls_in_2hop[i]),
            ch_type_ret: channel_overlap(&query_terms, &sec.ch_type_ret[i]),
            ch_file_path: channel_overlap(&query_terms, &sec.ch_file_path[i]),
        };

        let mut channels_hitting = 0usize;
        if cs.ch_self > 0.01 {
            channels_hitting += 1;
        }
        if cs.ch_calls_out > 0.01 {
            channels_hitting += 1;
        }
        if cs.ch_calls_in > 0.01 {
            channels_hitting += 1;
        }
        if cs.ch_calls_out_2hop > 0.01 {
            channels_hitting += 1;
        }
        if cs.ch_calls_in_2hop > 0.01 {
            channels_hitting += 1;
        }
        if cs.ch_type_ret > 0.01 {
            channels_hitting += 1;
        }
        if cs.ch_file_path > 0.01 {
            channels_hitting += 1;
        }

        let weighted_sum = W_SELF * cs.ch_self
            + W_CALLS_OUT * cs.ch_calls_out
            + W_CALLS_IN * cs.ch_calls_in
            + W_CALLS_OUT_2HOP * cs.ch_calls_out_2hop
            + W_CALLS_IN_2HOP * cs.ch_calls_in_2hop
            + W_TYPE_RET * cs.ch_type_ret
            + W_FILE_PATH * cs.ch_file_path;

        let diversity_mult = if channels_hitting >= 3 {
            1.0 + DIVERSITY_BONUS * (channels_hitting as f64 - 2.0).ln_1p()
        } else if channels_hitting >= 2 {
            1.0 + DIVERSITY_BONUS * 0.3
        } else {
            1.0
        };

        let sec_score = weighted_sum * diversity_mult;

        let kind_boost = match sec.symbol_kinds[i].as_str() {
            "function" | "method" | "constructor" => 1.3,
            "class" | "struct" | "interface" | "trait" => 1.2,
            "enum" | "type_alias" => 1.1,
            "constant" | "field" | "property" => 0.9,
            "module" | "section" => 0.7,
            _ => 1.0,
        };

        let path_lower = sec
            .file_paths
            .get(&sec.symbol_file_ids[i])
            .map(|p| p.to_lowercase())
            .unwrap_or_default();
        let test_penalty = if path_lower.contains("/test")
            || path_lower.contains("_test.")
            || path_lower.contains("/benches/")
            || path_lower.contains("/spec/")
        {
            0.3
        } else {
            1.0
        };

        let final_score = sec_score * kind_boost * test_penalty;

        if final_score > 0.001 {
            scored.push((i, final_score));
        }
    }

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored.truncate(top_k);

    scored
        .into_iter()
        .map(|(i, s)| (sec.symbol_ids[i], s))
        .collect()
}

pub fn sec_standalone_search_debug(
    query: &str,
    sec: &SecIndex,
    inv: &SecInvertedIndex,
    top_k: usize,
) -> Vec<(i64, f64, ChannelScores)> {
    let query_terms = build_query_terms(query, sec);
    if query_terms.is_empty() {
        return Vec::new();
    }

    let mut candidate_set: HashSet<usize> = HashSet::new();
    for qt in &query_terms {
        for variant in &qt.variants {
            if let Some(postings) = inv.postings.get(variant) {
                for p in postings {
                    candidate_set.insert(p.symbol_idx);
                }
            }
            for term in inv.postings.keys() {
                if term.contains(variant.as_str()) || variant.contains(term.as_str()) {
                    if let Some(postings) = inv.postings.get(term) {
                        for p in postings {
                            candidate_set.insert(p.symbol_idx);
                        }
                    }
                }
            }
        }
    }

    if candidate_set.is_empty() {
        return Vec::new();
    }

    let mut scored: Vec<(usize, f64, ChannelScores)> = Vec::with_capacity(candidate_set.len());

    for &i in &candidate_set {
        let cs = ChannelScores {
            ch_self: channel_overlap(&query_terms, &sec.ch_self[i]),
            ch_calls_out: channel_overlap(&query_terms, &sec.ch_calls_out[i]),
            ch_calls_in: channel_overlap(&query_terms, &sec.ch_calls_in[i]),
            ch_calls_out_2hop: channel_overlap(&query_terms, &sec.ch_calls_out_2hop[i]),
            ch_calls_in_2hop: channel_overlap(&query_terms, &sec.ch_calls_in_2hop[i]),
            ch_type_ret: channel_overlap(&query_terms, &sec.ch_type_ret[i]),
            ch_file_path: channel_overlap(&query_terms, &sec.ch_file_path[i]),
        };

        let mut channels_hitting = 0usize;
        if cs.ch_self > 0.01 {
            channels_hitting += 1;
        }
        if cs.ch_calls_out > 0.01 {
            channels_hitting += 1;
        }
        if cs.ch_calls_in > 0.01 {
            channels_hitting += 1;
        }
        if cs.ch_calls_out_2hop > 0.01 {
            channels_hitting += 1;
        }
        if cs.ch_calls_in_2hop > 0.01 {
            channels_hitting += 1;
        }
        if cs.ch_type_ret > 0.01 {
            channels_hitting += 1;
        }
        if cs.ch_file_path > 0.01 {
            channels_hitting += 1;
        }

        let weighted_sum = W_SELF * cs.ch_self
            + W_CALLS_OUT * cs.ch_calls_out
            + W_CALLS_IN * cs.ch_calls_in
            + W_CALLS_OUT_2HOP * cs.ch_calls_out_2hop
            + W_CALLS_IN_2HOP * cs.ch_calls_in_2hop
            + W_TYPE_RET * cs.ch_type_ret
            + W_FILE_PATH * cs.ch_file_path;

        let diversity_mult = if channels_hitting >= 3 {
            1.0 + DIVERSITY_BONUS * (channels_hitting as f64 - 2.0).ln_1p()
        } else if channels_hitting >= 2 {
            1.0 + DIVERSITY_BONUS * 0.3
        } else {
            1.0
        };

        let sec_score = weighted_sum * diversity_mult;

        let kind_boost = match sec.symbol_kinds[i].as_str() {
            "function" | "method" | "constructor" => 1.3,
            "class" | "struct" | "interface" | "trait" => 1.2,
            "enum" | "type_alias" => 1.1,
            "constant" | "field" | "property" => 0.9,
            "module" | "section" => 0.7,
            _ => 1.0,
        };

        let path_lower = sec
            .file_paths
            .get(&sec.symbol_file_ids[i])
            .map(|p| p.to_lowercase())
            .unwrap_or_default();
        let test_penalty = if path_lower.contains("/test")
            || path_lower.contains("_test.")
            || path_lower.contains("/benches/")
            || path_lower.contains("/spec/")
        {
            0.3
        } else {
            1.0
        };

        let final_score = sec_score * kind_boost * test_penalty;

        if final_score > 0.001 {
            scored.push((i, final_score, cs));
        }
    }

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored.truncate(top_k);

    scored
        .into_iter()
        .map(|(i, s, cs)| (sec.symbol_ids[i], s, cs))
        .collect()
}

pub fn sec_search(query: &str, index: &SecIndex, top_k: usize) -> Vec<(i64, f64)> {
    let query_terms = build_query_terms(query, index);
    if query_terms.is_empty() {
        return Vec::new();
    }

    let n = index.symbol_ids.len();
    let mut scored: Vec<(usize, f64)> = Vec::with_capacity(n);

    for i in 0..n {
        let cs_self = channel_overlap(&query_terms, &index.ch_self[i]);
        let cs_out = channel_overlap(&query_terms, &index.ch_calls_out[i]);
        let cs_in = channel_overlap(&query_terms, &index.ch_calls_in[i]);
        let cs_out2 = channel_overlap(&query_terms, &index.ch_calls_out_2hop[i]);
        let cs_in2 = channel_overlap(&query_terms, &index.ch_calls_in_2hop[i]);
        let cs_type = channel_overlap(&query_terms, &index.ch_type_ret[i]);
        let cs_path = channel_overlap(&query_terms, &index.ch_file_path[i]);

        let mut channels_hitting = 0usize;
        if cs_self > 0.01 {
            channels_hitting += 1;
        }
        if cs_out > 0.01 {
            channels_hitting += 1;
        }
        if cs_in > 0.01 {
            channels_hitting += 1;
        }
        if cs_out2 > 0.01 {
            channels_hitting += 1;
        }
        if cs_in2 > 0.01 {
            channels_hitting += 1;
        }
        if cs_type > 0.01 {
            channels_hitting += 1;
        }
        if cs_path > 0.01 {
            channels_hitting += 1;
        }

        let weighted_sum = W_SELF * cs_self
            + W_CALLS_OUT * cs_out
            + W_CALLS_IN * cs_in
            + W_CALLS_OUT_2HOP * cs_out2
            + W_CALLS_IN_2HOP * cs_in2
            + W_TYPE_RET * cs_type
            + W_FILE_PATH * cs_path;

        let diversity_mult = if channels_hitting >= 3 {
            1.0 + DIVERSITY_BONUS * (channels_hitting as f64 - 2.0).ln_1p()
        } else if channels_hitting >= 2 {
            1.0 + DIVERSITY_BONUS * 0.3
        } else {
            1.0
        };

        let sec_score = weighted_sum * diversity_mult;

        let kind_boost = match index.symbol_kinds[i].as_str() {
            "function" | "method" | "constructor" => 1.3,
            "class" | "struct" | "interface" | "trait" => 1.2,
            _ => 1.0,
        };

        let path_lower = index
            .file_paths
            .get(&index.symbol_file_ids[i])
            .map(|p| p.to_lowercase())
            .unwrap_or_default();
        let test_penalty = if path_lower.contains("/test")
            || path_lower.contains("_test.")
            || path_lower.contains("/benches/")
            || path_lower.contains("/spec/")
        {
            0.3
        } else {
            1.0
        };

        let final_score = sec_score * kind_boost * test_penalty;

        if final_score > 0.001 {
            scored.push((i, final_score));
        }
    }

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored.truncate(top_k);

    scored
        .into_iter()
        .map(|(i, s)| (index.symbol_ids[i], s))
        .collect()
}

pub fn sec_fusion_rerank(
    query: &str,
    bm25_candidates: &[(i64, f64)],
    sec: &SecIndex,
    inv: &SecInvertedIndex,
    top_k: usize,
) -> Vec<(i64, f64)> {
    let sec_solo = sec_standalone_search(query, sec, inv, 50);

    let mut merged: HashMap<i64, f64> = HashMap::new();
    let bm25_max = bm25_candidates
        .iter()
        .map(|(_, s)| *s)
        .fold(0.0f64, f64::max)
        .max(1e-10);
    for (id, score) in bm25_candidates {
        *merged.entry(*id).or_default() += score / bm25_max;
    }
    let sec_max = sec_solo
        .iter()
        .map(|(_, s)| *s)
        .fold(0.0f64, f64::max)
        .max(1e-10);
    for (id, score) in sec_solo {
        *merged.entry(id).or_default() += 0.5 * score / sec_max;
    }

    let mut candidates: Vec<(i64, f64)> = merged.into_iter().collect();
    candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    let cids: Vec<i64> = candidates.iter().map(|(id, _)| *id).collect();
    let cscores: Vec<f64> = candidates.iter().map(|(_, s)| *s).collect();

    sec_rerank(query, &cids, &cscores, sec, top_k)
}
