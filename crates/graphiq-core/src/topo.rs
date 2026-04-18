use std::collections::{HashMap, HashSet};

use crate::db::GraphDb;
use crate::edge::EdgeKind;

const MIRROR_MIN_ROUTES: usize = 2;
const MIRROR_MIN_SCORE: f64 = 0.6;
const MAX_SEEDS: usize = 15;

#[derive(Debug, Clone)]
struct RouteEntry {
    seed_id: i64,
    intermediate_id: i64,
    kind: EdgeKind,
}

pub fn topo_mirror_search(db: &GraphDb, seed_ids: &[i64], top_k: usize) -> Vec<(i64, f64)> {
    let seeds: Vec<i64> = seed_ids.iter().take(MAX_SEEDS).copied().collect();
    if seeds.len() < 2 {
        return Vec::new();
    }

    let seed_set: HashSet<i64> = seeds.iter().copied().collect();
    let mut routes: HashMap<i64, Vec<RouteEntry>> = HashMap::new();

    for &seed in &seeds {
        let mut d1: Vec<(i64, EdgeKind)> = Vec::new();

        if let Ok(edges) = db.edges_from(seed) {
            for e in &edges {
                if e.target_id != seed {
                    d1.push((e.target_id, e.kind));
                }
            }
        }
        if let Ok(edges) = db.edges_to(seed) {
            for e in &edges {
                if e.source_id != seed {
                    d1.push((e.source_id, e.kind));
                }
            }
        }

        for &(neighbor, kind) in &d1 {
            if seed_set.contains(&neighbor) {
                continue;
            }
            routes.entry(neighbor).or_default().push(RouteEntry {
                seed_id: seed,
                intermediate_id: 0,
                kind,
            });
        }

        for &(d1_id, d1_kind) in &d1 {
            if let Ok(edges) = db.edges_from(d1_id) {
                for e in &edges {
                    if e.target_id != seed && !seed_set.contains(&e.target_id) {
                        routes.entry(e.target_id).or_default().push(RouteEntry {
                            seed_id: seed,
                            intermediate_id: d1_id,
                            kind: d1_kind,
                        });
                    }
                }
            }
            if let Ok(edges) = db.edges_to(d1_id) {
                for e in &edges {
                    if e.source_id != seed && !seed_set.contains(&e.source_id) {
                        routes.entry(e.source_id).or_default().push(RouteEntry {
                            seed_id: seed,
                            intermediate_id: d1_id,
                            kind: d1_kind,
                        });
                    }
                }
            }
        }
    }

    let mut scored: Vec<(i64, f64)> = Vec::new();

    for (&candidate, candidate_routes) in &routes {
        if seed_set.contains(&candidate) {
            continue;
        }
        if candidate_routes.len() < MIRROR_MIN_ROUTES {
            continue;
        }

        let distinct_seeds: HashSet<i64> = candidate_routes.iter().map(|r| r.seed_id).collect();
        let distinct_intermediates: HashSet<i64> = candidate_routes
            .iter()
            .map(|r| r.intermediate_id)
            .filter(|&i| i != 0)
            .collect();

        let seed_div = distinct_seeds.len();
        let path_div = distinct_intermediates.len();

        if seed_div < 2 && path_div < 2 {
            continue;
        }

        let kind_set: HashSet<EdgeKind> = candidate_routes.iter().map(|r| r.kind).collect();
        let kind_div = kind_set.len().min(3);

        let has_calls = kind_set.contains(&EdgeKind::Calls);
        let has_structural = kind_set.contains(&EdgeKind::Contains)
            || kind_set.contains(&EdgeKind::Implements)
            || kind_set.contains(&EdgeKind::Extends);

        let structural_bonus = if has_calls && has_structural {
            1.3
        } else {
            1.0
        };

        let score = (seed_div as f64 * 0.45
            + path_div as f64 * 0.30
            + kind_div as f64 * 0.15
            + (candidate_routes.len() as f64 * 0.05).min(0.3))
            * structural_bonus;

        if score >= MIRROR_MIN_SCORE {
            scored.push((candidate, score));
        }
    }

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored.truncate(top_k);
    scored
}

pub fn topo_nl_boost(db: &GraphDb, fts_ids: &[i64], top_k: usize) -> Vec<(i64, f64)> {
    if fts_ids.len() < 2 {
        return Vec::new();
    }

    let seeds: Vec<i64> = fts_ids.iter().take(MAX_SEEDS).copied().collect();
    let mirror_results = topo_mirror_search(db, &seeds, top_k * 2);

    let fts_set: HashSet<i64> = fts_ids.iter().copied().collect();

    let mut co_reachable: HashMap<i64, f64> = HashMap::new();

    for &(candidate_id, mirror_score) in &mirror_results {
        if fts_set.contains(&candidate_id) {
            continue;
        }

        if let Ok(edges) = db.edges_from(candidate_id) {
            for e in &edges {
                if fts_set.contains(&e.target_id) {
                    *co_reachable.entry(candidate_id).or_insert(0.0) += e.kind.path_weight();
                }
            }
        }
        if let Ok(edges) = db.edges_to(candidate_id) {
            for e in &edges {
                if fts_set.contains(&e.source_id) {
                    *co_reachable.entry(candidate_id).or_insert(0.0) += e.kind.path_weight();
                }
            }
        }
    }

    let max_co = co_reachable
        .values()
        .cloned()
        .fold(0.0f64, f64::max)
        .max(1e-10);

    let mut results: Vec<(i64, f64)> = Vec::new();

    for (candidate_id, mirror_score) in mirror_results {
        if fts_set.contains(&candidate_id) {
            continue;
        }

        let co_score = co_reachable.get(&candidate_id).copied().unwrap_or(0.0) / max_co;
        let combined = mirror_score * (0.6 + 0.4 * co_score);

        if combined >= MIRROR_MIN_SCORE {
            results.push((candidate_id, combined));
        }
    }

    results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    results.truncate(top_k);
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::GraphDb;
    use crate::edge::EdgeKind;
    use crate::symbol::{SymbolBuilder, SymbolKind};

    fn setup_mirror_db() -> GraphDb {
        let db = GraphDb::open_in_memory().unwrap();
        let fid = db
            .upsert_file("src/lib.rs", "rust", "abc", 1000, 100)
            .unwrap();

        let symbols = vec![
            ("main", SymbolKind::Function, 1, 10),
            ("authenticate", SymbolKind::Function, 12, 25),
            ("verify_token", SymbolKind::Function, 27, 40),
            ("handle_error", SymbolKind::Function, 42, 55),
            ("parse_request", SymbolKind::Function, 57, 70),
            ("User", SymbolKind::Struct, 72, 90),
            ("AuthError", SymbolKind::Enum, 92, 100),
            ("Token", SymbolKind::Struct, 102, 115),
        ];

        let ids: Vec<i64> = symbols
            .iter()
            .map(|(name, kind, start, end)| {
                let sym = SymbolBuilder::new(
                    fid,
                    name.to_string(),
                    *kind,
                    format!("fn {}()", name),
                    "rust".to_string(),
                )
                .lines(*start, *end)
                .build();
                db.insert_symbol(&sym).unwrap()
            })
            .collect();

        // main -> authenticate, parse_request
        db.insert_edge(
            ids[0],
            ids[1],
            EdgeKind::Calls,
            1.0,
            serde_json::Value::Null,
        )
        .unwrap();
        db.insert_edge(
            ids[0],
            ids[4],
            EdgeKind::Calls,
            1.0,
            serde_json::Value::Null,
        )
        .unwrap();

        // authenticate -> verify_token, handle_error
        db.insert_edge(
            ids[1],
            ids[2],
            EdgeKind::Calls,
            1.0,
            serde_json::Value::Null,
        )
        .unwrap();
        db.insert_edge(
            ids[1],
            ids[3],
            EdgeKind::Calls,
            1.0,
            serde_json::Value::Null,
        )
        .unwrap();
        db.insert_edge(
            ids[1],
            ids[5],
            EdgeKind::References,
            0.5,
            serde_json::Value::Null,
        )
        .unwrap();

        // verify_token -> Token
        db.insert_edge(
            ids[2],
            ids[7],
            EdgeKind::References,
            0.5,
            serde_json::Value::Null,
        )
        .unwrap();

        // handle_error -> AuthError
        db.insert_edge(
            ids[3],
            ids[6],
            EdgeKind::References,
            0.5,
            serde_json::Value::Null,
        )
        .unwrap();

        // parse_request -> verify_token, handle_error (CONVERGENCE!)
        db.insert_edge(
            ids[4],
            ids[2],
            EdgeKind::Calls,
            1.0,
            serde_json::Value::Null,
        )
        .unwrap();
        db.insert_edge(
            ids[4],
            ids[3],
            EdgeKind::Calls,
            1.0,
            serde_json::Value::Null,
        )
        .unwrap();

        // User contains authenticate
        db.insert_edge(
            ids[5],
            ids[1],
            EdgeKind::Contains,
            0.8,
            serde_json::Value::Null,
        )
        .unwrap();

        db
    }

    #[test]
    fn test_mirror_finds_convergence() {
        let db = setup_mirror_db();

        // Seeds: authenticate(1) and parse_request(4) both reach verify_token(2) and handle_error(3)
        let results = topo_mirror_search(&db, &[1, 4], 10);

        let ids: Vec<i64> = results.iter().map(|(id, _)| *id).collect();

        assert!(
            ids.contains(&2),
            "verify_token should be a mirror point (reachable from both seeds)"
        );
        assert!(
            ids.contains(&3),
            "handle_error should be a mirror point (reachable from both seeds)"
        );
    }

    #[test]
    fn test_mirror_requires_convergence() {
        let db = setup_mirror_db();

        // Single seed: should find fewer mirrors
        let single = topo_mirror_search(&db, &[1], 10);
        let multi = topo_mirror_search(&db, &[1, 4], 10);

        assert!(
            multi.len() >= single.len(),
            "Multiple seeds should find at least as many mirrors"
        );
    }

    #[test]
    fn test_convergence_from_multiple_seeds() {
        let db = GraphDb::open_in_memory().unwrap();
        let fid = db
            .upsert_file("src/lib.rs", "rust", "abc", 1000, 100)
            .unwrap();

        let s1 = SymbolBuilder::new(
            fid,
            "a".into(),
            SymbolKind::Function,
            "fn a()".into(),
            "rust".into(),
        )
        .lines(1, 5)
        .build();
        let s2 = SymbolBuilder::new(
            fid,
            "b".into(),
            SymbolKind::Function,
            "fn b()".into(),
            "rust".into(),
        )
        .lines(6, 10)
        .build();
        let s3 = SymbolBuilder::new(
            fid,
            "c".into(),
            SymbolKind::Function,
            "fn c()".into(),
            "rust".into(),
        )
        .lines(11, 15)
        .build();

        let id1 = db.insert_symbol(&s1).unwrap();
        let id2 = db.insert_symbol(&s2).unwrap();
        let id3 = db.insert_symbol(&s3).unwrap();

        db.insert_edge(id1, id2, EdgeKind::Calls, 1.0, serde_json::Value::Null)
            .unwrap();
        db.insert_edge(id2, id3, EdgeKind::Calls, 1.0, serde_json::Value::Null)
            .unwrap();

        let results = topo_mirror_search(&db, &[id1, id2], 10);
        let ids: Vec<i64> = results.iter().map(|(id, _)| *id).collect();
        assert!(ids.contains(&id3), "c should be a mirror point");
    }

    #[test]
    fn test_empty_seeds() {
        let db = GraphDb::open_in_memory().unwrap();
        let results = topo_mirror_search(&db, &[], 10);
        assert!(results.is_empty());
    }

    #[test]
    fn test_single_seed_no_mirror() {
        let db = setup_mirror_db();
        let results = topo_mirror_search(&db, &[1], 10);
        assert!(results.is_empty(), "single seed should produce no mirrors");
    }

    #[test]
    fn test_nl_boost_filters_fts_ids() {
        let db = setup_mirror_db();
        let results = topo_nl_boost(&db, &[1, 4], 10);

        for &(id, _) in &results {
            assert_ne!(id, 1, "should not return FTS seed");
            assert_ne!(id, 4, "should not return FTS seed");
        }
    }
}
