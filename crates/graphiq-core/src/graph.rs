use rusqlite::params;

use crate::db::GraphDb;
use crate::edge::EdgeKind;
use crate::symbol::Symbol;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraverseDirection {
    Outgoing,
    Incoming,
}

#[derive(Debug, Clone)]
pub struct ExpansionEntry {
    pub symbol: Symbol,
    pub origin_id: i64,
    pub distance: usize,
    pub score: f64,
    pub edge_kinds: Vec<EdgeKind>,
}

pub fn bounded_bfs(
    db: &GraphDb,
    start_ids: &[i64],
    direction: TraverseDirection,
    edge_filter: &[EdgeKind],
    max_depth: usize,
) -> Vec<(i64, usize, Vec<EdgeKind>)> {
    if start_ids.is_empty() || max_depth == 0 {
        return Vec::new();
    }

    let conn = db.conn();
    let edge_kinds_sql: Vec<String> = edge_filter
        .iter()
        .map(|k| format!("'{}'", k.as_str()))
        .collect();
    let kinds_clause = if edge_kinds_sql.is_empty() {
        "1=1".into()
    } else {
        format!("e.kind IN ({})", edge_kinds_sql.join(", "))
    };

    let (src_col, tgt_col) = match direction {
        TraverseDirection::Outgoing => ("source_id", "target_id"),
        TraverseDirection::Incoming => ("target_id", "source_id"),
    };

    let start_list = start_ids
        .iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(",");

    let sql = format!(
        "WITH RECURSIVE bfs AS (
            SELECT {tgt_col} AS symbol_id, 1 AS distance, JSON_ARRAY(e.kind) AS path
            FROM edges e WHERE {src_col} IN ({start_list}) AND {kinds}
            UNION ALL
            SELECT e.{tgt_col}, b.distance + 1,
                   JSON_INSERT(b.path, '$[#]', e.kind)
            FROM edges e
            JOIN bfs b ON e.{src_col} = b.symbol_id
            WHERE b.distance < ?1 AND {kinds}
        )
        SELECT DISTINCT symbol_id, distance, path FROM bfs
        ORDER BY distance",
        tgt_col = tgt_col,
        src_col = src_col,
        start_list = start_list,
        kinds = kinds_clause,
    );

    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let rows = stmt
        .query_map(params![max_depth as i64], |row| {
            let symbol_id: i64 = row.get(0)?;
            let distance: usize = row.get::<_, i64>(1)? as usize;
            let path_str: String = row.get(2)?;
            let edge_kinds = parse_edge_kind_json(&path_str);
            Ok((symbol_id, distance, edge_kinds))
        })
        .ok();

    match rows {
        Some(r) => r.flatten().collect(),
        None => Vec::new(),
    }
}

fn parse_edge_kind_json(json: &str) -> Vec<EdgeKind> {
    let items: Vec<String> = serde_json::from_str(json).unwrap_or_default();
    items.iter().filter_map(|s| EdgeKind::from_str(s)).collect()
}

pub struct StructuralExpander<'a> {
    db: &'a GraphDb,
}

impl<'a> StructuralExpander<'a> {
    pub fn new(db: &'a GraphDb) -> Self {
        Self { db }
    }

    pub fn expand(
        &self,
        fts_results: &[crate::fts::FtsResult],
        top_n: usize,
        max_depth: usize,
    ) -> Vec<ExpansionEntry> {
        let seeds: Vec<i64> = fts_results
            .iter()
            .take(top_n)
            .map(|r| r.symbol.id)
            .collect();
        if seeds.is_empty() {
            return Vec::new();
        }

        let all_edge_kinds: Vec<EdgeKind> = vec![
            EdgeKind::Calls,
            EdgeKind::Contains,
            EdgeKind::Implements,
            EdgeKind::Extends,
            EdgeKind::Imports,
            EdgeKind::References,
            EdgeKind::Tests,
        ];

        let outgoing = bounded_bfs(
            self.db,
            &seeds,
            TraverseDirection::Outgoing,
            &all_edge_kinds,
            max_depth,
        );
        let incoming = bounded_bfs(
            self.db,
            &seeds,
            TraverseDirection::Incoming,
            &all_edge_kinds,
            max_depth,
        );

        let fts_scores: std::collections::HashMap<i64, f64> = fts_results
            .iter()
            .map(|r| (r.symbol.id, r.bm25_score))
            .collect();

        let mut best_scores: std::collections::HashMap<
            i64,
            (f64, i64, usize, Vec<EdgeKind>, usize),
        > = std::collections::HashMap::new();

        for (symbol_id, distance, edge_kinds) in outgoing.iter().chain(incoming.iter()) {
            if fts_scores.contains_key(symbol_id) {
                continue;
            }

            let pw: f64 = edge_kinds
                .iter()
                .map(|k| k.path_weight())
                .fold(1.0f64, f64::min);
            let best_fts = fts_scores.values().cloned().fold(0.0f64, f64::max);
            let candidate_score = best_fts * decay(*distance) * pw;

            let entry = best_scores.entry(*symbol_id).or_insert((
                0.0,
                seeds[0],
                *distance,
                edge_kinds.clone(),
                0,
            ));

            let path_count = entry.4 + 1;
            let multi_path_bonus = if path_count > 1 {
                0.05 * (path_count - 1).min(3) as f64
            } else {
                0.0
            };

            if candidate_score > entry.0 {
                *entry = (
                    candidate_score + multi_path_bonus,
                    seeds[0],
                    *distance,
                    edge_kinds.clone(),
                    path_count,
                );
            } else {
                entry.0 += multi_path_bonus;
                entry.4 = path_count;
            }
        }

        let mut entries = Vec::new();
        for (symbol_id, (score, origin_id, distance, edge_kinds, _)) in best_scores {
            if let Some(sym) = self.db.get_symbol(symbol_id).unwrap_or(None) {
                entries.push(ExpansionEntry {
                    symbol: sym,
                    origin_id,
                    distance,
                    score,
                    edge_kinds,
                });
            }
        }

        entries.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        entries
    }
}

fn decay(distance: usize) -> f64 {
    match distance {
        0 => 1.0,
        1 => 0.5,
        2 => 0.25,
        _ => 0.1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edge::EdgeKind;
    use crate::fts::FtsSearch;
    use crate::symbol::{SymbolBuilder, SymbolKind};

    fn setup_db_with_graph() -> GraphDb {
        let db = GraphDb::open_in_memory().unwrap();
        let fid = db
            .upsert_file("src/app.ts", "typescript", "abc", 1000, 100)
            .unwrap();

        let s1 = SymbolBuilder::new(
            fid,
            "main".into(),
            SymbolKind::Function,
            "fn main()".into(),
            "typescript".into(),
        )
        .lines(1, 3)
        .build();
        let s2 = SymbolBuilder::new(
            fid,
            "authenticate".into(),
            SymbolKind::Function,
            "fn authenticate()".into(),
            "typescript".into(),
        )
        .lines(5, 10)
        .build();
        let s3 = SymbolBuilder::new(
            fid,
            "verify".into(),
            SymbolKind::Function,
            "fn verify()".into(),
            "typescript".into(),
        )
        .lines(12, 15)
        .build();

        let id1 = db.insert_symbol(&s1).unwrap();
        let id2 = db.insert_symbol(&s2).unwrap();
        let id3 = db.insert_symbol(&s3).unwrap();

        db.insert_edge(id1, id2, EdgeKind::Calls, 1.0, serde_json::Value::Null)
            .unwrap();
        db.insert_edge(id2, id3, EdgeKind::Calls, 1.0, serde_json::Value::Null)
            .unwrap();

        db
    }

    #[test]
    fn test_bounded_bfs_outgoing() {
        let db = setup_db_with_graph();
        let results = bounded_bfs(
            &db,
            &[1],
            TraverseDirection::Outgoing,
            &[EdgeKind::Calls],
            2,
        );
        assert!(results.len() >= 2);
        let ids: Vec<i64> = results.iter().map(|(id, _, _)| *id).collect();
        assert!(ids.contains(&2));
        assert!(ids.contains(&3));
    }

    #[test]
    fn test_bounded_bfs_incoming() {
        let db = setup_db_with_graph();
        let results = bounded_bfs(
            &db,
            &[3],
            TraverseDirection::Incoming,
            &[EdgeKind::Calls],
            2,
        );
        assert!(!results.is_empty());
        let ids: Vec<i64> = results.iter().map(|(id, _, _)| *id).collect();
        assert!(ids.contains(&2));
    }

    #[test]
    fn test_bounded_bfs_depth_limit() {
        let db = setup_db_with_graph();
        let results = bounded_bfs(
            &db,
            &[1],
            TraverseDirection::Outgoing,
            &[EdgeKind::Calls],
            1,
        );
        let ids: Vec<i64> = results.iter().map(|(id, _, _)| *id).collect();
        assert!(ids.contains(&2));
        assert!(!ids.contains(&3));
    }

    #[test]
    fn test_structural_expand() {
        let db = setup_db_with_graph();
        let fts = FtsSearch::new(&db);
        let fts_results = fts.search("main", Some(20));

        let expander = StructuralExpander::new(&db);
        let expanded = expander.expand(&fts_results, 20, 2);

        let names: Vec<&str> = expanded.iter().map(|e| e.symbol.name.as_str()).collect();
        assert!(names.contains(&"authenticate") || names.contains(&"verify"));
    }

    #[test]
    fn test_decay() {
        assert!((decay(0) - 1.0).abs() < f64::EPSILON);
        assert!((decay(1) - 0.5).abs() < f64::EPSILON);
        assert!((decay(2) - 0.25).abs() < f64::EPSILON);
        assert!((decay(3) - 0.1).abs() < f64::EPSILON);
    }
}
