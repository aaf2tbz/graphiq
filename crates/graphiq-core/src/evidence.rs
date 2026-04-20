use std::collections::{HashMap, HashSet, VecDeque};

use crate::db::GraphDb;

pub struct EvidenceIndex {
    pub symbol_ids: Vec<i64>,
    pub symbol_names: Vec<String>,
    pub symbol_kinds: Vec<String>,
    pub symbol_sigs: Vec<Option<String>>,
    pub symbol_sources: Vec<String>,
    pub symbol_file_ids: Vec<i64>,
    pub symbol_hints: Vec<String>,
    pub file_paths: HashMap<i64, String>,
    pub outgoing: Vec<Vec<usize>>,
    pub outgoing_refs: Vec<Vec<usize>>,
    pub incoming: Vec<Vec<usize>>,
    pub incoming_refs: Vec<Vec<usize>>,
    pub id_to_idx: HashMap<i64, usize>,
}

pub fn build_evidence_index(db: &GraphDb) -> Result<EvidenceIndex, String> {
    let conn = db.conn();

    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.name, s.kind, s.signature, s.source, s.file_id, s.search_hints \
             FROM symbols s ORDER BY s.id",
        )
        .map_err(|e| e.to_string())?;
    let rows: Vec<(i64, String, String, Option<String>, String, i64, String)> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
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
    let symbol_sources: Vec<String> = rows.iter().map(|r| r.4.clone()).collect();
    let symbol_file_ids: Vec<i64> = rows.iter().map(|r| r.5).collect();
    let symbol_hints: Vec<String> = rows.iter().map(|r| r.6.clone()).collect();

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
    let mut outgoing_refs: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut incoming: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut incoming_refs: Vec<Vec<usize>> = vec![Vec::new(); n];

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
            outgoing[si].push(ti);
            incoming[ti].push(si);
            if kind == "references" {
                outgoing_refs[si].push(ti);
                incoming_refs[ti].push(si);
            }
        }
    }

    Ok(EvidenceIndex {
        symbol_ids,
        symbol_names,
        symbol_kinds,
        symbol_sigs,
        symbol_sources,
        symbol_file_ids,
        symbol_hints,
        file_paths,
        outgoing,
        outgoing_refs,
        incoming,
        incoming_refs,
        id_to_idx,
    })
}

#[derive(Clone)]
struct TermSeeds {
    term: String,
    indices: Vec<usize>,
    weights: Vec<f64>,
    name_match_indices: HashSet<usize>,
}

fn extract_query_terms(query: &str) -> Vec<String> {
    let stop_words: HashSet<&str> = [
        "how", "what", "where", "when", "why", "which", "who", "does", "do", "did", "is", "are",
        "was", "were", "be", "been", "being", "have", "has", "had", "will", "would", "could",
        "should", "may", "might", "can", "shall", "the", "a", "an", "of", "in", "to", "for", "on",
        "at", "by", "with", "from", "as", "into", "through", "and", "or", "but", "not", "that",
        "this", "these", "those", "it", "its", "if", "then", "than", "so", "up", "out", "two",
        "new", "all", "every", "has", "having",
    ]
    .iter()
    .cloned()
    .collect();

    let lowered = query.to_lowercase();
    lowered
        .split_whitespace()
        .filter(|w| w.len() >= 3 && !stop_words.contains(w))
        .map(|w| w.to_string())
        .collect()
}

fn normalize_plurals(word: &str) -> Vec<String> {
    let mut variants = vec![word.to_string()];
    let w = word.to_lowercase();
    if w.ends_with("ies") && w.len() > 4 {
        variants.push(format!("{}y", &w[..w.len() - 3]));
    } else if w.ends_with("ves") && w.len() > 4 {
        variants.push(format!("{}f", &w[..w.len() - 3]));
    } else if w.ends_with("ses") && w.len() > 4 {
        variants.push(format!("{}s", &w[..w.len() - 2]));
        variants.push(format!("{}is", &w[..w.len() - 2]));
    } else if w.ends_with("es") && w.len() > 3 {
        variants.push(format!("{}", &w[..w.len() - 2]));
        variants.push(format!("{}e", &w[..w.len() - 2]));
    } else if w.ends_with("s") && w.len() > 3 {
        variants.push(format!("{}", &w[..w.len() - 1]));
    }
    if w.ends_with("ing") && w.len() > 5 {
        variants.push(format!("{}", &w[..w.len() - 3]));
        variants.push(format!("{}e", &w[..w.len() - 3]));
    }
    if w.ends_with("ed") && w.len() > 4 {
        variants.push(format!("{}", &w[..w.len() - 2]));
        variants.push(format!("{}", &w[..w.len() - 1]));
    }
    if w.ends_with("tion") {
        variants.push(format!("{}te", &w[..w.len() - 4]));
        variants.push(format!("{}te", &w[..w.len() - 3]));
    }
    if w.ends_with("ment") && w.len() > 5 {
        variants.push(format!("{}", &w[..w.len() - 4]));
    }
    variants.sort_unstable();
    variants.dedup();
    variants
}

fn find_high_quality_seeds(term: &str, idx: &EvidenceIndex) -> (Vec<(usize, f64)>, HashSet<usize>) {
    let term_lower = term.to_lowercase();
    let n = idx.symbol_ids.len();
    let mut seeds: Vec<(usize, f64)> = Vec::new();
    let mut name_match_indices: HashSet<usize> = HashSet::new();
    let max_seeds = 200;

    let stems: Vec<String> = if term.len() > 4 {
        let base_stems = vec![
            term_lower.clone(),
            term_lower[..term.len() - 1].to_string(),
            term_lower[..term.len() - 2].to_string(),
        ];
        let plural_variants = normalize_plurals(term);
        let mut all = base_stems;
        for v in plural_variants {
            if !all.contains(&v) {
                all.push(v);
            }
        }
        all
    } else {
        let plural_variants = normalize_plurals(term);
        let mut all = vec![term_lower.clone()];
        for v in plural_variants {
            if !all.contains(&v) {
                all.push(v);
            }
        }
        all
    };

    for i in 0..n {
        let name_lower = idx.symbol_names[i].to_lowercase();

        if name_lower.len() > 100 {
            continue;
        }

        let decomp_lower = name_lower.replace('_', " ");

        let name_score = score_term_against_name(&stems, &name_lower, &decomp_lower);
        if name_score > 0.0 {
            seeds.push((i, name_score * 3.0));
            name_match_indices.insert(i);
            continue;
        }

        if let Some(ref sig) = idx.symbol_sigs[i] {
            let sig_lower = sig.to_lowercase();
            let mut sig_score: f64 = 0.0;
            for stem in &stems {
                if sig_lower.contains(stem) {
                    let type_words = [
                        "Result", "Option", "Vec", "String", "bool", "Error", "Config", "Status",
                        "Info", "Response", "Request",
                    ];
                    let name_has_type = type_words
                        .iter()
                        .any(|tw| name_lower.contains(&tw.to_lowercase()));
                    if name_has_type {
                        sig_score = sig_score.max(1.5);
                    } else {
                        sig_score = sig_score.max(0.8);
                    }
                }
            }
            if sig_score > 0.0 {
                seeds.push((i, sig_score));
                continue;
            }
        }

        let hints_lower = idx.symbol_hints[i].to_lowercase();
        let hints_len = hints_lower.len();
        if hints_len < 500 {
            for stem in &stems {
                if hints_lower.contains(stem) {
                    seeds.push((i, 0.3));
                    break;
                }
            }
        }

        if seeds.len() >= max_seeds * 2 {
            break;
        }
    }

    let mut seen: HashSet<usize> = HashSet::new();
    seeds.retain(|(i, _)| seen.insert(*i));

    let name_matched: HashSet<usize> = seeds
        .iter()
        .filter(|(_, w)| *w >= 2.0)
        .map(|(i, _)| *i)
        .collect();

    if name_matched.len() <= 50 {
        for &si in &name_matched {
            for &neighbor in &idx.incoming[si] {
                if neighbor == si
                    || idx.symbol_names[neighbor].len() > 100
                    || seen.contains(&neighbor)
                {
                    continue;
                }
                seeds.push((neighbor, 1.2));
                seen.insert(neighbor);
            }
            for &neighbor in &idx.outgoing[si] {
                if neighbor == si
                    || idx.symbol_names[neighbor].len() > 100
                    || seen.contains(&neighbor)
                {
                    continue;
                }
                seeds.push((neighbor, 0.8));
                seen.insert(neighbor);
            }
        }

        for &si in &name_matched {
            for &ref_target in &idx.outgoing_refs[si] {
                if ref_target == si
                    || idx.symbol_names[ref_target].len() > 100
                    || seen.contains(&ref_target)
                {
                    continue;
                }
                seeds.push((ref_target, 1.5));
                seen.insert(ref_target);
            }
            for &ref_source in &idx.incoming_refs[si] {
                if ref_source == si
                    || idx.symbol_names[ref_source].len() > 100
                    || seen.contains(&ref_source)
                {
                    continue;
                }
                seeds.push((ref_source, 1.3));
                seen.insert(ref_source);
            }
        }
    }

    let abbreviations: &[(&str, &[&str])] = &[
        ("fts", &["full", "text", "search"]),
        ("mcp", &["mcp", "model", "context", "protocol"]),
        ("auth", &["authenticate", "authentication", "authorization"]),
        ("db", &["database"]),
        ("config", &["configuration"]),
        ("sync", &["synchronize", "synchronization"]),
        ("async", &["asynchronous"]),
        ("init", &["initialize", "initialization"]),
        ("utils", &["utility", "utilities"]),
        ("mgr", &["manager"]),
        ("pwd", &["password"]),
        ("perm", &["permission", "permissions"]),
        ("authn", &["authentication"]),
        ("authz", &["authorization"]),
        ("scope", &["scope", "range", "boundary", "domain", "extent"]),
    ];

    for (abbr, expansions) in abbreviations {
        if term_lower == *abbr {
            for exp in *expansions {
                for i in 0..n {
                    if idx.symbol_names[i].len() > 100 {
                        continue;
                    }
                    let nl = idx.symbol_names[i].to_lowercase();
                    if nl.contains(exp) && seen.insert(i) {
                        seeds.push((i, 1.5));
                    }
                }
            }
        }
        for exp in *expansions {
            if term_lower == *exp {
                for i in 0..n {
                    if idx.symbol_names[i].len() > 100 {
                        continue;
                    }
                    let nl = idx.symbol_names[i].to_lowercase();
                    let dl = nl.replace('_', " ");
                    if nl.contains(abbr) && seen.insert(i) {
                        seeds.push((i, 1.5));
                    }
                    if dl.split_whitespace().any(|w| w == *abbr) && seen.insert(i) {
                        seeds.push((i, 1.5));
                    }
                }
            }
        }
    }

    let behavior_map: &[(&str, &[&str])] = &[
        (
            "failed",
            &["error", "fail", "catch", "handle", "throw", "reject"],
        ),
        ("start", &["start", "launch", "boot", "spawn", "init"]),
        ("stop", &["stop", "shutdown", "halt", "kill", "terminate"]),
        ("restart", &["restart", "reboot", "reload"]),
        (
            "manage",
            &["manager", "controller", "handler", "coordinator"],
        ),
        (
            "manages",
            &["manager", "controller", "handler", "coordinator"],
        ),
        (
            "controls",
            &[
                "control",
                "controller",
                "guard",
                "gate",
                "policy",
                "enforce",
                "scope",
                "scope",
            ],
        ),
        (
            "determines",
            &[
                "check", "validate", "verify", "evaluate", "resolve", "assess",
            ],
        ),
        ("tracks", &["track", "log", "record", "monitor", "observe"]),
        ("tracked", &["track", "log", "record", "monitor", "observe"]),
        (
            "processes",
            &["process", "handle", "transform", "parse", "convert"],
        ),
        (
            "process",
            &["process", "handle", "transform", "parse", "convert"],
        ),
        ("handles", &["handle", "handler", "catch", "process"]),
        (
            "creates",
            &["create", "build", "make", "new", "construct", "factory"],
        ),
        ("stores", &["store", "save", "persist", "insert", "write"]),
        ("healthy", &["health", "status", "diagnostics", "check"]),
        (
            "lifecycle",
            &["start", "stop", "restart", "init", "shutdown", "manager"],
        ),
        (
            "combines",
            &["merge", "combine", "hybrid", "join", "mix", "blend", "fuse"],
        ),
        (
            "connects",
            &["connector", "bridge", "link", "connect", "join", "wire"],
        ),
        (
            "validates",
            &["validate", "check", "verify", "assert", "guard", "ensure"],
        ),
        (
            "routes",
            &["router", "route", "dispatch", "redirect", "forward"],
        ),
        (
            "transforms",
            &["transform", "convert", "map", "adapt", "translate"],
        ),
        (
            "annotated",
            &["annotate", "mark", "tag", "label", "decorate"],
        ),
        ("registered", &["register", "add", "bind", "attach", "hook"]),
        (
            "invoked",
            &["invoke", "call", "trigger", "fire", "dispatch", "execute"],
        ),
        (
            "wake",
            &["notify", "wake", "signal", "interrupt", "trigger"],
        ),
        (
            "returning",
            &["return", "respond", "reply", "yield", "output"],
        ),
    ];

    for (verb, related) in behavior_map {
        if term_lower == *verb {
            for rel in *related {
                for i in 0..n {
                    if idx.symbol_names[i].len() > 100 || seen.contains(&i) {
                        continue;
                    }
                    let nl = idx.symbol_names[i].to_lowercase();
                    let dl = nl.replace('_', " ");
                    if dl.split_whitespace().any(|w| w == *rel) || nl.contains(rel) {
                        if seen.insert(i) {
                            seeds.push((i, 1.0));
                        }
                    }
                }
            }
            break;
        }
    }

    seeds.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    seeds.truncate(max_seeds);
    (seeds, name_match_indices)
}

fn score_term_against_name(stems: &[String], name_lower: &str, decomp_lower: &str) -> f64 {
    let mut best: f64 = 0.0;
    for stem in stems {
        if name_lower == stem {
            return 3.0;
        }
        if name_lower.contains(stem) {
            let ratio = stem.len() as f64 / name_lower.len().max(1) as f64;
            best = best.max(2.0 * ratio);
        }
        if decomp_lower.contains(stem) {
            let ratio = stem.len() as f64 / decomp_lower.len().max(1) as f64;
            best = best.max(1.5 * ratio);
        }
        for word in decomp_lower.split_whitespace() {
            if word == stem {
                return 2.5;
            }
            if word.starts_with(stem) || stem.starts_with(word) {
                best = best.max(1.0);
            }
        }
    }
    best
}

fn bfs_distances(
    seeds: &[(usize, f64)],
    adj: &[Vec<usize>],
    n: usize,
    max_depth: usize,
) -> Vec<f64> {
    let mut dist: Vec<f64> = vec![f64::INFINITY; n];
    let mut queue: VecDeque<(usize, f64)> = VecDeque::new();

    for &(seed, weight) in seeds {
        let d = 1.0 / weight;
        if d < dist[seed] {
            dist[seed] = d;
            queue.push_back((seed, d));
        }
    }

    while let Some((node, d)) = queue.pop_front() {
        if d >= max_depth as f64 {
            continue;
        }
        for &neighbor in &adj[node] {
            let new_d = d + 1.0;
            if new_d < dist[neighbor] {
                dist[neighbor] = new_d;
                queue.push_back((neighbor, new_d));
            }
        }
    }

    dist
}

pub fn evidence_search(query: &str, idx: &EvidenceIndex, top_k: usize) -> Vec<(i64, f64)> {
    let terms = extract_query_terms(query);
    if terms.is_empty() {
        return Vec::new();
    }

    let n = idx.symbol_ids.len();
    let max_depth = 4;

    let combined_adj: Vec<Vec<usize>> = (0..n)
        .map(|i| {
            let mut adj = idx.outgoing[i].clone();
            let mut incoming = idx.incoming[i].clone();
            adj.append(&mut incoming);
            adj.sort_unstable();
            adj.dedup();
            adj
        })
        .collect();

    let mut term_seeds: Vec<TermSeeds> = Vec::new();
    for term in &terms {
        let (seeds, name_matches) = find_high_quality_seeds(term, idx);
        if !seeds.is_empty() {
            term_seeds.push(TermSeeds {
                term: term.clone(),
                indices: seeds.iter().map(|(i, _)| *i).collect(),
                weights: seeds.iter().map(|(_, w)| *w).collect(),
                name_match_indices: name_matches,
            });
        }
    }

    if term_seeds.is_empty() {
        return Vec::new();
    }

    let alpha = 2.5;

    let mut distance_fields: Vec<Vec<f64>> = Vec::new();
    for ts in &term_seeds {
        let seeds: Vec<(usize, f64)> = ts
            .indices
            .iter()
            .zip(ts.weights.iter())
            .map(|(&i, &w)| (i, w))
            .collect();
        let dist = bfs_distances(&seeds, &combined_adj, n, max_depth);
        distance_fields.push(dist);
    }

    let n_terms = term_seeds.len() as f64;

    let term_stems: Vec<Vec<String>> = term_seeds
        .iter()
        .map(|ts| {
            let t = ts.term.to_lowercase();
            if t.len() > 4 {
                vec![
                    t.clone(),
                    t[..t.len() - 1].to_string(),
                    t[..t.len() - 2].to_string(),
                ]
            } else {
                vec![t.clone()]
            }
        })
        .collect();

    let seed_sets: Vec<HashSet<usize>> = term_seeds
        .iter()
        .map(|ts| ts.indices.iter().copied().collect())
        .collect();

    let mut scored: Vec<(usize, f64)> = (0..n)
        .map(|i| {
            let name_lower = idx.symbol_names[i].to_lowercase();
            let _decomp_lower = name_lower.replace('_', " ");

            let direct_name_hits = term_seeds
                .iter()
                .filter(|ts| ts.name_match_indices.contains(&i))
                .count() as f64;

            let direct_name_score: f64 = term_seeds
                .iter()
                .filter_map(|ts| {
                    if !ts.name_match_indices.contains(&i) {
                        return None;
                    }
                    ts.indices
                        .iter()
                        .zip(ts.weights.iter())
                        .find(|(&si, _)| si == i)
                        .map(|(_, &w)| w)
                })
                .sum();

            let _term_hit_ratio = direct_name_hits / n_terms;

            let intersection_bonus = if direct_name_hits >= 2.0 {
                let quality_avg = direct_name_score / direct_name_hits;
                (quality_avg * direct_name_hits).powf(1.0 + direct_name_hits * 0.5)
            } else if direct_name_hits == 1.0 {
                direct_name_score
            } else {
                0.0
            };

            let per_term_ev: Vec<f64> = distance_fields
                .iter()
                .zip(term_seeds.iter())
                .map(|(dist, ts)| {
                    let d = dist[i];
                    if d < 1.0 {
                        1.0
                    } else {
                        let evidence = (-alpha * d * 0.3).exp();
                        let seed_quality =
                            ts.weights.iter().sum::<f64>() / (ts.weights.len() as f64).max(1.0);
                        evidence * seed_quality
                    }
                })
                .collect();

            let convergence_product: f64 = per_term_ev.iter().product::<f64>();

            let n_matched = per_term_ev.iter().filter(|&&e| e > 0.3).count() as f64;
            let convergence: f64 = if convergence_product > 0.001 || n_matched < 2.0 {
                convergence_product
            } else {
                let top_k_ev: Vec<f64> = {
                    let mut sorted = per_term_ev.clone();
                    sorted.sort_by(|a, b| b.partial_cmp(a).unwrap());
                    sorted
                };
                let best_two: f64 = top_k_ev.iter().take(2).product();
                let coverage_penalty = (n_matched / n_terms).powf(0.5);
                best_two * coverage_penalty * 0.3
            };

            let min_evidence: f64 = distance_fields
                .iter()
                .map(|dist| {
                    let d = dist[i];
                    (-alpha * d * 0.3).exp().max(0.001)
                })
                .fold(f64::INFINITY, f64::min);

            let harmonic_balance = if distance_fields.len() > 1 {
                n_terms
                    / distance_fields
                        .iter()
                        .map(|dist| (-alpha * dist[i] * 0.3).exp().max(0.01))
                        .sum::<f64>()
            } else {
                1.0
            };

            let kind_weight = match idx.symbol_kinds[i].as_str() {
                "function" | "method" | "constructor" => 1.4,
                "class" | "struct" | "interface" | "trait" => 1.3,
                "enum" | "type_alias" => 1.1,
                "constant" | "field" | "property" => 0.9,
                "module" | "section" => 0.7,
                _ => 1.0,
            };

            let path_lower = idx
                .file_paths
                .get(&idx.symbol_file_ids[i])
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

            let callee_names: Vec<&str> = idx.outgoing[i]
                .iter()
                .take(20)
                .map(|&ni| idx.symbol_names[ni].as_str())
                .collect();

            let caller_names: Vec<&str> = idx.incoming[i]
                .iter()
                .take(20)
                .map(|&ni| idx.symbol_names[ni].as_str())
                .collect();

            let callee_term_coverage: f64 = term_seeds
                .iter()
                .enumerate()
                .filter(|(ti, ts)| {
                    let stems = &term_stems[*ti];
                    let t = ts.term.to_lowercase();
                    callee_names.iter().any(|nn| {
                        let nn_lower = nn.to_lowercase();
                        let nn_decomp = nn_lower.replace('_', " ");
                        stems.iter().any(|s| nn_lower.contains(s)) || nn_decomp.contains(&t)
                    })
                })
                .count() as f64
                / n_terms;

            let caller_term_coverage: f64 = term_seeds
                .iter()
                .filter(|ts| {
                    let t = ts.term.to_lowercase();
                    caller_names.iter().any(|nn| {
                        let nn_lower = nn.to_lowercase();
                        let nn_decomp = nn_lower.replace('_', " ");
                        nn_lower.contains(&t) || nn_decomp.contains(&t)
                    })
                })
                .count() as f64
                / n_terms;

            let bridging_score =
                if callee_term_coverage >= 0.5 && callee_term_coverage > caller_term_coverage {
                    let callee_count = callee_names.len() as f64;
                    let bridge_strength = callee_term_coverage * callee_count.ln_1p();
                    1.0 + bridge_strength * 2.5
                } else {
                    1.0
                };

            let all_neighbor_names: Vec<&str> = idx.outgoing[i]
                .iter()
                .chain(idx.incoming[i].iter())
                .take(30)
                .map(|&ni| idx.symbol_names[ni].as_str())
                .collect();

            let neighbor_coverage: f64 = term_seeds
                .iter()
                .filter(|ts| {
                    let t = ts.term.to_lowercase();
                    all_neighbor_names
                        .iter()
                        .any(|nn| nn.to_lowercase().contains(&t))
                })
                .count() as f64
                / n_terms;

            let neighbor_not_seed: f64 = term_seeds
                .iter()
                .enumerate()
                .filter(|(ti, ts)| {
                    if seed_sets[*ti].contains(&i) {
                        return false;
                    }
                    let t = ts.term.to_lowercase();
                    all_neighbor_names
                        .iter()
                        .any(|nn| nn.to_lowercase().contains(&t))
                })
                .count() as f64
                / n_terms;

            let topology_evidence = if neighbor_not_seed > 0.0 && direct_name_hits == 0.0 {
                1.0 + neighbor_not_seed * 3.0
            } else {
                1.0
            };

            let total = if direct_name_hits >= 2.0 {
                intersection_bonus
                    * convergence
                    * harmonic_balance
                    * kind_weight
                    * test_penalty
                    * bridging_score
                    * (1.0 + neighbor_coverage * 1.5)
            } else if direct_name_hits == 1.0 {
                direct_name_score
                    * 3.0
                    * convergence
                    * harmonic_balance
                    * kind_weight
                    * test_penalty
                    * bridging_score
                    * (1.0 + neighbor_coverage * 1.0)
            } else {
                convergence
                    * harmonic_balance
                    * kind_weight
                    * test_penalty
                    * (1.0 + neighbor_coverage * 1.0)
                    * min_evidence
                    * topology_evidence
                    * bridging_score.sqrt()
            };

            (i, total)
        })
        .filter(|(_, s)| *s > 0.0001)
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored.truncate(top_k);

    scored
        .into_iter()
        .map(|(i, s)| (idx.symbol_ids[i], s))
        .collect()
}

pub fn evidence_rerank(
    query: &str,
    candidate_ids: &[i64],
    candidate_scores: &[f64],
    idx: &EvidenceIndex,
) -> Vec<(i64, f64)> {
    let id_to_idx = &idx.id_to_idx;
    let terms = extract_query_terms(query);
    if terms.is_empty() {
        return candidate_ids
            .iter()
            .zip(candidate_scores.iter())
            .map(|(&id, &s)| (id, s))
            .filter(|(_, s)| *s > 0.0)
            .collect();
    }

    let all_term_seeds: Vec<TermSeeds> = terms
        .iter()
        .filter_map(|term| {
            let (seeds, name_matches) = find_high_quality_seeds(term, idx);
            if seeds.is_empty() {
                return None;
            }
            Some(TermSeeds {
                term: term.clone(),
                indices: seeds.iter().map(|(i, _)| *i).collect(),
                weights: seeds.iter().map(|(_, w)| *w).collect(),
                name_match_indices: name_matches,
            })
        })
        .collect();

    let n_terms = all_term_seeds.len() as f64;
    if n_terms == 0.0 {
        return candidate_ids
            .iter()
            .zip(candidate_scores.iter())
            .map(|(&id, &s)| (id, s))
            .filter(|(_, s)| *s > 0.0)
            .collect();
    }

    let scored: Vec<(i64, f64)> = candidate_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| {
            let base = candidate_scores.get(i).copied().unwrap_or(0.0);
            if base <= 0.0 {
                return (id, base);
            }

            if let Some(&sidx) = id_to_idx.get(&id) {
                let name_lower = idx.symbol_names[sidx].to_lowercase();

                let name_hit_count = all_term_seeds
                    .iter()
                    .filter(|ts| {
                        let t = ts.term.to_lowercase();
                        let stems = if t.len() > 4 {
                            vec![t.clone(), t[..t.len() - 1].to_string()]
                        } else {
                            vec![t.clone()]
                        };
                        stems.iter().any(|s| name_lower.contains(s))
                    })
                    .count() as f64;

                let concept_coverage = name_hit_count / n_terms;

                let neighbor_names: Vec<String> = idx.outgoing[sidx]
                    .iter()
                    .chain(idx.incoming[sidx].iter())
                    .take(20)
                    .map(|&ni| idx.symbol_names[ni].to_lowercase())
                    .collect();

                let neighbor_coverage: f64 = all_term_seeds
                    .iter()
                    .filter(|ts| {
                        let t = ts.term.to_lowercase();
                        neighbor_names.iter().any(|n| n.contains(&t))
                    })
                    .count() as f64
                    / n_terms;

                let seed_direct_hit = all_term_seeds
                    .iter()
                    .any(|ts| ts.name_match_indices.contains(&sidx));

                let seed_hit_count = all_term_seeds
                    .iter()
                    .filter(|ts| ts.name_match_indices.contains(&sidx))
                    .count() as f64;

                let seed_weight_sum: f64 = all_term_seeds
                    .iter()
                    .filter_map(|ts| {
                        ts.indices
                            .iter()
                            .zip(ts.weights.iter())
                            .find(|(&si, _)| si == sidx)
                            .map(|(_, &w)| w)
                    })
                    .sum();

                let direct_bonus = if seed_hit_count >= 2.0 {
                    let quality_avg = seed_weight_sum / seed_hit_count;
                    (quality_avg * seed_hit_count).powf(1.0 + seed_hit_count * 0.5)
                } else if seed_direct_hit {
                    seed_weight_sum.max(1.5)
                } else {
                    1.0
                };

                let callee_names: Vec<String> = idx.outgoing[sidx]
                    .iter()
                    .take(20)
                    .map(|&ni| idx.symbol_names[ni].to_lowercase())
                    .collect();

                let callee_coverage: f64 = all_term_seeds
                    .iter()
                    .filter(|ts| {
                        let t = ts.term.to_lowercase();
                        callee_names.iter().any(|n| n.contains(&t))
                    })
                    .count() as f64
                    / n_terms;

                let bridging_bonus = if callee_coverage >= 0.5 {
                    1.0 + callee_coverage * 2.5
                } else {
                    1.0
                };

                let evidence = direct_bonus
                    * (1.0 + concept_coverage * 1.5)
                    * (1.0 + neighbor_coverage * 1.0)
                    * bridging_bonus;

                (id, base * evidence)
            } else {
                (id, base)
            }
        })
        .filter(|(_, s)| *s > 0.0)
        .collect();

    let mut result = scored;
    result.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    result
}
