use std::collections::{HashMap, HashSet};

use crate::db::GraphDb;
use crate::edge::EdgeKind;
use crate::symbol::Symbol;

#[derive(Debug, Clone)]
pub struct Hyperedge {
    pub center_id: i64,
    pub member_ids: Vec<i64>,
    pub edge_kinds: Vec<(i64, EdgeKind)>,
}

#[derive(Debug, Clone)]
pub struct HypergraphIndex {
    pub hyperedges: HashMap<i64, Hyperedge>,
    pub centrality: HashMap<i64, f64>,
    pub adjacency: HashMap<i64, Vec<(i64, f64)>>,
    pub symbol_count: usize,
}

const MAX_HYPEREDGE_SIZE: usize = 30;
const CENTRALITY_ITERATIONS: usize = 15;
const CENTRALITY_DAMPING: f64 = 0.85;
const RANDOM_WALK_STEPS: usize = 3;
const RANDOM_WALK_TOP_K: usize = 50;

pub fn build_hypergraph_index(db: &GraphDb) -> HypergraphIndex {
    let conn = db.conn();

    let symbol_ids: Vec<i64> = {
        let mut stmt = conn
            .prepare("SELECT id FROM symbols ORDER BY id")
            .unwrap_or_else(|_| conn.prepare("SELECT id FROM symbols").unwrap());
        stmt.query_map([], |row| row.get(0))
            .ok()
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    };

    let symbol_count = symbol_ids.len();
    if symbol_count == 0 {
        return HypergraphIndex {
            hyperedges: HashMap::new(),
            centrality: HashMap::new(),
            adjacency: HashMap::new(),
            symbol_count: 0,
        };
    }

    let edges_out: HashMap<i64, Vec<(i64, EdgeKind)>> = load_edges_grouped(
        conn,
        "SELECT source_id, target_id, kind FROM edges ORDER BY source_id",
        true,
    );

    let edges_in: HashMap<i64, Vec<(i64, EdgeKind)>> = load_edges_grouped(
        conn,
        "SELECT source_id, target_id, kind FROM edges ORDER BY target_id",
        false,
    );

    let mut hyperedges: HashMap<i64, Hyperedge> = HashMap::with_capacity(symbol_ids.len());
    let mut adjacency: HashMap<i64, Vec<(i64, f64)>> = HashMap::with_capacity(symbol_ids.len());

    for &sid in &symbol_ids {
        let mut members: HashSet<i64> = HashSet::new();
        let mut edge_kinds: Vec<(i64, EdgeKind)> = Vec::new();

        if let Some(out) = edges_out.get(&sid) {
            for &(target_id, kind) in out {
                if target_id != sid && members.len() < MAX_HYPEREDGE_SIZE {
                    members.insert(target_id);
                    edge_kinds.push((target_id, kind));
                }
            }
        }

        if let Some(inp) = edges_in.get(&sid) {
            for &(source_id, kind) in inp {
                if source_id != sid && members.len() < MAX_HYPEREDGE_SIZE {
                    members.insert(source_id);
                    edge_kinds.push((source_id, kind));
                }
            }
        }

        let member_ids: Vec<i64> = members.into_iter().collect();

        let mut adj: Vec<(i64, f64)> = Vec::with_capacity(member_ids.len());
        for &(neighbor_id, kind) in &edge_kinds {
            let weight = hyperedge_weight(kind);
            adj.push((neighbor_id, weight));
        }

        hyperedges.insert(
            sid,
            Hyperedge {
                center_id: sid,
                member_ids: member_ids.clone(),
                edge_kinds,
            },
        );

        adjacency.insert(sid, adj);
    }

    let centrality = compute_centrality(&adjacency, symbol_count);

    HypergraphIndex {
        hyperedges,
        centrality,
        adjacency,
        symbol_count,
    }
}

fn load_edges_grouped(
    conn: &rusqlite::Connection,
    sql: &str,
    group_by_first: bool,
) -> HashMap<i64, Vec<(i64, EdgeKind)>> {
    let mut stmt = match conn.prepare(sql) {
        Ok(s) => s,
        Err(_) => return HashMap::new(),
    };
    let rows = stmt.query_map([], |row| {
        let source_id: i64 = row.get(0)?;
        let target_id: i64 = row.get(1)?;
        let kind_str: String = row.get(2)?;
        Ok((source_id, target_id, kind_str))
    });
    let mut map: HashMap<i64, Vec<(i64, EdgeKind)>> = HashMap::new();
    if let Some(rows) = rows.ok() {
        for row in rows.flatten() {
            let (source_id, target_id, kind_str) = row;
            let kind = EdgeKind::from_str(&kind_str).unwrap_or(EdgeKind::References);
            let key = if group_by_first { source_id } else { target_id };
            let neighbor = if group_by_first { target_id } else { source_id };
            map.entry(key).or_default().push((neighbor, kind));
        }
    }
    map
}

fn hyperedge_weight(kind: EdgeKind) -> f64 {
    match kind {
        EdgeKind::Calls => 1.0,
        EdgeKind::Contains => 0.7,
        EdgeKind::Implements => 0.9,
        EdgeKind::Extends => 0.9,
        EdgeKind::Overrides => 0.85,
        EdgeKind::Imports => 0.5,
        EdgeKind::References => 0.3,
        EdgeKind::Tests => 0.2,
        EdgeKind::ReExports => 0.4,
    }
}

fn compute_centrality(adjacency: &HashMap<i64, Vec<(i64, f64)>>, n: usize) -> HashMap<i64, f64> {
    let mut scores: HashMap<i64, f64> = HashMap::with_capacity(n);
    let init_score = 1.0 / n.max(1) as f64;
    for &id in adjacency.keys() {
        scores.insert(id, init_score);
    }

    let mut out_weights: HashMap<i64, f64> = HashMap::with_capacity(n);
    for (&id, neighbors) in adjacency {
        let total: f64 = neighbors.iter().map(|(_, w)| *w).sum();
        out_weights.insert(id, total);
    }

    for _ in 0..CENTRALITY_ITERATIONS {
        let mut incoming: HashMap<i64, f64> = HashMap::with_capacity(n);
        for (&id, neighbors) in adjacency {
            let total_out = out_weights.get(&id).copied().unwrap_or(1.0).max(1e-10);
            let current_score = scores.get(&id).copied().unwrap_or(0.0);
            for &(neighbor_id, weight) in neighbors {
                let contribution = current_score * (weight / total_out);
                *incoming.entry(neighbor_id).or_insert(0.0) += contribution;
            }
        }

        let teleport = (1.0 - CENTRALITY_DAMPING) / n.max(1) as f64;
        for (&id, score) in scores.iter_mut() {
            let walk_score = incoming.get(&id).copied().unwrap_or(0.0);
            *score = CENTRALITY_DAMPING * walk_score + teleport;
        }
    }

    let max_score = scores.values().cloned().fold(0.0f64, f64::max).max(1e-10);
    for score in scores.values_mut() {
        *score /= max_score;
    }

    scores
}

pub fn hypergraph_walk(index: &HypergraphIndex, seed_ids: &[i64], top_k: usize) -> Vec<(i64, f64)> {
    if seed_ids.is_empty() || index.symbol_count == 0 {
        return Vec::new();
    }

    let seed_set: HashSet<i64> = seed_ids.iter().copied().collect();
    let mut visit_count: HashMap<i64, f64> = HashMap::new();

    for &seed_id in seed_ids {
        *visit_count.entry(seed_id).or_insert(0.0) += 1.0;

        let mut current = seed_id;
        for step in 0..RANDOM_WALK_STEPS {
            let neighbors = match index.adjacency.get(&current) {
                Some(n) => n,
                None => break,
            };

            let total_weight: f64 = neighbors.iter().map(|(_, w)| *w).sum();
            if total_weight < 1e-10 {
                break;
            }

            let weighted: Vec<(i64, f64)> = neighbors
                .iter()
                .map(|&(id, w)| (id, w / total_weight))
                .collect();

            let mut rand_val = deterministic_hash(current, step, seed_id);
            let mut chosen = current;
            for &(id, prob) in &weighted {
                rand_val -= prob;
                if rand_val <= 0.0 {
                    chosen = id;
                    break;
                }
            }

            let step_decay = 0.7_f64.powi(step as i32 + 1);
            let centrality_boost = index.centrality.get(&chosen).copied().unwrap_or(0.0);
            let visit_score = step_decay * (0.7 + 0.3 * centrality_boost);

            *visit_count.entry(chosen).or_insert(0.0) += visit_score;
            current = chosen;
        }
    }

    let mut scored: Vec<(i64, f64)> = visit_count
        .into_iter()
        .filter(|(id, _)| !seed_set.contains(id))
        .map(|(id, count)| {
            let centrality = index.centrality.get(&id).copied().unwrap_or(0.0);
            let combined = count * (0.6 + 0.4 * centrality);
            (id, combined)
        })
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored.truncate(top_k);
    scored
}

fn deterministic_hash(symbol_id: i64, step: usize, seed_id: i64) -> f64 {
    let c1: u64 = 0x9e3779b97f4a7c15;
    let c2: u64 = 0x517cc1b727220a95;
    let c3: u64 = 0x6c62272e07bb0142;
    let mut h = (symbol_id as u64).wrapping_mul(c1);
    h ^= (step as u64).wrapping_mul(c2);
    h ^= (seed_id as u64).wrapping_mul(c3);
    h = h.wrapping_mul(c1);
    ((h >> 33) as f64) / (1u64 << 31) as f64
}

pub fn hypergraph_rerank_scores(index: &HypergraphIndex, candidate_ids: &[i64]) -> Vec<(i64, f64)> {
    if index.symbol_count == 0 {
        return Vec::new();
    }

    candidate_ids
        .iter()
        .filter_map(|&id| {
            let cent = index.centrality.get(&id).copied().unwrap_or(0.0);
            let degree = index.adjacency.get(&id).map(|n| n.len()).unwrap_or(0);
            let degree_norm = (degree as f64 / 20.0).min(1.0);
            let score = 0.6 * cent + 0.4 * degree_norm;
            Some((id, score))
        })
        .collect()
}

pub fn hypergraph_nl_boost(
    index: &HypergraphIndex,
    fts_ids: &[i64],
    top_k: usize,
) -> Vec<(i64, f64)> {
    if fts_ids.is_empty() || index.symbol_count == 0 {
        return Vec::new();
    }

    let seed_ids: Vec<i64> = fts_ids.iter().take(10).copied().collect();
    let walk_results = hypergraph_walk(index, &seed_ids, RANDOM_WALK_TOP_K);

    let mut co_occurrence: HashMap<i64, f64> = HashMap::new();
    for &seed_id in &seed_ids {
        if let Some(he) = index.hyperedges.get(&seed_id) {
            for &member_id in &he.member_ids {
                if !fts_ids.contains(&member_id) {
                    let weight: f64 = he
                        .edge_kinds
                        .iter()
                        .filter(|(id, _)| *id == member_id)
                        .map(|(_, k)| hyperedge_weight(*k))
                        .sum();
                    *co_occurrence.entry(member_id).or_insert(0.0) += weight;
                }
            }
        }
    }

    let max_co = co_occurrence
        .values()
        .cloned()
        .fold(0.0f64, f64::max)
        .max(1e-10);

    let mut merged: HashMap<i64, f64> = HashMap::new();
    for (id, score) in walk_results {
        *merged.entry(id).or_insert(0.0) += score;
    }
    for (id, count) in co_occurrence {
        let norm_count = count / max_co;
        *merged.entry(id).or_insert(0.0) += norm_count * 0.5;
    }

    let mut results: Vec<(i64, f64)> = merged.into_iter().collect();
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

    fn setup_graph_db() -> GraphDb {
        let db = GraphDb::open_in_memory().unwrap();
        let fid = db
            .upsert_file("src/lib.rs", "rust", "abc", 1000, 100)
            .unwrap();

        let symbols = vec![
            ("main", SymbolKind::Function, 1, 10),
            ("authenticate", SymbolKind::Function, 12, 25),
            ("verify_token", SymbolKind::Function, 27, 40),
            ("handle_error", SymbolKind::Function, 42, 55),
            ("User", SymbolKind::Struct, 57, 80),
            ("AuthError", SymbolKind::Enum, 82, 90),
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

        db.insert_edge(
            ids[0],
            ids[1],
            EdgeKind::Calls,
            1.0,
            serde_json::Value::Null,
        )
        .unwrap();
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
            ids[4],
            EdgeKind::References,
            0.5,
            serde_json::Value::Null,
        )
        .unwrap();
        db.insert_edge(
            ids[3],
            ids[5],
            EdgeKind::References,
            0.5,
            serde_json::Value::Null,
        )
        .unwrap();
        db.insert_edge(
            ids[4],
            ids[1],
            EdgeKind::Contains,
            0.8,
            serde_json::Value::Null,
        )
        .unwrap();

        db
    }

    #[test]
    fn test_build_hypergraph_index() {
        let db = setup_graph_db();
        let index = build_hypergraph_index(&db);

        assert_eq!(index.symbol_count, 6);
        assert_eq!(index.hyperedges.len(), 6);
        assert!(!index.centrality.is_empty());
    }

    #[test]
    fn test_hypergraph_walk() {
        let db = setup_graph_db();
        let index = build_hypergraph_index(&db);

        let results = hypergraph_walk(&index, &[1], 10);
        assert!(!results.is_empty());

        let ids: Vec<i64> = results.iter().map(|(id, _)| *id).collect();
        assert!(ids.contains(&2) || ids.contains(&3));
    }

    #[test]
    fn test_centrality_top_symbols() {
        let db = setup_graph_db();
        let index = build_hypergraph_index(&db);

        let mut sorted: Vec<(i64, f64)> = index.centrality.into_iter().collect();
        sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        assert!(!sorted.is_empty());
        assert!(sorted[0].1 > 0.0);
    }

    #[test]
    fn test_nl_boost_discovers_neighbors() {
        let db = setup_graph_db();
        let index = build_hypergraph_index(&db);

        let results = hypergraph_nl_boost(&index, &[1], 10);
        assert!(!results.is_empty());

        let ids: Vec<i64> = results.iter().map(|(id, _)| *id).collect();
        assert!(
            ids.iter().any(|id| *id != 1),
            "NL boost should discover symbols beyond the seed"
        );
    }
}
