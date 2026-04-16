use std::collections::HashMap;

use crate::db::GraphDb;
use crate::fts::FtsResult;
use crate::graph::ExpansionEntry;
use crate::symbol::{Symbol, SymbolKind, Visibility};

#[derive(Debug, Clone)]
pub struct HeuristicConfig {
    pub density: bool,
    pub entry_point: bool,
    pub export_bias: bool,
    pub test_proximity: bool,
    pub importance: bool,
    pub recency: bool,
    pub name_exact: bool,
}

impl Default for HeuristicConfig {
    fn default() -> Self {
        Self {
            density: true,
            entry_point: true,
            export_bias: true,
            test_proximity: true,
            importance: true,
            recency: true,
            name_exact: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScoreBreakdown {
    pub layer2_score: f64,
    pub heuristics: Vec<(&'static str, f64)>,
    pub heuristic_multiplier: f64,
    pub path_weight: f64,
    pub diversity_dampen: f64,
    pub final_score: f64,
}

#[derive(Debug, Clone)]
pub struct ScoredSymbol {
    pub symbol: Symbol,
    pub score: f64,
    pub breakdown: Option<ScoreBreakdown>,
    pub is_fts_hit: bool,
    pub file_path: Option<String>,
}

pub struct Reranker {
    config: HeuristicConfig,
    debug: bool,
    file_mtimes: HashMap<i64, i64>,
    tested_symbols: Vec<i64>,
    query_tokens: Vec<String>,
}

impl Reranker {
    pub fn new(db: &GraphDb, debug: bool) -> Self {
        let file_mtimes = load_file_mtimes(db);
        let tested_symbols = load_tested_symbols(db);
        Self {
            config: HeuristicConfig::default(),
            debug,
            file_mtimes,
            tested_symbols,
            query_tokens: Vec::new(),
        }
    }

    pub fn with_config(db: &GraphDb, config: HeuristicConfig, debug: bool) -> Self {
        let file_mtimes = load_file_mtimes(db);
        let tested_symbols = load_tested_symbols(db);
        Self {
            config,
            debug,
            file_mtimes,
            tested_symbols,
            query_tokens: Vec::new(),
        }
    }

    pub fn for_query(mut self, query: &str) -> Self {
        self.query_tokens = query
            .split_whitespace()
            .map(|t| t.to_lowercase())
            .filter(|t| t.len() >= 2)
            .collect();
        self
    }

    pub fn rerank(
        &self,
        fts_results: &[FtsResult],
        expanded: &[ExpansionEntry],
        file_paths: &HashMap<i64, String>,
        top_k: usize,
    ) -> Vec<ScoredSymbol> {
        let mut candidates: Vec<ScoredSymbol> = Vec::new();

        for fts in fts_results {
            candidates.push(ScoredSymbol {
                symbol: fts.symbol.clone(),
                score: fts.bm25_score,
                breakdown: None,
                is_fts_hit: true,
                file_path: file_paths.get(&fts.symbol.file_id).cloned(),
            });
        }

        for exp in expanded {
            candidates.push(ScoredSymbol {
                symbol: exp.symbol.clone(),
                score: exp.score,
                breakdown: None,
                is_fts_hit: false,
                file_path: file_paths.get(&exp.symbol.file_id).cloned(),
            });
        }

        self.apply_heuristics(&mut candidates);
        self.apply_diversity_dampen(&mut candidates);

        candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        candidates.truncate(top_k);
        candidates
    }

    fn apply_heuristics(&self, candidates: &mut [ScoredSymbol]) {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        for c in candidates.iter_mut() {
            let sym = &c.symbol;
            let line_count = if sym.line_end > sym.line_start {
                sym.line_end - sym.line_start
            } else {
                1
            };

            let density = if self.config.density {
                (80.0 / line_count as f64).min(1.0)
            } else {
                1.0
            };

            let entry_boost = if self.config.entry_point {
                let path = c.file_path.as_deref().unwrap_or("");
                if is_entry_point(path) {
                    1.15
                } else {
                    1.0
                }
            } else {
                1.0
            };

            let export_boost = if self.config.export_bias {
                if sym.visibility == Visibility::Public
                    && matches!(
                        sym.kind,
                        SymbolKind::Function
                            | SymbolKind::Class
                            | SymbolKind::Interface
                            | SymbolKind::Struct
                            | SymbolKind::Enum
                            | SymbolKind::Trait
                            | SymbolKind::TypeAlias
                    )
                {
                    1.1
                } else {
                    1.0
                }
            } else {
                1.0
            };

            let test_boost = if self.config.test_proximity {
                if self.tested_symbols.contains(&sym.id) {
                    1.1
                } else {
                    1.0
                }
            } else {
                1.0
            };

            let importance_factor = if self.config.importance {
                0.5 + 0.5 * sym.importance.min(1.0)
            } else {
                1.0
            };

            let recency = if self.config.recency {
                let mtime = self.file_mtimes.get(&sym.file_id).copied().unwrap_or(0);
                let days = ((now_ms - mtime) as f64) / 86400000.0;
                1.0 / (1.0 + days / 90.0)
            } else {
                1.0
            };

            let name_exact = if self.config.name_exact && !self.query_tokens.is_empty() {
                let name_lower = sym.name.to_lowercase();
                let decomposed_lower = sym.name_decomposed.to_lowercase();
                let matches_query = self
                    .query_tokens
                    .iter()
                    .all(|t| name_lower.contains(t) || decomposed_lower.contains(t));
                let exact_name = self.query_tokens.len() == 1 && name_lower == self.query_tokens[0];
                if exact_name {
                    1.5
                } else if matches_query {
                    1.25
                } else {
                    1.0
                }
            } else {
                1.0
            };

            let heuristic_multiplier = density
                * entry_boost
                * export_boost
                * test_boost
                * importance_factor
                * recency
                * name_exact;

            if self.debug {
                c.breakdown = Some(ScoreBreakdown {
                    layer2_score: c.score,
                    heuristics: vec![
                        ("density", density),
                        ("entry", entry_boost),
                        ("export", export_boost),
                        ("test_prox", test_boost),
                        ("importance", importance_factor),
                        ("recency", recency),
                        ("name_exact", name_exact),
                    ],
                    heuristic_multiplier,
                    path_weight: 1.0,
                    diversity_dampen: 1.0,
                    final_score: c.score * heuristic_multiplier,
                });
            }

            c.score *= heuristic_multiplier;
        }
    }

    fn apply_diversity_dampen(&self, candidates: &mut [ScoredSymbol]) {
        candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());

        let mut file_counts: HashMap<i64, usize> = HashMap::new();
        for c in candidates.iter_mut() {
            let count = file_counts.entry(c.symbol.file_id).or_insert(0);
            let dampen = 0.85_f64.powi(*count as i32);
            c.score *= dampen;
            if let Some(ref mut bd) = c.breakdown {
                bd.diversity_dampen = dampen;
                bd.final_score = c.score;
            }
            *count += 1;
        }
    }
}

fn load_file_mtimes(db: &GraphDb) -> HashMap<i64, i64> {
    let conn = db.conn();
    let mut stmt = match conn.prepare("SELECT id, mtime_ms FROM files") {
        Ok(s) => s,
        Err(_) => return HashMap::new(),
    };
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
        .ok();
    match rows {
        Some(r) => r.flatten().collect(),
        None => HashMap::new(),
    }
}

fn load_tested_symbols(db: &GraphDb) -> Vec<i64> {
    let conn = db.conn();
    let mut stmt = match conn.prepare("SELECT target_id FROM edges WHERE kind = 'tests'") {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = stmt.query_map([], |row| row.get::<_, i64>(0)).ok();
    match rows {
        Some(r) => r.flatten().collect(),
        None => Vec::new(),
    }
}

fn is_entry_point(path: &str) -> bool {
    let path_lower = path.to_lowercase();
    let patterns = ["/main.", "/index.", "/app.", "/server.", "/mod.", "/lib."];
    patterns.iter().any(|p| path_lower.contains(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_symbol(
        id: i64,
        name: &str,
        kind: SymbolKind,
        line_start: u32,
        line_end: u32,
    ) -> Symbol {
        Symbol {
            id,
            file_id: 1,
            name: name.into(),
            qualified_name: None,
            kind,
            line_start,
            line_end,
            signature: None,
            visibility: Visibility::Public,
            doc_comment: None,
            source: format!("fn {}()", name),
            name_decomposed: crate::tokenize::decompose_identifier(name),
            content_hash: "abc".into(),
            language: "typescript".into(),
            metadata: serde_json::Value::Null,
            importance: 0.9,
        }
    }

    fn setup_db() -> GraphDb {
        let db = GraphDb::open_in_memory().unwrap();
        db.upsert_file("src/main.ts", "typescript", "abc", 1000, 100)
            .unwrap();
        db
    }

    #[test]
    fn test_rerank_basic() {
        let db = setup_db();
        let reranker = Reranker::new(&db, false);

        let fts_results = vec![FtsResult {
            symbol: make_symbol(1, "authenticate", SymbolKind::Function, 1, 10),
            bm25_score: 5.0,
        }];

        let file_paths = HashMap::from([(1i64, "src/main.ts".into())]);
        let results = reranker.rerank(&fts_results, &[], &file_paths, 10);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].symbol.name, "authenticate");
        assert!(results[0].score > 0.0);
    }

    #[test]
    fn test_density_penalty() {
        let db = setup_db();
        let reranker = Reranker::new(&db, true);

        let small = FtsResult {
            symbol: make_symbol(1, "small", SymbolKind::Function, 1, 10),
            bm25_score: 5.0,
        };
        let large = FtsResult {
            symbol: make_symbol(2, "large", SymbolKind::Function, 1, 400),
            bm25_score: 5.0,
        };

        let file_paths = HashMap::from([(1i64, "src/main.ts".into())]);
        let results = reranker.rerank(&[small, large], &[], &file_paths, 10);

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].symbol.name, "small");
    }

    #[test]
    fn test_entry_point_boost() {
        let db = GraphDb::open_in_memory().unwrap();
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        db.upsert_file("src/main.ts", "typescript", "abc", now_ms, 100)
            .unwrap();

        let reranker = Reranker::new(&db, false);

        let s1 = FtsResult {
            symbol: make_symbol(1, "foo", SymbolKind::Function, 1, 10),
            bm25_score: 5.0,
        };

        let file_paths = HashMap::from([(1i64, "src/main.ts".into())]);
        let results = reranker.rerank(&[s1], &[], &file_paths, 10);

        assert_eq!(results.len(), 1);
        assert!(results[0].score > 5.0);
    }

    #[test]
    fn test_diversity_dampen() {
        let db = setup_db();
        let reranker = Reranker::new(&db, false);

        let fts_results = vec![
            FtsResult {
                symbol: make_symbol(1, "foo", SymbolKind::Function, 1, 10),
                bm25_score: 5.0,
            },
            FtsResult {
                symbol: make_symbol(2, "bar", SymbolKind::Function, 20, 30),
                bm25_score: 4.9,
            },
        ];

        let file_paths = HashMap::from([(1i64, "src/main.ts".into())]);
        let results = reranker.rerank(&fts_results, &[], &file_paths, 10);

        assert_eq!(results.len(), 2);
        assert!(results[0].score > results[1].score);
    }

    #[test]
    fn test_is_entry_point() {
        assert!(is_entry_point("src/main.ts"));
        assert!(is_entry_point("src/index.js"));
        assert!(is_entry_point("src/server.rs"));
        assert!(!is_entry_point("src/utils.ts"));
    }
}
