use crate::db::GraphDb;
use rusqlite::params;
use std::collections::{HashMap, HashSet};

pub struct DeepGraphStats {
    pub type_flow_edges: usize,
    pub error_type_edges: usize,
    pub data_shape_edges: usize,
    pub string_literal_edges: usize,
    pub comment_ref_edges: usize,
}

fn extract_type_tokens(signature: &str) -> Vec<String> {
    let mut types = Vec::new();

    let keywords: HashSet<&str> = [
        "fn", "pub", "async", "const", "mut", "self", "Self", "super", "crate",
        "static", "unsafe", "extern", "where", "impl", "let", "return", "if",
        "else", "for", "while", "match", "struct", "enum", "trait", "type",
        "mod", "use", "class", "function", "var", "def", "export", "import",
        "from", "interface", "extends", "implements", "void", "string", "number",
        "boolean", "any", "unknown", "never", "true", "false", "nil", "None",
        "Some", "Ok", "Err", "ref", "move", "dyn", "Box", "Vec", "Arc",
        "Rc", "Option", "Result", "Map", "Set", "HashMap", "HashSet", "String",
        "Record", "Partial", "Required", "Readonly", "Pick", "Omit",
        "Awaited", "Parameters", "Args", "Context", "Next",
        "Config", "Options", "Props", "State", "Data", "Value",
        "keyof", "infer", "typeof", "instanceof", "in", "of", "as",
        "u8", "u16", "u32", "u64", "i8", "i16", "i32", "i64",
        "f32", "f64", "usize", "isize", "bool", "str", "int", "uint",
        "float", "double", "byte", "short", "long", "char",
    ]
    .into_iter()
    .collect();

    let mut current = String::new();
    let mut in_angle = 0usize;

    for c in signature.chars() {
        match c {
            '(' | ')' | ',' | ':' | '[' | ']' | '{' | '}' | '=' | '&' | '+' | '-' | '*' | '/' | '|' | ';' | '!' | '?' | '~' | '^' | '%' | '#' | '@' | '$' => {
                if !current.trim().is_empty() {
                    let trimmed = current.trim();
                    if trimmed.len() >= 2 && !keywords.contains(trimmed) && !trimmed.contains(' ') {
                        if !(trimmed.chars().all(|c| c.is_ascii_lowercase()) && trimmed.len() <= 3) {
                            types.push(trimmed.to_string());
                        }
                    }
                }
                current.clear();
                in_angle = 0;
            }
            '<' => {
                if !current.trim().is_empty() {
                    let trimmed = current.trim();
                    if trimmed.len() >= 2 && !keywords.contains(trimmed) && !trimmed.contains(' ') {
                        if !(trimmed.chars().all(|c| c.is_ascii_lowercase()) && trimmed.len() <= 3) {
                            types.push(trimmed.to_string());
                        }
                    }
                }
                current.clear();
                in_angle += 1;
            }
            '>' => {
                if !current.trim().is_empty() {
                    let trimmed = current.trim();
                    if trimmed.len() >= 2 && !keywords.contains(trimmed) && !trimmed.contains(' ') {
                        if !(trimmed.chars().all(|c| c.is_ascii_lowercase()) && trimmed.len() <= 3) {
                            types.push(trimmed.to_string());
                        }
                    }
                }
                current.clear();
                in_angle = in_angle.saturating_sub(1);
            }
            ' ' => {
                if in_angle > 0 {
                    current.push(c);
                } else {
                    if !current.trim().is_empty() {
                        let trimmed = current.trim();
                        if trimmed.len() >= 2 && !keywords.contains(trimmed) && !trimmed.contains(' ') {
                            if !(trimmed.chars().all(|c| c.is_ascii_lowercase()) && trimmed.len() <= 3) {
                                types.push(trimmed.to_string());
                            }
                        }
                    }
                    current.clear();
                }
            }
            _ => {
                current.push(c);
            }
        }
    }

    if !current.trim().is_empty() {
        let trimmed = current.trim();
        if trimmed.len() >= 2 && !keywords.contains(trimmed) && !trimmed.contains(' ') {
            if !(trimmed.chars().all(|c| c.is_ascii_lowercase()) && trimmed.len() <= 3) {
                types.push(trimmed.to_string());
            }
        }
    }

    types
}

fn is_error_type(type_name: &str) -> bool {
    let lower = type_name.to_lowercase();
    lower.contains("error")
        || lower.contains("exception")
        || lower.contains("fault")
        || lower.ends_with("err")
        || lower.ends_with("exception")
        || lower.starts_with("err")
}

fn extract_field_accesses(source: &str) -> HashSet<String> {
    let mut fields = HashSet::new();
    let bytes = source.as_bytes();

    let skip_methods: HashSet<&str> = [
        "len", "push", "pop", "map", "filter", "unwrap", "expect", "clone",
        "to", "as_ref", "as_mut", "into", "from", "iter", "collect", "next",
        "ok", "err", "is_ok", "is_err", "is_some", "is_none", "get", "set",
        "take", "insert", "remove", "contains", "clear", "with", "new",
        "default", "then", "and_then", "or_else", "unwrap_or", "to_string",
        "to_vec", "keys", "values", "entries", "lock", "spawn", "join",
        "send", "recv", "wait", "notify", "lock", "read", "write", "flush",
        "close", "open", "seek", "status", "code", "message", "name",
        "json", "text", "body", "headers", "first", "last", "count", "find",
        "flat", "reduce", "fold", "forEach", "every", "some", "sort", "reverse",
        "slice", "splice", "concat", "join", "split", "trim", "start",
        "end", "replace", "repeat", "pad", "char", "byte", "line", "col",
        "bind", "call", "apply", "has", "add", "delete", "size",
    ]
    .into_iter()
    .collect();

    for i in 0..bytes.len().saturating_sub(1) {
        if bytes[i] == b'.' {
            let mut end = i + 1;
            while end < bytes.len()
                && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_')
            {
                end += 1;
            }
            let field = std::str::from_utf8(&bytes[i + 1..end]).unwrap_or("");
            if field.len() >= 2 && !skip_methods.contains(field) && !field.chars().all(|c| c.is_ascii_uppercase()) {
                fields.insert(field.to_string());
            }
        }
    }

    fields
}

pub fn compute_deep_graph_edges(db: &GraphDb) -> Result<DeepGraphStats, Box<dyn std::error::Error>> {
    let conn = db.conn();

    let mut sym_stmt = conn.prepare(
        "SELECT id, name, kind, signature, source, file_id \
         FROM symbols WHERE kind IN ('function', 'method', 'constructor', 'macro')",
    )?;
    let symbols: Vec<(i64, String, String, Option<String>, String, i64)> = sym_stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get(3)?,
                row.get::<_, String>(4)?,
                row.get(5)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();
    drop(sym_stmt);

    let n = symbols.len();
    let total_syms = n as f64;

    let mut type_to_symbols: HashMap<String, Vec<usize>> = HashMap::new();
    let mut field_to_symbols: HashMap<String, Vec<usize>> = HashMap::new();

    for (i, (_id, _name, _kind, sig, source, _file_id)) in symbols.iter().enumerate() {
        let sig_str = sig.as_deref().unwrap_or("");
        for t in extract_type_tokens(sig_str) {
            type_to_symbols.entry(t).or_default().push(i);
        }
        for f in extract_field_accesses(source) {
            field_to_symbols.entry(f).or_default().push(i);
        }
    }

    let mut type_insert = conn.prepare(
        "INSERT OR IGNORE INTO edges (source_id, target_id, kind, weight, metadata) \
         VALUES (?1, ?2, 'shares_type', ?3, ?4)",
    )?;
    let mut error_insert = conn.prepare(
        "INSERT OR IGNORE INTO edges (source_id, target_id, kind, weight, metadata) \
         VALUES (?1, ?2, 'shares_error_type', ?3, ?4)",
    )?;
    let mut data_insert = conn.prepare(
        "INSERT OR IGNORE INTO edges (source_id, target_id, kind, weight, metadata) \
         VALUES (?1, ?2, 'shares_data_shape', ?3, ?4)",
    )?;

    let mut type_flow_count = 0usize;
    let mut error_type_count = 0usize;
    let mut data_shape_count = 0usize;

    for (type_name, sym_indices) in &type_to_symbols {
        if sym_indices.len() < 2 || sym_indices.len() > 150 {
            continue;
        }
        let count = sym_indices.len() as f64;
        let rarity = (total_syms / count).ln().max(0.1);
        let base_weight = (0.15 * rarity).min(0.40);
        let is_err = is_error_type(type_name);

        let insert = if is_err { &mut error_insert } else { &mut type_insert };
        let kind_weight = if is_err { 0.55 } else { 0.40 };

        for w in sym_indices.windows(2) {
            let ai = w[0];
            let bi = w[1];
            if ai == bi {
                continue;
            }
            let a_id = symbols[ai].0;
            let b_id = symbols[bi].0;
            let cross = symbols[ai].5 != symbols[bi].5;
            let final_w = (if cross { base_weight * 1.5 } else { base_weight }).min(kind_weight);
            if final_w < 0.04 {
                continue;
            }

            let meta = serde_json::json!({
                "type": type_name,
                "rarity": rarity,
                "shared_by": count as usize,
                "cross_module": cross,
            });
            insert.execute(params![a_id, b_id, final_w, meta.to_string()])?;
            if is_err {
                error_type_count += 1;
            } else {
                type_flow_count += 1;
            }
        }
    }

    for (field_name, sym_indices) in &field_to_symbols {
        if sym_indices.len() < 2 || sym_indices.len() > 80 {
            continue;
        }
        let count = sym_indices.len() as f64;
        let rarity = (total_syms / count).ln().max(0.1);
        let base_weight = (0.08 * rarity).min(0.25);

        for w in sym_indices.windows(2) {
            let ai = w[0];
            let bi = w[1];
            if ai == bi {
                continue;
            }
            let a_id = symbols[ai].0;
            let b_id = symbols[bi].0;
            let cross = symbols[ai].5 != symbols[bi].5;
            let final_w = if cross { base_weight * 1.5 } else { base_weight };
            if final_w < 0.03 {
                continue;
            }

            let meta = serde_json::json!({
                "field": field_name,
                "rarity": rarity,
                "shared_by": count as usize,
                "cross_module": cross,
            });
            data_insert.execute(params![a_id, b_id, final_w, meta.to_string()])?;
            data_shape_count += 1;
        }
    }

    Ok(DeepGraphStats {
        type_flow_edges: type_flow_count,
        error_type_edges: error_type_count,
        data_shape_edges: data_shape_count,
        string_literal_edges: 0,
        comment_ref_edges: 0,
    })
}

fn extract_string_literals(source: &str) -> Vec<String> {
    let mut literals = Vec::new();
    let bytes = source.as_bytes();
    let mut i = 0;
    let mut in_single = false;
    let mut in_double = false;
    let mut current = String::new();

    while i < bytes.len() {
        let c = bytes[i];
        if in_single {
            if c == b'\'' && (i + 1 >= bytes.len() || bytes[i + 1] != b'\'') {
                if current.len() >= 4 {
                    literals.push(current.clone());
                }
                current.clear();
                in_single = false;
            } else if c == b'\\' && i + 1 < bytes.len() {
                i += 2;
                continue;
            } else {
                current.push(c as char);
            }
        } else if in_double {
            if c == b'"' && (i + 1 >= bytes.len() || bytes[i + 1] != b'"') {
                if current.len() >= 4 {
                    literals.push(current.clone());
                }
                current.clear();
                in_double = false;
            } else if c == b'\\' && i + 1 < bytes.len() {
                i += 2;
                continue;
            } else {
                current.push(c as char);
            }
        } else if c == b'\'' && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric()) {
            in_single = true;
            current.clear();
        } else if c == b'"' && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric()) {
            in_double = true;
            current.clear();
        }
        i += 1;
    }

    literals
}

fn extract_comment_refs(source: &str, symbol_names: &[String]) -> HashSet<String> {
    let mut refs = HashSet::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("//") && !trimmed.starts_with('*') && !trimmed.starts_with('#') {
            continue;
        }
        let comment_text = trimmed
            .trim_start_matches("//")
            .trim_start_matches('*')
            .trim_start_matches('#')
            .trim();
        for name in symbol_names {
            if comment_text.contains(name.as_str()) {
                refs.insert(name.clone());
            }
        }
    }
    refs
}

pub fn compute_source_graph_edges(db: &GraphDb) -> Result<DeepGraphStats, Box<dyn std::error::Error>> {
    let conn = db.conn();

    let mut sym_stmt = conn.prepare(
        "SELECT id, name, kind, source, file_id FROM symbols WHERE kind IN ('function', 'method', 'constructor', 'macro', 'struct', 'class', 'enum', 'trait', 'interface')",
    )?;
    let symbols: Vec<(i64, String, String, String, i64)> = sym_stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get(4)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();
    drop(sym_stmt);

    let n = symbols.len();
    let total_syms = n as f64;
    let symbol_names: Vec<String> = symbols.iter().map(|(_, name, _, _, _)| name.clone()).collect();

    let mut string_to_symbols: HashMap<String, Vec<usize>> = HashMap::new();
    let mut comment_to_symbols: HashMap<String, Vec<usize>> = HashMap::new();

    for (i, (_id, name, _kind, source, _file_id)) in symbols.iter().enumerate() {
        for lit in extract_string_literals(source) {
            let lower = lit.to_lowercase();
            if lower.contains("error")
                || lower.contains("panic")
                || lower.contains("invalid")
                || lower.contains("timeout")
                || lower.contains("shutdown")
                || lower.contains("closed")
                || lower.contains("refused")
                || lower.contains("unauthorized")
                || lower.contains("forbidden")
                || lower.contains("not found")
                || lower.contains("already")
                || lower.contains("overflow")
                || lower.contains("deadlock")
            {
                string_to_symbols.entry(lit.to_lowercase()).or_default().push(i);
            }
        }
        for ref_name in extract_comment_refs(source, &symbol_names) {
            if ref_name != *name {
                comment_to_symbols.entry(ref_name).or_default().push(i);
            }
        }
    }

    let mut string_insert = conn.prepare(
        "INSERT OR IGNORE INTO edges (source_id, target_id, kind, weight, metadata) \
         VALUES (?1, ?2, 'shares_error_type', ?3, ?4)",
    )?;
    let mut comment_insert = conn.prepare(
        "INSERT OR IGNORE INTO edges (source_id, target_id, kind, weight, metadata) \
         VALUES (?1, ?2, 'shares_type', ?3, ?4)",
    )?;

    let mut string_count = 0usize;
    let mut comment_count = 0usize;

    for (string_val, sym_indices) in &string_to_symbols {
        if sym_indices.len() < 2 || sym_indices.len() > 50 {
            continue;
        }
        let count = sym_indices.len() as f64;
        let rarity = (total_syms / count).ln().max(0.1);
        let base_weight = (0.12 * rarity).min(0.45);

        for w in sym_indices.windows(2) {
            let ai = w[0];
            let bi = w[1];
            if ai == bi {
                continue;
            }
            let a_id = symbols[ai].0;
            let b_id = symbols[bi].0;
            let cross = symbols[ai].4 != symbols[bi].4;
            let final_w = (if cross { base_weight * 1.5 } else { base_weight }).min(0.55);
            if final_w < 0.04 {
                continue;
            }

            let meta = serde_json::json!({
                "string": string_val,
                "rarity": rarity,
                "shared_by": count as usize,
                "cross_module": cross,
            });
            string_insert.execute(params![a_id, b_id, final_w, meta.to_string()])?;
            string_count += 1;
        }
    }

    for (ref_name, sym_indices) in &comment_to_symbols {
        if sym_indices.len() < 2 || sym_indices.len() > 80 {
            continue;
        }
        let count = sym_indices.len() as f64;
        let rarity = (total_syms / count).ln().max(0.1);
        let base_weight = (0.10 * rarity).min(0.35);

        for w in sym_indices.windows(2) {
            let ai = w[0];
            let bi = w[1];
            if ai == bi {
                continue;
            }
            let a_id = symbols[ai].0;
            let b_id = symbols[bi].0;
            let cross = symbols[ai].4 != symbols[bi].4;
            let final_w = (if cross { base_weight * 1.5 } else { base_weight }).min(0.40);
            if final_w < 0.03 {
                continue;
            }

            let meta = serde_json::json!({
                "comment_ref": ref_name,
                "rarity": rarity,
                "shared_by": count as usize,
                "cross_module": cross,
            });
            comment_insert.execute(params![a_id, b_id, final_w, meta.to_string()])?;
            comment_count += 1;
        }
    }

    Ok(DeepGraphStats {
        type_flow_edges: 0,
        error_type_edges: 0,
        data_shape_edges: 0,
        string_literal_edges: string_count,
        comment_ref_edges: comment_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_type_tokens() {
        let sig = "fn handle_request(&self, req: Request<Body>) -> Response<Body>";
        let tokens = extract_type_tokens(sig);
        assert!(tokens.contains(&"Request".to_string()));
        assert!(tokens.contains(&"Body".to_string()));
        assert!(tokens.contains(&"Response".to_string()));
    }

    #[test]
    fn test_extract_type_tokens_typescript() {
        let sig = "async function authenticateUser(token: string): Promise<User>";
        let tokens = extract_type_tokens(sig);
        assert!(tokens.contains(&"authenticateUser".to_string()));
        assert!(tokens.contains(&"token".to_string()));
        assert!(tokens.contains(&"Promise".to_string()));
        assert!(tokens.contains(&"User".to_string()));
    }

    #[test]
    fn test_is_error_type() {
        assert!(is_error_type("std::io::Error"));
        assert!(is_error_type("JoinError"));
        assert!(is_error_type("ParseError"));
        assert!(!is_error_type("Request"));
        assert!(!is_error_type("Response"));
    }

    #[test]
    fn test_extract_field_accesses() {
        let src = "self.config.timeout = 30; let addr = user.socket_addr.clone();";
        let fields = extract_field_accesses(src);
        assert!(fields.contains("config"));
        assert!(fields.contains("timeout"));
        assert!(fields.contains("socket_addr"));
    }
}
