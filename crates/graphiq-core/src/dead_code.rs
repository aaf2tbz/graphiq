//! Dead code detection — find symbols with zero incoming callers.
//!
//! Identifies symbols that have no incoming Calls, References, or Overrides
//! edges, minus a set of exemption rules (entry points, exported API surface,
//! trait definitions, test subjects, constructors, etc.).
//!
//! Key function: [`detect_dead_code`] — returns grouped results by file.

use crate::db::GraphDb;
use crate::symbol::{SymbolKind, Visibility};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadCodeResult {
    pub total_dead_symbols: usize,
    pub estimated_dead_loc: u32,
    pub files: Vec<FileDeadCode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDeadCode {
    pub path: String,
    pub dead_symbols: Vec<String>,
    pub dead_loc: u32,
}

#[derive(Debug, Clone)]
struct SymbolInfo {
    id: i64,
    name: String,
    kind: SymbolKind,
    visibility: Visibility,
    file_id: i64,
    line_start: u32,
    line_end: u32,
}

pub fn detect_dead_code(db: &GraphDb) -> Result<DeadCodeResult, String> {
    let conn = db.conn();

    let mut stmt = conn
        .prepare(
            "SELECT id, file_id, name, kind, visibility, line_start, line_end
             FROM symbols",
        )
        .map_err(|e| format!("query symbols: {e}"))?;

    let rows: Vec<SymbolInfo> = stmt
        .query_map([], |row| {
            let kind_str: String = row.get(3)?;
            let vis_str: String = row.get(4)?;
            Ok(SymbolInfo {
                id: row.get(0)?,
                file_id: row.get(1)?,
                name: row.get(2)?,
                kind: SymbolKind::from_str(&kind_str).unwrap_or(SymbolKind::Function),
                visibility: Visibility::from_str(&vis_str).unwrap_or(Visibility::Public),
                line_start: row.get(5)?,
                line_end: row.get(6)?,
            })
        })
        .map_err(|e| format!("iterate symbols: {e}"))?
        .filter_map(|r| r.ok())
        .filter(|s| {
            !matches!(
                s.kind,
                SymbolKind::Import | SymbolKind::Export | SymbolKind::Section
            )
        })
        .collect();

    let mut stmt_incoming = conn
        .prepare(
            "SELECT target_id, kind FROM edges WHERE kind IN ('calls', 'references', 'overrides')",
        )
        .map_err(|e| format!("query incoming edges: {e}"))?;

    let has_callers: HashSet<i64> = stmt_incoming
        .query_map([], |row| {
            let target_id: i64 = row.get(0)?;
            Ok(target_id)
        })
        .map_err(|e| format!("iterate incoming: {e}"))?
        .filter_map(|r| r.ok())
        .collect();

    let mut stmt_extends_target = conn
        .prepare(
            "SELECT target_id FROM edges WHERE kind IN ('extends', 'implements')",
        )
        .map_err(|e| format!("query extends edges: {e}"))?;

    let extends_targets: HashSet<i64> = stmt_extends_target
        .query_map([], |row| row.get::<_, i64>(0))
        .map_err(|e| format!("iterate extends: {e}"))?
        .filter_map(|r| r.ok())
        .collect();

    let mut stmt_tests_target = conn
        .prepare("SELECT target_id FROM edges WHERE kind = 'tests'")
        .map_err(|e| format!("query tests edges: {e}"))?;

    let tests_targets: HashSet<i64> = stmt_tests_target
        .query_map([], |row| row.get::<_, i64>(0))
        .map_err(|e| format!("iterate tests: {e}"))?
        .filter_map(|r| r.ok())
        .collect();

    let mut stmt_contains_parent = conn
        .prepare("SELECT source_id FROM edges WHERE kind = 'contains'")
        .map_err(|e| format!("query contains edges: {e}"))?;

    let _contained_children: HashSet<i64> = stmt_contains_parent
        .query_map([], |row| row.get::<_, i64>(1))
        .map_err(|e| format!("iterate contains: {e}"))?
        .filter_map(|r| r.ok())
        .collect();

    let mut stmt_incoming_all = conn
        .prepare("SELECT target_id FROM edges WHERE kind = 'contains'")
        .map_err(|e| format!("query contains incoming: {e}"))?;

    let has_contains_incoming: HashSet<i64> = stmt_incoming_all
        .query_map([], |row| row.get::<_, i64>(0))
        .map_err(|e| format!("iterate contains incoming: {e}"))?
        .filter_map(|r| r.ok())
        .collect();

    let role_hints = load_role_hints(db);

    let test_files = load_test_files(db);

    let public_api_files = load_public_api_files(db);

    let mut dead: Vec<&SymbolInfo> = Vec::new();

    for sym in &rows {
        if has_callers.contains(&sym.id) {
            continue;
        }

        if is_exempt(
            sym,
            &extends_targets,
            &tests_targets,
            &has_contains_incoming,
            &role_hints,
            &test_files,
            &public_api_files,
        ) {
            continue;
        }

        dead.push(sym);
    }

    let mut file_map: HashMap<i64, Vec<&SymbolInfo>> = HashMap::new();
    for sym in &dead {
        file_map.entry(sym.file_id).or_default().push(sym);
    }

    let mut files: Vec<FileDeadCode> = Vec::new();
    let mut total_dead_loc = 0u32;

    for (file_id, syms) in &file_map {
        let path = db
            .file_path_for_id(*file_id)
            .ok()
            .flatten()
            .unwrap_or_else(|| format!("file_id:{}", file_id));

        let dead_loc: u32 = syms.iter().map(|s| s.line_end.saturating_sub(s.line_start).max(1)).sum();
        total_dead_loc += dead_loc;

        let mut names: Vec<String> = syms.iter().map(|s| s.name.clone()).collect();
        names.sort();

        files.push(FileDeadCode {
            path,
            dead_symbols: names,
            dead_loc,
        });
    }

    files.sort_by(|a, b| b.dead_loc.cmp(&a.dead_loc));

    Ok(DeadCodeResult {
        total_dead_symbols: dead.len(),
        estimated_dead_loc: total_dead_loc,
        files,
    })
}

fn is_exempt(
    sym: &SymbolInfo,
    extends_targets: &HashSet<i64>,
    tests_targets: &HashSet<i64>,
    has_contains_incoming: &HashSet<i64>,
    role_hints: &HashSet<i64>,
    test_files: &HashSet<i64>,
    public_api_files: &HashSet<i64>,
) -> bool {
    if role_hints.contains(&sym.id) {
        return true;
    }

    if extends_targets.contains(&sym.id) {
        return true;
    }

    if tests_targets.contains(&sym.id) {
        return true;
    }

    if has_contains_incoming.contains(&sym.id) {
        return true;
    }

    if matches!(
        sym.kind,
        SymbolKind::Trait | SymbolKind::Interface | SymbolKind::Constructor
    ) {
        return true;
    }

    let name_lower = sym.name.to_lowercase();
    if name_lower == "new"
        || name_lower == "init"
        || name_lower == "drop"
        || name_lower == "default"
    {
        return true;
    }

    if !test_files.contains(&sym.file_id)
        && sym.visibility == Visibility::Public
        && !matches!(sym.kind, SymbolKind::Field | SymbolKind::Constant)
    {
        if public_api_files.contains(&sym.file_id) || sym.visibility == Visibility::Public {
            return true;
        }
    }

    false
}

fn load_role_hints(db: &GraphDb) -> HashSet<i64> {
    let conn = db.conn();
    let mut stmt = match conn.prepare(
        "SELECT symbol_id FROM symbol_structural_roles WHERE roles LIKE '%entry_point%'",
    ) {
        Ok(s) => s,
        Err(_) => {
            let fallback = HashSet::new();
            let mut s2 = match conn.prepare(
                "SELECT s.id FROM symbols s WHERE s.search_hints LIKE '%entry point%'",
            ) {
                Ok(st) => st,
                Err(_) => return fallback,
            };
            let rows: Vec<i64> = s2
                .query_map([], |row| row.get(0))
                .ok()
                .map(|r| r.flatten().collect())
                .unwrap_or_default();
            return rows.into_iter().collect();
        }
    };

    let rows: Vec<i64> = stmt
        .query_map([], |row| row.get(0))
        .ok()
        .map(|r| r.flatten().collect())
        .unwrap_or_default();

    rows.into_iter().collect()
}

fn load_test_files(db: &GraphDb) -> HashSet<i64> {
    let conn = db.conn();
    let mut stmt = match conn.prepare(
        "SELECT id FROM files WHERE path LIKE '%test%' OR path LIKE '%spec%' OR path LIKE '%__tests__%'",
    ) {
        Ok(s) => s,
        Err(_) => return HashSet::new(),
    };

    let rows: Vec<i64> = stmt
        .query_map([], |row| row.get(0))
        .ok()
        .map(|r| r.flatten().collect())
        .unwrap_or_default();

    rows.into_iter().collect()
}

fn load_public_api_files(db: &GraphDb) -> HashSet<i64> {
    let conn = db.conn();
    let mut stmt = match conn.prepare(
        "SELECT id FROM files WHERE path LIKE '%/index.%' OR path LIKE '%/mod.%' OR path LIKE '%/lib.%' OR path LIKE '%/main.%' OR path LIKE '%/public.%'",
    ) {
        Ok(s) => s,
        Err(_) => return HashSet::new(),
    };

    let rows: Vec<i64> = stmt
        .query_map([], |row| row.get(0))
        .ok()
        .map(|r| r.flatten().collect())
        .unwrap_or_default();

    rows.into_iter().collect()
}

pub fn format_dead_code_report(result: &DeadCodeResult) -> String {
    let mut lines = Vec::new();
    lines.push(format!(
        "Dead Code: {} symbols, ~{} LOC",
        result.total_dead_symbols, result.estimated_dead_loc
    ));
    lines.push(String::new());

    for file in &result.files {
        lines.push(format!(
            "  {} ({} dead, ~{} LOC)",
            file.path,
            file.dead_symbols.len(),
            file.dead_loc
        ));
        for name in &file.dead_symbols {
            lines.push(format!("    - {}", name));
        }
    }

    if result.files.is_empty() {
        lines.push("No dead code detected.".into());
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edge::EdgeKind;
    use crate::symbol::{Symbol, SymbolBuilder, Visibility};

    fn make_symbol(id: i64, name: &str, kind: SymbolKind) -> Symbol {
        SymbolBuilder::new(1, name.into(), kind, format!("fn {}() {{}}", name), "rust".into())
            .lines(1, 5)
            .build()
    }

    #[test]
    fn test_dead_code_result_format() {
        let result = DeadCodeResult {
            total_dead_symbols: 3,
            estimated_dead_loc: 45,
            files: vec![FileDeadCode {
                path: "src/legacy.rs".into(),
                dead_symbols: vec!["old_fn".into(), "unused_helper".into()],
                dead_loc: 30,
            }],
        };
        let report = format_dead_code_report(&result);
        assert!(report.contains("Dead Code: 3 symbols"));
        assert!(report.contains("~45 LOC"));
        assert!(report.contains("src/legacy.rs"));
        assert!(report.contains("old_fn"));
    }

    #[test]
    fn test_trait_always_exempt() {
        let db = GraphDb::open_in_memory().unwrap();
        let file_id = db.upsert_file("src/lib.rs", "rust", "abc", 0, 10).unwrap();

        let sym = SymbolBuilder::new(file_id, "MyTrait".into(), SymbolKind::Trait, "trait MyTrait {}".into(), "rust".into())
            .lines(1, 3)
            .build();
        db.insert_symbol(&sym).unwrap();

        let result = detect_dead_code(&db).unwrap();
        assert_eq!(result.total_dead_symbols, 0, "trait definitions should be exempt");
    }

    #[test]
    fn test_constructor_exempt() {
        let db = GraphDb::open_in_memory().unwrap();
        let file_id = db.upsert_file("src/lib.rs", "rust", "abc", 0, 10).unwrap();

        let sym = SymbolBuilder::new(file_id, "new".into(), SymbolKind::Constructor, "fn new() -> Self".into(), "rust".into())
            .lines(1, 3)
            .build();
        db.insert_symbol(&sym).unwrap();

        let result = detect_dead_code(&db).unwrap();
        assert_eq!(result.total_dead_symbols, 0, "constructors should be exempt");
    }

    #[test]
    fn test_uncalled_function_detected() {
        let db = GraphDb::open_in_memory().unwrap();
        let file_id = db.upsert_file("src/legacy.rs", "rust", "abc", 0, 10).unwrap();

        let sym = SymbolBuilder::new(file_id, "unused_fn".into(), SymbolKind::Function, "fn unused_fn() {}".into(), "rust".into())
            .lines(1, 5)
            .visibility(Visibility::Private)
            .build();
        db.insert_symbol(&sym).unwrap();

        let result = detect_dead_code(&db).unwrap();
        assert_eq!(result.total_dead_symbols, 1);
        assert_eq!(result.files[0].dead_symbols, vec!["unused_fn"]);
    }

    #[test]
    fn test_called_function_not_dead() {
        let db = GraphDb::open_in_memory().unwrap();
        let file_id = db.upsert_file("src/lib.rs", "rust", "abc", 0, 10).unwrap();

        let caller = SymbolBuilder::new(file_id, "caller".into(), SymbolKind::Function, "fn caller() {}".into(), "rust".into())
            .lines(1, 5)
            .visibility(Visibility::Public)
            .build();
        let callee = SymbolBuilder::new(file_id, "callee".into(), SymbolKind::Function, "fn callee() {}".into(), "rust".into())
            .lines(6, 10)
            .visibility(Visibility::Private)
            .build();

        let caller_id = db.insert_symbol(&caller).unwrap();
        let callee_id = db.insert_symbol(&callee).unwrap();
        db.insert_edge(caller_id, callee_id, EdgeKind::Calls, 1.0, serde_json::json!({}))
            .unwrap();

        let result = detect_dead_code(&db).unwrap();
        assert_eq!(result.total_dead_symbols, 0, "called function should not be dead");
    }

    #[test]
    fn test_public_function_exempt() {
        let db = GraphDb::open_in_memory().unwrap();
        let file_id = db.upsert_file("src/lib.rs", "rust", "abc", 0, 10).unwrap();

        let sym = SymbolBuilder::new(file_id, "public_api".into(), SymbolKind::Function, "fn public_api() {}".into(), "rust".into())
            .lines(1, 5)
            .visibility(Visibility::Public)
            .build();
        db.insert_symbol(&sym).unwrap();

        let result = detect_dead_code(&db).unwrap();
        assert_eq!(result.total_dead_symbols, 0, "public functions should be exempt");
    }
}
