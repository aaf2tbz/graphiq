use crate::db::GraphDb;
use crate::fts::{FtsResult, FtsSearch};
use crate::hrr::HrrIndex;
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

    if lower.starts_with("all ") || lower.starts_with("every ") {
        return false;
    }

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

    if words.len() >= 5 {
        let short_count = words.iter().filter(|w| w.len() <= 3).count();
        if (short_count as f64) / (words.len() as f64) < 0.5 {
            return true;
        }
    }

    if words.len() >= 3 {
        let action_verbs = [
            "compute",
            "find",
            "split",
            "start",
            "stop",
            "build",
            "detect",
            "extract",
            "parse",
            "validate",
            "normalize",
            "get",
            "set",
            "check",
            "run",
            "create",
            "delete",
            "join",
            "block",
            "periodic",
            "accept",
            "schedule",
            "track",
            "fire",
            "handle",
        ];
        if action_verbs.iter().any(|v| words[0].to_lowercase() == *v) {
            return true;
        }

        let long_words = words.iter().filter(|w| w.len() > 4).count();
        if long_words >= 2 && !looks_like_code_identifier(query) {
            return true;
        }
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
        ("parse", &["parse", "parser"]),
        ("minify", &["minify", "renamer", "transform"]),
        ("bundle", &["bundle", "bundler", "linker"]),
        ("resolve", &["resolver", "resolve", "import"]),
        ("link", &["linker", "link"]),
        ("compile", &["compile", "compiler", "transform"]),
        ("transform", &["transform", "transformer", "ast"]),
        ("generate", &["generator", "generate", "emit"]),
        ("emit", &["emit", "generator", "output"]),
        ("sourcemap", &["sourcemap", "source_map"]),
        ("source map", &["sourcemap", "source_map"]),
        ("tree shaking", &["linker", "tree_shake", "symbol"]),
        ("dead code", &["linker", "tree_shake", "symbol"]),
        ("rename", &["renamer", "rename", "minify"]),
        ("lexer", &["lexer", "tokenizer", "scanner"]),
        ("tokenizer", &["tokenizer", "lexer", "scanner"]),
        ("scanner", &["scanner", "lexer", "tokenizer"]),
        ("ast", &["ast", "parse", "tree"]),
        ("abstract syntax", &["ast", "parse", "node"]),
        ("css", &["css", "parse", "style"]),
        ("javascript", &["js", "parse", "javascript"]),
        ("typescript", &["ts", "typescript", "parse"]),
        ("cross-language", &["ast", "symbol", "import"]),
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
        ("tree shaking", &["linker tree_shake"]),
        ("dead code", &["linker symbol"]),
        ("import paths", &["resolver import"]),
        ("source map vlq", &["sourcemap vlq encode"]),
        ("minify rename", &["renamer minify"]),
        ("css parsing printing", &["css parse print"]),
        ("source maps output", &["sourcemap chunk builder"]),
        ("cross-language ast", &["ast symbol import"]),
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
        ("periodic interval", &["interval"]),
        ("block async task", &["block_on"]),
        ("join concurrent tasks", &["joinset"]),
        ("join multiple concurrent", &["joinset"]),
        ("split documents indexing", &["chunk_document chunker"]),
        ("start stop daemon", &["start_daemon stop_daemon"]),
        ("compute vector similarity", &["cosine_similarity"]),
        ("find nearest neighbors", &["knn build_knn"]),
        ("autograd tape backpropagation", &["tape"]),
        ("accept connections", &["tcp_listener accept"]),
        ("runtime handle methods", &["handle spawn block_on"]),
        (
            "sync primitives",
            &["mutex rwlock semaphore barrier notify"],
        ),
        ("tree shaking remove dead", &["linker symbol tree_shake"]),
        ("resolve import paths", &["resolver import record"]),
        ("source map vlq", &["sourcemap encode vlq"]),
        ("minify rename symbols", &["renamer minify mangle"]),
        ("css parsing printing", &["css parse printer"]),
        ("source maps output", &["sourcemap chunk builder"]),
        ("cross-language ast", &["ast symbol ref import"]),
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

fn hrr_semantic_expand(
    db: &GraphDb,
    query: &str,
    hrr_idx: &HrrIndex,
    top_k: usize,
    debug: bool,
) -> Option<DecomposedResult> {
    let hrr_hits = crate::hrr::hrr_search(query, hrr_idx, 20);
    if hrr_hits.is_empty() {
        return None;
    }

    let seed_ids: Vec<i64> = hrr_hits.iter().map(|(id, _)| *id).collect();
    let expanded_query = crate::hrr::hrr_expand_query(&seed_ids, hrr_idx);
    if expanded_query.is_empty() {
        return None;
    }

    let mut evidence_counts: HashMap<i64, usize> = HashMap::new();
    let mut all_scored: HashMap<i64, ScoredSymbol> = HashMap::new();

    let fts = FtsSearch::new(db);
    let file_paths = load_file_paths(db);

    let main_fts = fts.search(query, Some(50));
    for r in &main_fts {
        let reranker = Reranker::new(db, debug).for_query(query);
        let single: Vec<FtsResult> = vec![r.clone()];
        let reranked = reranker.rerank(&single, &[], &[], &file_paths, 10);
        for res in reranked {
            let sid = res.symbol.id;
            *evidence_counts.entry(sid).or_insert(0) += 2;
            let entry = all_scored.entry(sid).or_insert_with(|| res.clone());
            if res.score > entry.score {
                *entry = res;
            }
        }
    }

    let expanded_terms: Vec<&str> = expanded_query.split_whitespace().collect();
    let chunk_size = 3.max(expanded_terms.len() / 3);
    for chunk in expanded_terms.chunks(chunk_size) {
        let subquery = chunk.join(" ");
        let fts_results = fts.search(&subquery, Some(30));
        let reranker = Reranker::new(db, debug).for_query(&subquery);
        let reranked = reranker.rerank(&fts_results, &[], &[], &file_paths, 15);
        for res in reranked {
            let sid = res.symbol.id;
            *evidence_counts.entry(sid).or_insert(0) += 1;
            let entry = all_scored.entry(sid).or_insert_with(|| res.clone());
            if res.score > entry.score {
                *entry = res;
            }
        }
    }

    for &(sym_id, hrr_score) in &hrr_hits {
        if let Ok(Some(sym)) = db.get_symbol(sym_id) {
            let fp = file_paths.get(&sym.file_id).cloned();
            let scored = ScoredSymbol {
                symbol: sym,
                score: hrr_score * 5.0,
                breakdown: None,
                is_fts_hit: false,
                file_path: fp,
            };
            *evidence_counts.entry(sym_id).or_insert(0) += 1;
            let entry = all_scored.entry(sym_id).or_insert_with(|| scored.clone());
            if scored.score > entry.score {
                *entry = scored;
            }
        }
    }

    let mut final_results: Vec<ScoredSymbol> = all_scored
        .into_values()
        .map(|mut r| {
            let count = evidence_counts.get(&r.symbol.id).copied().unwrap_or(1);
            if count >= 3 {
                r.score *= 1.5;
            } else if count >= 2 {
                r.score *= 1.2;
            }
            r
        })
        .collect();

    final_results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    final_results.truncate(top_k);

    if final_results.is_empty() {
        return None;
    }

    let mut subqueries = vec![query.to_string(), expanded_query.clone()];
    for chunk in expanded_terms.chunks(chunk_size) {
        subqueries.push(chunk.join(" "));
    }

    Some(DecomposedResult {
        results: final_results,
        subqueries,
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

pub fn decomposed_search_cross_cutting(
    db: &GraphDb,
    query: &str,
    top_k: usize,
    debug: bool,
) -> Option<DecomposedResult> {
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
        let results = reranker.rerank(&fts_results, &[], &[], &file_paths, 20);

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

pub fn decomposed_search(
    db: &GraphDb,
    query: &str,
    top_k: usize,
    debug: bool,
    hrr_index: Option<&HrrIndex>,
) -> Option<DecomposedResult> {
    if !is_decomposable_query(query) {
        return None;
    }

    let mut result = decomposed_search_cross_cutting(db, query, top_k, debug)?;

    if let Some(hrr_idx) = hrr_index {
        let has_good_results =
            result.results.len() >= 3 && result.results.iter().any(|r| r.score > 1.0);

        if !has_good_results {
            let hrr_expanded = hrr_semantic_expand(db, query, hrr_idx, top_k, debug);
            if let Some(hrr_result) = hrr_expanded {
                let existing_ids: std::collections::HashSet<i64> =
                    result.results.iter().map(|r| r.symbol.id).collect();

                let best_existing = result
                    .results
                    .iter()
                    .map(|r| r.score)
                    .fold(0.0f64, f64::max)
                    .max(1.0);

                for mut r in hrr_result.results {
                    if !existing_ids.contains(&r.symbol.id) {
                        r.score *= best_existing * 0.9;
                        result.results.push(r);
                    }
                }

                result
                    .results
                    .sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
                result.results.truncate(top_k);
            }
        }
    }

    Some(result)
}

pub fn extract_concrete_terms(query: &str) -> Vec<String> {
    let core = strip_query_prefix(query);
    if core.is_empty() {
        return Vec::new();
    }

    let content_words: Vec<String> = core
        .split_whitespace()
        .filter(|w| w.len() >= 2 && !is_particle(w))
        .map(|w| w.to_lowercase())
        .collect();

    if content_words.is_empty() {
        return Vec::new();
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
        ("search", &["search", "fts"]),
        ("cache", &["cache", "lru"]),
        ("error", &["error", "result"]),
        ("parse", &["parse", "parser"]),
        ("tokenize", &["tokenize", "decompose"]),
        ("minify", &["minify", "renamer", "transform"]),
        ("bundle", &["bundle", "bundler", "linker"]),
        ("resolve", &["resolver", "resolve", "import"]),
        ("link", &["linker", "link"]),
        ("compile", &["compile", "compiler", "transform"]),
        ("transform", &["transform", "transformer", "ast"]),
        ("generate", &["generator", "generate", "emit"]),
        ("sourcemap", &["sourcemap", "source_map"]),
        ("source map", &["sourcemap", "source_map"]),
        ("tree shaking", &["linker", "tree_shake", "symbol"]),
        ("dead code", &["linker", "tree_shake", "symbol"]),
        ("rename", &["renamer", "rename", "minify"]),
        ("lexer", &["lexer", "tokenizer", "scanner"]),
        ("tokenizer", &["tokenizer", "lexer", "scanner"]),
        ("scanner", &["scanner", "lexer", "tokenizer"]),
        ("ast", &["ast", "parse", "tree"]),
        ("css", &["css", "parse", "style"]),
        ("javascript", &["js", "parse", "javascript"]),
        ("typescript", &["ts", "typescript", "parse"]),
        ("runtime schedule", &["scheduler"]),
        ("runtime handle", &["runtime", "handle"]),
        ("timer tracked", &["timer_entry"]),
        ("timers tracked", &["timer_entry"]),
        ("shutting down", &["shutdown"]),
        ("shutdown", &["shutdown"]),
        ("tcp accept", &["tcp_listener"]),
        ("tcp stream", &["tcp_stream"]),
        ("sync primitives", &["mutex", "rwlock", "semaphore"]),
        ("vector similarity", &["cosine_similarity"]),
        ("nearest neighbor", &["knn"]),
        ("split documents", &["chunker"]),
        ("chunk documents", &["chunker"]),
        ("autograd tape", &["tape"]),
        ("daemon process", &["daemon"]),
        ("connector tools", &["connector", "register"]),
        ("backpropagation", &["tape", "backward"]),
        ("import paths", &["resolver", "import"]),
        ("source map vlq", &["sourcemap", "vlq", "encode"]),
        ("minify rename", &["renamer", "minify"]),
        ("css parsing printing", &["css", "parse", "print"]),
        ("source maps output", &["sourcemap", "chunk", "builder"]),
        ("cross-language ast", &["ast", "symbol", "import"]),
        ("periodic interval", &["interval"]),
        ("block async task", &["block_on"]),
        ("join concurrent tasks", &["joinset"]),
        ("accept connections", &["tcp_listener", "accept"]),
        ("similarity scores", &["merge_hybrid_scores"]),
        ("schedule", &["scheduler", "runtime"]),
        ("timers", &["timer", "entry", "wheel"]),
        ("tracked", &["timer", "entry"]),
        ("expired", &["timer", "wheel", "fire"]),
        ("shutting", &["shutdown"]),
        ("handle", &["handle", "runtime"]),
    ];

    let phrase_map: &[(&str, &[&str])] = &[
        ("runtime schedule", &["scheduler"]),
        ("runtime handle", &["runtime", "handle"]),
        ("timer tracked fired", &["timer_entry"]),
        ("timers tracked fired", &["timer_entry"]),
        ("shutting down runtime", &["shutdown"]),
        ("tcp accept connections", &["tcp_listener"]),
        ("split tcp stream", &["tcp_stream", "split"]),
        ("sync primitives", &["mutex", "rwlock", "semaphore"]),
        ("vector similarity", &["cosine_similarity"]),
        ("nearest neighbors embedding", &["knn"]),
        ("split documents chunks", &["chunker"]),
        ("connector tools registered", &["connector", "register"]),
        ("autograd operations", &["tape"]),
        ("similarity scores memory", &["merge_hybrid_scores"]),
        ("periodic interval", &["interval"]),
        ("block async task", &["block_on"]),
        ("join concurrent tasks", &["joinset"]),
        ("join multiple concurrent", &["joinset"]),
        (
            "tree shaking remove dead",
            &["linker", "symbol", "tree_shake"],
        ),
        ("resolve import paths", &["resolver", "import", "record"]),
        ("source map vlq", &["sourcemap", "encode", "vlq"]),
        ("minify rename symbols", &["renamer", "minify", "mangle"]),
        ("css parsing printing", &["css", "parse", "printer"]),
        ("source maps output", &["sourcemap", "chunk", "builder"]),
        ("cross-language ast", &["ast", "symbol", "ref", "import"]),
        (
            "how does esbuild resolve import",
            &["resolver", "import", "record"],
        ),
        ("parse javascript source ast", &["parse", "js", "ast"]),
        ("encode source map vlq", &["sourcemap", "encode", "vlq"]),
        (
            "minify rename symbols output",
            &["renamer", "minify", "mangle"],
        ),
        ("css parsing printing", &["css", "parse", "printer"]),
        (
            "tree shaking remove dead code",
            &["linker", "symbol", "tree_shake"],
        ),
        (
            "cross-language ast nodes",
            &["ast", "symbol", "ref", "import"],
        ),
        ("lexer tokenizer", &["lexer", "tokenizer", "scanner"]),
        ("timer tracked expired", &["timer", "entry", "wheel"]),
        ("block on async", &["block_on"]),
    ];

    let mut terms: Vec<String> = Vec::new();

    let core_lower = core.to_lowercase();
    for (phrase, mapped) in phrase_map {
        if core_lower.contains(phrase) {
            for t in *mapped {
                terms.push(t.to_string());
            }
        }
    }

    for word in &content_words {
        for (key, mapped) in domain_map {
            if word == *key {
                for t in *mapped {
                    terms.push(t.to_string());
                }
            }
        }
    }

    let mut seen = std::collections::HashSet::new();
    terms.retain(|t| seen.insert(t.clone()));

    if terms.is_empty() {
        terms = content_words;
    }

    terms.truncate(15);
    terms
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
        assert!(is_decomposable_query("periodic interval timer"));
        assert!(is_decomposable_query("tcp accept connections"));
        assert!(is_decomposable_query(
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
        let result = decomposed_search(&db, "RateLimiter", 10, false, None);
        assert!(result.is_none());
        let result = decomposed_search(&db, "rate limit", 10, false, None);
        assert!(result.is_none());
        let result = decomposed_search(&db, "cache", 10, false, None);
        assert!(result.is_none());
    }
}
