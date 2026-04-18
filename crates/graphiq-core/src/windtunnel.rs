use std::collections::{HashMap, HashSet};

use crate::db::GraphDb;
use crate::lsa::extract_terms;
use crate::tokenize::decompose_identifier;

const HASH_BITS: usize = 128;
const N_CHANNELS: usize = 7;
const TOP_K_TERMS: usize = 50;

const CH_SELF: usize = 0;
const CH_CALLS_OUT: usize = 1;
const CH_CALLS_IN: usize = 2;
const CH_OUT_2HOP: usize = 3;
const CH_IN_2HOP: usize = 4;
const CH_TYPE_RET: usize = 5;
const CH_FILE_PATH: usize = 6;

const CHANNEL_WEIGHTS: [f64; N_CHANNELS] = [3.0, 1.5, 1.5, 0.7, 0.7, 1.0, 0.5];
const DIVERSITY_BONUS: f64 = 2.5;

const STOP_WORDS: &[&str] = &[
    "the", "a", "an", "is", "are", "was", "were", "be", "been", "being", "have", "has", "had",
    "do", "does", "did", "will", "would", "could", "should", "may", "might", "can", "shall", "to",
    "of", "in", "for", "on", "with", "at", "by", "from", "as", "into", "through", "and", "or",
    "but", "not", "that", "this", "these", "those", "it", "its", "if", "then", "than", "so", "up",
    "out", "new", "all", "every", "how", "what", "where", "when", "why", "which", "who", "whom",
    "there", "here", "no", "nor", "just", "very", "also", "some", "any", "each", "both", "few",
    "more", "most", "other", "such", "only", "own", "same",
];

type SimHash = [u64; 2];

fn fnv1a_64(data: &[u8], offset: u64) -> u64 {
    let mut h = offset;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn term_hash(term: &str) -> SimHash {
    let bytes = term.as_bytes();
    [
        fnv1a_64(bytes, 0xcbf29ce484222325),
        fnv1a_64(bytes, 0x9e3779b97f4a7c15),
    ]
}

fn simhash_weighted_top(terms: &[(String, f64)]) -> SimHash {
    let mut acc = [0.0f64; HASH_BITS];
    for (term, weight) in terms {
        let h = term_hash(term);
        for bit in 0..64 {
            if (h[0] >> bit) & 1 == 1 {
                acc[bit] += weight;
            } else {
                acc[bit] -= weight;
            }
            if (h[1] >> bit) & 1 == 1 {
                acc[64 + bit] += weight;
            } else {
                acc[64 + bit] -= weight;
            }
        }
    }
    let mut result = [0u64; 2];
    for bit in 0..64 {
        if acc[bit] > 0.0 {
            result[0] |= 1u64 << bit;
        }
        if acc[64 + bit] > 0.0 {
            result[1] |= 1u64 << bit;
        }
    }
    result
}

fn hamming_similarity(a: &SimHash, b: &SimHash) -> f64 {
    let diff_bits = (a[0] ^ b[0]).count_ones() + (a[1] ^ b[1]).count_ones();
    1.0 - (diff_bits as f64 / HASH_BITS as f64)
}

fn wt_tokenize(text: &str) -> Vec<String> {
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

fn top_k_idf_terms(
    bag: &HashMap<String, f64>,
    idf: &HashMap<String, f64>,
    k: usize,
) -> Vec<(String, f64)> {
    let mut scored: Vec<(String, f64)> = bag
        .iter()
        .map(|(term, &tf)| {
            let idf_val = idf.get(term).copied().unwrap_or(1.0);
            (term.clone(), tf * idf_val)
        })
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored.truncate(k);
    scored
}

pub struct WindtunnelIndex {
    pub n: usize,
    pub symbol_ids: Vec<i64>,
    pub symbol_names: Vec<String>,
    pub symbol_kinds: Vec<String>,
    pub symbol_file_ids: Vec<i64>,
    pub file_paths: HashMap<i64, String>,

    channel_hashes: Vec<[SimHash; N_CHANNELS]>,
    bridging: Vec<f64>,
    global_idf: HashMap<String, f64>,

    channel_bags: [Vec<HashMap<String, f64>>; N_CHANNELS],
    outgoing: Vec<Vec<usize>>,
    incoming: Vec<Vec<usize>>,
    id_to_idx: HashMap<i64, usize>,
}

pub fn build_windtunnel_index(db: &GraphDb) -> Result<WindtunnelIndex, String> {
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

    eprintln!("  Windtunnel: building self channels...");
    let ch_self: Vec<HashMap<String, f64>> = (0..n)
        .map(|i| {
            let name_terms = wt_tokenize(&symbol_names[i]);
            let kind_terms = wt_tokenize(&symbol_kinds[i]);
            let sig_terms = match &symbol_sigs[i] {
                Some(sig) => wt_tokenize(sig),
                None => Vec::new(),
            };
            let source_terms = {
                let src = &symbol_sources[i];
                if src.len() > 8000 {
                    wt_tokenize(&src[..8000])
                } else {
                    wt_tokenize(src)
                }
            };
            let doc_terms = match &symbol_docs[i] {
                Some(doc) => wt_tokenize(doc),
                None => Vec::new(),
            };
            let hint_terms = wt_tokenize(&symbol_hints[i]);
            let mut all = name_terms;
            all.extend(kind_terms);
            all.extend(sig_terms);
            all.extend(source_terms);
            all.extend(doc_terms);
            all.extend(hint_terms);
            term_bag(&all)
        })
        .collect();

    eprintln!("  Windtunnel: propagating 1-hop outgoing...");
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

    eprintln!("  Windtunnel: propagating 1-hop incoming...");
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

    eprintln!("  Windtunnel: propagating 2-hop outgoing...");
    let ch_out_2hop: Vec<HashMap<String, f64>> = (0..n)
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

    eprintln!("  Windtunnel: propagating 2-hop incoming...");
    let ch_in_2hop: Vec<HashMap<String, f64>> = (0..n)
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

    eprintln!("  Windtunnel: extracting type_ret channels...");
    let ch_type_ret: Vec<HashMap<String, f64>> = (0..n)
        .map(|i| {
            match &symbol_sigs[i] {
                Some(sig) => {
                    let sig_lower = sig.to_lowercase();
                    let mut type_terms: Vec<String> = Vec::new();
                    for chunk in sig_lower.split(
                        &[':', '-', '<', '>', '(', ')', '[', ']', '{', '}', ',', '=', '+', '|', '&'][..],
                    ) {
                        let tokens = wt_tokenize(chunk);
                        type_terms.extend(tokens);
                    }
                    type_terms.sort_unstable();
                    type_terms.dedup();
                    term_bag(&type_terms)
                }
                None => HashMap::new(),
            }
        })
        .collect();

    eprintln!("  Windtunnel: building file_path channels...");
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

    eprintln!("  Windtunnel: computing global IDF...");
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

    eprintln!("  Windtunnel: computing IDF-weighted SimHash per channel...");
    let all_channels: [Vec<HashMap<String, f64>>; N_CHANNELS] = [
        ch_self, ch_calls_out, ch_calls_in, ch_out_2hop, ch_in_2hop, ch_type_ret, ch_file_path,
    ];

    let channel_hashes: Vec<[SimHash; N_CHANNELS]> = (0..n)
        .map(|i| {
            let mut hashes = [[0u64; 2]; N_CHANNELS];
            for c in 0..N_CHANNELS {
                let top_terms = top_k_idf_terms(&all_channels[c][i], &global_idf, TOP_K_TERMS);
                hashes[c] = simhash_weighted_top(&top_terms);
            }
            hashes
        })
        .collect();

    eprintln!("  Windtunnel: computing bridging potential...");
    let bridging: Vec<f64> = (0..n)
        .map(|i| {
            if all_channels[CH_CALLS_OUT][i].is_empty() {
                return 0.0;
            }
            let self_terms: HashSet<&str> =
                all_channels[CH_SELF][i].keys().map(|s| s.as_str()).collect();
            let out_terms: HashSet<&str> =
                all_channels[CH_CALLS_OUT][i].keys().map(|s| s.as_str()).collect();
            let novel = out_terms.iter().filter(|t| !self_terms.contains(*t)).count();
            let total = out_terms.len().max(1);
            (novel as f64 / total as f64) * (1.0 + (outgoing[i].len() as f64).ln_1p() * 0.3)
        })
        .collect();

    eprintln!(
        "  Windtunnel: done ({} symbols, {} bits/channel, top-{} IDF terms)",
        n, HASH_BITS, TOP_K_TERMS
    );

    Ok(WindtunnelIndex {
        n,
        symbol_ids,
        symbol_names,
        symbol_kinds,
        symbol_file_ids,
        file_paths,
        channel_hashes,
        bridging,
        global_idf,
        channel_bags: all_channels,
        outgoing,
        incoming,
        id_to_idx,
    })
}

struct QueryTerm {
    text: String,
    variants: Vec<String>,
    idf: f64,
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

fn build_query_terms(query: &str, idf: &HashMap<String, f64>) -> Vec<QueryTerm> {
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

fn compute_query_hash(query_terms: &[QueryTerm], _idf: &HashMap<String, f64>) -> SimHash {
    let mut merged: Vec<(String, f64)> = Vec::new();
    for qt in query_terms {
        let idf_val = qt.idf;
        for variant in &qt.variants {
            merged.push((variant.clone(), idf_val));
        }
    }
    if merged.is_empty() {
        return [0u64; 2];
    }
    let mut deduped: HashMap<String, f64> = HashMap::new();
    for (term, weight) in merged {
        let entry = deduped.entry(term).or_insert(0.0);
        *entry = (*entry).max(weight);
    }
    let mut top: Vec<(String, f64)> = deduped.into_iter().collect();
    top.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    top.truncate(TOP_K_TERMS);
    simhash_weighted_top(&top)
}

fn kind_boost(kind: &str) -> f64 {
    match kind {
        "function" | "method" | "constructor" => 1.3,
        "class" | "struct" | "interface" | "trait" => 1.2,
        "enum" | "type_alias" => 1.1,
        "constant" | "field" | "property" => 0.9,
        "module" | "section" => 0.7,
        _ => 1.0,
    }
}

fn test_penalty(file_paths: &HashMap<i64, String>, file_id: i64) -> f64 {
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

fn score_symbol(
    i: usize,
    query_hash: &SimHash,
    query_terms: &[QueryTerm],
    idx: &WindtunnelIndex,
) -> f64 {
    let mut channel_sims = [0.0f64; N_CHANNELS];

    channel_sims[CH_SELF] = hamming_similarity(query_hash, &idx.channel_hashes[i][CH_SELF]);
    channel_sims[CH_CALLS_OUT] = hamming_similarity(query_hash, &idx.channel_hashes[i][CH_CALLS_OUT]);
    channel_sims[CH_CALLS_IN] = hamming_similarity(query_hash, &idx.channel_hashes[i][CH_CALLS_IN]);
    channel_sims[CH_OUT_2HOP] = hamming_similarity(query_hash, &idx.channel_hashes[i][CH_OUT_2HOP]);
    channel_sims[CH_IN_2HOP] = hamming_similarity(query_hash, &idx.channel_hashes[i][CH_IN_2HOP]);
    channel_sims[CH_TYPE_RET] = hamming_similarity(query_hash, &idx.channel_hashes[i][CH_TYPE_RET]);
    channel_sims[CH_FILE_PATH] = hamming_similarity(query_hash, &idx.channel_hashes[i][CH_FILE_PATH]);

    for c in 0..N_CHANNELS {
        channel_sims[c] = (channel_sims[c] - 0.5).max(0.0) * 2.0;
    }

    let max_sim = channel_sims.iter().cloned().fold(0.0f64, f64::max);
    if max_sim < 0.01 {
        return 0.0;
    }

    let sq_sum: f64 = channel_sims.iter().map(|s| s * s).sum();
    if sq_sum < 1e-10 {
        return 0.0;
    }

    let adaptive: [f64; N_CHANNELS] = {
        let mut w = [0.0f64; N_CHANNELS];
        for c in 0..N_CHANNELS {
            w[c] = (channel_sims[c] * channel_sims[c]) / sq_sum;
        }
        w
    };

    let mut channels_hitting = 0usize;
    for c in 0..N_CHANNELS {
        if channel_sims[c] > 0.01 {
            channels_hitting += 1;
        }
    }

    let simhash_score: f64 = (0..N_CHANNELS)
        .map(|c| adaptive[c] * CHANNEL_WEIGHTS[c] * channel_sims[c])
        .sum();

    let overlap_self = channel_overlap(query_terms, &idx.channel_bags[CH_SELF][i]);
    let overlap_out = channel_overlap(query_terms, &idx.channel_bags[CH_CALLS_OUT][i]);
    let overlap_in = channel_overlap(query_terms, &idx.channel_bags[CH_CALLS_IN][i]);
    let overlap_type = channel_overlap(query_terms, &idx.channel_bags[CH_TYPE_RET][i]);

    let overlap_score = 3.0 * overlap_self + 1.5 * overlap_out + 1.5 * overlap_in + 1.0 * overlap_type;

    let diversity_mult = if channels_hitting >= 3 {
        1.0 + DIVERSITY_BONUS * (channels_hitting as f64 - 2.0).ln_1p()
    } else if channels_hitting >= 2 {
        1.0 + DIVERSITY_BONUS * 0.3
    } else {
        1.0
    };

    let kb = kind_boost(&idx.symbol_kinds[i]);
    let tp = test_penalty(&idx.file_paths, idx.symbol_file_ids[i]);
    let bridge = 1.0 + idx.bridging[i] * 1.5;

    (simhash_score * 0.6 + overlap_score * 0.4) * diversity_mult * kb * tp * bridge
}

pub fn windtunnel_search(query: &str, idx: &WindtunnelIndex, top_k: usize) -> Vec<(i64, f64)> {
    let query_terms = build_query_terms(query, &idx.global_idf);
    if query_terms.is_empty() {
        return Vec::new();
    }

    let query_hash = compute_query_hash(&query_terms, &idx.global_idf);

    let mut scored: Vec<(usize, f64)> = (0..idx.n)
        .filter_map(|i| {
            let s = score_symbol(i, &query_hash, &query_terms, idx);
            if s > 0.001 {
                Some((i, s))
            } else {
                None
            }
        })
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored.truncate(top_k);

    scored
        .into_iter()
        .map(|(i, s)| (idx.symbol_ids[i], s))
        .collect()
}

pub fn windtunnel_rerank(
    query: &str,
    candidate_ids: &[i64],
    candidate_scores: &[f64],
    idx: &WindtunnelIndex,
    top_k: usize,
) -> Vec<(i64, f64)> {
    let query_terms = build_query_terms(query, &idx.global_idf);
    if query_terms.is_empty() {
        return candidate_ids
            .iter()
            .zip(candidate_scores.iter())
            .filter(|(_, &s)| s > 0.0)
            .take(top_k)
            .map(|(&id, &s)| (id, s))
            .collect();
    }

    let query_hash = compute_query_hash(&query_terms, &idx.global_idf);

    let mut scored: Vec<(i64, f64)> = Vec::with_capacity(candidate_ids.len());

    for (i, &id) in candidate_ids.iter().enumerate() {
        let base = candidate_scores.get(i).copied().unwrap_or(0.0);
        if base <= 0.0 {
            continue;
        }

        if let Some(&sym_idx) = idx.id_to_idx.get(&id) {
            let wt_score = score_symbol(sym_idx, &query_hash, &query_terms, idx);
            let has_self_hit = channel_overlap(&query_terms, &idx.channel_bags[CH_SELF][sym_idx]) > 0.1;

            let final_score = if has_self_hit {
                base * (1.0 + wt_score * 2.0)
            } else {
                base * (1.0 + wt_score * 1.0)
            };
            scored.push((id, final_score));
        } else {
            scored.push((id, base));
        }
    }

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored.truncate(top_k);
    scored
}

pub fn windtunnel_fusion(
    query: &str,
    bm25_candidates: &[(i64, f64)],
    idx: &WindtunnelIndex,
    top_k: usize,
) -> Vec<(i64, f64)> {
    let wt_solo = windtunnel_search(query, idx, 50);

    let mut merged: HashMap<i64, f64> = HashMap::new();

    let bm25_max = bm25_candidates
        .iter()
        .map(|(_, s)| *s)
        .fold(0.0f64, f64::max)
        .max(1e-10);
    for (id, score) in bm25_candidates {
        *merged.entry(*id).or_default() += score / bm25_max;
    }

    let wt_max = wt_solo
        .iter()
        .map(|(_, s)| *s)
        .fold(0.0f64, f64::max)
        .max(1e-10);
    for (id, score) in wt_solo {
        *merged.entry(id).or_default() += 0.5 * score / wt_max;
    }

    let mut candidates: Vec<(i64, f64)> = merged.into_iter().collect();
    candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    let cids: Vec<i64> = candidates.iter().map(|(id, _)| *id).collect();
    let cscores: Vec<f64> = candidates.iter().map(|(_, s)| *s).collect();

    windtunnel_rerank(query, &cids, &cscores, idx, top_k)
}
