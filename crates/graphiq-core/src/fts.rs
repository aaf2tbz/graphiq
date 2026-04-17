use rusqlite::params;

use crate::db::GraphDb;
use crate::symbol::{Symbol, SymbolKind, Visibility};

#[derive(Debug, Clone)]
pub struct FtsResult {
    pub symbol: Symbol,
    pub bm25_score: f64,
}

#[derive(Debug, Clone)]
pub struct FtsConfig {
    pub max_candidates: usize,
    pub column_weights: [f64; 10],
}

impl Default for FtsConfig {
    fn default() -> Self {
        Self {
            max_candidates: 200,
            column_weights: [
                10.0, // name
                8.0,  // name_decomposed
                6.0,  // qualified_name
                4.0,  // signature
                1.0,  // source
                3.0,  // doc_comment
                3.5,  // file_path
                0.5,  // kind
                0.5,  // language
                5.0,  // search_hints
            ],
        }
    }
}

pub struct FtsSearch<'a> {
    db: &'a GraphDb,
    config: FtsConfig,
}

impl<'a> FtsSearch<'a> {
    pub fn new(db: &'a GraphDb) -> Self {
        Self {
            db,
            config: FtsConfig::default(),
        }
    }

    pub fn with_config(db: &'a GraphDb, config: FtsConfig) -> Self {
        Self { db, config }
    }

    pub fn search(&self, query: &str, limit: Option<usize>) -> Vec<FtsResult> {
        let limit = limit.unwrap_or(self.config.max_candidates);
        let tokens = tokenize_query(query);
        if tokens.is_empty() {
            return Vec::new();
        }

        let content_tokens: Vec<String> = tokens
            .iter()
            .filter(|t| !is_stop_word(t))
            .cloned()
            .collect();

        let and_tokens = if content_tokens.len() >= 2 {
            &content_tokens
        } else {
            &tokens
        };
        let and_query = build_fts_query(and_tokens, false);
        let results = self.run_fts_query(&and_query, limit);

        let fallback_threshold = if content_tokens.len() >= 3 { 30 } else { 10 };
        if results.len() < fallback_threshold {
            let or_query = build_fts_query(&tokens, true);
            let or_results = self.run_fts_query(&or_query, limit);
            let mut merged = results;
            for r in or_results {
                if !merged.iter().any(|e| e.symbol.id == r.symbol.id) {
                    merged.push(r);
                }
            }
            merged.sort_by(|a, b| b.bm25_score.partial_cmp(&a.bm25_score).unwrap());
            merged.truncate(limit);
            merged
        } else {
            results
        }
    }

    fn run_fts_query(&self, fts_query: &str, limit: usize) -> Vec<FtsResult> {
        let w = &self.config.column_weights;
        let sql = format!(
            "SELECT sym.id, sym.file_id, sym.name, sym.qualified_name, sym.kind, sym.line_start, sym.line_end,
                    sym.signature, sym.visibility, sym.doc_comment, sym.source, sym.name_decomposed,
                    sym.content_hash, sym.language, sym.metadata, sym.importance, sym.search_hints,
                    bm25(symbols_fts, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}) as score
             FROM symbols_fts
             JOIN symbols sym ON sym.id = symbols_fts.rowid
             WHERE symbols_fts MATCH ?1
             ORDER BY score
             LIMIT ?2",
            w[0], w[1], w[2], w[3], w[4], w[5], w[6], w[7], w[8], w[9],
        );

        let conn = self.db.conn();
        let mut stmt = match conn.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };

        let rows = match stmt.query_map(params![fts_query, limit as i64], |row| {
            Ok(row_to_fts_result(row))
        }) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };

        let mut results: Vec<FtsResult> = rows
            .flatten()
            .filter(|r| r.bm25_score.is_finite() && r.bm25_score > 0.0)
            .collect();
        results.sort_by(|a, b| b.bm25_score.partial_cmp(&a.bm25_score).unwrap());
        results
    }
}

fn row_to_fts_result(row: &rusqlite::Row) -> FtsResult {
    let hash_bytes: Vec<u8> = row.get(12).unwrap_or_default();
    let kind_str: String = row.get(4).unwrap_or_default();
    let vis_str: String = row.get(8).unwrap_or_default();
    let meta_str: String = row.get(14).unwrap_or_else(|_| "{}".into());

    let symbol = Symbol {
        id: row.get(0).unwrap_or(0),
        file_id: row.get(1).unwrap_or(0),
        name: row.get(2).unwrap_or_default(),
        qualified_name: row.get(3).unwrap_or_default(),
        kind: SymbolKind::from_str(&kind_str).unwrap_or(SymbolKind::Section),
        line_start: row.get(5).unwrap_or(0),
        line_end: row.get(6).unwrap_or(0),
        signature: row.get(7).unwrap_or_default(),
        visibility: Visibility::from_str(&vis_str).unwrap_or(Visibility::Public),
        doc_comment: row.get(9).unwrap_or_default(),
        source: row.get(10).unwrap_or_default(),
        name_decomposed: row.get(11).unwrap_or_default(),
        content_hash: String::from_utf8_lossy(&hash_bytes).to_string(),
        language: row.get(13).unwrap_or_default(),
        metadata: serde_json::from_str(&meta_str).unwrap_or(serde_json::Value::Null),
        importance: row.get(15).unwrap_or(0.5),
        search_hints: row.get(16).unwrap_or_default(),
    };
    let score: f64 = row.get(17).unwrap_or(0.0);
    FtsResult {
        symbol,
        bm25_score: if score < 0.0 { -score } else { 0.0 },
    }
}

fn tokenize_query(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .map(|t| t.to_lowercase())
        .filter(|t| t.len() >= 2)
        .collect()
}

fn is_stop_word(token: &str) -> bool {
    matches!(
        token,
        "the"
            | "a"
            | "an"
            | "is"
            | "are"
            | "was"
            | "were"
            | "be"
            | "been"
            | "being"
            | "have"
            | "has"
            | "had"
            | "do"
            | "does"
            | "did"
            | "will"
            | "would"
            | "could"
            | "should"
            | "may"
            | "might"
            | "can"
            | "shall"
            | "of"
            | "in"
            | "to"
            | "for"
            | "on"
            | "at"
            | "by"
            | "with"
            | "from"
            | "as"
            | "into"
            | "through"
            | "and"
            | "or"
            | "but"
            | "not"
            | "no"
            | "if"
            | "that"
            | "this"
            | "these"
            | "those"
            | "it"
            | "its"
            | "my"
            | "your"
            | "his"
            | "her"
            | "their"
            | "all"
            | "each"
            | "every"
            | "any"
            | "some"
            | "how"
            | "what"
            | "which"
            | "who"
            | "when"
            | "where"
            | "why"
            | "there"
            | "here"
            | "than"
            | "then"
            | "so"
            | "up"
            | "out"
            | "about"
            | "just"
    )
}

fn build_fts_query(tokens: &[String], is_or: bool) -> String {
    let joiner = if is_or { " OR " } else { " AND " };
    tokens
        .iter()
        .map(|t| format!("\"{}\"*", t.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(joiner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol::SymbolBuilder;

    fn setup_db_with_symbols() -> GraphDb {
        let db = GraphDb::open_in_memory().unwrap();
        let fid = db
            .upsert_file("src/auth.ts", "typescript", "abc", 1000, 100)
            .unwrap();

        let s1 = SymbolBuilder::new(
            fid,
            "authenticateUser".into(),
            SymbolKind::Function,
            "fn authenticateUser(token: string): Promise<User>".into(),
            "typescript".into(),
        )
        .lines(1, 10)
        .signature("fn authenticateUser(token: string): Promise<User>")
        .build();
        db.insert_symbol(&s1).unwrap();

        let s2 = SymbolBuilder::new(
            fid,
            "rateLimitMiddleware".into(),
            SymbolKind::Function,
            "fn rateLimitMiddleware(ctx: Context): Response".into(),
            "typescript".into(),
        )
        .lines(12, 25)
        .signature("fn rateLimitMiddleware(ctx: Context): Response")
        .build();
        db.insert_symbol(&s2).unwrap();

        let s3 = SymbolBuilder::new(
            fid,
            "AuthService".into(),
            SymbolKind::Class,
            "class AuthService { authenticate(token: string) }".into(),
            "typescript".into(),
        )
        .lines(27, 50)
        .signature("class AuthService")
        .build();
        db.insert_symbol(&s3).unwrap();

        db
    }

    #[test]
    fn test_fts_exact_match() {
        let db = setup_db_with_symbols();
        let fts = FtsSearch::new(&db);
        let results = fts.search("authenticateUser", None);
        assert!(!results.is_empty());
        assert_eq!(results[0].symbol.name, "authenticateUser");
    }

    #[test]
    fn test_fts_decomposed_match() {
        let db = setup_db_with_symbols();
        let fts = FtsSearch::new(&db);
        let results = fts.search("rate limit", None);
        assert!(!results.is_empty());
        assert!(results
            .iter()
            .any(|r| r.symbol.name == "rateLimitMiddleware"));
    }

    #[test]
    fn test_fts_partial_match() {
        let db = setup_db_with_symbols();
        let fts = FtsSearch::new(&db);
        let results = fts.search("auth", None);
        assert!(results.len() >= 2);
    }

    #[test]
    fn test_fts_empty_query() {
        let db = setup_db_with_symbols();
        let fts = FtsSearch::new(&db);
        let results = fts.search("", None);
        assert!(results.is_empty());
    }

    #[test]
    fn test_fts_no_results() {
        let db = setup_db_with_symbols();
        let fts = FtsSearch::new(&db);
        let results = fts.search("xyzzyNonExistent", None);
        assert!(results.is_empty());
    }

    #[test]
    fn test_build_fts_query_and() {
        let tokens = vec!["rate".into(), "limit".into()];
        let q = build_fts_query(&tokens, false);
        assert!(q.contains("AND"));
        assert!(q.contains("\"rate\"*"));
    }

    #[test]
    fn test_build_fts_query_or() {
        let tokens = vec!["auth".into(), "user".into()];
        let q = build_fts_query(&tokens, true);
        assert!(q.contains("OR"));
    }
}
