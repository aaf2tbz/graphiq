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

struct GooberCandidate {
    idx: usize,
    bm25_score: f64,
    coverage_score: f64,
    coverage_count: usize,
    name_score: f64,
    is_seed: bool,
    walk_evidence: f64,
    seed_paths: HashSet<usize>,
}

pub fn goober_search(
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

    let mut candidates: HashMap<usize, GooberCandidate> = HashMap::new();

    for &(id, score) in bm25_seeds.iter().take(30) {
        if let Some(&i) = idx.id_to_idx.get(&id) {
            let (cov_score, cov_count) = term_match_score(&query_terms, &idx.term_sets[i]);
            let (name_s, _) = name_coverage(&query_terms, &idx.term_sets[i].name_terms);

            let mut sp = HashSet::new();
            sp.insert(i);

            candidates.insert(
                i,
                GooberCandidate {
                    idx: i,
                    bm25_score: score / bm25_max,
                    coverage_score: cov_score,
                    coverage_count: cov_count,
                    name_score: name_s,
                    is_seed: true,
                    walk_evidence: 0.0,
                    seed_paths: sp,
                },
            );
        }
    }

    let mut idf_sorted: Vec<f64> = query_terms.iter().map(|qt| qt.idf).collect();
    idf_sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let idf_threshold = idf_sorted[idf_sorted.len() / 2];

    let seed_indices: Vec<usize> = candidates.keys().cloned().collect();

    for &seed_i in seed_indices.iter().take(8) {
        let mut queue: VecDeque<(usize, f64, usize)> = VecDeque::new();
        let mut visited: HashSet<usize> = HashSet::new();
        visited.insert(seed_i);

        for edge in idx.outgoing[seed_i].iter().take(10) {
            if !visited.contains(&edge.target) {
                queue.push_back((edge.target, edge.weight, 1));
                visited.insert(edge.target);
            }
        }
        for edge in idx.incoming[seed_i].iter().take(10) {
            if !visited.contains(&edge.target) {
                queue.push_back((edge.target, edge.weight, 1));
                visited.insert(edge.target);
            }
        }

        let mut expanded = 0usize;
        while let Some((neighbor_i, edge_w, depth)) = queue.pop_front() {
            if depth > 2 || expanded >= 25 {
                break;
            }

            let has_specific = query_terms
                .iter()
                .filter(|qt| qt.idf >= idf_threshold)
                .any(|qt| per_term_match(&idx.term_sets[neighbor_i], qt) > 0.0);

            if !has_specific {
                continue;
            }

            let (cov_score, cov_count) =
                term_match_score(&query_terms, &idx.term_sets[neighbor_i]);
            if cov_count == 0 {
                continue;
            }

            let proximity = 0.5_f64.powi(depth as i32);
            let evidence = cov_score * proximity * edge_w;

            let entry = candidates.entry(neighbor_i).or_insert_with(|| {
                let (ns, _) = name_coverage(&query_terms, &idx.term_sets[neighbor_i].name_terms);
                GooberCandidate {
                    idx: neighbor_i,
                    bm25_score: 0.0,
                    coverage_score: cov_score,
                    coverage_count: cov_count,
                    name_score: ns,
                    is_seed: false,
                    walk_evidence: 0.0,
                    seed_paths: HashSet::new(),
                }
            });

            if !entry.is_seed {
                entry.coverage_score = entry.coverage_score.max(cov_score);
                entry.coverage_count = entry.coverage_count.max(cov_count);
            }
            entry.walk_evidence += evidence;
            entry.seed_paths.insert(seed_i);
            expanded += 1;

            if depth < 2 {
                let next: Vec<(usize, f64)> = idx.outgoing[neighbor_i]
                    .iter()
                    .chain(idx.incoming[neighbor_i].iter())
                    .take(6)
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

    let mut scored: Vec<(usize, f64)> = candidates
        .values()
        .filter_map(|c| {
            if !c.is_seed && c.seed_paths.len() < 2 {
                return None;
            }

            let cov_norm = if idf_sum > 0.0 {
                c.coverage_score / idf_sum
            } else {
                0.0
            };
            let name_norm = if idf_sum > 0.0 {
                c.name_score / idf_sum
            } else {
                0.0
            };
            let walk_norm = if idf_sum > 0.0 {
                c.walk_evidence / idf_sum
            } else {
                0.0
            };

            let base = if c.is_seed {
                3.0 * c.bm25_score + 1.5 * cov_norm.min(0.5) + 2.0 * name_norm.min(0.5)
            } else {
                1.5 * cov_norm + 2.0 * name_norm + walk_norm
            };

            let coverage_frac = if n_qt > 0 {
                c.coverage_count as f64 / n_qt as f64
            } else {
                0.0
            };

            let seed_bonus = if c.is_seed { 1.15 } else { 1.0 };
            let kb = kind_boost(&idx.symbol_kinds[c.idx]);
            let tp = test_penalty(&idx.file_paths, idx.symbol_file_ids[c.idx]);

            let raw = base * coverage_frac.powf(0.3) * seed_bonus * kb * tp;

            if raw > 0.0 {
                Some((c.idx, raw))
            } else {
                None
            }
        })
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    let bm25_confident = bm25_seeds.len() >= 2
        && bm25_seeds[0].1 / bm25_seeds[1].1.max(1e-10) > 1.2;
    if bm25_confident {
        if let Some(&lock_i) = bm25_seeds
            .first()
            .and_then(|(id, _)| idx.id_to_idx.get(id))
        {
            if let Some(pos) = scored.iter().position(|(i, _)| *i == lock_i) {
                if pos > 0 {
                    let (li, ls) = scored.remove(pos);
                    scored.insert(0, (li, ls + 1e6));
                }
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

#[derive(Clone)]
struct SecChannelVec {
    ch_self: f64,
    ch_name: f64,
    ch_sig: f64,
    ch_out_1hop: f64,
    ch_in_1hop: f64,
    ch_out_2hop: f64,
    ch_in_2hop: f64,
}

fn compute_sec_channels(
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

fn negentropy(channels: &SecChannelVec) -> f64 {
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

fn channel_coherence(
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

struct GooberV3Candidate {
    idx: usize,
    bm25_score: f64,
    coverage_score: f64,
    coverage_count: usize,
    name_score: f64,
    is_seed: bool,
    walk_evidence: f64,
    seed_paths: HashSet<usize>,
    ng_score: f64,
    coherence_score: f64,
}

pub fn goober_v3_search(
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

    let mut candidates: HashMap<usize, GooberV3Candidate> = HashMap::new();

    for &(id, score) in bm25_seeds.iter().take(30) {
        if let Some(&i) = idx.id_to_idx.get(&id) {
            let (cov_score, cov_count) = term_match_score(&query_terms, &idx.term_sets[i]);
            let (name_s, _) = name_coverage(&query_terms, &idx.term_sets[i].name_terms);

            let channels = compute_sec_channels(&query_terms, idx, i);
            let ng = negentropy(&channels);
            let coherence = channel_coherence(&query_terms, idx, i);

            let mut sp = HashSet::new();
            sp.insert(i);

            candidates.insert(
                i,
                GooberV3Candidate {
                    idx: i,
                    bm25_score: score / bm25_max,
                    coverage_score: cov_score,
                    coverage_count: cov_count,
                    name_score: name_s,
                    is_seed: true,
                    walk_evidence: 0.0,
                    seed_paths: sp,
                    ng_score: ng,
                    coherence_score: coherence,
                },
            );
        }
    }

    let mut idf_sorted: Vec<f64> = query_terms.iter().map(|qt| qt.idf).collect();
    idf_sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let idf_threshold = idf_sorted[idf_sorted.len() / 2];

    let seed_indices: Vec<usize> = candidates.keys().cloned().collect();

    for &seed_i in seed_indices.iter().take(8) {
        let mut queue: VecDeque<(usize, f64, usize)> = VecDeque::new();
        let mut visited: HashSet<usize> = HashSet::new();
        visited.insert(seed_i);

        for edge in idx.outgoing[seed_i].iter().take(10) {
            if !visited.contains(&edge.target) {
                queue.push_back((edge.target, edge.weight, 1));
                visited.insert(edge.target);
            }
        }
        for edge in idx.incoming[seed_i].iter().take(10) {
            if !visited.contains(&edge.target) {
                queue.push_back((edge.target, edge.weight, 1));
                visited.insert(edge.target);
            }
        }

        let mut expanded = 0usize;
        while let Some((neighbor_i, edge_w, depth)) = queue.pop_front() {
            if depth > 2 || expanded >= 25 {
                break;
            }

            let has_specific = query_terms
                .iter()
                .filter(|qt| qt.idf >= idf_threshold)
                .any(|qt| per_term_match(&idx.term_sets[neighbor_i], qt) > 0.0);

            if !has_specific {
                continue;
            }

            let (cov_score, cov_count) =
                term_match_score(&query_terms, &idx.term_sets[neighbor_i]);
            if cov_count == 0 {
                continue;
            }

            let proximity = 0.5_f64.powi(depth as i32);
            let evidence = cov_score * proximity * edge_w;

            let channels = compute_sec_channels(&query_terms, idx, neighbor_i);
            let ng = negentropy(&channels);
            let coherence = channel_coherence(&query_terms, idx, neighbor_i);

            let entry = candidates.entry(neighbor_i).or_insert_with(|| {
                let (ns, _) = name_coverage(&query_terms, &idx.term_sets[neighbor_i].name_terms);
                GooberV3Candidate {
                    idx: neighbor_i,
                    bm25_score: 0.0,
                    coverage_score: cov_score,
                    coverage_count: cov_count,
                    name_score: ns,
                    is_seed: false,
                    walk_evidence: 0.0,
                    seed_paths: HashSet::new(),
                    ng_score: ng,
                    coherence_score: coherence,
                }
            });

            if !entry.is_seed {
                entry.coverage_score = entry.coverage_score.max(cov_score);
                entry.coverage_count = entry.coverage_count.max(cov_count);
                entry.ng_score = entry.ng_score.max(ng);
                entry.coherence_score = entry.coherence_score.max(coherence);
            }
            entry.walk_evidence += evidence;
            entry.seed_paths.insert(seed_i);
            expanded += 1;

            if depth < 2 {
                let next: Vec<(usize, f64)> = idx.outgoing[neighbor_i]
                    .iter()
                    .chain(idx.incoming[neighbor_i].iter())
                    .take(6)
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

    let max_ng = candidates
        .values()
        .map(|c| c.ng_score)
        .fold(0.0f64, f64::max)
        .max(1e-10);

    let max_coherence = candidates
        .values()
        .map(|c| c.coherence_score)
        .fold(0.0f64, f64::max)
        .max(1e-10);

    let mut scored: Vec<(usize, f64)> = candidates
        .values()
        .filter_map(|c| {
            if !c.is_seed && c.seed_paths.len() < 2 {
                return None;
            }

            let cov_norm = if idf_sum > 0.0 {
                c.coverage_score / idf_sum
            } else {
                0.0
            };
            let name_norm = if idf_sum > 0.0 {
                c.name_score / idf_sum
            } else {
                0.0
            };
            let walk_norm = if idf_sum > 0.0 {
                c.walk_evidence / idf_sum
            } else {
                0.0
            };

            let base = if c.is_seed {
                3.0 * c.bm25_score + 1.5 * cov_norm.min(0.5) + 2.0 * name_norm.min(0.5)
            } else {
                1.5 * cov_norm + 2.0 * name_norm + walk_norm
            };

            let coverage_frac = if n_qt > 0 {
                c.coverage_count as f64 / n_qt as f64
            } else {
                0.0
            };

            let ng_norm = c.ng_score / max_ng;
            let coh_norm = c.coherence_score / max_coherence;
            let ng_boost = 1.0 + 0.25 * ng_norm + 0.15 * coh_norm;

            let seed_bonus = if c.is_seed { 1.15 } else { 1.0 };
            let kb = kind_boost(&idx.symbol_kinds[c.idx]);
            let tp = test_penalty(&idx.file_paths, idx.symbol_file_ids[c.idx]);

            let raw = base * coverage_frac.powf(0.3) * ng_boost * seed_bonus * kb * tp;

            if raw > 0.0 {
                Some((c.idx, raw))
            } else {
                None
            }
        })
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    let bm25_confident = bm25_seeds.len() >= 2
        && bm25_seeds[0].1 / bm25_seeds[1].1.max(1e-10) > 1.2;
    if bm25_confident {
        if let Some(&lock_i) = bm25_seeds
            .first()
            .and_then(|(id, _)| idx.id_to_idx.get(id))
        {
            if let Some(pos) = scored.iter().position(|(i, _)| *i == lock_i) {
                if pos > 0 {
                    let (li, ls) = scored.remove(pos);
                    scored.insert(0, (li, ls + 1e6));
                }
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

enum QueryIntent {
    Navigational,
    Informational,
}

fn classify_query(
    query_terms: &[QueryTerm],
    idx: &CruncherIndex,
    bm25_seeds: &[(i64, f64)],
) -> QueryIntent {
    if bm25_seeds.is_empty() {
        return QueryIntent::Informational;
    }

    let avg_idf = if !query_terms.is_empty() {
        query_terms.iter().map(|qt| qt.idf).sum::<f64>() / query_terms.len() as f64
    } else {
        0.0
    };

    let n_qt = query_terms.len();

    if n_qt <= 3 {
        if let Some(&rank1_i) = bm25_seeds
            .first()
            .and_then(|(id, _)| idx.id_to_idx.get(id))
        {
            let name_terms = &idx.term_sets[rank1_i].name_terms;
            let name_hits: usize = query_terms
                .iter()
                .filter(|qt| qt.variants.iter().any(|v| name_terms.contains(v)))
                .count();
            if name_hits >= (n_qt + 1) / 2 {
                return QueryIntent::Navigational;
            }
        }
    }

    if let Some(&rank1_i) = bm25_seeds
        .first()
        .and_then(|(id, _)| idx.id_to_idx.get(id))
    {
        let name_terms = &idx.term_sets[rank1_i].name_terms;
        let high_idf_name_hits: usize = query_terms
            .iter()
            .filter(|qt| qt.idf > avg_idf && qt.variants.iter().any(|v| name_terms.contains(v)))
            .count();
        let high_idf_total = query_terms.iter().filter(|qt| qt.idf > avg_idf).count();
        if high_idf_total > 0 && high_idf_name_hits == high_idf_total {
            return QueryIntent::Navigational;
        }
    }

    if bm25_seeds.len() >= 2 {
        let gap = bm25_seeds[0].1 / bm25_seeds[1].1.max(1e-10);
        if gap > 1.5 {
            if let Some(&rank1_i) = bm25_seeds
                .first()
                .and_then(|(id, _)| idx.id_to_idx.get(id))
            {
                let name_terms = &idx.term_sets[rank1_i].name_terms;
                let any_name_hit = query_terms
                    .iter()
                    .any(|qt| qt.variants.iter().any(|v| name_terms.contains(v)));
                if any_name_hit {
                    return QueryIntent::Navigational;
                }
            }
        }
    }

    QueryIntent::Informational
}

pub fn goober_v4_search(
    query: &str,
    idx: &CruncherIndex,
    bm25_seeds: &[(i64, f64)],
    top_k: usize,
) -> Vec<(i64, f64)> {
    let query_terms = build_query_terms(query, &idx.global_idf);
    if query_terms.is_empty() {
        return bm25_seeds.to_vec();
    }

    let intent = classify_query(&query_terms, idx, bm25_seeds);

    let n_qt = query_terms.len();
    let idf_sum: f64 = query_terms.iter().map(|qt| qt.idf).sum();

    let bm25_max = bm25_seeds
        .iter()
        .map(|(_, s)| *s)
        .fold(0.0f64, f64::max)
        .max(1e-10);

    let mut candidates: HashMap<usize, GooberV3Candidate> = HashMap::new();

    for &(id, score) in bm25_seeds.iter().take(30) {
        if let Some(&i) = idx.id_to_idx.get(&id) {
            let (cov_score, cov_count) = term_match_score(&query_terms, &idx.term_sets[i]);
            let (name_s, _) = name_coverage(&query_terms, &idx.term_sets[i].name_terms);

            let channels = compute_sec_channels(&query_terms, idx, i);
            let ng = negentropy(&channels);
            let coherence = channel_coherence(&query_terms, idx, i);

            let mut sp = HashSet::new();
            sp.insert(i);

            candidates.insert(
                i,
                GooberV3Candidate {
                    idx: i,
                    bm25_score: score / bm25_max,
                    coverage_score: cov_score,
                    coverage_count: cov_count,
                    name_score: name_s,
                    is_seed: true,
                    walk_evidence: 0.0,
                    seed_paths: sp,
                    ng_score: ng,
                    coherence_score: coherence,
                },
            );
        }
    }

    let mut idf_sorted: Vec<f64> = query_terms.iter().map(|qt| qt.idf).collect();
    idf_sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let idf_threshold = idf_sorted[idf_sorted.len() / 2];

    if matches!(intent, QueryIntent::Informational) {
        let seed_indices: Vec<usize> = candidates.keys().cloned().collect();

        for &seed_i in seed_indices.iter().take(8) {
            let mut queue: VecDeque<(usize, f64, usize)> = VecDeque::new();
            let mut visited: HashSet<usize> = HashSet::new();
            visited.insert(seed_i);

            for edge in idx.outgoing[seed_i].iter().take(10) {
                if !visited.contains(&edge.target) {
                    queue.push_back((edge.target, edge.weight, 1));
                    visited.insert(edge.target);
                }
            }
            for edge in idx.incoming[seed_i].iter().take(10) {
                if !visited.contains(&edge.target) {
                    queue.push_back((edge.target, edge.weight, 1));
                    visited.insert(edge.target);
                }
            }

            let mut expanded = 0usize;
            while let Some((neighbor_i, edge_w, depth)) = queue.pop_front() {
                if depth > 2 || expanded >= 25 {
                    break;
                }

                let has_specific = query_terms
                    .iter()
                    .filter(|qt| qt.idf >= idf_threshold)
                    .any(|qt| per_term_match(&idx.term_sets[neighbor_i], qt) > 0.0);

                if !has_specific {
                    continue;
                }

                let (cov_score, cov_count) =
                    term_match_score(&query_terms, &idx.term_sets[neighbor_i]);
                if cov_count == 0 {
                    continue;
                }

                let proximity = 0.5_f64.powi(depth as i32);
                let evidence = cov_score * proximity * edge_w;

                let channels = compute_sec_channels(&query_terms, idx, neighbor_i);
                let ng = negentropy(&channels);
                let coherence = channel_coherence(&query_terms, idx, neighbor_i);

                let entry = candidates.entry(neighbor_i).or_insert_with(|| {
                    let (ns, _) =
                        name_coverage(&query_terms, &idx.term_sets[neighbor_i].name_terms);
                    GooberV3Candidate {
                        idx: neighbor_i,
                        bm25_score: 0.0,
                        coverage_score: cov_score,
                        coverage_count: cov_count,
                        name_score: ns,
                        is_seed: false,
                        walk_evidence: 0.0,
                        seed_paths: HashSet::new(),
                        ng_score: ng,
                        coherence_score: coherence,
                    }
                });

                if !entry.is_seed {
                    entry.coverage_score = entry.coverage_score.max(cov_score);
                    entry.coverage_count = entry.coverage_count.max(cov_count);
                    entry.ng_score = entry.ng_score.max(ng);
                    entry.coherence_score = entry.coherence_score.max(coherence);
                }
                entry.walk_evidence += evidence;
                entry.seed_paths.insert(seed_i);
                expanded += 1;

                if depth < 2 {
                    let next: Vec<(usize, f64)> = idx.outgoing[neighbor_i]
                        .iter()
                        .chain(idx.incoming[neighbor_i].iter())
                        .take(6)
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

    let max_ng = candidates
        .values()
        .map(|c| c.ng_score)
        .fold(0.0f64, f64::max)
        .max(1e-10);

    let max_coherence = candidates
        .values()
        .map(|c| c.coherence_score)
        .fold(0.0f64, f64::max)
        .max(1e-10);

    let (bm25_w, cov_w, name_w, ng_w, coh_w) = match intent {
        QueryIntent::Navigational => (5.0, 0.8, 1.0, 0.1, 0.05),
        QueryIntent::Informational => (3.0, 1.5, 2.0, 0.25, 0.15),
    };

    let mut scored: Vec<(usize, f64)> = candidates
        .values()
        .filter_map(|c| {
            if !c.is_seed && c.seed_paths.len() < 2 {
                return None;
            }

            let cov_norm = if idf_sum > 0.0 {
                c.coverage_score / idf_sum
            } else {
                0.0
            };
            let name_norm = if idf_sum > 0.0 {
                c.name_score / idf_sum
            } else {
                0.0
            };
            let walk_norm = if idf_sum > 0.0 {
                c.walk_evidence / idf_sum
            } else {
                0.0
            };

            let base = if c.is_seed {
                let cov_cap = if matches!(intent, QueryIntent::Navigational) {
                    cov_norm.min(0.2)
                } else {
                    cov_norm.min(0.5)
                };
                let name_cap = if matches!(intent, QueryIntent::Navigational) {
                    name_norm.min(0.3)
                } else {
                    name_norm.min(0.5)
                };
                bm25_w * c.bm25_score + cov_w * cov_cap + name_w * name_cap
            } else {
                1.5 * cov_norm + 2.0 * name_norm + walk_norm
            };

            let coverage_frac = if n_qt > 0 {
                c.coverage_count as f64 / n_qt as f64
            } else {
                0.0
            };

            let ng_norm = c.ng_score / max_ng;
            let coh_norm = c.coherence_score / max_coherence;
            let ng_boost = 1.0 + ng_w * ng_norm + coh_w * coh_norm;

            let seed_bonus = if c.is_seed { 1.15 } else { 1.0 };
            let kb = kind_boost(&idx.symbol_kinds[c.idx]);
            let tp = test_penalty(&idx.file_paths, idx.symbol_file_ids[c.idx]);

            let raw = base * coverage_frac.powf(0.3) * ng_boost * seed_bonus * kb * tp;

            if raw > 0.0 {
                Some((c.idx, raw))
            } else {
                None
            }
        })
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    let bm25_confident = bm25_seeds.len() >= 2
        && bm25_seeds[0].1 / bm25_seeds[1].1.max(1e-10) > 1.2;
    if bm25_confident {
        if let Some(&lock_i) = bm25_seeds
            .first()
            .and_then(|(id, _)| idx.id_to_idx.get(id))
        {
            if let Some(pos) = scored.iter().position(|(i, _)| *i == lock_i) {
                if pos > 0 {
                    let (li, ls) = scored.remove(pos);
                    scored.insert(0, (li, ls + 1e6));
                }
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

// --- Holographic name matching for V5 ---

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

fn holo_complex_mul(a: &(Vec<f64>, Vec<f64>), b: &(Vec<f64>, Vec<f64>)) -> (Vec<f64>, Vec<f64>) {
    let n = a.0.len();
    let mut re = vec![0.0; n];
    let mut im = vec![0.0; n];
    for i in 0..n {
        re[i] = a.0[i] * b.0[i] - a.1[i] * b.1[i];
        im[i] = a.0[i] * b.1[i] + a.1[i] * b.0[i];
    }
    (re, im)
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

pub struct HoloIndex {
    pub name_holos: Vec<Vec<f64>>,
    term_freq: HashMap<String, (Vec<f64>, Vec<f64>)>,
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

fn holo_query_name_cosine(query: &str, hi: &HoloIndex, symbol_i: usize) -> f64 {
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
        }
    }
    holo_normalize(&mut q_holo);

    holo_cosine(&q_holo, &hi.name_holos[symbol_i])
}

struct V5Candidate {
    idx: usize,
    bm25_score: f64,
    coverage_score: f64,
    coverage_count: usize,
    name_score: f64,
    is_seed: bool,
    walk_evidence: f64,
    seed_paths: HashSet<usize>,
    ng_score: f64,
    coherence_score: f64,
    holo_name_sim: f64,
}

pub fn goober_v5_search(
    query: &str,
    idx: &CruncherIndex,
    hi: &HoloIndex,
    bm25_seeds: &[(i64, f64)],
    top_k: usize,
) -> Vec<(i64, f64)> {
    let query_terms = build_query_terms(query, &idx.global_idf);
    if query_terms.is_empty() {
        return bm25_seeds.to_vec();
    }

    let intent = classify_query(&query_terms, idx, bm25_seeds);

    let n_qt = query_terms.len();
    let idf_sum: f64 = query_terms.iter().map(|qt| qt.idf).sum();

    let bm25_max = bm25_seeds
        .iter()
        .map(|(_, s)| *s)
        .fold(0.0f64, f64::max)
        .max(1e-10);

    let query_specificity = if n_qt > 0 {
        let high_idf_count = query_terms.iter().filter(|qt| qt.idf > 1.0).count();
        high_idf_count as f64 / n_qt as f64
    } else {
        0.0
    };

    let mut candidates: HashMap<usize, V5Candidate> = HashMap::new();

    for &(id, score) in bm25_seeds.iter().take(MAX_SEEDS) {
        if let Some(&i) = idx.id_to_idx.get(&id) {
            let (cov_score, cov_count) = term_match_score(&query_terms, &idx.term_sets[i]);
            let (name_s, _) = name_coverage(&query_terms, &idx.term_sets[i].name_terms);

            let channels = compute_sec_channels(&query_terms, idx, i);
            let ng = negentropy(&channels);
            let coherence = channel_coherence(&query_terms, idx, i);
            let holo_name = holo_query_name_cosine(query, hi, i);

            let mut sp = HashSet::new();
            sp.insert(i);

            candidates.insert(
                i,
                V5Candidate {
                    idx: i,
                    bm25_score: score / bm25_max,
                    coverage_score: cov_score,
                    coverage_count: cov_count,
                    name_score: name_s,
                    is_seed: true,
                    walk_evidence: 0.0,
                    seed_paths: sp,
                    ng_score: ng,
                    coherence_score: coherence,
                    holo_name_sim: holo_name,
                },
            );
        }
    }

    let mut idf_sorted: Vec<f64> = query_terms.iter().map(|qt| qt.idf).collect();
    idf_sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let idf_threshold = idf_sorted[idf_sorted.len() / 2];

    if matches!(intent, QueryIntent::Informational) {
        let seed_indices: Vec<usize> = candidates.keys().cloned().collect();

        for &seed_i in seed_indices.iter().take(8) {
            let mut queue: VecDeque<(usize, f64, usize)> = VecDeque::new();
            let mut visited: HashSet<usize> = HashSet::new();
            visited.insert(seed_i);

            for edge in idx.outgoing[seed_i].iter().take(10) {
                if !visited.contains(&edge.target) {
                    queue.push_back((edge.target, edge.weight, 1));
                    visited.insert(edge.target);
                }
            }
            for edge in idx.incoming[seed_i].iter().take(10) {
                if !visited.contains(&edge.target) {
                    queue.push_back((edge.target, edge.weight, 1));
                    visited.insert(edge.target);
                }
            }

            let mut expanded = 0usize;
            while let Some((neighbor_i, edge_w, depth)) = queue.pop_front() {
                if depth > 2 || expanded >= 25 {
                    break;
                }

                let has_specific = query_terms
                    .iter()
                    .filter(|qt| qt.idf >= idf_threshold)
                    .any(|qt| per_term_match(&idx.term_sets[neighbor_i], qt) > 0.0);

                if !has_specific {
                    continue;
                }

                let (cov_score, cov_count) =
                    term_match_score(&query_terms, &idx.term_sets[neighbor_i]);
                if cov_count == 0 {
                    continue;
                }

                let proximity = 0.5_f64.powi(depth as i32);
                let evidence = cov_score * proximity * edge_w;

                let channels = compute_sec_channels(&query_terms, idx, neighbor_i);
                let ng = negentropy(&channels);
                let coherence = channel_coherence(&query_terms, idx, neighbor_i);
                let holo_name = holo_query_name_cosine(query, hi, neighbor_i);

                let entry = candidates.entry(neighbor_i).or_insert_with(|| {
                    let (ns, _) =
                        name_coverage(&query_terms, &idx.term_sets[neighbor_i].name_terms);
                    V5Candidate {
                        idx: neighbor_i,
                        bm25_score: 0.0,
                        coverage_score: cov_score,
                        coverage_count: cov_count,
                        name_score: ns,
                        is_seed: false,
                        walk_evidence: 0.0,
                        seed_paths: HashSet::new(),
                        ng_score: ng,
                        coherence_score: coherence,
                        holo_name_sim: holo_name,
                    }
                });

                if !entry.is_seed {
                    entry.coverage_score = entry.coverage_score.max(cov_score);
                    entry.coverage_count = entry.coverage_count.max(cov_count);
                    entry.ng_score = entry.ng_score.max(ng);
                    entry.coherence_score = entry.coherence_score.max(coherence);
                    entry.holo_name_sim = entry.holo_name_sim.max(holo_name);
                }
                entry.walk_evidence += evidence;
                entry.seed_paths.insert(seed_i);
                expanded += 1;

                if depth < 2 {
                    let next: Vec<(usize, f64)> = idx.outgoing[neighbor_i]
                        .iter()
                        .chain(idx.incoming[neighbor_i].iter())
                        .take(6)
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

    let max_ng = candidates
        .values()
        .map(|c| c.ng_score)
        .fold(0.0f64, f64::max)
        .max(1e-10);

    let max_coherence = candidates
        .values()
        .map(|c| c.coherence_score)
        .fold(0.0f64, f64::max)
        .max(1e-10);

    let (bm25_w, cov_w, name_w, ng_w, coh_w) = match intent {
        QueryIntent::Navigational => (5.0, 0.8, 1.0, 0.1, 0.05),
        QueryIntent::Informational => (3.0, 1.5, 2.0, 0.25, 0.15),
    };

    let holo_gate = 0.25f64;
    let holo_max_w = 2.0;

    let mut scored: Vec<(usize, f64)> = candidates
        .values()
        .filter_map(|c| {
            if !c.is_seed && c.seed_paths.len() < 2 {
                return None;
            }

            let cov_norm = if idf_sum > 0.0 { c.coverage_score / idf_sum } else { 0.0 };
            let name_norm = if idf_sum > 0.0 { c.name_score / idf_sum } else { 0.0 };
            let walk_norm = if idf_sum > 0.0 { c.walk_evidence / idf_sum } else { 0.0 };

            let base = if c.is_seed {
                let cov_cap = if matches!(intent, QueryIntent::Navigational) {
                    cov_norm.min(0.2)
                } else {
                    cov_norm.min(0.5)
                };
                let name_cap = if matches!(intent, QueryIntent::Navigational) {
                    name_norm.min(0.3)
                } else {
                    name_norm.min(0.5)
                };
                bm25_w * c.bm25_score + cov_w * cov_cap + name_w * name_cap
            } else {
                1.5 * cov_norm + 2.0 * name_norm + walk_norm
            };

            let coverage_frac = if n_qt > 0 {
                c.coverage_count as f64 / n_qt as f64
            } else {
                0.0
            };

            let ng_norm = c.ng_score / max_ng;
            let coh_norm = c.coherence_score / max_coherence;
            let ng_boost = 1.0 + ng_w * ng_norm + coh_w * coh_norm;

            let holo_additive = if c.holo_name_sim > holo_gate {
                let excess = (c.holo_name_sim - holo_gate) / (1.0 - holo_gate);
                let w = holo_max_w * query_specificity * excess;
                w
            } else {
                0.0
            };

            let seed_bonus = if c.is_seed { 1.15 } else { 1.0 };
            let kb = kind_boost(&idx.symbol_kinds[c.idx]);
            let tp = test_penalty(&idx.file_paths, idx.symbol_file_ids[c.idx]);

            let raw = (base + holo_additive) * coverage_frac.powf(0.3) * ng_boost * seed_bonus * kb * tp;

            if raw > 0.0 {
                Some((c.idx, raw))
            } else {
                None
            }
        })
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    let bm25_confident = bm25_seeds.len() >= 2
        && bm25_seeds[0].1 / bm25_seeds[1].1.max(1e-10) > 1.2;
    if bm25_confident {
        if let Some(&lock_i) = bm25_seeds
            .first()
            .and_then(|(id, _)| idx.id_to_idx.get(id))
        {
            if let Some(pos) = scored.iter().position(|(i, _)| *i == lock_i) {
                if pos > 0 {
                    let (li, ls) = scored.remove(pos);
                    scored.insert(0, (li, ls + 1e6));
                }
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
            ("validate_input", SymbolKind::Function, 11, 20),
            ("AppConfig", SymbolKind::Struct, 21, 30),
            ("handle_request", SymbolKind::Function, 31, 40),
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
        let hi = build_holo_index(db, &ci);
        let fts = FtsSearch::new(db);
        let bm25: Vec<(i64, f64)> = fts
            .search(query, Some(30))
            .into_iter()
            .map(|r| (r.symbol.id, r.bm25_score))
            .collect();

        let _ = goober_search(query, &ci, &bm25, 10);
        let _ = goober_v3_search(query, &ci, &bm25, 10);
        let _ = goober_v4_search(query, &ci, &bm25, 10);
        let _ = goober_v5_search(query, &ci, &hi, &bm25, 10);
        let _ = cruncher_search(query, &ci, &bm25, 10);
        let _ = cruncher_v2_search(query, &ci, &bm25, 10);
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
