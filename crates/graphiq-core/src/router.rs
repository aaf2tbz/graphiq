#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RetrievalPath {
    LexicalStructural,
    SemanticStructural,
    Dual,
}

pub struct RouterDecision {
    pub path: RetrievalPath,
    pub confidence: f64,
}

pub fn route_query(query: &str) -> RouterDecision {
    let tokens: Vec<&str> = query.split_whitespace().collect();
    if tokens.is_empty() {
        return RouterDecision {
            path: RetrievalPath::LexicalStructural,
            confidence: 1.0,
        };
    }

    let lower = query.to_lowercase();

    let has_code_ident = looks_like_code_identifier(query);
    let has_file_ext = looks_like_file_path(query);
    let has_tech_token = tokens.iter().any(|t| is_tech_token(t));
    let has_wh_prefix = has_wh_prefix(&lower);
    let has_action_verb = has_action_verb(tokens.first().copied());
    let avg_word_len =
        tokens.iter().map(|t| t.len() as f64).sum::<f64>() / tokens.len().max(1) as f64;
    let short_ratio = tokens.iter().filter(|t| t.len() <= 3).count() as f64 / tokens.len() as f64;
    let has_camel_or_snake = (query.chars().filter(|c| c.is_uppercase()).count() >= 2
        && query.chars().any(|c| c == '_'))
        || tokens.iter().any(|t| t.contains('_') && t.len() > 3);

    let mut lexical_score = 0.0f64;
    let mut semantic_score = 0.0f64;

    if lower.starts_with("all ") || lower.starts_with("every ") {
        lexical_score += 3.0;
    }
    if has_code_ident {
        lexical_score += 3.0;
    }
    if has_file_ext {
        lexical_score += 3.0;
    }
    if has_tech_token {
        lexical_score += 2.0;
    }
    if has_camel_or_snake {
        lexical_score += 2.0;
    }
    if avg_word_len > 6.0 {
        lexical_score += 1.0;
    }
    if short_ratio < 0.3 {
        lexical_score += 0.5;
    }
    if !has_wh_prefix && !has_action_verb {
        lexical_score += 1.0;
    }
    if tokens.len() == 1 {
        lexical_score += 2.0;
    }

    if has_wh_prefix {
        semantic_score += 3.0;
    }
    if has_action_verb {
        semantic_score += 2.0;
    }
    if tokens.len() >= 5 {
        semantic_score += 1.5;
    }
    if avg_word_len < 5.0 {
        semantic_score += 0.5;
    }
    if !has_camel_or_snake && !has_file_ext {
        semantic_score += 1.0;
    }
    if looks_like_question(&lower) {
        semantic_score += 1.0;
    }

    let (path, confidence) = if lexical_score >= semantic_score + 1.5 {
        (
            RetrievalPath::LexicalStructural,
            lexical_score / (lexical_score + semantic_score + 0.01),
        )
    } else if semantic_score >= lexical_score + 5.0 {
        (
            RetrievalPath::SemanticStructural,
            semantic_score / (lexical_score + semantic_score + 0.01),
        )
    } else {
        (RetrievalPath::Dual, 0.5)
    };

    RouterDecision { path, confidence }
}

fn has_wh_prefix(lower: &str) -> bool {
    lower.starts_with("how ")
        || lower.starts_with("what ")
        || lower.starts_with("where ")
        || lower.starts_with("why ")
        || lower.starts_with("when ")
        || lower.starts_with("who ")
        || lower.starts_with("which ")
}

fn has_action_verb(first: Option<&str>) -> bool {
    first
        .map(|w| {
            matches!(
                w.to_lowercase().as_str(),
                "find"
                    | "get"
                    | "search"
                    | "show"
                    | "list"
                    | "compute"
                    | "calculate"
                    | "parse"
                    | "validate"
                    | "check"
                    | "handle"
                    | "create"
                    | "delete"
                    | "update"
                    | "build"
                    | "generate"
                    | "encode"
                    | "decode"
                    | "scan"
                    | "insert"
                    | "prune"
                    | "embed"
                    | "run"
                    | "send"
                    | "receive"
                    | "acquire"
                    | "wait"
                    | "spawn"
                    | "set"
                    | "convert"
                    | "substitute"
                    | "generate"
            )
        })
        .unwrap_or(false)
}

fn looks_like_question(lower: &str) -> bool {
    lower.contains("?") || has_wh_prefix(lower)
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

fn is_tech_token(t: &str) -> bool {
    matches!(
        t.to_lowercase().as_str(),
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
            | "tcp"
            | "udp"
            | "io"
            | "rpc"
            | "lsm"
            | "hrr"
            | "ndcg"
            | "rrf"
            | "bm25"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_route_code_queries() {
        assert_eq!(
            route_query("RateLimiter").path,
            RetrievalPath::LexicalStructural
        );
        assert_eq!(
            route_query("insertMemory").path,
            RetrievalPath::LexicalStructural
        );
        assert_eq!(
            route_query("cache.rs").path,
            RetrievalPath::LexicalStructural
        );
        assert_eq!(route_query("bm25").path, RetrievalPath::LexicalStructural);
        assert_eq!(route_query("search").path, RetrievalPath::LexicalStructural);
        assert_eq!(route_query("embed").path, RetrievalPath::LexicalStructural);
    }

    #[test]
    fn test_route_nl_queries() {
        assert_eq!(
            route_query("how does the extraction pipeline work").path,
            RetrievalPath::SemanticStructural
        );
        assert_eq!(
            route_query("how are errors propagated through the system").path,
            RetrievalPath::SemanticStructural
        );
        assert_eq!(
            route_query("what determines the ranking order of search results").path,
            RetrievalPath::SemanticStructural
        );
    }

    #[test]
    fn test_route_descriptive_queries() {
        let d1 = route_query("compute cosine similarity between embeddings");
        assert!(matches!(
            d1.path,
            RetrievalPath::SemanticStructural | RetrievalPath::Dual
        ));
    }

    #[test]
    fn test_route_file_path() {
        assert_eq!(
            route_query("search.rs").path,
            RetrievalPath::LexicalStructural
        );
        assert_eq!(
            route_query("sourcemap.go").path,
            RetrievalPath::LexicalStructural
        );
    }

    #[test]
    fn test_route_error_debug() {
        let d = route_query("failed to start daemon");
        assert!(matches!(
            d.path,
            RetrievalPath::SemanticStructural | RetrievalPath::Dual
        ));
    }

    #[test]
    fn test_route_cross_cutting() {
        let d = route_query("all symbol kinds in the type system");
        assert!(matches!(
            d.path,
            RetrievalPath::LexicalStructural | RetrievalPath::Dual
        ));
    }
}
