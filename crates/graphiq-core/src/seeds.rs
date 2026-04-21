use std::collections::{HashMap, HashSet};

use crate::db::GraphDb;
use crate::fts::{FtsConfig, FtsSearch};
use crate::query_family::QueryFamily;

pub fn bm25_seeds<'a>(db: &'a GraphDb, query: &str, family: QueryFamily) -> (Vec<(i64, f64)>, FtsSearch<'a>) {
    let fts = match family {
        QueryFamily::NaturalAbstract
        | QueryFamily::NaturalDescriptive
        | QueryFamily::ErrorDebug
        | QueryFamily::CrossCuttingSet => {
            FtsSearch::with_config(db, FtsConfig::for_natural_language())
        }
        _ => FtsSearch::new(db),
    };
    let fts_results = fts.search(query, Some(200));
    let seeds: Vec<(i64, f64)> = fts_results
        .iter()
        .map(|r| (r.symbol.id, r.bm25_score))
        .collect();
    (seeds, fts)
}

pub fn per_term_fts_expansion(
    fts: &FtsSearch<'_>,
    query: &str,
    existing_seeds: &[(i64, f64)],
    _family: QueryFamily,
) -> Vec<(i64, f64)> {
    let terms: Vec<String> = query
        .split_whitespace()
        .map(|t| t.to_lowercase())
        .filter(|t| t.len() >= 3 && ![
            "the", "are", "was", "were", "has", "had", "does",
            "how", "what", "which", "when", "where", "why",
            "all", "each", "every", "any", "some", "not",
            "and", "but", "for", "from", "with", "that",
            "this", "does", "can", "after", "before",
            "during", "between", "through", "into", "over",
            "under", "without",
        ].contains(&t.as_str()))
        .collect();

    if terms.is_empty() {
        return Vec::new();
    }

    let existing_ids: HashSet<i64> = existing_seeds.iter().map(|(id, _)| *id).collect();
    let mut candidates: HashMap<i64, f64> = HashMap::new();

    for term in &terms {
        let mut search_variants: Vec<String> = vec![term.clone()];

        let stemmed = crate::tokenize::stem_word(term);
        if stemmed != *term {
            search_variants.push(stemmed);
        }

        if let Some(syns) = crate::fts::get_synonyms(term) {
            for syn in syns.iter().take(3) {
                search_variants.push(syn.to_string());
            }
        }

        search_variants.sort_unstable();
        search_variants.dedup();

        for variant in &search_variants {
            let fts_results = fts.search(variant, Some(50));
            for r in &fts_results {
                if !existing_ids.contains(&r.symbol.id) {
                    let score = r.bm25_score.max(0.1);
                    *candidates.entry(r.symbol.id).or_insert(0.0) += score;
                }
            }
        }
    }

    let n_terms = terms.len() as f64;
    let mut results: Vec<(i64, f64)> = candidates
        .into_iter()
        .map(|(id, total_score)| (id, total_score / n_terms))
        .filter(|(_, score)| *score > 0.0)
        .collect();
    results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0)));
    results
}

pub fn graph_aware_expansion(
    db: &GraphDb,
    family: QueryFamily,
    existing_seeds: &[(i64, f64)],
) -> Vec<(i64, f64)> {
    let existing_ids: HashSet<i64> = existing_seeds.iter().map(|(id, _)| *id).collect();
    if existing_ids.is_empty() {
        return Vec::new();
    }

    let edge_kinds = match family {
        QueryFamily::ErrorDebug => vec!["shares_error_type"],
        QueryFamily::CrossCuttingSet => vec!["shares_type", "shares_data_shape"],
        QueryFamily::NaturalAbstract | QueryFamily::NaturalDescriptive => {
            vec!["shares_error_type", "shares_type", "shares_data_shape"]
        }
        QueryFamily::Relationship => vec!["shares_type", "shares_error_type"],
        _ => return Vec::new(),
    };

    let kind_filter = edge_kinds.iter()
        .map(|k| format!("'{}'", k))
        .collect::<Vec<_>>()
        .join(", ");

    let conn = db.conn();
    let mut candidates: HashMap<i64, f64> = HashMap::new();

    for &(sid, _score) in existing_seeds.iter().take(30) {
        let query_str = format!(
            "SELECT target_id, weight FROM edges \
             WHERE source_id = ?1 AND kind IN ({}) \
             LIMIT 30",
            kind_filter
        );
        if let Ok(mut stmt) = conn.prepare(&query_str) {
            if let Ok(rows) = stmt.query_map(rusqlite::params![sid], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?))
            }) {
                for (tid, w) in rows.filter_map(|r| r.ok()) {
                    if !existing_ids.contains(&tid) {
                        *candidates.entry(tid).or_insert(0.0) += w.max(0.1);
                    }
                }
            }
        }
    }

    let mut results: Vec<(i64, f64)> = candidates.into_iter().collect();
    results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0)));
    results.truncate(50);
    results
}

pub fn numeric_bridge_seeds(
    db: &GraphDb,
    query: &str,
    existing_seeds: &[(i64, f64)],
) -> Vec<(i64, f64)> {
    let numbers: Vec<String> = query
        .split(|c: char| !c.is_ascii_digit() && c != '.' && c != 'x' && c != 'X')
        .filter(|s| {
            if s.is_empty() { return false; }
            if s.len() == 1 { return false; }
            let s_lower = s.to_lowercase();
            if s_lower.starts_with("0x") && s.len() > 2 { return true; }
            if s.contains('.') { return true; }
            s.parse::<u64>().map_or(false, |n| n > 1)
        })
        .map(|s| s.to_lowercase())
        .collect();

    if numbers.is_empty() {
        return Vec::new();
    }

    let existing_ids: HashSet<i64> = existing_seeds.iter().map(|(id, _)| *id).collect();
    let mut candidates: HashMap<i64, f64> = HashMap::new();

    let conn = db.conn();
    for num in &numbers {
        let pattern = format!("%\"literal\":\"{}%", num);
        if let Ok(mut stmt) = conn.prepare(
            "SELECT source_id, target_id, weight FROM edges \
             WHERE kind IN ('shares_constant', 'references_constant') \
             AND metadata LIKE ?1"
        ) {
            if let Ok(rows) = stmt.query_map([&pattern], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, f64>(2)?))
            }) {
                for (src, tgt, w) in rows.filter_map(|r| r.ok()) {
                    for &id in &[src, tgt] {
                        if !existing_ids.contains(&id) {
                            *candidates.entry(id).or_insert(0.0) += w.max(0.1);
                        }
                    }
                }
            }
        }
    }

    let mut results: Vec<(i64, f64)> = candidates.into_iter().collect();
    results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0)));
    results
}

pub struct SeedConfig {
    pub family: QueryFamily,
    pub allow_per_term: bool,
    pub allow_graph: bool,
    pub allow_numeric: bool,
    pub allow_source_scan: bool,
}

impl SeedConfig {
    pub fn for_family(family: QueryFamily) -> Self {
        let is_nl = matches!(family,
            QueryFamily::NaturalAbstract
            | QueryFamily::NaturalDescriptive
            | QueryFamily::ErrorDebug
            | QueryFamily::CrossCuttingSet
        );
        Self {
            family,
            allow_per_term: is_nl,
            allow_graph: is_nl,
            allow_numeric: is_nl,
            allow_source_scan: family == QueryFamily::ErrorDebug,
        }
    }
}

pub fn source_scan_seeds(
    db: &GraphDb,
    query: &str,
    existing_seeds: &[(i64, f64)],
) -> Vec<(i64, f64)> {
    let lower = query.to_lowercase();
    let words: Vec<&str> = query.split_whitespace().collect();

    let error_signals: &[&str] = &[
        "error", "panic", "failed", "failure", "deadlock", "timeout", "crash",
        "exception", "abort", "refused", "overflow", "underflow",
        "detected", "leaked", "mismatch", "conflict", "invalid",
    ];

    let generic: HashSet<&str> = [
        "the", "a", "an", "is", "are", "was", "to", "when", "after",
        "during", "with", "without", "inside", "via", "in", "for",
        "from", "of", "and", "or", "not", "async", "task", "context",
    ].iter().copied().collect();

    let mut phrases: Vec<String> = Vec::new();

    for cap in regex::Regex::new(r#""([^"]{3,})""#).unwrap().captures_iter(query) {
        phrases.push(cap[1].to_string());
    }
    for cap in regex::Regex::new(r"'([^']{3,})'").unwrap().captures_iter(query) {
        phrases.push(cap[1].to_string());
    }

    for sig in error_signals {
        if !lower.contains(sig) {
            continue;
        }
        for (i, w) in words.iter().enumerate() {
            if !w.to_lowercase().contains(sig) {
                continue;
            }
            let start = i.saturating_sub(2);
            let end = (i + 3).min(words.len());
            let window = &words[start..end];
            let has_specific = window.iter().any(|w| {
                let wl = w.to_lowercase();
                !generic.contains(wl.as_str())
                    && !error_signals.iter().any(|s| wl.contains(s))
            });
            if has_specific {
                let phrase = window.join(" ");
                if phrase.len() >= 6 {
                    phrases.push(phrase);
                }
            }
        }
    }

    phrases.sort_unstable();
    phrases.dedup();
    if phrases.is_empty() {
        return Vec::new();
    }

    let existing_ids: HashSet<i64> = existing_seeds.iter().map(|(id, _)| *id).collect();
    let conn = db.conn();
    let mut candidates: HashMap<i64, f64> = HashMap::new();

    for phrase in &phrases {
        let escaped = phrase.replace('%', "\\%").replace('_', "\\_");
        let pat = format!("%{}%", escaped);
        if let Ok(mut stmt) = conn.prepare(
            "SELECT id FROM symbols WHERE source LIKE ?1 ESCAPE '\\' LIMIT 50"
        ) {
            if let Ok(rows) = stmt.query_map([&pat], |row| row.get::<_, i64>(0)) {
                for id in rows.filter_map(|r| r.ok()) {
                    if !existing_ids.contains(&id) {
                        *candidates.entry(id).or_insert(0.0) += 1.0;
                    }
                }
            }
        }
    }

    let mut results: Vec<(i64, f64)> = candidates.into_iter().collect();
    results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0)));
    results
}

pub fn generate_seeds(
    db: &GraphDb,
    query: &str,
    config: &SeedConfig,
) -> (Vec<(i64, f64)>, usize, Vec<(i64, f64)>) {
    let (mut seeds, fts) = bm25_seeds(db, query, config.family);
    let total_fts = seeds.len();
    let original_bm25 = seeds.clone();

    if config.allow_per_term {
        let term_seeds = per_term_fts_expansion(&fts, query, &seeds, config.family);
        for (id, score) in term_seeds {
            if !seeds.iter().any(|(sid, _)| *sid == id) {
                seeds.push((id, score));
            }
        }
    }

    if config.allow_source_scan {
        let scan_seeds = source_scan_seeds(db, query, &seeds);
        for (id, score) in scan_seeds {
            if !seeds.iter().any(|(sid, _)| *sid == id) {
                seeds.push((id, score));
            }
        }
    }

    if config.allow_numeric {
        let bridge_seeds = numeric_bridge_seeds(db, query, &seeds);
        for (id, score) in bridge_seeds {
            if !seeds.iter().any(|(sid, _)| *sid == id) {
                seeds.push((id, score));
            }
        }
    }

    if config.allow_graph {
        let graph_seeds = graph_aware_expansion(db, config.family, &seeds);
        for (id, score) in graph_seeds {
            if !seeds.iter().any(|(sid, _)| *sid == id) {
                seeds.push((id, score));
            }
        }
    }

    (seeds, total_fts, original_bm25)
}
