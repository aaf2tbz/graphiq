use std::collections::{HashMap, HashSet, VecDeque};

use rusqlite::params;

use crate::db::GraphDb;
use crate::edge::{EdgeKind, EvidenceKind, EvidenceProfile};

pub struct EdgeEvidenceIndex {
    pub edge_evidence: HashMap<(i64, i64, String), EvidenceProfile>,
    pub symbol_files: HashMap<i64, i64>,
    pub symbol_visibilities: HashMap<i64, String>,
    pub symbol_motifs: HashMap<i64, Vec<String>>,
}

pub fn infer_edge_evidence(db: &GraphDb) -> Result<EdgeEvidenceIndex, String> {
    let conn = db.conn();

    let mut symbol_files: HashMap<i64, i64> = HashMap::new();
    let mut symbol_visibilities: HashMap<i64, String> = HashMap::new();
    let mut symbol_names: HashMap<i64, String> = HashMap::new();

    {
        let mut stmt = conn
            .prepare("SELECT id, file_id, visibility, name FROM symbols")
            .map_err(|e| e.to_string())?;
        let rows: Vec<(i64, i64, String, String)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                ))
            })
            .map_err(|e| e.to_string())?
            .flatten()
            .collect();

        for (id, file_id, vis, name) in &rows {
            symbol_files.insert(*id, *file_id);
            symbol_visibilities.insert(*id, vis.clone());
            symbol_names.insert(*id, name.clone());
        }
    }

    let symbol_motifs = detect_motif_members(db);

    let mut outgoing: HashMap<i64, Vec<i64>> = HashMap::new();
    let mut incoming: HashMap<i64, Vec<i64>> = HashMap::new();
    let mut edges: Vec<(i64, i64, String, i64)> = Vec::new();

    {
        let mut stmt = conn
            .prepare("SELECT id, source_id, target_id, kind FROM edges")
            .map_err(|e| e.to_string())?;
        let rows: Vec<(i64, i64, i64, String)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                ))
            })
            .map_err(|e| e.to_string())?
            .flatten()
            .collect();

        for (edge_id, src, tgt, kind) in &rows {
            outgoing.entry(*src).or_default().push(*tgt);
            incoming.entry(*tgt).or_default().push(*src);
            edges.push((*src, *tgt, kind.clone(), *edge_id));
        }
    }

    let multi_path = compute_multiplicity(&outgoing, &incoming);

    let mut edge_evidence: HashMap<(i64, i64, String), EvidenceProfile> = HashMap::new();

    for (src, tgt, kind_str, _edge_id) in &edges {
        let cross_module = match (symbol_files.get(src), symbol_files.get(tgt)) {
            (Some(f1), Some(f2)) => f1 != f2,
            _ => false,
        };

        let cross_visibility = match (
            symbol_visibilities.get(src).map(|s| s.as_str()),
            symbol_visibilities.get(tgt).map(|s| s.as_str()),
        ) {
            (Some("public"), Some("private"))
            | (Some("public"), Some("protected"))
            | (Some("private"), Some("public")) => true,
            _ => false,
        };

        let multiplicity = multi_path
            .get(&(*src, *tgt))
            .copied()
            .unwrap_or(1);

        let src_motifs = symbol_motifs.get(src).map(|v| v.as_slice()).unwrap_or(&[]);
        let tgt_motifs = symbol_motifs.get(tgt).map(|v| v.as_slice()).unwrap_or(&[]);
        let has_motif = !src_motifs.is_empty() || !tgt_motifs.is_empty();

        let kind = EdgeKind::from_str(kind_str);

        let motif_name = if has_motif {
            let all_motifs: Vec<&str> = src_motifs
                .iter()
                .chain(tgt_motifs.iter())
                .map(|s| s.as_str())
                .collect();
            Some(all_motifs.join(","))
        } else {
            None
        };

        let evidence_kind = classify_evidence(
            &kind,
            cross_module,
            cross_visibility,
            multiplicity,
            has_motif,
        );

        edge_evidence.insert(
            (*src, *tgt, kind_str.clone()),
            EvidenceProfile {
                kind: evidence_kind,
                multiplicity,
                cross_module,
                cross_visibility,
                motif_name,
            },
        );
    }

    Ok(EdgeEvidenceIndex {
        edge_evidence,
        symbol_files,
        symbol_visibilities,
        symbol_motifs,
    })
}

fn classify_evidence(
    kind: &Option<EdgeKind>,
    cross_module: bool,
    cross_visibility: bool,
    multiplicity: u32,
    has_motif: bool,
) -> EvidenceKind {
    let edge_kind = match kind {
        Some(k) => k,
        None => return EvidenceKind::Incidental,
    };

    match edge_kind {
        EdgeKind::Implements | EdgeKind::Extends => {
            if cross_visibility || cross_module {
                EvidenceKind::Boundary
            } else {
                EvidenceKind::Direct
            }
        }
        EdgeKind::Calls | EdgeKind::Contains | EdgeKind::Imports => {
            if multiplicity >= 3 {
                EvidenceKind::Reinforcing
            } else if has_motif {
                EvidenceKind::Structural
            } else if cross_module || cross_visibility {
                EvidenceKind::Boundary
            } else {
                EvidenceKind::Direct
            }
        }
        EdgeKind::Tests => EvidenceKind::Direct,
        EdgeKind::Overrides => {
            if cross_visibility {
                EvidenceKind::Boundary
            } else {
                EvidenceKind::Direct
            }
        }
        EdgeKind::References => {
            if multiplicity >= 3 {
                EvidenceKind::Reinforcing
            } else if cross_module {
                EvidenceKind::Structural
            } else {
                EvidenceKind::Incidental
            }
        }
        EdgeKind::ReExports => {
            if cross_module {
                EvidenceKind::Boundary
            } else {
                EvidenceKind::Direct
            }
        }
        EdgeKind::SharesConstant => {
            if multiplicity >= 3 {
                EvidenceKind::Reinforcing
            } else if cross_module {
                EvidenceKind::Structural
            } else {
                EvidenceKind::Incidental
            }
        }
        EdgeKind::ReferencesConstant => {
            if cross_module {
                EvidenceKind::Structural
            } else {
                EvidenceKind::Direct
            }
        }
    }
}

fn compute_multiplicity(
    outgoing: &HashMap<i64, Vec<i64>>,
    incoming: &HashMap<i64, Vec<i64>>,
) -> HashMap<(i64, i64), u32> {
    let mut multiplicity: HashMap<(i64, i64), u32> = HashMap::new();

    for (&src, targets) in outgoing {
        for &tgt in targets {
            *multiplicity.entry((src, tgt)).or_insert(0) += 1;
        }
    }

    for (&tgt, sources) in incoming {
        for &src in sources {
            *multiplicity.entry((src, tgt)).or_insert(0) += 1;
        }
    }

    let all_nodes: HashSet<i64> = outgoing
        .keys()
        .chain(outgoing.values().flat_map(|v| v.iter()))
        .copied()
        .collect();

    let combined: HashMap<i64, Vec<i64>> = all_nodes
        .iter()
        .map(|&node| {
            let mut neighbors: Vec<i64> = Vec::new();
            if let Some(out) = outgoing.get(&node) {
                neighbors.extend(out.iter().copied());
            }
            if let Some(inc) = incoming.get(&node) {
                neighbors.extend(inc.iter().copied());
            }
            neighbors.sort_unstable();
            neighbors.dedup();
            (node, neighbors)
        })
        .collect();

    let max_depth = 3;

    let keys: Vec<i64> = outgoing
        .keys()
        .copied()
        .chain(incoming.keys().copied())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    let sample_size = 500.min(keys.len());
    let sampled_keys: Vec<i64> = if keys.len() <= sample_size {
        keys
    } else {
        let step = keys.len() / sample_size;
        keys.into_iter().step_by(step).collect()
    };

    for &src in &sampled_keys {
        let bfs_paths = bfs_path_count(src, &combined, max_depth);
        for ((s, t), count) in bfs_paths {
            if count >= 2 {
                let entry = multiplicity.entry((s, t)).or_insert(0);
                *entry = (*entry).max(count);
            }
        }
    }

    multiplicity
}

fn bfs_path_count(
    start: i64,
    adj: &HashMap<i64, Vec<i64>>,
    max_depth: usize,
) -> HashMap<(i64, i64), u32> {
    let mut path_count: HashMap<(i64, i64), u32> = HashMap::new();
    let mut visited: HashSet<i64> = HashSet::new();
    let mut queue: VecDeque<(i64, usize)> = VecDeque::new();

    visited.insert(start);
    queue.push_back((start, 0));

    while let Some((node, depth)) = queue.pop_front() {
        if depth >= max_depth {
            continue;
        }
        if let Some(neighbors) = adj.get(&node) {
            for &neighbor in neighbors {
                if neighbor == start {
                    continue;
                }
                *path_count.entry((start, neighbor)).or_insert(0) += 1;
                if visited.insert(neighbor) {
                    queue.push_back((neighbor, depth + 1));
                }
            }
        }
    }

    path_count
}

fn detect_motif_members(db: &GraphDb) -> HashMap<i64, Vec<String>> {
    let conn = db.conn();
    let mut result: HashMap<i64, Vec<String>> = HashMap::new();

    let mut out_stmt = conn
        .prepare(
            "SELECT source_id, kind FROM edges WHERE kind IN ('calls', 'contains', 'implements')",
        )
        .unwrap();
    let out_rows: Vec<(i64, String)> = out_stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .ok()
        .map(|r| r.flatten().collect())
        .unwrap_or_default();

    let mut call_out_count: HashMap<i64, usize> = HashMap::new();
    let mut contains_count: HashMap<i64, usize> = HashMap::new();
    let mut implements_count: HashMap<i64, usize> = HashMap::new();

    for (src, kind) in &out_rows {
        match kind.as_str() {
            "calls" => *call_out_count.entry(*src).or_insert(0) += 1,
            "contains" => *contains_count.entry(*src).or_insert(0) += 1,
            "implements" => *implements_count.entry(*src).or_insert(0) += 1,
            _ => {}
        }
    }

    let mut in_stmt = conn
        .prepare("SELECT target_id FROM edges WHERE kind = 'calls'")
        .unwrap();
    let in_rows: Vec<i64> = in_stmt
        .query_map([], |row| row.get(0))
        .ok()
        .map(|r| r.flatten().collect())
        .unwrap_or_default();

    let mut call_in_count: HashMap<i64, usize> = HashMap::new();
    for tgt in &in_rows {
        *call_in_count.entry(*tgt).or_insert(0) += 1;
    }

    let all_ids: HashSet<i64> = call_out_count
        .keys()
        .chain(call_in_count.keys())
        .chain(contains_count.keys())
        .chain(implements_count.keys())
        .copied()
        .collect();

    for &id in &all_ids {
        let co = call_out_count.get(&id).copied().unwrap_or(0);
        let ci = call_in_count.get(&id).copied().unwrap_or(0);
        let ct = contains_count.get(&id).copied().unwrap_or(0);
        let im = implements_count.get(&id).copied().unwrap_or(0);

        let mut motifs = Vec::new();

        if co >= 5 && ci <= 2 {
            motifs.push("orchestrator".to_string());
        }
        if ct >= 3 {
            motifs.push("hub".to_string());
        }
        if ci >= 5 && co <= 2 {
            motifs.push("sink".to_string());
        }
        if co >= 1 && ci >= 1 && co <= 3 && ci <= 3 {
            motifs.push("connector".to_string());
        }
        if im >= 1 && co <= 1 {
            motifs.push("leaf".to_string());
        }

        if !motifs.is_empty() {
            result.insert(id, motifs);
        }
    }

    result
}

pub fn write_edge_evidence(
    db: &GraphDb,
    evidence: &EdgeEvidenceIndex,
) -> Result<usize, String> {
    let conn = db.conn();
    let mut updated = 0;

    for ((src, tgt, kind_str), profile) in &evidence.edge_evidence {
        let meta = serde_json::json!({
            "evidence": {
                "kind": profile.kind.as_str(),
                "multiplicity": profile.multiplicity,
                "cross_module": profile.cross_module,
                "cross_visibility": profile.cross_visibility,
                "motif": profile.motif_name,
            }
        });

        let result = conn.execute(
            "UPDATE edges SET metadata = ?1 WHERE source_id = ?2 AND target_id = ?3 AND kind = ?4",
            params![meta.to_string(), src, tgt, kind_str],
        );

        if let Ok(n) = result {
            updated += n;
        }
    }

    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edge::EdgeKind;
    use crate::symbol::{SymbolBuilder, SymbolKind, Visibility};

    fn setup_db() -> GraphDb {
        let db = GraphDb::open_in_memory().unwrap();
        let f1 = db.upsert_file("src/auth.ts", "typescript", "a", 1000, 50).unwrap();
        let f2 = db.upsert_file("src/utils.ts", "typescript", "b", 1000, 30).unwrap();

        let s1 = SymbolBuilder::new(f1, "main".into(), SymbolKind::Function, "fn main()".into(), "typescript".into())
            .lines(1, 5)
            .visibility(Visibility::Public)
            .build();
        let s2 = SymbolBuilder::new(f1, "authenticate".into(), SymbolKind::Function, "fn auth()".into(), "typescript".into())
            .lines(7, 15)
            .visibility(Visibility::Public)
            .build();
        let s3 = SymbolBuilder::new(f2, "hashPassword".into(), SymbolKind::Function, "fn hash()".into(), "typescript".into())
            .lines(1, 5)
            .visibility(Visibility::Private)
            .build();
        let s4 = SymbolBuilder::new(f1, "validate".into(), SymbolKind::Function, "fn val()".into(), "typescript".into())
            .lines(17, 22)
            .visibility(Visibility::Private)
            .build();

        let id1 = db.insert_symbol(&s1).unwrap();
        let id2 = db.insert_symbol(&s2).unwrap();
        let id3 = db.insert_symbol(&s3).unwrap();
        let id4 = db.insert_symbol(&s4).unwrap();

        db.insert_edge(id1, id2, EdgeKind::Calls, 1.0, serde_json::Value::Null).unwrap();
        db.insert_edge(id2, id3, EdgeKind::Calls, 1.0, serde_json::Value::Null).unwrap();
        db.insert_edge(id2, id4, EdgeKind::Calls, 1.0, serde_json::Value::Null).unwrap();
        db.insert_edge(id4, id3, EdgeKind::References, 0.4, serde_json::Value::Null).unwrap();

        db
    }

    #[test]
    fn test_infer_evidence_basic() {
        let db = setup_db();
        let evidence = infer_edge_evidence(&db).unwrap();

        assert!(!evidence.edge_evidence.is_empty(), "should have evidence for edges");
    }

    #[test]
    fn test_cross_module_detection() {
        let db = setup_db();
        let evidence = infer_edge_evidence(&db).unwrap();

        let auth_to_hash = evidence.edge_evidence.get(&(2, 3, "calls".to_string()));
        assert!(auth_to_hash.is_some(), "should have evidence for authenticate->hashPassword");
        let profile = auth_to_hash.unwrap();
        assert!(profile.cross_module, "authenticate->hashPassword should be cross-module");
    }

    #[test]
    fn test_cross_visibility_detection() {
        let db = setup_db();
        let evidence = infer_edge_evidence(&db).unwrap();

        let main_to_auth = evidence.edge_evidence.get(&(1, 2, "calls".to_string()));
        assert!(main_to_auth.is_some());
        let profile = main_to_auth.unwrap();
        assert!(
            !profile.cross_visibility,
            "public->public should not be cross-visibility"
        );

        let val_to_hash = evidence.edge_evidence.get(&(4, 3, "references".to_string()));
        assert!(val_to_hash.is_some());
    }

    #[test]
    fn test_write_evidence_updates_db() {
        let db = setup_db();
        let evidence = infer_edge_evidence(&db).unwrap();
        let updated = write_edge_evidence(&db, &evidence).unwrap();

        assert!(updated > 0, "should update at least one edge");

        let edges = db.edges_from(1).unwrap();
        let call_edge = edges.iter().find(|e| e.kind == EdgeKind::Calls).unwrap();
        assert!(call_edge.metadata.is_object(), "metadata should be a JSON object");
        let ev = &call_edge.metadata["evidence"];
        assert!(ev["kind"].is_string(), "evidence should have kind");
    }

    #[test]
    fn test_classify_implements_as_boundary() {
        let db = GraphDb::open_in_memory().unwrap();
        let fid = db.upsert_file("src/main.ts", "typescript", "a", 1000, 10).unwrap();
        let s1 = SymbolBuilder::new(fid, "trait".into(), SymbolKind::Interface, "interface".into(), "typescript".into())
            .visibility(Visibility::Public)
            .build();
        let s2 = SymbolBuilder::new(fid, "impl".into(), SymbolKind::Class, "class".into(), "typescript".into())
            .visibility(Visibility::Private)
            .build();
        let id1 = db.insert_symbol(&s1).unwrap();
        let id2 = db.insert_symbol(&s2).unwrap();
        db.insert_edge(id2, id1, EdgeKind::Implements, 0.8, serde_json::Value::Null).unwrap();

        let evidence = infer_edge_evidence(&db).unwrap();
        let profile = evidence.edge_evidence.get(&(id2, id1, "implements".to_string())).unwrap();
        assert_eq!(profile.kind, EvidenceKind::Boundary, "implements with cross-visibility should be boundary");
    }

    #[test]
    fn test_incidental_for_references() {
        let db = GraphDb::open_in_memory().unwrap();
        let fid = db.upsert_file("src/main.ts", "typescript", "a", 1000, 10).unwrap();
        let s1 = SymbolBuilder::new(fid, "a".into(), SymbolKind::Function, "fn".into(), "typescript".into())
            .visibility(Visibility::Public)
            .build();
        let s2 = SymbolBuilder::new(fid, "b".into(), SymbolKind::Function, "fn".into(), "typescript".into())
            .visibility(Visibility::Public)
            .build();
        let id1 = db.insert_symbol(&s1).unwrap();
        let id2 = db.insert_symbol(&s2).unwrap();
        db.insert_edge(id1, id2, EdgeKind::References, 0.4, serde_json::Value::Null).unwrap();

        let evidence = infer_edge_evidence(&db).unwrap();
        let profile = evidence.edge_evidence.get(&(id1, id2, "references".to_string())).unwrap();
        assert_eq!(profile.kind, EvidenceKind::Incidental, "simple reference should be incidental");
    }
}
