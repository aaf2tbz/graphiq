use std::collections::{HashMap, HashSet, VecDeque};

use crate::db::GraphDb;
use crate::lsa::extract_terms;
use crate::tokenize::decompose_identifier;

const TOP_K_TERMS: usize = 30;
const MAX_SEEDS: usize = 30;
const EXPANSION_BREADTH: usize = 50;
const WALK_DEPTH: usize = 3;

const ALPHA: f64 = 1.0;
const BETA: f64 = 2.5;
const GAMMA: f64 = 1.5;
const DELTA: f64 = 2.0;

const STOP: &[&str] = &[
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

#[derive(Clone)]
struct Edge {
    target: usize,
    weight: f64,
    kind_weight: f64,
}

pub struct CruncherIndex {
    pub n: usize,
    pub symbol_ids: Vec<i64>,
    pub symbol_names: Vec<String>,
    pub symbol_kinds: Vec<String>,
    pub symbol_file_ids: Vec<i64>,
    pub file_paths: HashMap<i64, String>,

    outgoing: Vec<Vec<Edge>>,
    incoming: Vec<Vec<Edge>>,

    term_sets: Vec<TermSet>,
    global_idf: HashMap<String, f64>,

    bridging: Vec<f64>,
    id_to_idx: HashMap<i64, usize>,
}

struct TermSet {
    terms: HashMap<String, f64>,
    name_terms: HashSet<String>,
    sig_terms: HashSet<String>,
}

struct QueryTerm {
    text: String,
    variants: Vec<String>,
    idf: f64,
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
                cr_tokenize(&src[..4000])
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

    let top_idf: HashMap<String, f64> = {
        let mut scored: Vec<(String, f64)> = global_idf
            .iter()
            .map(|(t, &idf)| (t.clone(), idf))
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let cutoff = if scored.len() > 500 { scored[500].1 } else { 0.0 };
        global_idf
            .iter()
            .filter(|(_, &idf)| idf >= cutoff)
            .map(|(t, &idf)| (t.clone(), idf))
            .collect()
    };

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
    })
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

fn term_match_score(query_terms: &[QueryTerm], term_set: &TermSet) -> (f64, usize) {
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

fn name_coverage(query_terms: &[QueryTerm], name_terms: &HashSet<String>) -> (f64, usize) {
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

struct Candidate {
    idx: usize,
    bm25_score: f64,
    coverage_score: f64,
    coverage_count: usize,
    name_score: f64,
    name_count: usize,
    structural_score: f64,
    structural_paths: usize,
    bridging_score: f64,
    is_seed: bool,
}

pub fn cruncher_search(
    query: &str,
    idx: &CruncherIndex,
    bm25_seeds: &[(i64, f64)],
    top_k: usize,
) -> Vec<(i64, f64)> {
    let query_terms = build_query_terms(query, &idx.global_idf);
    if query_terms.is_empty() {
        return bm25_seeds.to_vec();
    }

    let n_qt = query_terms.len();
    let idf_sum: f64 = query_terms.iter().map(|qt| qt.idf).sum();

    let mut candidates: HashMap<usize, Candidate> = HashMap::new();

    let bm25_max = bm25_seeds
        .iter()
        .map(|(_, s)| *s)
        .fold(0.0f64, f64::max)
        .max(1e-10);

    for &(id, score) in bm25_seeds.iter().take(MAX_SEEDS) {
        if let Some(&i) = idx.id_to_idx.get(&id) {
            let (cov_score, cov_count) = term_match_score(&query_terms, &idx.term_sets[i]);
            let (name_s, name_c) = name_coverage(&query_terms, &idx.term_sets[i].name_terms);
            candidates.insert(
                i,
                Candidate {
                    idx: i,
                    bm25_score: score / bm25_max,
                    coverage_score: cov_score,
                    coverage_count: cov_count,
                    name_score: name_s,
                    name_count: name_c,
                    structural_score: 0.0,
                    structural_paths: 0,
                    bridging_score: 0.0,
                    is_seed: true,
                },
            );
        }
    }

    let seed_indices: Vec<usize> = candidates.keys().cloned().collect();

    for &seed_i in &seed_indices {
        let mut queue: VecDeque<(usize, f64, usize)> = VecDeque::new();
        let mut visited: HashSet<usize> = HashSet::new();
        visited.insert(seed_i);

        for edge in &idx.outgoing[seed_i] {
            if !visited.contains(&edge.target) {
                queue.push_back((edge.target, edge.weight, 1));
                visited.insert(edge.target);
            }
        }
        for edge in &idx.incoming[seed_i] {
            if !visited.contains(&edge.target) {
                queue.push_back((edge.target, edge.weight, 1));
                visited.insert(edge.target);
            }
        }

        let mut expanded_count = 0usize;
        while let Some((neighbor_i, edge_w, depth)) = queue.pop_front() {
            if depth > WALK_DEPTH || expanded_count >= EXPANSION_BREADTH {
                break;
            }

            let proximity = 1.0 / (1.0 + depth as f64);
            let (cov_score, cov_count) = term_match_score(&query_terms, &idx.term_sets[neighbor_i]);

            if cov_count > 0 {
                let walk_signal = proximity * edge_w * cov_score;

                let entry = candidates.entry(neighbor_i).or_insert_with(|| {
                    Candidate {
                        idx: neighbor_i,
                        bm25_score: 0.0,
                        coverage_score: cov_score,
                        coverage_count: cov_count,
                        name_score: 0.0,
                        name_count: 0,
                        structural_score: 0.0,
                        structural_paths: 0,
                        bridging_score: 0.0,
                        is_seed: false,
                    }
                });

                if !entry.is_seed {
                    entry.coverage_score = entry.coverage_score.max(cov_score);
                    entry.coverage_count = entry.coverage_count.max(cov_count);
                }
                entry.structural_score += walk_signal;
                entry.structural_paths += 1;
                expanded_count += 1;

                if depth < WALK_DEPTH {
                    let edges_out = &idx.outgoing[neighbor_i];
                    let edges_in = &idx.incoming[neighbor_i];
                    let next: Vec<(usize, f64)> = edges_out
                        .iter()
                        .chain(edges_in.iter())
                        .take(10)
                        .filter(|e| !visited.contains(&e.target))
                        .map(|e| (e.target, e.weight.min(edge_w)))
                        .collect();
                    for (next_i, next_w) in next {
                        visited.insert(next_i);
                        queue.push_back((next_i, next_w, depth + 1));
                    }
                }
            }
        }
    }

    let term_seed_map: Vec<HashSet<usize>> = {
        let mut map = vec![HashSet::new(); n_qt];
        for (&i, cand) in &candidates {
            if cand.is_seed {
                for (ti, qt) in query_terms.iter().enumerate() {
                    for variant in &qt.variants {
                        if idx.term_sets[i].terms.contains_key(variant) {
                            map[ti].insert(i);
                            break;
                        }
                        for nt in idx.term_sets[i].terms.keys() {
                            if nt.contains(variant) || variant.contains(nt.as_str()) {
                                map[ti].insert(i);
                                break;
                            }
                        }
                    }
                }
            }
        }
        map
    };

    let mut scored: Vec<(usize, f64)> = candidates
        .values()
        .filter_map(|c| {
            if !c.is_seed && c.coverage_count == 0 && c.structural_paths == 0 {
                return None;
            }

            let bm25_norm = c.bm25_score;

            let coverage_norm = if n_qt > 0 {
                c.coverage_score / idf_sum
            } else {
                0.0
            };

            let coverage_frac = if n_qt > 0 {
                c.coverage_count as f64 / n_qt as f64
            } else {
                0.0
            };

            let name_norm = if idf_sum > 0.0 {
                c.name_score / idf_sum
            } else {
                0.0
            };

            let structural_norm = c.structural_score / idf_sum.max(1.0);

            let bridge = if c.structural_paths > 0 && n_qt >= 2 {
                let mut terms_covered_by_neighbors = vec![false; n_qt];
                let mut check_count = 0usize;
                for edge in idx.outgoing[c.idx].iter().take(20) {
                    for (ti, qt) in query_terms.iter().enumerate() {
                        if terms_covered_by_neighbors[ti] {
                            continue;
                        }
                        for variant in &qt.variants {
                            if idx.term_sets[edge.target].terms.contains_key(variant) {
                                terms_covered_by_neighbors[ti] = true;
                                break;
                            }
                        }
                    }
                    check_count += 1;
                    if terms_covered_by_neighbors.iter().all(|&b| b) {
                        break;
                    }
                }
                let neighbor_coverage = terms_covered_by_neighbors
                    .iter()
                    .filter(|&&b| b)
                    .count() as f64
                    / n_qt as f64;

                let mut cross_term_paths = 0usize;
                for ti in 0..n_qt {
                    for tj in (ti + 1)..n_qt {
                        if !term_seed_map[ti].is_empty() && !term_seed_map[tj].is_empty() {
                            let ti_has = term_seed_map[ti].iter().any(|&s| {
                                idx.outgoing[s]
                                    .iter()
                                    .any(|e| e.target == c.idx || idx.outgoing[e.target].iter().any(|e2| e2.target == c.idx))
                            });
                            let tj_has = term_seed_map[tj].iter().any(|&s| {
                                idx.incoming[s]
                                    .iter()
                                    .any(|e| e.target == c.idx || idx.incoming[e.target].iter().any(|e2| e2.target == c.idx))
                            });
                            if ti_has || tj_has {
                                cross_term_paths += 1;
                            }
                        }
                    }
                }

                let bridge_score = neighbor_coverage * 0.5
                    + (cross_term_paths as f64 / (n_qt * (n_qt - 1) / 2).max(1) as f64) * 0.5;
                1.0 + DELTA * bridge_score
            } else {
                1.0
            };

            let kb = kind_boost(&idx.symbol_kinds[c.idx]);
            let tp = test_penalty(&idx.file_paths, idx.symbol_file_ids[c.idx]);
            let br = 1.0 + idx.bridging[c.idx] * 1.5;

            let multi_term_bonus = if n_qt >= 2 && c.coverage_count >= 2 {
                1.0 + 0.3 * (c.coverage_count as f64 - 1.0)
            } else {
                1.0
            };

            let seed_bonus = if c.is_seed { 1.2 } else { 1.0 };

            let raw = (ALPHA * bm25_norm + BETA * coverage_norm + GAMMA * name_norm)
                * (1.0 + GAMMA * structural_norm)
                * coverage_frac.powf(0.3)
                * multi_term_bonus
                * bridge
                * kb
                * tp
                * br
                * seed_bonus;

            if raw > 0.0 {
                Some((c.idx, raw))
            } else {
                None
            }
        })
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    let mut results: Vec<(i64, f64)> = Vec::with_capacity(top_k);
    let mut file_counts: HashMap<i64, usize> = HashMap::new();

    for (i, score) in scored {
        let fid = idx.symbol_file_ids[i];
        let fc = file_counts.entry(fid).or_insert(0);
        if *fc >= 3 {
            continue;
        }
        *fc += 1;
        results.push((idx.symbol_ids[i], score));
        if results.len() >= top_k {
            break;
        }
    }

    results
}

struct V2Candidate {
    idx: usize,
    bm25_score: f64,
    energy: Vec<f64>,
    name_score: f64,
    name_count: usize,
    is_seed: bool,
    seed_paths: HashSet<usize>,
}

const ENERGY_DEPTH: usize = 3;
const ENERGY_BREADTH: usize = 50;
const ENERGY_DECAY: f64 = 0.5;
const INTERFERENCE_MIN_ENERGY: f64 = 0.1;

fn per_term_energy(query_terms: &[QueryTerm], term_set: &TermSet) -> Vec<f64> {
    let n = query_terms.len();
    let mut energy = vec![0.0f64; n];
    for (ti, qt) in query_terms.iter().enumerate() {
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
        energy[ti] = best * qt.idf;
    }
    energy
}

fn interference_score(energy: &[f64]) -> f64 {
    let k = energy.len() as f64;
    if k <= 1.0 {
        return energy.iter().sum::<f64>();
    }
    let sum: f64 = energy.iter().sum();
    let norm: f64 = energy.iter().map(|e| e * e).sum::<f64>().sqrt();
    if norm < 1e-12 {
        return 0.0;
    }
    let uniform_norm = k.sqrt();
    sum / (norm * uniform_norm)
}

fn per_term_match(term_set: &TermSet, qt: &QueryTerm) -> f64 {
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

pub fn cruncher_v2_search(
    query: &str,
    idx: &CruncherIndex,
    bm25_seeds: &[(i64, f64)],
    top_k: usize,
) -> Vec<(i64, f64)> {
    let query_terms = build_query_terms(query, &idx.global_idf);
    if query_terms.is_empty() {
        return bm25_seeds.to_vec();
    }

    let n_qt = query_terms.len();
    let idf_sum: f64 = query_terms.iter().map(|qt| qt.idf).sum();

    let bm25_max = bm25_seeds
        .iter()
        .map(|(_, s)| *s)
        .fold(0.0f64, f64::max)
        .max(1e-10);

    let bm25_second = if bm25_seeds.len() >= 2 {
        bm25_seeds[1].1
    } else {
        0.0
    };
    let bm25_confident =
        !bm25_seeds.is_empty() && bm25_second > 0.0 && bm25_seeds[0].1 / bm25_second > 1.2;

    let mut candidates: HashMap<usize, V2Candidate> = HashMap::new();

    for &(id, score) in bm25_seeds.iter().take(MAX_SEEDS) {
        if let Some(&i) = idx.id_to_idx.get(&id) {
            let energy = per_term_energy(&query_terms, &idx.term_sets[i]);
            let (name_s, name_c) = name_coverage(&query_terms, &idx.term_sets[i].name_terms);
            let mut sp = HashSet::new();
            sp.insert(i);
            candidates.insert(
                i,
                V2Candidate {
                    idx: i,
                    bm25_score: score / bm25_max,
                    energy,
                    name_score: name_s,
                    name_count: name_c,
                    is_seed: true,
                    seed_paths: sp,
                },
            );
        }
    }

    let confident_idx = if bm25_confident {
        bm25_seeds
            .first()
            .and_then(|(id, _)| idx.id_to_idx.get(id))
            .copied()
    } else {
        None
    };

    let seed_indices: Vec<usize> = candidates.keys().cloned().collect();

    for &seed_i in &seed_indices {
        let mut queue: VecDeque<(usize, f64, usize, usize)> = VecDeque::new();
        let mut visited: HashSet<usize> = HashSet::new();
        visited.insert(seed_i);

        for edge in &idx.outgoing[seed_i] {
            if !visited.contains(&edge.target) {
                queue.push_back((edge.target, edge.weight, 1, seed_i));
                visited.insert(edge.target);
            }
        }
        for edge in &idx.incoming[seed_i] {
            if !visited.contains(&edge.target) {
                queue.push_back((edge.target, edge.weight, 1, seed_i));
                visited.insert(edge.target);
            }
        }

        let mut expanded_count = 0usize;
        while let Some((neighbor_i, edge_w, depth, origin_seed)) = queue.pop_front() {
            if depth > ENERGY_DEPTH || expanded_count >= ENERGY_BREADTH {
                break;
            }

            let proximity = ENERGY_DECAY.powi(depth as i32);

            let mut any_match = false;
            for (ti, qt) in query_terms.iter().enumerate() {
                let term_e = per_term_match(&idx.term_sets[neighbor_i], qt);
                if term_e > 0.0 {
                    any_match = true;
                    let propagated = term_e * proximity * edge_w;

                    let entry = candidates.entry(neighbor_i).or_insert_with(|| V2Candidate {
                        idx: neighbor_i,
                        bm25_score: 0.0,
                        energy: vec![0.0; n_qt],
                        name_score: 0.0,
                        name_count: 0,
                        is_seed: false,
                        seed_paths: HashSet::new(),
                    });

                    entry.energy[ti] += propagated;
                    entry.seed_paths.insert(origin_seed);

                    if !entry.is_seed {
                        let (name_s, _name_c) =
                            name_coverage(&query_terms, &idx.term_sets[neighbor_i].name_terms);
                        if name_s > entry.name_score {
                            entry.name_score = name_s;
                        }
                    }
                }
            }

            if any_match {
                expanded_count += 1;

                if depth < ENERGY_DEPTH {
                    let edges_out = &idx.outgoing[neighbor_i];
                    let edges_in = &idx.incoming[neighbor_i];
                    let next: Vec<(usize, f64)> = edges_out
                        .iter()
                        .chain(edges_in.iter())
                        .take(8)
                        .filter(|e| !visited.contains(&e.target))
                        .map(|e| (e.target, e.weight * edge_w))
                        .collect();
                    for (next_i, next_w) in next {
                        visited.insert(next_i);
                        queue.push_back((next_i, next_w, depth + 1, origin_seed));
                    }
                }
            }
        }
    }

    let seed_energy_max: f64 = candidates
        .values()
        .filter(|c| c.is_seed)
        .map(|c| c.energy.iter().sum::<f64>())
        .fold(0.0f64, f64::max)
        .max(1e-10);

    let mut scored: Vec<(usize, f64)> = candidates
        .values()
        .filter_map(|c| {
            let energy_sum: f64 = c.energy.iter().sum();
            if !c.is_seed && energy_sum < 1e-12 {
                return None;
            }

            let bm25_norm = c.bm25_score;

            let (cov_score, cov_count) = term_match_score(&query_terms, &idx.term_sets[c.idx]);
            let coverage_norm = if n_qt > 0 { cov_score / idf_sum } else { 0.0 };
            let coverage_frac = if n_qt > 0 {
                c.energy.iter().filter(|&&e| e > 0.0).count() as f64 / n_qt as f64
            } else {
                0.0
            };

            let name_norm = if idf_sum > 0.0 {
                c.name_score / idf_sum
            } else {
                0.0
            };

            let interference = if energy_sum > INTERFERENCE_MIN_ENERGY * seed_energy_max {
                interference_score(&c.energy)
            } else {
                0.0
            };

            let structural_norm = if c.is_seed {
                interference * idf_sum.max(1.0)
            } else {
                let rel_energy = energy_sum / seed_energy_max;
                if rel_energy > 0.05 && interference > 0.0 {
                    rel_energy * interference * idf_sum.max(1.0)
                } else {
                    return None;
                }
            };

            let kb = kind_boost(&idx.symbol_kinds[c.idx]);
            let tp = test_penalty(&idx.file_paths, idx.symbol_file_ids[c.idx]);
            let br = 1.0 + idx.bridging[c.idx] * 1.5;
            let seed_bonus = if c.is_seed { 1.2 } else { 1.0 };

            let yoyo = if !c.is_seed && c.seed_paths.len() < 2 {
                0.5
            } else if !c.is_seed {
                1.0 + 0.1 * (c.seed_paths.len().min(5) - 2) as f64
            } else {
                1.0
            };

            let out_set: HashSet<usize> =
                idx.outgoing[c.idx].iter().map(|e| e.target).collect();
            let in_set: HashSet<usize> =
                idx.incoming[c.idx].iter().map(|e| e.target).collect();
            let overlap = out_set.intersection(&in_set).count() as f64;
            let max_deg = (out_set.len().max(in_set.len()) as f64).max(1.0);
            let hub_score = overlap / max_deg;
            let hub_dampen = 1.0 / (1.0 + hub_score * hub_score);

            let multi_term_bonus = if n_qt >= 2 && cov_count >= 2 {
                1.0 + 0.3 * (cov_count as f64 - 1.0)
            } else {
                1.0
            };

            let raw = (ALPHA * bm25_norm + BETA * coverage_norm + GAMMA * name_norm)
                * (1.0 + GAMMA * structural_norm)
                * coverage_frac.powf(0.3)
                * multi_term_bonus
                * kb
                * tp
                * br
                * seed_bonus
                * yoyo
                * hub_dampen;

            if raw > 0.0 {
                Some((c.idx, raw))
            } else {
                None
            }
        })
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    if let Some(lock_idx) = confident_idx {
        let lock_pos = scored.iter().position(|(i, _)| *i == lock_idx);
        if let Some(pos) = lock_pos {
            if pos > 0 {
                let (locked_i, locked_s) = scored.remove(pos);
                scored.insert(0, (locked_i, locked_s + 1e6));
            }
        }
    }

    let mut results: Vec<(i64, f64)> = Vec::with_capacity(top_k);
    let mut file_counts: HashMap<i64, usize> = HashMap::new();

    for (i, score) in scored {
        let fid = idx.symbol_file_ids[i];
        let fc = file_counts.entry(fid).or_insert(0);
        if *fc >= 3 {
            continue;
        }
        *fc += 1;
        results.push((idx.symbol_ids[i], score));
        if results.len() >= top_k {
            break;
        }
    }

    results
}

pub fn cruncher_search_standalone(
    query: &str,
    idx: &CruncherIndex,
    top_k: usize,
) -> Vec<(i64, f64)> {
    let query_terms = build_query_terms(query, &idx.global_idf);
    if query_terms.is_empty() {
        return Vec::new();
    }

    let n_qt = query_terms.len();
    let idf_sum: f64 = query_terms.iter().map(|qt| qt.idf).sum();

    let mut scored: Vec<(usize, f64)> = Vec::with_capacity(idx.n);

    for i in 0..idx.n {
        let (cov_score, cov_count) = term_match_score(&query_terms, &idx.term_sets[i]);
        let (name_s, _name_c) = name_coverage(&query_terms, &idx.term_sets[i].name_terms);

        if cov_count == 0 && name_s == 0.0 {
            continue;
        }

        let coverage_frac = if n_qt > 0 {
            cov_count as f64 / n_qt as f64
        } else {
            0.0
        };

        let coverage_norm = cov_score / idf_sum.max(1.0);
        let name_norm = name_s / idf_sum.max(1.0);

        let multi_term_bonus = if n_qt >= 2 && cov_count >= 2 {
            1.0 + 0.3 * (cov_count as f64 - 1.0)
        } else {
            1.0
        };

        let kb = kind_boost(&idx.symbol_kinds[i]);
        let tp = test_penalty(&idx.file_paths, idx.symbol_file_ids[i]);
        let br = 1.0 + idx.bridging[i] * 1.5;

        let raw = (BETA * coverage_norm + GAMMA * name_norm)
            * coverage_frac.powf(0.3)
            * multi_term_bonus
            * kb
            * tp
            * br;

        if raw > 0.001 {
            scored.push((i, raw));
        }
    }

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    let mut results: Vec<(i64, f64)> = Vec::with_capacity(top_k);
    let mut file_counts: HashMap<i64, usize> = HashMap::new();

    for (i, score) in scored {
        let fid = idx.symbol_file_ids[i];
        let fc = file_counts.entry(fid).or_insert(0);
        if *fc >= 3 {
            continue;
        }
        *fc += 1;
        results.push((idx.symbol_ids[i], score));
        if results.len() >= top_k {
            break;
        }
    }

    results
}
