use rusqlite::params;
use std::collections::{HashMap, HashSet, VecDeque};

use crate::db::GraphDb;
use crate::role_query::query_to_expected_roles;
use crate::roles::RoleTag;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WalkDir {
    Forward,
    Backward,
    Both,
}

#[derive(Debug, Clone)]
pub enum TraversalOp {
    FindByRole(RoleTag),
    FindByName(String),
    Walk {
        direction: WalkDir,
        edge_kind: Option<String>,
        depth: usize,
    },
    Collect,
}

#[derive(Debug, Clone)]
pub struct TraversalHit {
    pub symbol_id: i64,
    pub score: f64,
    pub depth: usize,
}

#[derive(Debug, Clone)]
pub enum QueryIntent {
    TextSearch,
    GraphTraversal {
        ops: Vec<TraversalOp>,
        seed: Option<String>,
    },
    Hybrid {
        text: String,
        traversal_ops: Vec<TraversalOp>,
    },
}

pub fn classify_query(query: &str) -> QueryIntent {
    let lower = query.to_lowercase();
    let stripped = strip_prefix(&lower);
    let tokens: Vec<&str> = stripped.split_whitespace().collect();

    if tokens.len() < 2 {
        return QueryIntent::TextSearch;
    }

    let original_tokens: Vec<&str> = query.split_whitespace().collect();
    let code_count = original_tokens.iter().filter(|t| is_code_token(t)).count();

    if let Some(plan) = try_connection_pattern(&lower, &tokens, &original_tokens) {
        return make_intent(query, plan, code_count);
    }

    if let Some(plan) = try_caller_pattern(&lower, &tokens, &original_tokens) {
        return make_intent(query, plan, code_count);
    }

    if let Some(plan) = try_callee_pattern(&lower, &tokens, &original_tokens) {
        return make_intent(query, plan, code_count);
    }

    if let Some(plan) = try_usage_pattern(&lower, &tokens, &original_tokens) {
        return make_intent(query, plan, code_count);
    }

    let roles = query_to_expected_roles(query);
    if !roles.is_empty() {
        let mut ops = Vec::new();
        for role in &roles {
            ops.push(TraversalOp::FindByRole(*role));
        }
        ops.push(TraversalOp::Walk {
            direction: WalkDir::Both,
            edge_kind: None,
            depth: 2,
        });
        ops.push(TraversalOp::Collect);
        return QueryIntent::GraphTraversal { ops, seed: None };
    }

    if let Some(plan) = try_generic_two_token(&lower, &tokens, &original_tokens) {
        return QueryIntent::Hybrid {
            text: query.to_string(),
            traversal_ops: plan,
        };
    }

    QueryIntent::TextSearch
}

fn make_intent(query: &str, plan: Vec<TraversalOp>, code_count: usize) -> QueryIntent {
    if code_count >= 2 {
        QueryIntent::GraphTraversal {
            ops: plan,
            seed: None,
        }
    } else {
        QueryIntent::Hybrid {
            text: query.to_string(),
            traversal_ops: plan,
        }
    }
}

fn strip_prefix(lower: &str) -> String {
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
        "who ",
        "which ",
    ];
    let mut s = lower.trim().to_string();
    for prefix in &prefixes {
        if s.starts_with(prefix) {
            s = s[prefix.len()..].to_string();
        }
    }
    let suffixes = [
        " work",
        " happen",
        " occur",
        " get",
        " function",
        " implemented",
        " managed",
    ];
    for suffix in &suffixes {
        if s.ends_with(suffix) {
            s = s[..s.len() - suffix.len()].to_string();
        }
    }
    s.trim().to_string()
}

fn is_code_token(t: &&str) -> bool {
    let s = *t;
    s.len() > 4 && s.contains('_')
        || s.contains("::")
        || s.chars()
            .enumerate()
            .any(|(i, c)| i > 0 && c.is_uppercase())
        || matches!(
            *t,
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
}

fn try_connection_pattern(
    lower: &str,
    tokens: &[&str],
    original_tokens: &[&str],
) -> Option<Vec<TraversalOp>> {
    let patterns: &[(&str, &str)] = &[
        ("how does", "connect"),
        ("how does", "relate"),
        ("how does", "link"),
        ("what connects", "to"),
        ("how are", "connected"),
        ("how are", "linked"),
        ("relationship", "between"),
    ];
    for &(p1, p2) in patterns {
        if lower.contains(p1) && lower.contains(p2) {
            let code_tokens: Vec<String> = tokens
                .iter()
                .filter(|t| is_code_token(t))
                .map(|t| t.to_string())
                .collect();
            if code_tokens.len() >= 2 {
                return Some(vec![
                    TraversalOp::FindByName(code_tokens[0].clone()),
                    TraversalOp::Walk {
                        direction: WalkDir::Forward,
                        edge_kind: None,
                        depth: 3,
                    },
                    TraversalOp::Walk {
                        direction: WalkDir::Backward,
                        edge_kind: None,
                        depth: 2,
                    },
                    TraversalOp::Collect,
                ]);
            }
        }
    }
    None
}

fn try_caller_pattern(
    lower: &str,
    tokens: &[&str],
    original_tokens: &[&str],
) -> Option<Vec<TraversalOp>> {
    let caller_patterns: &[&str] = &[
        "what calls",
        "who calls",
        "what invokes",
        "who invokes",
        "where is",
        "used",
        "what uses",
        "who uses",
        "callers of",
        "callers for",
    ];
    for p in caller_patterns {
        if lower.contains(p) {
            let code_tokens: Vec<String> = original_tokens
                .iter()
                .filter(|t| is_code_token(t))
                .map(|t| t.to_string())
                .collect();
            if !code_tokens.is_empty() {
                return Some(vec![
                    TraversalOp::FindByName(code_tokens[0].clone()),
                    TraversalOp::Walk {
                        direction: WalkDir::Backward,
                        edge_kind: Some("calls".into()),
                        depth: 3,
                    },
                    TraversalOp::Collect,
                ]);
            }
        }
    }
    None
}

fn try_callee_pattern(
    lower: &str,
    tokens: &[&str],
    original_tokens: &[&str],
) -> Option<Vec<TraversalOp>> {
    let callee_patterns: &[&str] = &[
        "what does call",
        "what does invoke",
        "callees of",
        "callees for",
    ];
    for p in callee_patterns {
        if lower.contains(p) {
            let code_tokens: Vec<String> = original_tokens
                .iter()
                .filter(|t| is_code_token(t))
                .map(|t| t.to_string())
                .collect();
            if !code_tokens.is_empty() {
                return Some(vec![
                    TraversalOp::FindByName(code_tokens[0].clone()),
                    TraversalOp::Walk {
                        direction: WalkDir::Forward,
                        edge_kind: Some("calls".into()),
                        depth: 3,
                    },
                    TraversalOp::Collect,
                ]);
            }
        }
    }
    None
}

fn try_usage_pattern(
    lower: &str,
    tokens: &[&str],
    original_tokens: &[&str],
) -> Option<Vec<TraversalOp>> {
    let usage_patterns: &[&str] = &[
        "where is",
        "defined",
        "where is",
        "implemented",
        "where is",
        "created",
        "where is",
        "declared",
    ];
    for p in usage_patterns {
        if lower.contains(p) {
            let code_tokens: Vec<String> = original_tokens
                .iter()
                .filter(|t| is_code_token(t))
                .map(|t| t.to_string())
                .collect();
            if !code_tokens.is_empty() {
                return Some(vec![
                    TraversalOp::FindByName(code_tokens[0].clone()),
                    TraversalOp::Walk {
                        direction: WalkDir::Both,
                        edge_kind: None,
                        depth: 1,
                    },
                    TraversalOp::Collect,
                ]);
            }
        }
    }
    None
}

fn try_generic_two_token(
    lower: &str,
    tokens: &[&str],
    original_tokens: &[&str],
) -> Option<Vec<TraversalOp>> {
    if !lower.starts_with("how") && !lower.starts_with("what") && !lower.starts_with("where") {
        return None;
    }
    if !crate::decompose::is_decomposable_query(lower) {
        return None;
    }
    let code_tokens: Vec<String> = original_tokens
        .iter()
        .filter(|t| is_code_token(t))
        .map(|t| t.to_string())
        .collect();
    if code_tokens.len() >= 2 {
        return Some(vec![
            TraversalOp::FindByName(code_tokens[0].clone()),
            TraversalOp::Walk {
                direction: WalkDir::Forward,
                edge_kind: None,
                depth: 2,
            },
            TraversalOp::FindByName(code_tokens[1].clone()),
            TraversalOp::Walk {
                direction: WalkDir::Backward,
                edge_kind: None,
                depth: 2,
            },
            TraversalOp::Collect,
        ]);
    }
    None
}

pub fn execute_plan(
    ops: &[TraversalOp],
    db: &GraphDb,
    fts_seeds: Option<&[(i64, f64)]>,
) -> Vec<TraversalHit> {
    let mut candidates: Vec<TraversalHit> = Vec::new();

    for op in ops {
        match op {
            TraversalOp::FindByRole(role) => {
                let hits = find_by_role(db, role);
                if candidates.is_empty() {
                    candidates = hits;
                } else {
                    merge_hits(&mut candidates, &hits);
                }
            }
            TraversalOp::FindByName(name) => {
                let hits = find_by_name(db, name);
                if candidates.is_empty() {
                    candidates = hits;
                } else {
                    merge_hits(&mut candidates, &hits);
                }
            }
            TraversalOp::Walk {
                direction,
                edge_kind,
                depth,
            } => {
                if !candidates.is_empty() {
                    let walked =
                        walk_graph(db, &candidates, *direction, edge_kind.as_deref(), *depth);
                    candidates = walked;
                }
            }
            TraversalOp::Collect => {}
        }
    }

    if let Some(seeds) = fts_seeds {
        for &(id, score) in seeds {
            if !candidates.iter().any(|c| c.symbol_id == id) {
                candidates.push(TraversalHit {
                    symbol_id: id,
                    score: score * 0.5,
                    depth: 0,
                });
            }
        }
    }

    candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    candidates
}

fn find_by_name(db: &GraphDb, name: &str) -> Vec<TraversalHit> {
    let conn = db.conn();
    let pattern = format!("%{}%", name);
    let rows: Vec<(i64, f64)> = match conn.prepare(
        "SELECT id, importance FROM symbols WHERE name LIKE ?1 OR name_decomposed LIKE ?1 LIMIT 50",
    ) {
        Ok(mut stmt) => stmt
            .query_map(params![pattern], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1).unwrap_or(0.5)))
            })
            .ok()
            .map(|r| r.filter_map(Result::ok).collect())
            .unwrap_or_default(),
        Err(_) => return Vec::new(),
    };
    rows.into_iter()
        .map(|(id, imp)| TraversalHit {
            symbol_id: id,
            score: 0.5 + imp,
            depth: 0,
        })
        .collect()
}

fn find_by_role(db: &GraphDb, role: &RoleTag) -> Vec<TraversalHit> {
    let role_str = role.as_str();
    let role_terms = role.fts_terms();
    let conn = db.conn();
    let pattern = format!("%{}%", role_str);
    let rows: Vec<(i64, f64)> = match conn
        .prepare("SELECT id, importance FROM symbols WHERE search_hints LIKE ?1 LIMIT 100")
    {
        Ok(mut stmt) => stmt
            .query_map(params![pattern], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1).unwrap_or(0.5)))
            })
            .ok()
            .map(|r| r.filter_map(Result::ok).collect())
            .unwrap_or_default(),
        Err(_) => return Vec::new(),
    };

    if rows.is_empty() {
        let terms: Vec<&str> = role_terms.split_whitespace().collect();
        for term in &terms {
            let p = format!("%{}%", term);
            if let Ok(mut s) = conn
                .prepare("SELECT id, importance FROM symbols WHERE search_hints LIKE ?1 LIMIT 50")
            {
                let t_rows: Vec<(i64, f64)> = s
                    .query_map(params![p], |row| {
                        Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1).unwrap_or(0.5)))
                    })
                    .ok()
                    .map(|r| r.filter_map(Result::ok).collect())
                    .unwrap_or_default();
                return t_rows
                    .into_iter()
                    .map(|(id, imp)| TraversalHit {
                        symbol_id: id,
                        score: 0.4 + imp,
                        depth: 0,
                    })
                    .collect();
            }
        }
        return Vec::new();
    }

    rows.into_iter()
        .map(|(id, imp)| TraversalHit {
            symbol_id: id,
            score: 0.4 + imp,
            depth: 0,
        })
        .collect()
}

fn walk_graph(
    db: &GraphDb,
    seeds: &[TraversalHit],
    direction: WalkDir,
    edge_kind: Option<&str>,
    max_depth: usize,
) -> Vec<TraversalHit> {
    let mut visited: HashSet<i64> = seeds.iter().map(|h| h.symbol_id).collect();
    let mut results: Vec<TraversalHit> = seeds.to_vec();
    let mut frontier: VecDeque<(i64, usize)> =
        seeds.iter().map(|h| (h.symbol_id, 0usize)).collect();

    while let Some((id, depth)) = frontier.pop_front() {
        if depth >= max_depth {
            continue;
        }

        let edges = match direction {
            WalkDir::Forward => db.edges_from(id).unwrap_or_default(),
            WalkDir::Backward => db.edges_to(id).unwrap_or_default(),
            WalkDir::Both => {
                let mut e = db.edges_from(id).unwrap_or_default();
                e.extend(db.edges_to(id).unwrap_or_default());
                e
            }
        };

        for edge in &edges {
            if let Some(kind) = edge_kind {
                if edge.kind.as_str() != kind {
                    continue;
                }
            }
            let target = match direction {
                WalkDir::Forward => edge.target_id,
                WalkDir::Backward => edge.source_id,
                WalkDir::Both => {
                    if edge.source_id == id {
                        edge.target_id
                    } else {
                        edge.source_id
                    }
                }
            };
            if visited.insert(target) {
                let score = 1.0 / ((depth + 1) as f64);
                results.push(TraversalHit {
                    symbol_id: target,
                    score,
                    depth: depth + 1,
                });
                frontier.push_back((target, depth + 1));
            }
        }
    }

    results
}

fn merge_hits(existing: &mut Vec<TraversalHit>, new_hits: &[TraversalHit]) {
    let existing_map: HashMap<i64, f64> = existing.iter().map(|h| (h.symbol_id, h.score)).collect();
    for hit in new_hits {
        if let Some(&old_score) = existing_map.get(&hit.symbol_id) {
            if let Some(e) = existing.iter_mut().find(|h| h.symbol_id == hit.symbol_id) {
                e.score = old_score + hit.score * 0.5;
            }
        } else {
            existing.push(hit.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_code_query() {
        let intent = classify_query("RateLimiter");
        assert!(matches!(intent, QueryIntent::TextSearch));
    }

    #[test]
    fn test_classify_short_query() {
        let intent = classify_query("cache");
        assert!(matches!(intent, QueryIntent::TextSearch));
    }

    #[test]
    fn test_classify_what_calls() {
        let intent = classify_query("what calls authenticateUser");
        assert!(matches!(intent, QueryIntent::Hybrid { .. }));
    }

    #[test]
    fn test_classify_error_propagation() {
        let intent = classify_query("how are errors propagated");
        assert!(matches!(intent, QueryIntent::GraphTraversal { .. }));
    }

    #[test]
    fn test_classify_error_handling() {
        let intent = classify_query("what handles errors in the system");
        assert!(matches!(intent, QueryIntent::GraphTraversal { .. }));
    }

    #[test]
    fn test_classify_how_does_connect() {
        let intent = classify_query("how does SearchEngine connect to FtsSearch");
        assert!(matches!(intent, QueryIntent::GraphTraversal { .. }));
    }

    #[test]
    fn test_classify_nl_descriptive() {
        let intent = classify_query("what validates input before processing");
        assert!(matches!(intent, QueryIntent::GraphTraversal { .. }));
    }

    #[test]
    fn test_classify_validate() {
        let intent = classify_query("what validates input");
        assert!(matches!(intent, QueryIntent::GraphTraversal { .. }));
    }
}
