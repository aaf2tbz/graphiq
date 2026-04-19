use crate::db::GraphDb;
use crate::symbol::{Symbol, SymbolKind, Visibility};
use std::collections::HashMap;

pub struct FileMatch {
    pub file_id: i64,
    pub path: String,
    pub score: f64,
}

pub struct FileRouterResult {
    pub file_matches: Vec<FileMatch>,
    pub ranked_symbols: Vec<(Symbol, f64)>,
}

fn path_tokens(path: &str) -> Vec<String> {
    let lower = path.to_lowercase();
    let no_ext = lower.rsplit_once('.').map(|(base, _)| base).unwrap_or(&lower);
    let mut tokens: Vec<String> = Vec::new();
    for segment in no_ext.split(&['/', '\\', '-'][..]) {
        if segment.contains('_') {
            for sub in segment.split('_') {
                if !sub.is_empty() {
                    tokens.push(sub.to_string());
                }
            }
        } else {
            for part in segment_split_camel(segment) {
                if !part.is_empty() {
                    tokens.push(part.to_string());
                }
            }
        }
    }
    tokens
}

fn segment_split_camel(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    for (i, c) in s.char_indices().skip(1) {
        if c.is_uppercase() {
            if start < i {
                parts.push(&s[start..i]);
            }
            start = i;
        }
    }
    if start < s.len() {
        parts.push(&s[start..]);
    }
    if parts.is_empty() && !s.is_empty() {
        parts.push(s);
    }
    parts
}

fn score_file_match(query_tokens: &[String], file_path: &str) -> f64 {
    let lower_path = file_path.to_lowercase();
    let file_toks = path_tokens(file_path);

    let basename_with_ext = lower_path.rsplit('/').next().unwrap_or(&lower_path);
    let basename = basename_with_ext
        .rsplit_once('.')
        .map(|(b, _)| b)
        .unwrap_or(basename_with_ext);

    let basename_exact = query_tokens.iter().filter(|t| !matches!(t.as_str(), "go" | "rs" | "ts" | "js" | "py")).last()
        .map_or(false, |t| t.eq_ignore_ascii_case(basename));

    let mut score = 0.0;

    if basename_exact {
        score += 10.0;
    }

    let path_segments: Vec<&str> = lower_path.split('/').collect();
    let non_ext_tokens: Vec<&String> = query_tokens
        .iter()
        .filter(|t| !matches!(t.as_str(), "go" | "rs" | "ts" | "js" | "py"))
        .collect();

    let subseq_score = path_subsequence_score(&non_ext_tokens, &path_segments);
    score += subseq_score;

    score += 3.0 / (path_segments.len() as f64);

    let unmatched_segments = path_segments.len().saturating_sub(non_ext_tokens.len());
    score -= unmatched_segments as f64 * 0.5;

    if lower_path.contains("/src/") {
        score += 0.1;
    }
    if lower_path.contains("/tests/") || lower_path.contains("/benches/") {
        score -= 1.0;
    }
    score += 2.0 / (file_path.len() as f64);

    for qt in &non_ext_tokens {
        let mut best: f64 = 0.0;
        for ft in &file_toks {
            if &ft == qt {
                best = best.max(3.0);
            } else if ft.starts_with(qt.as_str()) || qt.starts_with(ft.as_str()) {
                best = best.max(1.5);
            }
        }
        if lower_path.contains(qt.as_str()) {
            best = best.max(0.5);
        }
        score += best;
    }

    score
}

fn path_subsequence_score(query_toks: &[&String], path_segments: &[&str]) -> f64 {
    if query_toks.is_empty() || path_segments.is_empty() {
        return 0.0;
    }
    let mut qi = 0;
    let mut consecutive = 0;
    let mut max_consecutive = 0;
    let mut last_match_idx = None;

    for (pi, seg) in path_segments.iter().enumerate() {
        let seg_base = seg.rsplit_once('.').map(|(b, _)| b).unwrap_or(seg);
        if qi < query_toks.len() && (seg == query_toks[qi] || seg_base == query_toks[qi]) {
            match last_match_idx {
                Some(li) if pi == li + 1 => consecutive += 1,
                _ => consecutive = 1,
            }
            max_consecutive = max_consecutive.max(consecutive);
            last_match_idx = Some(pi);
            qi += 1;
        }
    }

    if qi < query_toks.len() {
        return 0.0;
    }

    max_consecutive as f64 * 3.0
}

fn score_symbol_for_file(sym: &Symbol, incoming_counts: &HashMap<i64, usize>) -> f64 {
    let mut score = 0.0;

    match sym.visibility {
        Visibility::Public => score += 3.0,
        Visibility::Package => score += 2.0,
        Visibility::Protected => score += 1.0,
        Visibility::Private | Visibility::Anonymous => {}
    }

    match sym.kind {
        SymbolKind::Struct | SymbolKind::Class | SymbolKind::Enum | SymbolKind::Interface => score += 4.0,
        SymbolKind::Trait => score += 3.5,
        SymbolKind::Function | SymbolKind::Method => score += 2.0,
        SymbolKind::Constructor => score += 3.0,
        _ => {}
    }

    let incoming = *incoming_counts.get(&sym.id).unwrap_or(&0) as f64;
    score += incoming.min(30.0).ln_1p();

    score += 1.0 / (1.0 + sym.line_start as f64 / 1000.0);

    let line_span = sym.line_end.saturating_sub(sym.line_start) as f64;
    if line_span > 5.0 {
        score += 0.5;
    }

    score
}

pub fn file_route_query(db: &GraphDb, query: &str, top_k: usize) -> FileRouterResult {
    let query_lower = query.to_lowercase();
    let query_toks: Vec<String> = query_lower
        .split_whitespace()
        .flat_map(|t| t.split(&['/', '\\', '.'][..]))
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
        .collect();

    let mut stmt = db.conn()
        .prepare("SELECT id, path FROM files")
        .expect("files query");

    let files: Vec<(i64, String)> = stmt
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .expect("file rows")
        .flatten()
        .collect();

    let mut file_scores: Vec<FileMatch> = files
        .into_iter()
        .filter_map(|(id, path)| {
            let s = score_file_match(&query_toks, &path);
            if s > 0.0 {
                Some(FileMatch {
                    file_id: id,
                    path,
                    score: s,
                })
            } else {
                None
            }
        })
        .collect();

    file_scores.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    file_scores.retain(|fm| fm.score >= 3.0);
    file_scores.truncate(5);

    let incoming_counts: HashMap<i64, usize> = db
        .incoming_edges_grouped()
        .map(|rows| {
            let mut counts = HashMap::new();
            for (target_id, _, _) in rows {
                *counts.entry(target_id).or_insert(0) += 1;
            }
            counts
        })
        .unwrap_or_default();

    let mut ranked_symbols: Vec<(Symbol, f64)> = Vec::new();

    for fm in &file_scores {
        let file_decay = 1.0 / (1.0 + ranked_symbols.len() as f64);
        if let Ok(syms) = db.symbols_by_file(fm.file_id) {
            let mut file_syms: Vec<(Symbol, f64)> = syms
                .into_iter()
                .filter(|s| {
                    !matches!(
                        s.kind,
                        SymbolKind::Import | SymbolKind::Constant | SymbolKind::Section
                    )
                })
                .map(|s| {
                    let sym_score = score_symbol_for_file(&s, &incoming_counts) * file_decay;
                    (s, sym_score)
                })
                .collect();

            file_syms.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

            for (sym, sc) in file_syms {
                if ranked_symbols.len() >= top_k {
                    break;
                }
                ranked_symbols.push((sym, sc));
            }
        }

        if ranked_symbols.len() >= top_k {
            break;
        }
    }

    ranked_symbols.truncate(top_k);

    FileRouterResult {
        file_matches: file_scores,
        ranked_symbols,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_tokens_basic() {
        let toks = path_tokens("src/runtime/scheduler/worker.rs");
        let tok_strs: Vec<&str> = toks.iter().map(|s| s.as_str()).collect();
        assert!(tok_strs.contains(&"src"));
        assert!(tok_strs.contains(&"runtime"));
        assert!(tok_strs.contains(&"scheduler"));
        assert!(tok_strs.contains(&"worker"));
    }

    #[test]
    fn test_path_tokens_underscore() {
        let toks = path_tokens("forge_config.rs");
        let tok_strs: Vec<&str> = toks.iter().map(|s| s.as_str()).collect();
        assert!(tok_strs.contains(&"forge"));
        assert!(tok_strs.contains(&"config"));
    }

    #[test]
    fn test_score_file_exact_basename() {
        let toks: Vec<String> = vec!["predictor".to_string()];
        let score = score_file_match(&toks, "src/predictor.rs");
        assert!(score >= 10.0, "exact basename match should score >= 10, got {score}");
    }

    #[test]
    fn test_score_file_path_prefix() {
        let toks: Vec<String> = vec!["runtime".to_string(), "scheduler".to_string(), "worker".to_string()];
        let score = score_file_match(&toks, "src/runtime/scheduler/worker.rs");
        assert!(score > 5.0, "full path match should score > 5, got {score}");
    }

    #[test]
    fn test_score_file_no_match() {
        let toks: Vec<String> = vec!["predictor".to_string()];
        let score = score_file_match(&toks, "src/runtime/scheduler/worker.rs");
        assert!(score < 3.0, "unrelated file should score < 3.0, got {score}");
    }

    #[test]
    fn test_segment_split_camel() {
        let parts = segment_split_camel("lowerNullishCoalescing");
        assert_eq!(parts, vec!["lower", "Nullish", "Coalescing"]);
    }
}
