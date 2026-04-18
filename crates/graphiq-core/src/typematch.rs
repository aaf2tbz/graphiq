use std::collections::HashMap;

use crate::db::GraphDb;
use crate::symbol::{Symbol, SymbolKind};

#[derive(Debug, Clone)]
pub struct TypePattern {
    pub return_category: ReturnCategory,
    pub param_count: usize,
    pub param_categories: Vec<ParamCategory>,
    pub has_self: bool,
    pub is_async: bool,
    pub kind: SymbolKind,
}

impl Default for TypePattern {
    fn default() -> Self {
        Self {
            return_category: ReturnCategory::Unknown,
            param_count: 0,
            param_categories: Vec::new(),
            has_self: false,
            is_async: false,
            kind: SymbolKind::Function,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReturnCategory {
    Void,
    Bool,
    Result,
    Option,
    Collection,
    String,
    Number,
    Unknown,
}

impl Default for ReturnCategory {
    fn default() -> Self {
        ReturnCategory::Unknown
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ParamCategory {
    Value,
    String,
    Bool,
    Collection,
    Callback,
    SelfType,
    Config,
    Id,
    Unknown,
}

impl Default for ParamCategory {
    fn default() -> Self {
        ParamCategory::Unknown
    }
}

#[derive(Debug, Clone)]
pub struct IntentPattern {
    pub return_categories: Vec<ReturnCategory>,
    pub preferred_param_count: Option<usize>,
    pub param_categories: Vec<ParamCategory>,
    pub prefer_async: bool,
    pub prefer_method: bool,
    pub prefer_function: bool,
    pub weight: f64,
}

pub fn extract_type_pattern(sym: &Symbol) -> TypePattern {
    let sig = match &sym.signature {
        Some(s) if !s.is_empty() => s.as_str(),
        _ => return TypePattern::default(),
    };

    let lower = sig.to_lowercase();

    let has_self = lower.contains("&self") || lower.contains("self:") || lower.contains("this.");
    let is_async = lower.contains("async ");

    let return_category = classify_return(&lower);
    let (param_count, param_categories) = classify_params(&lower, has_self);

    TypePattern {
        return_category,
        param_count,
        param_categories,
        has_self,
        is_async,
        kind: sym.kind,
    }
}

fn classify_return(lower: &str) -> ReturnCategory {
    let lower = lower.to_lowercase();
    let ret = if let Some(pos) = lower.rfind("->") {
        &lower[pos + 2..]
    } else if let Some(pos) = lower.rfind("):") {
        &lower[pos + 2..]
    } else if let Some(pos) = lower.rfind("): ") {
        &lower[pos + 2..]
    } else if let Some(pos) = lower.find("returns ") {
        &lower[pos + 8..]
    } else {
        return ReturnCategory::Void;
    };

    let ret = ret
        .split(|c: char| c == '{' || c == ',' || c == ';')
        .next()
        .unwrap_or(ret)
        .trim();

    if ret.contains("bool") {
        ReturnCategory::Bool
    } else if ret.contains("result") || ret.contains("error") || ret.contains("throw") {
        ReturnCategory::Result
    } else if ret.contains("option") || ret.contains("nullable") || ret.contains("null") {
        ReturnCategory::Option
    } else if ret.contains("promise<") {
        let inner = ret.trim_start_matches("promise<").trim_end_matches('>');
        classify_return(inner)
    } else if ret.contains("vec<")
        || ret.contains("[]")
        || ret.contains("array")
        || ret.contains("list")
        || ret.contains("set")
        || ret.contains("map<")
        || ret.contains("iterator")
        || ret.contains("iter")
        || ret.contains("slice")
        || ret.contains("readonly ")
        || ret.contains("collection")
    {
        ReturnCategory::Collection
    } else if ret.contains("str")
        || ret.contains("string")
        || ret.contains("path")
        || ret.contains("pathbuf")
    {
        ReturnCategory::String
    } else if ret.contains("int")
        || ret.contains("i32")
        || ret.contains("i64")
        || ret.contains("u32")
        || ret.contains("u64")
        || ret.contains("f32")
        || ret.contains("f64")
        || ret.contains("usize")
        || ret.contains("isize")
        || ret.contains("number")
        || ret.contains("float")
        || ret.contains("double")
    {
        ReturnCategory::Number
    } else if ret.contains("void") || ret == "" || ret == "()" {
        ReturnCategory::Void
    } else {
        ReturnCategory::Unknown
    }
}

fn classify_params(lower: &str, _has_self: bool) -> (usize, Vec<ParamCategory>) {
    let paren_open = match lower.find('(') {
        Some(p) => p,
        None => return (0, Vec::new()),
    };
    let paren_close = match lower[paren_open..].rfind(')') {
        Some(p) => paren_open + p,
        None => return (0, Vec::new()),
    };

    let params_str = &lower[paren_open + 1..paren_close];
    if params_str.trim().is_empty() || params_str.trim() == "self" || params_str.trim() == "&self" {
        return (0, Vec::new());
    }

    let mut categories: Vec<ParamCategory> = Vec::new();
    let mut count = 0;

    for param in split_params(params_str) {
        let trimmed = param.trim();
        if trimmed.is_empty()
            || trimmed == "self"
            || trimmed == "&self"
            || trimmed.starts_with("self:")
        {
            continue;
        }

        count += 1;
        let cat =
            if trimmed.contains("str") || trimmed.contains("string") || trimmed.contains("path") {
                ParamCategory::String
            } else if trimmed.contains("bool")
                || trimmed.contains("boolean")
                || trimmed.contains("flag")
            {
                ParamCategory::Bool
            } else if trimmed.contains("vec<")
                || trimmed.contains("[]")
                || trimmed.contains("array")
                || trimmed.contains("slice")
                || trimmed.contains("readonly")
                || trimmed.contains("list")
            {
                ParamCategory::Collection
            } else if trimmed.contains("fn")
                || trimmed.contains("callback")
                || trimmed.contains("closure")
                || trimmed.contains("=>")
            {
                ParamCategory::Callback
            } else if trimmed.contains("config")
                || trimmed.contains("options")
                || trimmed.contains("settings")
                || trimmed.contains("opts")
            {
                ParamCategory::Config
            } else if trimmed.contains("id")
                && (trimmed.contains("i32")
                    || trimmed.contains("i64")
                    || trimmed.contains("usize")
                    || trimmed.contains("number"))
            {
                ParamCategory::Id
            } else {
                ParamCategory::Value
            };

        categories.push(cat);
    }

    (count, categories)
}

fn split_params(params: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    let bytes = params.as_bytes();

    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'(' | b'<' | b'[' => depth += 1,
            b')' | b'>' | b']' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                parts.push(&params[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if start < params.len() {
        parts.push(&params[start..]);
    }
    parts
}

pub fn classify_nl_intent(query: &str) -> Vec<IntentPattern> {
    let lower = query.to_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();
    let mut intents: Vec<IntentPattern> = Vec::new();

    let has_check = words.iter().any(|w| {
        *w == "check"
            || *w == "verify"
            || *w == "validate"
            || *w == "is"
            || *w == "are"
            || *w == "has"
            || *w == "have"
    });
    let has_error = words.iter().any(|w| {
        *w == "error" || *w == "failure" || *w == "fail" || *w == "exception" || *w == "throw"
    });
    let has_find = words.iter().any(|w| {
        *w == "find"
            || *w == "search"
            || *w == "lookup"
            || *w == "locate"
            || *w == "get"
            || *w == "fetch"
            || *w == "retrieve"
    });
    let has_list = words.iter().any(|w| {
        *w == "list" || *w == "all" || *w == "every" || *w == "collect" || *w == "enumerate"
    });
    let has_create = words.iter().any(|w| {
        *w == "create"
            || *w == "build"
            || *w == "make"
            || *w == "new"
            || *w == "construct"
            || *w == "generate"
            || *w == "produce"
    });
    let has_transform = words.iter().any(|w| {
        *w == "transform"
            || *w == "convert"
            || *w == "map"
            || *w == "encode"
            || *w == "decode"
            || *w == "parse"
            || *w == "serialize"
    });
    let has_send = words.iter().any(|w| {
        *w == "send"
            || *w == "receive"
            || *w == "write"
            || *w == "post"
            || *w == "emit"
            || *w == "dispatch"
            || *w == "notify"
    });
    let has_delete = words.iter().any(|w| {
        *w == "delete"
            || *w == "remove"
            || *w == "clear"
            || *w == "drop"
            || *w == "destroy"
            || *w == "prune"
            || *w == "clean"
    });
    let has_count = words
        .iter()
        .any(|w| *w == "count" || *w == "how many" || *w == "number" || *w == "size");
    let has_handle = words.iter().any(|w| {
        *w == "handle"
            || *w == "process"
            || *w == "manage"
            || *w == "run"
            || *w == "execute"
            || *w == "perform"
    });
    let has_compare = words.iter().any(|w| {
        *w == "compare" || *w == "match" || *w == "equal" || *w == "similar" || *w == "difference"
    });
    let has_config = words
        .iter()
        .any(|w| *w == "config" || *w == "setting" || *w == "option" || *w == "configure");
    let has_connect = words
        .iter()
        .any(|w| *w == "connect" || *w == "join" || *w == "link" || *w == "bind" || *w == "attach");

    if has_check {
        intents.push(IntentPattern {
            return_categories: vec![
                ReturnCategory::Bool,
                ReturnCategory::Result,
                ReturnCategory::Option,
            ],
            preferred_param_count: Some(1),
            param_categories: vec![
                ParamCategory::Value,
                ParamCategory::String,
                ParamCategory::Id,
            ],
            prefer_async: false,
            prefer_method: false,
            prefer_function: false,
            weight: 0.8,
        });
    }

    if has_error {
        intents.push(IntentPattern {
            return_categories: vec![ReturnCategory::Result, ReturnCategory::Void],
            preferred_param_count: None,
            param_categories: vec![ParamCategory::Value, ParamCategory::String],
            prefer_async: false,
            prefer_method: false,
            prefer_function: false,
            weight: 0.7,
        });
    }

    if has_find {
        intents.push(IntentPattern {
            return_categories: vec![
                ReturnCategory::Option,
                ReturnCategory::Result,
                ReturnCategory::Collection,
                ReturnCategory::Unknown,
            ],
            preferred_param_count: Some(1),
            param_categories: vec![
                ParamCategory::String,
                ParamCategory::Id,
                ParamCategory::Value,
            ],
            prefer_async: false,
            prefer_method: true,
            prefer_function: false,
            weight: 0.7,
        });
    }

    if has_list {
        intents.push(IntentPattern {
            return_categories: vec![ReturnCategory::Collection],
            preferred_param_count: Some(0),
            param_categories: vec![],
            prefer_async: false,
            prefer_method: true,
            prefer_function: true,
            weight: 0.8,
        });
    }

    if has_create {
        intents.push(IntentPattern {
            return_categories: vec![ReturnCategory::Unknown, ReturnCategory::Result],
            preferred_param_count: Some(2),
            param_categories: vec![
                ParamCategory::Config,
                ParamCategory::String,
                ParamCategory::Value,
            ],
            prefer_async: false,
            prefer_method: false,
            prefer_function: true,
            weight: 0.6,
        });
    }

    if has_transform {
        intents.push(IntentPattern {
            return_categories: vec![
                ReturnCategory::Unknown,
                ReturnCategory::String,
                ReturnCategory::Result,
            ],
            preferred_param_count: Some(1),
            param_categories: vec![
                ParamCategory::String,
                ParamCategory::Value,
                ParamCategory::Collection,
            ],
            prefer_async: false,
            prefer_method: true,
            prefer_function: true,
            weight: 0.7,
        });
    }

    if has_send {
        intents.push(IntentPattern {
            return_categories: vec![
                ReturnCategory::Result,
                ReturnCategory::Void,
                ReturnCategory::Unknown,
            ],
            preferred_param_count: Some(1),
            param_categories: vec![
                ParamCategory::Value,
                ParamCategory::String,
                ParamCategory::Collection,
            ],
            prefer_async: true,
            prefer_method: true,
            prefer_function: false,
            weight: 0.8,
        });
    }

    if has_delete {
        intents.push(IntentPattern {
            return_categories: vec![
                ReturnCategory::Void,
                ReturnCategory::Result,
                ReturnCategory::Bool,
            ],
            preferred_param_count: Some(1),
            param_categories: vec![
                ParamCategory::String,
                ParamCategory::Id,
                ParamCategory::Value,
            ],
            prefer_async: false,
            prefer_method: true,
            prefer_function: false,
            weight: 0.7,
        });
    }

    if has_count {
        intents.push(IntentPattern {
            return_categories: vec![ReturnCategory::Number, ReturnCategory::Collection],
            preferred_param_count: Some(0),
            param_categories: vec![],
            prefer_async: false,
            prefer_method: true,
            prefer_function: true,
            weight: 0.7,
        });
    }

    if has_handle {
        intents.push(IntentPattern {
            return_categories: vec![
                ReturnCategory::Result,
                ReturnCategory::Void,
                ReturnCategory::Unknown,
            ],
            preferred_param_count: Some(1),
            param_categories: vec![ParamCategory::Value, ParamCategory::Config],
            prefer_async: true,
            prefer_method: true,
            prefer_function: false,
            weight: 0.5,
        });
    }

    if has_compare {
        intents.push(IntentPattern {
            return_categories: vec![
                ReturnCategory::Number,
                ReturnCategory::Bool,
                ReturnCategory::Unknown,
            ],
            preferred_param_count: Some(2),
            param_categories: vec![
                ParamCategory::Value,
                ParamCategory::String,
                ParamCategory::Collection,
            ],
            prefer_async: false,
            prefer_method: true,
            prefer_function: true,
            weight: 0.7,
        });
    }

    if has_config {
        intents.push(IntentPattern {
            return_categories: vec![ReturnCategory::Unknown, ReturnCategory::Result],
            preferred_param_count: Some(0),
            param_categories: vec![],
            prefer_async: false,
            prefer_method: false,
            prefer_function: false,
            weight: 0.5,
        });
    }

    if has_connect {
        intents.push(IntentPattern {
            return_categories: vec![ReturnCategory::Result, ReturnCategory::Void],
            preferred_param_count: Some(1),
            param_categories: vec![
                ParamCategory::String,
                ParamCategory::Config,
                ParamCategory::Value,
            ],
            prefer_async: true,
            prefer_method: true,
            prefer_function: false,
            weight: 0.6,
        });
    }

    intents.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap());
    intents.truncate(5);
    intents
}

pub fn score_type_match(pattern: &TypePattern, intents: &[IntentPattern]) -> f64 {
    if intents.is_empty() {
        return 0.0;
    }

    let mut best_score = 0.0f64;

    for intent in intents {
        let mut score = 0.0;
        let mut max_possible = 0.0;

        max_possible += 3.0;
        if intent.return_categories.contains(&pattern.return_category) {
            score += 3.0;
        } else {
            score += partial_return_match(pattern.return_category, &intent.return_categories) * 1.5;
        }

        max_possible += 1.5;
        if let Some(pref_count) = intent.preferred_param_count {
            let diff = (pattern.param_count as i32 - pref_count as i32).unsigned_abs();
            score += (1.5 * (1.0 - diff as f64 / 5.0).max(0.0));
        } else {
            score += 0.75;
        }

        max_possible += 1.0;
        if intent.prefer_async && pattern.is_async {
            score += 1.0;
        } else if intent.prefer_async && !pattern.is_async {
            score += 0.2;
        } else {
            score += 0.5;
        }

        max_possible += 0.5;
        if intent.prefer_method && pattern.has_self {
            score += 0.5;
        } else if intent.prefer_method && !pattern.has_self {
            score += 0.1;
        } else {
            score += 0.25;
        }

        max_possible += 0.5;
        if intent.prefer_function && !pattern.has_self {
            score += 0.5;
        } else if intent.prefer_function && pattern.has_self {
            score += 0.1;
        } else {
            score += 0.25;
        }

        let param_score =
            score_param_categories(&pattern.param_categories, &intent.param_categories);
        max_possible += 1.0;
        score += param_score;

        let normalized = if max_possible > 0.0 {
            score / max_possible
        } else {
            0.0
        };
        let weighted = normalized * intent.weight;

        if weighted > best_score {
            best_score = weighted;
        }
    }

    best_score
}

fn partial_return_match(actual: ReturnCategory, expected: &[ReturnCategory]) -> f64 {
    match actual {
        ReturnCategory::Unknown => 0.3,
        ReturnCategory::Void => 0.1,
        ReturnCategory::String => {
            if expected.contains(&ReturnCategory::String) {
                1.0
            } else {
                0.0
            }
        }
        ReturnCategory::Number => {
            if expected.contains(&ReturnCategory::Number) {
                1.0
            } else {
                0.0
            }
        }
        ReturnCategory::Bool => {
            if expected.contains(&ReturnCategory::Bool) {
                1.0
            } else {
                0.0
            }
        }
        ReturnCategory::Collection => {
            if expected.contains(&ReturnCategory::Collection) {
                1.0
            } else {
                0.0
            }
        }
        ReturnCategory::Result => {
            if expected.contains(&ReturnCategory::Result) {
                1.0
            } else if expected.contains(&ReturnCategory::Option) {
                0.4
            } else {
                0.0
            }
        }
        ReturnCategory::Option => {
            if expected.contains(&ReturnCategory::Option) {
                1.0
            } else if expected.contains(&ReturnCategory::Result) {
                0.4
            } else {
                0.0
            }
        }
    }
}

fn score_param_categories(actual: &[ParamCategory], expected: &[ParamCategory]) -> f64 {
    if expected.is_empty() {
        return if actual.is_empty() { 1.0 } else { 0.5 };
    }
    if actual.is_empty() {
        return 0.3;
    }

    let mut matches = 0usize;
    for exp in expected {
        if actual.iter().any(|a| *a == *exp) {
            matches += 1;
        }
    }

    matches as f64 / expected.len() as f64
}

pub fn typematch_search(db: &GraphDb, query: &str, top_k: usize) -> Vec<(i64, f64)> {
    let intents = classify_nl_intent(query);
    if intents.is_empty() {
        return Vec::new();
    }

    let conn = db.conn();
    let mut stmt = match conn.prepare(
        "SELECT id, name, kind, signature FROM symbols WHERE signature IS NOT NULL AND signature != ''",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let mut scored: Vec<(i64, f64)> = Vec::new();

    let rows = stmt.query_map([], |row| {
        let id: i64 = row.get(0)?;
        let name: String = row.get(1)?;
        let kind_str: String = row.get(2)?;
        let signature: String = row.get(3)?;
        Ok((id, name, kind_str, signature))
    });

    if let Some(rows) = rows.ok() {
        for row in rows.flatten() {
            let (id, name, kind_str, signature) = row;
            let kind = SymbolKind::from_str(&kind_str).unwrap_or(SymbolKind::Function);

            let sym = Symbol {
                id,
                file_id: 0,
                name,
                qualified_name: None,
                kind,
                line_start: 0,
                line_end: 0,
                signature: Some(signature),
                visibility: crate::symbol::Visibility::Public,
                doc_comment: None,
                source: String::new(),
                name_decomposed: String::new(),
                content_hash: String::new(),
                language: String::new(),
                metadata: serde_json::Value::Null,
                importance: 0.5,
                search_hints: String::new(),
            };

            let pattern = extract_type_pattern(&sym);
            let score = score_type_match(&pattern, &intents);

            if score > 0.2 {
                scored.push((id, score));
            }
        }
    }

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored.truncate(top_k);
    scored
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_return_rust() {
        assert_eq!(classify_return("fn foo() -> bool"), ReturnCategory::Bool);
        assert_eq!(
            classify_return("fn foo() -> Result<String, Error>"),
            ReturnCategory::Result
        );
        assert_eq!(
            classify_return("fn foo() -> Option<i64>"),
            ReturnCategory::Option
        );
        assert_eq!(
            classify_return("fn foo() -> Vec<Symbol>"),
            ReturnCategory::Collection
        );
        assert_eq!(
            classify_return("fn foo() -> String"),
            ReturnCategory::String
        );
        assert_eq!(classify_return("fn foo() -> i64"), ReturnCategory::Number);
        assert_eq!(classify_return("fn foo()"), ReturnCategory::Void);
        assert_eq!(classify_return("fn foo() -> ()"), ReturnCategory::Void);
    }

    #[test]
    fn test_classify_return_typescript() {
        assert_eq!(
            classify_return("function foo(): boolean"),
            ReturnCategory::Bool
        );
        assert_eq!(
            classify_return("function foo(): Promise<void>"),
            ReturnCategory::Void
        );
        assert_eq!(
            classify_return("function foo(): string[]"),
            ReturnCategory::Collection
        );
        assert_eq!(
            classify_return("function foo(): readonly string[]"),
            ReturnCategory::Collection
        );
    }

    #[test]
    fn test_classify_params() {
        let (count, cats) = classify_params("(x: i64, y: string, z: bool)", false);
        assert_eq!(count, 3);
        assert!(cats.contains(&ParamCategory::Value));
        assert!(cats.contains(&ParamCategory::String));
        assert!(cats.contains(&ParamCategory::Bool));
    }

    #[test]
    fn test_classify_params_with_self() {
        let (count, _cats) = classify_params("(&self, id: i64, name: string)", true);
        assert_eq!(count, 2);
    }

    #[test]
    fn test_classify_params_empty() {
        let (count, cats) = classify_params("()", false);
        assert_eq!(count, 0);
        assert!(cats.is_empty());
    }

    #[test]
    fn test_nl_intent_check() {
        let intents = classify_nl_intent("check if the daemon is running");
        assert!(!intents.is_empty());
        assert!(intents
            .iter()
            .any(|i| i.return_categories.contains(&ReturnCategory::Bool)));
    }

    #[test]
    fn test_nl_intent_error() {
        let intents = classify_nl_intent("how are errors propagated");
        assert!(!intents.is_empty());
        assert!(intents
            .iter()
            .any(|i| i.return_categories.contains(&ReturnCategory::Result)));
    }

    #[test]
    fn test_nl_intent_list() {
        let intents = classify_nl_intent("list all installed connectors");
        assert!(!intents.is_empty());
        assert!(intents
            .iter()
            .any(|i| i.return_categories.contains(&ReturnCategory::Collection)));
    }

    #[test]
    fn test_nl_intent_send() {
        let intents = classify_nl_intent("send a message to the channel");
        assert!(!intents.is_empty());
        assert!(intents.iter().any(|i| i.prefer_async));
    }

    #[test]
    fn test_score_type_match_bool_checker() {
        let pattern = TypePattern {
            return_category: ReturnCategory::Bool,
            param_count: 0,
            param_categories: vec![],
            has_self: true,
            is_async: false,
            kind: SymbolKind::Method,
        };

        let intents = classify_nl_intent("check if installed");
        let score = score_type_match(&pattern, &intents);
        assert!(score > 0.3);
    }

    #[test]
    fn test_score_type_match_result_function() {
        let pattern = TypePattern {
            return_category: ReturnCategory::Result,
            param_count: 2,
            param_categories: vec![ParamCategory::String, ParamCategory::Config],
            has_self: false,
            is_async: false,
            kind: SymbolKind::Function,
        };

        let intents = classify_nl_intent("create a new connection");
        let score = score_type_match(&pattern, &intents);
        assert!(score > 0.2);
    }

    #[test]
    fn test_no_intent_for_short_queries() {
        let intents = classify_nl_intent("foo");
        assert!(intents.is_empty());
    }
}
