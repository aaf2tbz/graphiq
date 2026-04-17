use std::collections::HashMap;

use crate::db::GraphDb;
use crate::directory_expand::DirectorySibling;
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

#[derive(Debug, Clone, Default)]
pub struct EvidenceChannels {
    pub lexical: bool,
    pub structural: bool,
    pub test: bool,
    pub path: bool,
    pub hints: bool,
}

impl EvidenceChannels {
    pub fn count(&self) -> usize {
        [
            self.lexical,
            self.structural,
            self.test,
            self.path,
            self.hints,
        ]
        .iter()
        .filter(|&&b| b)
        .count()
    }

    pub fn agreement_mult(&self) -> f64 {
        match self.count() {
            0 => 0.9,
            1 => 0.95,
            2 => 1.05,
            3 => 1.12,
            4 => 1.18,
            _ => 1.22,
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
    pub channels: EvidenceChannels,
    pub channel_agreement: f64,
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
    file_path_query: bool,
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
            file_path_query: false,
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
            file_path_query: false,
        }
    }

    pub fn for_query(mut self, query: &str) -> Self {
        self.query_tokens = query
            .split_whitespace()
            .map(|t| t.to_lowercase())
            .filter(|t| t.len() >= 2)
            .collect();
        self.file_path_query = looks_like_file_path(query);
        self
    }

    pub fn rerank(
        &self,
        fts_results: &[FtsResult],
        expanded: &[ExpansionEntry],
        cross_cutting: &[DirectorySibling],
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

        if !cross_cutting.is_empty() {
            let best_fts_score = fts_results
                .iter()
                .map(|r| r.bm25_score)
                .fold(0.0f64, f64::max);
            for sib in cross_cutting {
                let fp = file_paths.get(&sib.symbol.file_id).cloned();
                candidates.push(ScoredSymbol {
                    symbol: sib.symbol.clone(),
                    score: best_fts_score * sib.proximity,
                    breakdown: None,
                    is_fts_hit: false,
                    file_path: fp,
                });
            }
        }

        self.apply_heuristics(&mut candidates);
        self.apply_diversity_dampen(&mut candidates);

        candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        candidates.truncate(top_k);
        candidates
    }

    pub fn apply_heuristics(&self, candidates: &mut [ScoredSymbol]) {
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

            let is_nl = is_nl_query(&self.query_tokens);
            let path = c.file_path.as_deref().unwrap_or("");
            let in_test_file = is_test_file(path);

            let test_file_penalty = if is_nl && in_test_file { 0.5 } else { 1.0 };

            let production_boost = if is_nl && !in_test_file { 1.5 } else { 1.0 };

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
                if is_container_kind(sym.kind) {
                    if exact_name {
                        1.0
                    } else if matches_query {
                        1.0
                    } else {
                        1.0
                    }
                } else if exact_name {
                    1.5
                } else if matches_query {
                    1.25
                } else {
                    1.0
                }
            } else {
                1.0
            };

            let name_coverage = if self.query_tokens.len() <= 3 && !self.query_tokens.is_empty() {
                let decomp_tokens: Vec<&str> = sym.name_decomposed.split_whitespace().collect();
                if decomp_tokens.is_empty() {
                    1.0
                } else {
                    let matched = decomp_tokens
                        .iter()
                        .filter(|dt| {
                            self.query_tokens
                                .iter()
                                .any(|qt| dt.contains(qt.as_str()) || qt.as_str().contains(**dt))
                        })
                        .count();
                    let coverage = matched as f64 / decomp_tokens.len() as f64;
                    1.0 + (0.5 * coverage)
                }
            } else {
                1.0
            };

            let module_shadow = if is_container_kind(sym.kind) && !self.query_tokens.is_empty() {
                0.75
            } else {
                1.0
            };

            let file_path_boost = if self.file_path_query {
                let path = c.file_path.as_deref().unwrap_or("");
                let stem = path
                    .rsplit('/')
                    .next()
                    .unwrap_or(path)
                    .rsplit_once('.')
                    .map(|(n, _)| n)
                    .unwrap_or(path)
                    .to_lowercase();
                let name_lower = sym.name.to_lowercase();
                let decomp_lower = sym.name_decomposed.to_lowercase();

                if name_lower == stem && is_primary_definition(sym.kind) {
                    2.0
                } else if decomp_lower.contains(&stem) && is_primary_definition(sym.kind) {
                    1.5
                } else if name_lower == stem {
                    1.3
                } else {
                    let matches_file = self
                        .query_tokens
                        .iter()
                        .any(|t| stem.contains(t) || t.contains(&stem));
                    if matches_file && is_primary_definition(sym.kind) {
                        1.2
                    } else {
                        1.0
                    }
                }
            } else {
                1.0
            };

            let full_coverage = {
                let content_tokens: Vec<&String> = self
                    .query_tokens
                    .iter()
                    .filter(|t| !is_query_stop_word(t))
                    .collect();
                if content_tokens.len() >= 3 {
                    let decomp_lower = sym.name_decomposed.to_lowercase();
                    let hints_lower = sym.search_hints.to_lowercase();
                    let covered = content_tokens
                        .iter()
                        .filter(|t| {
                            decomp_lower.contains(t.as_str()) || hints_lower.contains(t.as_str())
                        })
                        .count();
                    if covered == content_tokens.len() {
                        1.3
                    } else {
                        1.0
                    }
                } else {
                    1.0
                }
            };

            let mut channels = EvidenceChannels::default();
            channels.lexical = name_exact > 1.0;
            channels.structural = !c.is_fts_hit;
            channels.test = test_boost > 1.0;
            channels.path = file_path_boost > 1.0;
            channels.hints = full_coverage > 1.0;
            let channel_agreement = channels.agreement_mult();

            let heuristic_multiplier = density
                * entry_boost
                * export_boost
                * test_boost
                * test_file_penalty
                * production_boost
                * importance_factor
                * recency
                * name_exact
                * name_coverage
                * module_shadow
                * file_path_boost
                * full_coverage
                * channel_agreement;

            if self.debug {
                c.breakdown = Some(ScoreBreakdown {
                    layer2_score: c.score,
                    heuristics: vec![
                        ("density", density),
                        ("entry", entry_boost),
                        ("export", export_boost),
                        ("test_prox", test_boost),
                        ("test_penalty", test_file_penalty),
                        ("prod_boost", production_boost),
                        ("importance", importance_factor),
                        ("recency", recency),
                        ("name_exact", name_exact),
                        ("name_coverage", name_coverage),
                        ("module_shadow", module_shadow),
                        ("file_path_boost", file_path_boost),
                        ("full_coverage", full_coverage),
                        ("channel_agree", channel_agreement),
                    ],
                    heuristic_multiplier,
                    path_weight: 1.0,
                    diversity_dampen: 1.0,
                    final_score: c.score * heuristic_multiplier,
                    channels,
                    channel_agreement,
                });
            }

            c.score *= heuristic_multiplier;
        }
    }

    pub fn apply_diversity_dampen(&self, candidates: &mut [ScoredSymbol]) {
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

fn is_container_kind(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Module | SymbolKind::Namespace | SymbolKind::Section | SymbolKind::Import
    )
}

fn is_primary_definition(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Struct
            | SymbolKind::Class
            | SymbolKind::Enum
            | SymbolKind::Interface
            | SymbolKind::Trait
    )
}

fn looks_like_file_path(query: &str) -> bool {
    let extensions = [
        ".rs", ".ts", ".tsx", ".js", ".jsx", ".py", ".go", ".java", ".c", ".cpp", ".h", ".rb",
        ".yaml", ".yml", ".toml", ".json", ".html", ".css", ".scss",
    ];
    let lower = query.to_lowercase();
    extensions.iter().any(|ext| lower.contains(ext))
}

fn is_nl_query(tokens: &[String]) -> bool {
    if tokens.len() < 3 {
        return false;
    }
    let has_code_pattern = tokens.iter().any(|t| {
        t.contains('_') && t.len() > 4
            || t.contains("::")
            || t.chars().filter(|c| c.is_uppercase()).count() >= 2
    });
    if has_code_pattern {
        return false;
    }
    let short_count = tokens.iter().filter(|t| t.len() <= 3).count();
    (short_count as f64) / (tokens.len() as f64) < 0.5
}

fn is_test_file(path: &str) -> bool {
    let lower = path.to_lowercase();
    let patterns = [
        "/test",
        "/tests/",
        "/__tests__/",
        "/spec/",
        "_test.",
        "_spec.",
        ".test.",
        ".spec.",
        "test_",
        "/benches/",
        "/benchmark/",
    ];
    patterns.iter().any(|p| lower.contains(p))
}

fn is_query_stop_word(token: &str) -> bool {
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
            search_hints: String::new(),
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
        let results = reranker.rerank(&fts_results, &[], &[], &file_paths, 10);

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
        let results = reranker.rerank(&[small, large], &[], &[], &file_paths, 10);

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
        let results = reranker.rerank(&[s1], &[], &[], &file_paths, 10);

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
        let results = reranker.rerank(&fts_results, &[], &[], &file_paths, 10);

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
