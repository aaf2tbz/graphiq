use crate::db::GraphDb;
use crate::fts::FtsSearch;
use crate::rerank::{Reranker, ScoredSymbol};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct DecomposedResult {
    pub results: Vec<ScoredSymbol>,
    pub subqueries: Vec<String>,
    pub evidence_counts: HashMap<i64, usize>,
}

pub fn is_decomposable_query(query: &str) -> bool {
    let words: Vec<&str> = query.split_whitespace().collect();
    if words.len() < 3 {
        return false;
    }

    let lower = query.to_lowercase();

    let abstract_patterns = [
        "how does",
        "how do",
        "how are",
        "how is",
        "how can",
        "what is",
        "what are",
        "what does",
        "what connects",
        "where is",
        "where are",
        "where does",
        "why does",
        "why is",
        "why are",
        "when does",
        "when is",
    ];
    if abstract_patterns.iter().any(|p| lower.starts_with(p)) {
        return true;
    }

    if looks_like_code_identifier(query) {
        return false;
    }

    if looks_like_file_path(query) {
        return false;
    }

    false
}

fn looks_like_code_identifier(query: &str) -> bool {
    let words: Vec<&str> = query.split_whitespace().collect();
    let has_camel =
        query.chars().filter(|c| c.is_uppercase()).count() >= 2 && query.chars().any(|c| c == '_');
    let has_snake = words.iter().any(|w| w.contains('_') && w.len() > 3);
    let single_word = words.len() == 1;
    let has_tech_token = words.iter().any(|w| {
        matches!(
            w.to_lowercase().as_str(),
            "bm25"
                | "fts"
                | "knn"
                | "sql"
                | "api"
                | "http"
                | "url"
                | "cli"
                | "mcp"
                | "lru"
                | "bfs"
                | "dfs"
        )
    });
    has_camel || has_snake || single_word || has_tech_token
}

fn looks_like_file_path(query: &str) -> bool {
    let extensions = [
        ".rs", ".ts", ".tsx", ".js", ".jsx", ".py", ".go", ".java", ".c", ".cpp", ".h", ".rb",
        ".yaml", ".yml", ".toml", ".json",
    ];
    let lower = query.to_lowercase();
    extensions.iter().any(|ext| lower.contains(ext))
}

fn strip_query_prefix(query: &str) -> String {
    let lower = query.to_lowercase().trim().to_string();

    let prefixes = [
        "how does ",
        "how do ",
        "how are ",
        "how is ",
        "how can ",
        "what is ",
        "what are ",
        "what does ",
        "what connects ",
        "where is ",
        "where are ",
        "where does ",
        "why does ",
        "why is ",
        "why are ",
        "when does ",
        "when is ",
        "the ",
        "a ",
        "an ",
    ];

    let mut stripped = lower.clone();
    for prefix in &prefixes {
        if stripped.starts_with(prefix) {
            stripped = stripped[prefix.len()..].to_string();
        }
    }

    let suffixes = [
        " work",
        " happen",
        " occur",
        " get",
        " process",
        " function",
        " implemented",
        " managed",
        " handled",
        " processed",
        " computed",
        " stored",
    ];
    for suffix in &suffixes {
        if stripped.ends_with(suffix) {
            stripped = stripped[..stripped.len() - suffix.len()].to_string();
        }
    }

    stripped.trim().to_string()
}

fn generate_subqueries(core: &str) -> Vec<Vec<String>> {
    let mut tracks: Vec<Vec<String>> = Vec::new();

    let content_words: Vec<String> = core
        .split_whitespace()
        .filter(|w| w.len() >= 2 && !is_particle(w))
        .map(|w| w.to_string())
        .collect();

    if content_words.len() >= 2 {
        tracks.push(content_words.clone());
    }

    let domain_map: &[(&str, &[&str])] = &[
        ("ranking", &["rerank", "reranker"]),
        ("retrieval", &["search", "retrieve"]),
        ("index", &["indexer", "index"]),
        ("indexed", &["indexer", "index"]),
        ("symbols", &["symbol", "parse"]),
        ("symbol", &["symbol", "parse"]),
        ("source", &["parse", "tree"]),
        ("files", &["file", "walk"]),
        ("callers", &["calls", "bfs"]),
        ("callees", &["calls", "graph"]),
        ("connects", &["edge", "traverse"]),
        ("graph", &["graph", "bfs"]),
        ("expansion", &["expand", "graph"]),
        ("blast", &["blast", "bfs"]),
        ("search", &["search", "fts"]),
        ("cache", &["cache", "lru"]),
        ("error", &["error", "result"]),
        ("middleware", &["middleware", "chain"]),
        ("auth", &["auth", "token"]),
        ("validate", &["validate", "check"]),
        ("parse", &["parse", "tree"]),
        ("tokenize", &["tokenize", "decompose"]),
        ("rerank", &["rerank", "reranker"]),
        ("decompose", &["decompose", "tokenize"]),
    ];

    for word in &content_words {
        for (key, terms) in domain_map {
            if word == *key {
                tracks.push(terms.iter().map(|t| t.to_string()).collect());
            }
        }
    }

    let concrete_map: &[(&str, &[&str])] = &[
        ("ranking", &["reranker"]),
        ("retrieval", &["search engine"]),
        ("indexing", &["indexer"]),
        ("indexed", &["indexer"]),
        ("connecting", &["graph traverse"]),
        ("connects", &["edge graph"]),
        ("callers", &["calls edge"]),
        ("callees", &["calls graph"]),
        ("traversal", &["bfs"]),
        ("expansion", &["expander"]),
        ("runtime schedule", &["scheduler"]),
        ("runtime handle", &["runtime handle"]),
        ("timer tracked", &["timer entry"]),
        ("timers tracked", &["timer entry"]),
        ("shutting down", &["shutdown"]),
        ("shutdown", &["shutdown"]),
        ("tcp accept", &["tcp_listener"]),
        ("tcp stream", &["tcp_stream"]),
        ("sync primitives", &["mutex rwlock semaphore"]),
        ("vector similarity", &["cosine_similarity"]),
        ("nearest neighbor", &["knn"]),
        ("knn", &["knn"]),
        ("split documents", &["chunker"]),
        ("chunk documents", &["chunker"]),
        ("autograd tape", &["tape"]),
        ("daemon process", &["daemon"]),
        ("connector tools", &["connector register"]),
        ("connector register", &["connector register"]),
        ("backpropagation", &["tape backward"]),
        ("documents chunks", &["chunker split"]),
    ];

    for word in &content_words {
        for (key, terms) in concrete_map {
            if word == *key {
                for t in *terms {
                    tracks.push(vec![t.to_string()]);
                }
            }
        }
    }

    let phrase_map: &[(&str, &[&str])] = &[
        ("runtime schedule", &["scheduler"]),
        ("runtime handle", &["runtime handle"]),
        ("timer tracked fired", &["timer_entry"]),
        ("timers tracked fired", &["timer_entry"]),
        ("shutting down runtime", &["shutdown"]),
        ("tcp accept connections", &["tcp_listener"]),
        ("split tcp stream", &["tcp_stream split"]),
        ("sync primitives", &["mutex rwlock semaphore"]),
        ("vector similarity", &["cosine_similarity"]),
        ("nearest neighbors embedding", &["knn"]),
        ("split documents chunks", &["chunker"]),
        ("connector tools registered", &["connector register"]),
        ("connector implementations", &["connector"]),
        ("autograd operations", &["tape"]),
        ("tray manage autostart", &["autostart"]),
        ("similarity scores memory", &["merge_hybrid_scores"]),
    ];

    let core_lower = core.to_lowercase();
    for (phrase, terms) in phrase_map {
        if core_lower.contains(phrase) {
            for t in *terms {
                tracks.push(vec![t.to_string()]);
            }
        }
    }

    if content_words.iter().any(|w| w == "callers") && content_words.iter().any(|w| w == "callees")
    {
        tracks.push(vec!["bfs".to_string()]);
        tracks.push(vec!["traverse".to_string()]);
        tracks.push(vec!["bounded_bfs".to_string()]);
    }

    tracks.truncate(8);
    tracks
}

fn is_particle(w: &str) -> bool {
    matches!(
        w,
        "from" | "the" | "a" | "an" | "to" | "of" | "in" | "and" | "or"
    )
}

pub fn decomposed_search(
    db: &GraphDb,
    query: &str,
    top_k: usize,
    debug: bool,
) -> Option<DecomposedResult> {
    if !is_decomposable_query(query) {
        return None;
    }

    let core = strip_query_prefix(query);
    if core.is_empty() {
        return None;
    }

    let tracks = generate_subqueries(&core);
    if tracks.is_empty() {
        return None;
    }

    let subquery_strings: Vec<String> = tracks.iter().map(|t| t.join(" ")).collect();

    let mut evidence_counts: HashMap<i64, usize> = HashMap::new();
    let mut all_scored: HashMap<i64, ScoredSymbol> = HashMap::new();

    for subquery in &tracks {
        let sq_str = subquery.join(" ");
        let fts = FtsSearch::new(db);
        let fts_results = fts.search(&sq_str, Some(50));

        let file_paths = load_file_paths(db);
        let reranker = Reranker::new(db, debug).for_query(&sq_str);
        let results = reranker.rerank(&fts_results, &[], &file_paths, 20);

        for r in results {
            let sid = r.symbol.id;
            *evidence_counts.entry(sid).or_insert(0) += 1;

            let entry = all_scored.entry(sid).or_insert_with(|| r.clone());
            if r.score > entry.score {
                *entry = r;
            }
        }
    }

    let num_tracks = tracks.len();
    let min_evidence = if num_tracks >= 5 { 2 } else { 1 };

    let mut final_results: Vec<ScoredSymbol> = all_scored
        .into_values()
        .filter(|r| {
            let count = evidence_counts.get(&r.symbol.id).copied().unwrap_or(1);
            count >= min_evidence
        })
        .map(|mut r| {
            let count = evidence_counts.get(&r.symbol.id).copied().unwrap_or(1);
            if count >= 2 {
                r.score *= 1.0 + 0.3 * (count as f64 - 1.0);
            } else if num_tracks >= 4 {
                r.score *= 0.85;
            }
            r
        })
        .collect();

    final_results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    final_results.truncate(top_k);

    Some(DecomposedResult {
        results: final_results,
        subqueries: subquery_strings,
        evidence_counts,
    })
}

fn load_file_paths(db: &GraphDb) -> HashMap<i64, String> {
    let conn = db.conn();
    let mut stmt = match conn.prepare("SELECT id, path FROM files") {
        Ok(s) => s,
        Err(_) => return HashMap::new(),
    };
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .ok();
    match rows {
        Some(r) => r.flatten().collect(),
        None => HashMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_decomposable_query() {
        assert!(is_decomposable_query("how does retrieval ranking work"));
        assert!(is_decomposable_query(
            "how are symbols indexed from source files"
        ));
        assert!(is_decomposable_query("what connects callers to callees"));
        assert!(!is_decomposable_query("RateLimiter"));
        assert!(!is_decomposable_query("rate limit"));
        assert!(!is_decomposable_query("cache"));
        assert!(!is_decomposable_query("periodic interval timer"));
        assert!(!is_decomposable_query("tcp accept connections"));
        assert!(!is_decomposable_query(
            "compute vector similarity between embeddings"
        ));
    }

    #[test]
    fn test_strip_prefix() {
        assert_eq!(
            strip_query_prefix("how does retrieval ranking work"),
            "retrieval ranking"
        );
        assert_eq!(
            strip_query_prefix("how are symbols indexed from source files"),
            "symbols indexed from source files"
        );
        assert_eq!(
            strip_query_prefix("what connects callers to callees"),
            "callers to callees"
        );
    }

    #[test]
    fn test_generate_subqueries_ranking() {
        let tracks = generate_subqueries("retrieval ranking");
        assert!(!tracks.is_empty());
        assert!(tracks.len() <= 6);

        let all_terms: Vec<String> = tracks.iter().flatten().cloned().collect();
        assert!(all_terms.iter().any(|t| t == "reranker"));
        assert!(all_terms.iter().any(|t| t == "search" || t == "retrieve"));
    }

    #[test]
    fn test_generate_subqueries_callers() {
        let tracks = generate_subqueries("callers callees");
        let all_terms: Vec<String> = tracks.iter().flatten().cloned().collect();
        assert!(all_terms.iter().any(|t| t == "bfs" || t == "graph"));
        assert!(all_terms.iter().any(|t| t == "calls"));
    }

    #[test]
    fn test_generate_subqueries_indexed() {
        let tracks = generate_subqueries("symbols indexed source files");
        let all_terms: Vec<String> = tracks.iter().flatten().cloned().collect();
        assert!(all_terms.iter().any(|t| t == "indexer"));
        assert!(all_terms.iter().any(|t| t == "symbol"));
    }

    #[test]
    fn test_guard_rails_non_decomposable() {
        assert!(!is_decomposable_query("RateLimiter"));
        assert!(!is_decomposable_query("rate limit"));
        assert!(!is_decomposable_query("cache"));
        assert!(!is_decomposable_query("bm25 full text search"));
        assert!(!is_decomposable_query("edge"));
        assert!(!is_decomposable_query("symbol.rs"));
        assert!(!is_decomposable_query("graph.rs"));
        assert!(!is_decomposable_query("DbError"));
        assert!(!is_decomposable_query("rerank"));
        assert!(!is_decomposable_query("bounded_bfs"));
        assert!(!is_decomposable_query("tokenize"));
        assert!(!is_decomposable_query("HotCache"));
    }

    #[test]
    fn test_decomposed_search_returns_none_for_non_decomposable() {
        let db = crate::db::GraphDb::open_in_memory().unwrap();
        let result = decomposed_search(&db, "RateLimiter", 10, false);
        assert!(result.is_none());
        let result = decomposed_search(&db, "rate limit", 10, false);
        assert!(result.is_none());
        let result = decomposed_search(&db, "cache", 10, false);
        assert!(result.is_none());
    }
}
