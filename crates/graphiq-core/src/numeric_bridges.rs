use std::collections::HashMap;

use rusqlite::params;

use crate::db::GraphDb;
use crate::edge::EdgeKind;

struct SymInfo {
    id: i64,
    name: String,
    source: String,
    kind: String,
    file_path: String,
}

pub struct ConstantEntry {
    pub literal: String,
    pub named: Option<String>,
    pub count: usize,
    pub rarity: f64,
    pub symbols: Vec<ConstantSymbol>,
}

pub struct ConstantSymbol {
    pub name: String,
    pub kind: String,
    pub file: String,
    pub line: u32,
}

pub fn query_constants(
    db: &GraphDb,
    filter: Option<&str>,
    top: usize,
) -> Result<Vec<ConstantEntry>, Box<dyn std::error::Error>> {
    let conn = db.conn();

    let mut sym_stmt = conn.prepare(
        "SELECT s.id, s.name, s.source, s.kind, s.line_start, f.path \
         FROM symbols s JOIN files f ON s.file_id = f.id \
         WHERE s.visibility = 'public'"
    )?;

    let symbols: Vec<(i64, String, String, String, u32, String)> = sym_stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, u32>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?
        .flatten()
        .collect();
    drop(sym_stmt);

    let n_symbols = symbols.len() as f64;
    let mut literal_map: HashMap<String, Vec<(i64, String, String, u32, String)>> = HashMap::new();

    for (id, name, source, kind, line, file) in &symbols {
        let literals = extract_numeric_literals(source);
        for lit in &literals {
            literal_map
                .entry(lit.clone())
                .or_default()
                .push((*id, name.clone(), kind.clone(), *line, file.clone()));
        }
    }

    let filter_lower = filter.map(|f| f.to_lowercase());

    let mut entries: Vec<ConstantEntry> = literal_map
        .into_iter()
        .filter(|(_, syms)| syms.len() >= 2)
        .filter(|(lit, _)| {
            if let Some(ref f) = filter_lower {
                lit.to_lowercase().contains(f)
            } else {
                true
            }
        })
        .map(|(literal, syms)| {
            let rarity = (n_symbols / syms.len() as f64).ln();
            let named = find_const_name_for_literal(&symbols, &literal);
            ConstantEntry {
                count: syms.len(),
                literal,
                named,
                symbols: syms
                    .into_iter()
                    .take(10)
                    .map(|(_, name, kind, line, file)| ConstantSymbol {
                        name,
                        kind,
                        file,
                        line,
                    })
                    .collect(),
                rarity,
            }
        })
        .collect();

    entries.sort_by(|a, b| {
        b.rarity
            .partial_cmp(&a.rarity)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    entries.truncate(top);
    Ok(entries)
}

fn find_const_name_for_literal(
    symbols: &[(i64, String, String, String, u32, String)],
    literal: &str,
) -> Option<String> {
    for (_, name, source, kind, _, _) in symbols {
        if *kind == "constant" {
            if let Some(val) = extract_constant_value(source) {
                if val == literal {
                    return Some(name.clone());
                }
            }
        }
    }
    None
}

pub struct NumericBridgeStats {
    pub literals_found: usize,
    pub constants_found: usize,
    pub bridge_edges_created: usize,
}

pub fn compute_numeric_bridges(db: &GraphDb) -> Result<NumericBridgeStats, Box<dyn std::error::Error>> {
    let conn = db.conn();

    let mut sym_stmt = conn.prepare(
        "SELECT s.id, s.name, s.source, s.kind, f.path \
         FROM symbols s JOIN files f ON s.file_id = f.id \
         WHERE s.visibility = 'public'"
    )?;

    let symbols: Vec<SymInfo> = sym_stmt
        .query_map([], |row| {
            Ok(SymInfo {
                id: row.get::<_, i64>(0)?,
                name: row.get::<_, String>(1)?,
                source: row.get::<_, String>(2)?,
                kind: row.get::<_, String>(3)?,
                file_path: row.get::<_, String>(4)?,
            })
        })?
        .flatten()
        .collect();
    drop(sym_stmt);

    let mut literal_to_symbols: HashMap<String, Vec<(i64, String)>> = HashMap::new();
    let mut const_name_to_id: HashMap<String, i64> = HashMap::new();
    let mut const_name_to_literal: HashMap<String, String> = HashMap::new();
    let mut total_literals = 0usize;
    let mut total_constants = 0usize;

    for sym in &symbols {
        let literals = extract_numeric_literals(&sym.source);
        for lit in &literals {
            literal_to_symbols
                .entry(lit.clone())
                .or_default()
                .push((sym.id, sym.file_path.clone()));
        }
        total_literals += literals.len();

        if sym.kind == "constant" {
            if let Some(lit_value) = extract_constant_value(&sym.source) {
                const_name_to_id.insert(sym.name.clone(), sym.id);
                const_name_to_literal.insert(sym.name.clone(), lit_value.clone());
                literal_to_symbols
                    .entry(lit_value.clone())
                    .or_default()
                    .push((sym.id, sym.file_path.clone()));
                total_constants += 1;
            }
        }
    }

    let n_symbols = symbols.len() as f64;
    let mut bridge_edges = 0usize;

    let mut insert_stmt = conn.prepare(
        "INSERT OR IGNORE INTO edges (source_id, target_id, kind, weight, metadata)
         VALUES (?1, ?2, ?3, ?4, ?5)"
    )?;

    for (literal, sym_entries) in &literal_to_symbols {
        if sym_entries.len() < 2 {
            continue;
        }

        let ratio = sym_entries.len() as f64 / n_symbols;
        if ratio > 0.3 && sym_entries.len() > 30 {
            continue;
        }

        let rarity = (n_symbols / sym_entries.len() as f64).ln().max(0.1);
        let weight = 0.15 * rarity;
        if weight < 0.05 {
            continue;
        }

        let unique_files: std::collections::HashSet<&str> = sym_entries
            .iter()
            .map(|(_, f)| f.as_str())
            .collect();
        let cross_module_bonus = if unique_files.len() > 1 { 1.5 } else { 0.5 };
        let final_weight = weight * cross_module_bonus;
        if final_weight < 0.08 {
            continue;
        }

        let metadata = serde_json::json!({
            "literal": literal,
            "rarity": rarity,
            "shared_by": sym_entries.len(),
            "cross_module": unique_files.len() > 1
        });

        let ids: Vec<i64> = sym_entries.iter().map(|(id, _)| *id).collect();
        let max_per_literal = 50;
        let mut created = 0;
        'outer: for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                if created >= max_per_literal {
                    break 'outer;
                }
                let (a, b) = if ids[i] < ids[j] {
                    (ids[i], ids[j])
                } else {
                    (ids[j], ids[i])
                };
                insert_stmt.execute(params![
                    a, b,
                    EdgeKind::SharesConstant.as_str(),
                    final_weight,
                    metadata.to_string(),
                ])?;
                bridge_edges += 1;
                created += 1;
            }
        }
    }

    let mut const_ref_stmt = conn.prepare(
        "INSERT OR IGNORE INTO edges (source_id, target_id, kind, weight, metadata)
         VALUES (?1, ?2, ?3, ?4, ?5)"
    )?;

    let mut name_to_ids: HashMap<&str, Vec<i64>> = HashMap::new();
    for sym in &symbols {
        name_to_ids.entry(&sym.name).or_default().push(sym.id);
    }

    for (const_name, &const_id) in &const_name_to_id {
        if let Some(users) = find_constant_usage(&symbols, const_name) {
            let metadata = serde_json::json!({
                "constant": const_name,
                "value": const_name_to_literal.get(const_name).unwrap_or(&String::new())
            });
            for user_id in users {
                if user_id != const_id {
                    const_ref_stmt.execute(params![
                        user_id, const_id,
                        EdgeKind::ReferencesConstant.as_str(),
                        EdgeKind::ReferencesConstant.path_weight(),
                        metadata.to_string(),
                    ])?;
                    bridge_edges += 1;
                }
            }
        }
    }

    drop(insert_stmt);
    drop(const_ref_stmt);

    Ok(NumericBridgeStats {
        literals_found: total_literals,
        constants_found: total_constants,
        bridge_edges_created: bridge_edges,
    })
}

fn extract_numeric_literals(source: &str) -> Vec<String> {
    let mut literals = Vec::new();
    let mut current = String::new();
    let mut in_number = false;
    let mut has_dot = false;
    let mut prev_char = '\0';
    let mut hex = false;

    for c in source.chars() {
        if in_number {
            if c.is_ascii_digit() {
                current.push(c);
            } else if c == '.' && !has_dot && !hex {
                has_dot = true;
                current.push(c);
            } else if (c == 'x' || c == 'X') && current == "0" {
                hex = true;
                current.push(c);
            } else if hex && (c.is_ascii_hexdigit() || c == '_') {
                if c != '_' {
                    current.push(c);
                }
            } else if c == '_' && !hex {
                continue;
            } else {
                let literal = current.trim();
                if should_include_literal(literal) {
                    let normalized = normalize_literal(literal);
                    literals.push(normalized);
                }
                in_number = false;
                has_dot = false;
                hex = false;
                current.clear();
            }
        } else if c.is_ascii_digit() {
            if !prev_char.is_alphabetic() && prev_char != '_' && prev_char != '.' {
                in_number = true;
                has_dot = false;
                hex = false;
                current.clear();
                current.push(c);
            }
        }
        prev_char = c;
    }

    if in_number {
        let literal = current.trim();
        if should_include_literal(literal) {
            let normalized = normalize_literal(literal);
            literals.push(normalized);
        }
    }

    literals.sort();
    literals.dedup();
    literals
}

fn should_include_literal(s: &str) -> bool {
    if s.is_empty() || s.len() > 20 {
        return false;
    }
    if s == "0" || s == "1" {
        return false;
    }
    if s.starts_with("0x") && s.len() <= 3 {
        return false;
    }
    let numeric_part: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    if numeric_part.is_empty() {
        return false;
    }
    if numeric_part.len() < 2 && !s.starts_with("0x") {
        let val: f64 = numeric_part.parse().unwrap_or(0.0);
        if val < 2.0 {
            return false;
        }
    }
    true
}

fn normalize_literal(s: &str) -> String {
    if s.starts_with("0x") || s.starts_with("0X") {
        return s.to_lowercase();
    }
    if let Ok(val) = s.parse::<f64>() {
        if val == val.floor() && val.abs() < 1e15 {
            return format!("{}", val as i64);
        }
    }
    s.to_string()
}

fn extract_constant_value(source: &str) -> Option<String> {
    let trimmed = source.trim();

    if let Some(eq_pos) = trimmed.find('=') {
        let after_eq = trimmed[eq_pos + 1..].trim();
        let value_part = after_eq
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .trim_end_matches(',');
        let literals = extract_numeric_literals(value_part);
        if !literals.is_empty() {
            return Some(literals[0].clone());
        }
        if value_part.starts_with('"') || value_part.starts_with('\'') {
            return None;
        }
    }

    if let Some(colon_pos) = trimmed.rfind(':') {
        let after_colon = trimmed[colon_pos + 1..].trim();
        if after_colon.starts_with('=') {
            let value_part = after_colon[1..]
                .split(';')
                .next()
                .unwrap_or("")
                .trim();
            let literals = extract_numeric_literals(value_part);
            if !literals.is_empty() {
                return Some(literals[0].clone());
            }
        }
    }

    None
}

fn find_constant_usage(
    symbols: &[SymInfo],
    const_name: &str,
) -> Option<Vec<i64>> {
    let upper = const_name.to_uppercase();
    let lower = const_name.to_lowercase();
    let mut users = Vec::new();

    for sym in symbols {
        if sym.source.contains(const_name)
            || sym.source.contains(&upper)
            || sym.source.contains(&lower)
        {
            users.push(sym.id);
        }
    }

    if users.is_empty() {
        None
    } else {
        Some(users)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_integers() {
        let src = "let x = 42; let y = 3850; let z = 0;";
        let lits = extract_numeric_literals(src);
        assert!(lits.contains(&"42".to_string()));
        assert!(lits.contains(&"3850".to_string()));
        assert!(!lits.contains(&"0".to_string()));
    }

    #[test]
    fn test_extract_floats() {
        let src = "let threshold = 0.25; let decay = 1.5;";
        let lits = extract_numeric_literals(src);
        assert!(lits.contains(&"0.25".to_string()));
        assert!(lits.contains(&"1.5".to_string()));
    }

    #[test]
    fn test_extract_hex() {
        let src = "let mask = 0xFF00; let ctrl = 0x1A;";
        let lits = extract_numeric_literals(src);
        assert!(lits.contains(&"0xff00".to_string()));
        assert!(lits.contains(&"0x1a".to_string()));
    }

    #[test]
    fn test_ignore_in_identifiers() {
        let src = "base64 encode utf8 charset";
        let lits = extract_numeric_literals(src);
        assert!(!lits.contains(&"64".to_string()));
        assert!(!lits.contains(&"8".to_string()));
    }

    #[test]
    fn test_normalize_float_to_int() {
        assert_eq!(normalize_literal("42.0"), "42");
        assert_eq!(normalize_literal("0.25"), "0.25");
    }

    #[test]
    fn test_constant_value_extraction() {
        let src = "const MAX_RETRIES: usize = 3;";
        assert_eq!(extract_constant_value(src), Some("3".to_string()));
    }

    #[test]
    fn test_constant_float_value() {
        let src = "const SIMILARITY_THRESHOLD: f64 = 0.25;";
        assert_eq!(extract_constant_value(src), Some("0.25".to_string()));
    }

    #[test]
    fn test_rarity_weighting() {
        let n: f64 = 1000.0;
        let rare_idf: f64 = (n / 3.0_f64).ln().max(0.1);
        let common_idf: f64 = (n / 500.0_f64).ln().max(0.1);
        let rare_w = 0.3 * rare_idf;
        let common_w = 0.3 * common_idf;
        assert!(rare_w > common_w, "rare literal {} should weigh more than common {}", rare_w, common_w);
        assert!(common_w < 0.5, "very common literal should have low weight: {}", common_w);
    }

    #[test]
    fn test_edge_kind_roundtrip() {
        assert_eq!(EdgeKind::from_str("shares_constant"), Some(EdgeKind::SharesConstant));
        assert_eq!(EdgeKind::from_str("references_constant"), Some(EdgeKind::ReferencesConstant));
        assert_eq!(EdgeKind::SharesConstant.as_str(), "shares_constant");
        assert_eq!(EdgeKind::ReferencesConstant.as_str(), "references_constant");
    }

    #[test]
    fn test_path_weights_sensible() {
        assert!(EdgeKind::ReferencesConstant.path_weight() > EdgeKind::SharesConstant.path_weight());
        assert!(EdgeKind::SharesConstant.path_weight() < EdgeKind::References.path_weight());
    }

    #[test]
    fn test_compute_bridges_end_to_end() {
        let db = GraphDb::open_in_memory().unwrap();

        let file_a = db.upsert_file("handler.rs", "rust", "abc123", 0, 10).unwrap();
        let file_b = db.upsert_file("responder.rs", "rust", "def456", 0, 10).unwrap();
        let file_c = db.upsert_file("checker.rs", "rust", "ghi789", 0, 10).unwrap();

        db.insert_symbol(&crate::symbol::SymbolBuilder::new(
            file_a, "handler".into(), crate::symbol::SymbolKind::Function,
            "fn handler() { send(429); retry(3); log(200); }".into(), "rust".into(),
        ).lines(1, 1).build()).unwrap();

        db.insert_symbol(&crate::symbol::SymbolBuilder::new(
            file_b, "responder".into(), crate::symbol::SymbolKind::Function,
            "fn responder() { respond(429); log(200); }".into(), "rust".into(),
        ).lines(1, 1).build()).unwrap();

        db.insert_symbol(&crate::symbol::SymbolBuilder::new(
            file_c, "checker".into(), crate::symbol::SymbolKind::Function,
            "fn checker() { validate(200); }".into(), "rust".into(),
        ).lines(1, 1).build()).unwrap();

        let stats = compute_numeric_bridges(&db).unwrap();
        assert!(stats.literals_found > 0, "should find numeric literals");
        assert!(stats.bridge_edges_created > 0, "should create bridge edges for shared literals: got {} edges from {} literals", stats.bridge_edges_created, stats.literals_found);
    }
}
