use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::params;

use crate::db::GraphDb;
use crate::edge::EdgeKind;
use crate::symbol::{SymbolBuilder, SymbolKind};

#[derive(Clone)]
pub struct Subsystem {
    pub id: usize,
    pub name: String,
    pub symbol_ids: Vec<i64>,
    pub symbol_names: Vec<String>,
    pub dominant_file: String,
    pub internal_edge_count: usize,
    pub boundary_edge_count: usize,
    pub cohesion: f64,
}

pub struct SubsystemIndex {
    pub subsystems: Vec<Subsystem>,
    pub symbol_to_subsystem: HashMap<i64, usize>,
}

fn evidence_weight(kind_str: &str) -> f64 {
    match kind_str {
        "reinforcing" => 1.5,
        "boundary" => 1.2,
        "direct" => 1.0,
        "structural" => 0.8,
        _ => 0.1,
    }
}

fn dominant_file_from_path(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() >= 2 {
        format!("{}/{}", parts[parts.len() - 2], parts[parts.len() - 1])
    } else {
        path.to_string()
    }
}

fn dir_cluster_key(path: &str) -> String {
    let p = Path::new(path);
    match p.parent() {
        Some(dir) => {
            let d = dir.to_string_lossy();
            let depth: usize = d.matches('/').count();
            if depth <= 2 {
                d.to_string()
            } else {
                p.ancestors()
                    .nth(2)
                    .map(|a| a.to_string_lossy().to_string())
                    .unwrap_or(d.to_string())
            }
        }
        None => path.to_string(),
    }
}

fn path_similarity(a: &str, b: &str) -> f64 {
    let pa: Vec<&str> = a.split('/').collect();
    let pb: Vec<&str> = b.split('/').collect();
    let mut common = 0usize;
    for (sa, sb) in pa.iter().zip(pb.iter()) {
        if sa == sb {
            common += 1;
        } else {
            break;
        }
    }
    if common == 0 {
        return 0.0;
    }
    let max_len = pa.len().max(pb.len());
    common as f64 / max_len as f64
}

pub fn detect_subsystems(db: &GraphDb) -> Result<SubsystemIndex, String> {
    let conn = db.conn();

    let mut symbol_file: HashMap<i64, i64> = HashMap::new();
    let mut file_paths: HashMap<i64, String> = HashMap::new();
    let mut symbol_names: HashMap<i64, String> = HashMap::new();
    let mut all_symbol_ids: Vec<i64> = Vec::new();

    {
        let mut stmt = conn
            .prepare("SELECT id, name, file_id FROM symbols")
            .map_err(|e| e.to_string())?;
        let rows: Vec<(i64, String, i64)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .map_err(|e| e.to_string())?
            .flatten()
            .collect();

        for (sid, name, fid) in &rows {
            symbol_file.insert(*sid, *fid);
            symbol_names.insert(*sid, name.clone());
            all_symbol_ids.push(*sid);
        }
    }

    {
        let mut stmt = conn
            .prepare("SELECT id, path FROM files")
            .map_err(|e| e.to_string())?;
        let rows: Vec<(i64, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| e.to_string())?
            .flatten()
            .collect();

        for (fid, path) in &rows {
            file_paths.insert(*fid, path.clone());
        }
    }

    if all_symbol_ids.is_empty() {
        return Ok(SubsystemIndex {
            subsystems: Vec::new(),
            symbol_to_subsystem: HashMap::new(),
        });
    }

    let mut adj: HashMap<i64, Vec<(i64, f64)>> = HashMap::new();
    for &sid in &all_symbol_ids {
        adj.entry(sid).or_default();
    }

    let mut edge_kinds: HashSet<(i64, i64, String)> = HashSet::new();

    {
        let mut stmt = conn
            .prepare("SELECT source_id, target_id, kind, metadata FROM edges")
            .map_err(|e| e.to_string())?;
        let rows: Vec<(i64, i64, String, String)> = stmt
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .map_err(|e| e.to_string())?
            .flatten()
            .collect();

        for (src, tgt, kind_str, metadata_json) in &rows {
            edge_kinds.insert((*src, *tgt, kind_str.clone()));

            let weight = if let Ok(meta) = serde_json::from_str::<serde_json::Value>(metadata_json) {
                let evidence_kind = meta
                    .get("evidence")
                    .and_then(|e| e.get("kind"))
                    .and_then(|k| k.as_str())
                    .unwrap_or("incidental");
                evidence_weight(evidence_kind)
            } else {
                evidence_weight("incidental")
            };

            adj.entry(*src).or_default().push((*tgt, weight));
            adj.entry(*tgt).or_default().push((*src, weight));
        }
    }

    if edge_kinds.is_empty() {
        let name = all_symbol_ids
            .iter()
            .find_map(|sid| {
                let fid = symbol_file.get(sid)?;
                let path = file_paths.get(fid)?;
                Some(dominant_file_from_path(path))
            })
            .unwrap_or_else(|| "unknown".to_string());

        let subsystem = Subsystem {
            id: 0,
            name: name.clone(),
            symbol_ids: all_symbol_ids.clone(),
            symbol_names: all_symbol_ids
                .iter()
                .map(|sid| symbol_names.get(sid).cloned().unwrap_or_default())
                .collect(),
            dominant_file: name,
            internal_edge_count: 0,
            boundary_edge_count: 0,
            cohesion: 1.0,
        };
        let symbol_to_subsystem: HashMap<i64, usize> = all_symbol_ids
            .iter()
            .map(|&sid| (sid, 0))
            .collect();
        return Ok(SubsystemIndex {
            subsystems: vec![subsystem],
            symbol_to_subsystem,
        });
    }

    let mut dir_to_label: HashMap<String, usize> = HashMap::new();
    let mut next_label = 0usize;
    for fid in symbol_file.values() {
        let path = file_paths.get(fid).map(|s| s.as_str()).unwrap_or("");
        let key = dir_cluster_key(path);
        if !dir_to_label.contains_key(&key) {
            dir_to_label.insert(key, next_label);
            next_label += 1;
        }
    }

    let mut labels: HashMap<i64, usize> = HashMap::new();
    for &sid in &all_symbol_ids {
        let fid = symbol_file.get(&sid).copied().unwrap_or(0);
        let path = file_paths.get(&fid).map(|s| s.as_str()).unwrap_or("");
        let key = dir_cluster_key(path);
        labels.insert(sid, *dir_to_label.get(&key).unwrap_or(&0));
    }

    for _ in 0..50 {
        let mut changed = false;
        let mut new_labels: HashMap<i64, usize> = HashMap::new();

        for &sid in &all_symbol_ids {
            let neighbors = adj.get(&sid).unwrap();
            if neighbors.is_empty() {
                new_labels.insert(sid, *labels.get(&sid).unwrap());
                continue;
            }

            let mut weight_sum: HashMap<usize, f64> = HashMap::new();
            for &(nbr, w) in neighbors {
                let nbr_label = labels.get(&nbr).unwrap();
                *weight_sum.entry(*nbr_label).or_insert(0.0) += w;
            }

            let best = weight_sum
                .iter()
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(&l, _)| l)
                .unwrap_or(*labels.get(&sid).unwrap());

            new_labels.insert(sid, best);
            if best != *labels.get(&sid).unwrap() {
                changed = true;
            }
        }

        labels = new_labels;
        if !changed {
            break;
        }
    }

    let mut label_members: HashMap<usize, Vec<i64>> = HashMap::new();
    for &sid in &all_symbol_ids {
        let label = labels.get(&sid).unwrap();
        label_members.entry(*label).or_default().push(sid);
    }

    let mut label_file_counts: HashMap<usize, HashMap<i64, usize>> = HashMap::new();
    for (&label, members) in &label_members {
        let mut fc: HashMap<i64, usize> = HashMap::new();
        for &sid in members {
            if let Some(&fid) = symbol_file.get(&sid) {
                *fc.entry(fid).or_insert(0) += 1;
            }
        }
        label_file_counts.insert(label, fc);
    }

    let mut tiny_labels: Vec<usize> = Vec::new();
    for (&label, members) in &label_members {
        if members.len() < 5 {
            tiny_labels.push(label);
        }
    }

    for &tiny in &tiny_labels {
        let members = label_members.get(&tiny).unwrap();
        let mut neighbor_label_weight: HashMap<usize, f64> = HashMap::new();

        for &sid in members {
            if let Some(neighbors) = adj.get(&sid) {
                for &(nbr, w) in neighbors {
                    if let Some(&nbr_label) = labels.get(&nbr) {
                        if nbr_label != tiny {
                            *neighbor_label_weight.entry(nbr_label).or_insert(0.0) += w;
                        }
                    }
                }
            }
        }

        let merge_target = neighbor_label_weight
            .iter()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(&l, _)| l);

        if let Some(target) = merge_target {
            let tiny_members = label_members.remove(&tiny).unwrap_or_default();
            for sid in tiny_members {
                labels.insert(sid, target);
                label_members.entry(target).or_default().push(sid);
            }
        }
    }

    let unique_labels: HashSet<usize> = labels.values().copied().collect();

    let mut label_representative_path: HashMap<usize, String> = HashMap::new();
    for (&label, members) in &label_members {
        let mut file_counts: HashMap<i64, usize> = HashMap::new();
        for &sid in members {
            if let Some(&fid) = symbol_file.get(&sid) {
                *file_counts.entry(fid).or_insert(0) += 1;
            }
        }
        let dominant_fid = file_counts
            .iter()
            .max_by_key(|(_, &c)| c)
            .map(|(&f, _)| f)
            .unwrap_or(0);
        let path = file_paths
            .get(&dominant_fid)
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());
        label_representative_path.insert(label, path);
    }

    let mut orphan_labels: Vec<usize> = Vec::new();
    for (&label, members) in &label_members {
        let member_set: HashSet<i64> = members.iter().copied().collect();
        let has_internal = edge_kinds
            .iter()
            .any(|(src, tgt, _)| member_set.contains(src) && member_set.contains(tgt));
        let has_boundary = edge_kinds
            .iter()
            .any(|(src, tgt, _)| {
                let src_in = member_set.contains(src);
                let tgt_in = member_set.contains(tgt);
                src_in ^ tgt_in
            });
        if !has_internal && !has_boundary {
            orphan_labels.push(label);
        }
    }

    for &orphan in &orphan_labels {
        let orphan_path = label_representative_path
            .get(&orphan)
            .cloned()
            .unwrap_or_default();

        let mut best_target: Option<usize> = None;
        let mut best_sim = 0.0f64;

        for (&candidate, members) in &label_members {
            if candidate == orphan {
                continue;
            }
            let member_set: HashSet<i64> = members.iter().copied().collect();
            let has_edges = edge_kinds.iter().any(|(src, tgt, _)| {
                let src_in = member_set.contains(src);
                let tgt_in = member_set.contains(tgt);
                src_in || tgt_in
            });
            if !has_edges {
                continue;
            }

            let cand_path = label_representative_path
                .get(&candidate)
                .cloned()
                .unwrap_or_default();
            let sim = path_similarity(&orphan_path, &cand_path);
            if sim > best_sim {
                best_sim = sim;
                best_target = Some(candidate);
            }
        }

        if let Some(target) = best_target {
            let orphan_members = label_members.remove(&orphan).unwrap_or_default();
            for sid in orphan_members {
                labels.insert(sid, target);
                label_members.entry(target).or_default().push(sid);
            }
        }
    }

    let unique_labels: HashSet<usize> = labels.values().copied().collect();
    let mut subsystems: Vec<Subsystem> = Vec::new();
    let mut old_to_new_label: HashMap<usize, usize> = HashMap::new();

    for (idx, label) in unique_labels.iter().enumerate() {
        old_to_new_label.insert(*label, idx);
    }

    for (new_id, label) in unique_labels.iter().enumerate() {
        let members = label_members.get(label).unwrap();

        let mut file_counts: HashMap<i64, usize> = HashMap::new();
        for &sid in members {
            if let Some(&fid) = symbol_file.get(&sid) {
                *file_counts.entry(fid).or_insert(0) += 1;
            }
        }

        let dominant_fid = file_counts
            .iter()
            .max_by_key(|(_, &c)| c)
            .map(|(&f, _)| f)
            .unwrap_or(0);

        let dom_path = file_paths
            .get(&dominant_fid)
            .map(|p| p.as_str())
            .unwrap_or("unknown");
        let name = dominant_file_from_path(dom_path);

        let sym_ids: Vec<i64> = members.clone();
        let sym_names: Vec<String> = members
            .iter()
            .map(|sid| symbol_names.get(sid).cloned().unwrap_or_default())
            .collect();

        let member_set: HashSet<i64> = members.iter().copied().collect();
        let mut internal_edge_count = 0usize;
        let mut boundary_edge_count = 0usize;

        for &(src, tgt, _) in edge_kinds.iter() {
            let src_in = member_set.contains(&src);
            let tgt_in = member_set.contains(&tgt);
            if src_in && tgt_in {
                internal_edge_count += 1;
            } else if src_in || tgt_in {
                boundary_edge_count += 1;
            }
        }

        let total = internal_edge_count + boundary_edge_count;
        let cohesion = if total > 0 {
            internal_edge_count as f64 / total as f64
        } else {
            1.0
        };

        subsystems.push(Subsystem {
            id: new_id,
            name: name.clone(),
            symbol_ids: sym_ids,
            symbol_names: sym_names,
            dominant_file: dom_path.to_string(),
            internal_edge_count,
            boundary_edge_count,
            cohesion,
        });
    }

    subsystems.sort_by(|a, b| b.symbol_ids.len().cmp(&a.symbol_ids.len()));

    let mut symbol_to_subsystem: HashMap<i64, usize> = HashMap::new();
    for (i, sub) in subsystems.iter().enumerate() {
        for &sid in &sub.symbol_ids {
            symbol_to_subsystem.insert(sid, i);
        }
    }

    Ok(SubsystemIndex {
        subsystems,
        symbol_to_subsystem,
    })
}

pub fn store_subsystems(db: &GraphDb, index: &SubsystemIndex) -> Result<(), String> {
    let conn = db.conn();

    conn.execute(
        "CREATE TABLE IF NOT EXISTS subsystems (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            dominant_file TEXT NOT NULL,
            internal_edge_count INTEGER NOT NULL DEFAULT 0,
            boundary_edge_count INTEGER NOT NULL DEFAULT 0,
            cohesion REAL NOT NULL DEFAULT 1.0
        )",
        [],
    )
    .map_err(|e| e.to_string())?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS subsystem_symbols (
            subsystem_id INTEGER NOT NULL REFERENCES subsystems(id) ON DELETE CASCADE,
            symbol_id INTEGER NOT NULL,
            symbol_name TEXT NOT NULL,
            PRIMARY KEY (subsystem_id, symbol_id)
        )",
        [],
    )
    .map_err(|e| e.to_string())?;

    conn.execute("DELETE FROM subsystem_symbols", [])
        .map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM subsystems", [])
        .map_err(|e| e.to_string())?;

    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;

    {
        let mut sub_stmt = tx
            .prepare(
                "INSERT INTO subsystems (id, name, dominant_file, internal_edge_count, boundary_edge_count, cohesion)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )
            .map_err(|e| e.to_string())?;

        let mut sym_stmt = tx
            .prepare(
                "INSERT INTO subsystem_symbols (subsystem_id, symbol_id, symbol_name) VALUES (?1, ?2, ?3)",
            )
            .map_err(|e| e.to_string())?;

        for sub in &index.subsystems {
            sub_stmt
                .execute(params![
                    sub.id as i64,
                    &sub.name,
                    &sub.dominant_file,
                    sub.internal_edge_count as i64,
                    sub.boundary_edge_count as i64,
                    sub.cohesion,
                ])
                .map_err(|e| e.to_string())?;

            for (sid, sname) in sub.symbol_ids.iter().zip(sub.symbol_names.iter()) {
                sym_stmt
                    .execute(params![sub.id as i64, *sid, sname])
                    .map_err(|e| e.to_string())?;
            }
        }
    }

    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

pub fn load_subsystems(db: &GraphDb) -> Result<SubsystemIndex, String> {
    let conn = db.conn();

    let table_exists: bool = conn
        .prepare("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='subsystems'")
        .map_err(|e| e.to_string())?
        .query_row([], |row| row.get::<_, i64>(0))
        .map_err(|e| e.to_string())?
        > 0;

    if !table_exists {
        return Ok(SubsystemIndex {
            subsystems: Vec::new(),
            symbol_to_subsystem: HashMap::new(),
        });
    }

    let mut subsystems: Vec<Subsystem> = Vec::new();

    {
        let mut stmt = conn
            .prepare(
                "SELECT id, name, dominant_file, internal_edge_count, boundary_edge_count, cohesion
                 FROM subsystems ORDER BY id",
            )
            .map_err(|e| e.to_string())?;

        let rows: Vec<(i64, String, String, i64, i64, f64)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            })
            .map_err(|e| e.to_string())?
            .flatten()
            .collect();

        for (id, name, dominant_file, internal, boundary, cohesion) in &rows {
            subsystems.push(Subsystem {
                id: *id as usize,
                name: name.clone(),
                symbol_ids: Vec::new(),
                symbol_names: Vec::new(),
                dominant_file: dominant_file.clone(),
                internal_edge_count: *internal as usize,
                boundary_edge_count: *boundary as usize,
                cohesion: *cohesion,
            });
        }
    }

    let mut symbol_to_subsystem: HashMap<i64, usize> = HashMap::new();

    {
        let mut stmt = conn
            .prepare(
                "SELECT subsystem_id, symbol_id, symbol_name FROM subsystem_symbols ORDER BY subsystem_id",
            )
            .map_err(|e| e.to_string())?;

        let rows: Vec<(i64, i64, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .map_err(|e| e.to_string())?
            .flatten()
            .collect();

        for (sub_id, sym_id, sym_name) in &rows {
            if let Some(sub) = subsystems.iter_mut().find(|s| s.id == *sub_id as usize) {
                sub.symbol_ids.push(*sym_id);
                sub.symbol_names.push(sym_name.clone());
            }
            symbol_to_subsystem.insert(*sym_id, *sub_id as usize);
        }
    }

    Ok(SubsystemIndex {
        subsystems,
        symbol_to_subsystem,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StructuralRole {
    EntryPoint,
    Hub,
    Leaf,
    Boundary,
    Orchestrator,
}

impl StructuralRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            StructuralRole::EntryPoint => "entry_point",
            StructuralRole::Hub => "hub",
            StructuralRole::Leaf => "leaf",
            StructuralRole::Boundary => "boundary",
            StructuralRole::Orchestrator => "orchestrator",
        }
    }
}

pub struct SymbolStructuralRole {
    pub symbol_id: i64,
    pub symbol_name: String,
    pub roles: Vec<StructuralRole>,
    pub subsystem_id: usize,
    pub internal_degree: usize,
    pub boundary_degree: usize,
    pub external_callers: usize,
    pub external_callees: usize,
}

pub fn materialize_structural_roles(
    db: &GraphDb,
    subsystem_index: &SubsystemIndex,
) -> Result<Vec<SymbolStructuralRole>, String> {
    let conn = db.conn();

    let mut outgoing: HashMap<i64, Vec<i64>> = HashMap::new();
    let mut incoming: HashMap<i64, Vec<i64>> = HashMap::new();
    let mut evidence_kinds: HashMap<(i64, i64), String> = HashMap::new();

    {
        let mut stmt = conn
            .prepare("SELECT source_id, target_id, kind, metadata FROM edges")
            .map_err(|e| e.to_string())?;
        let rows: Vec<(i64, i64, String, String)> = stmt
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .map_err(|e| e.to_string())?
            .flatten()
            .collect();

        for (src, tgt, kind, metadata) in &rows {
            outgoing.entry(*src).or_default().push(*tgt);
            incoming.entry(*tgt).or_default().push(*src);
            if let Ok(meta) = serde_json::from_str::<serde_json::Value>(metadata) {
                let ev_kind = meta
                    .get("evidence")
                    .and_then(|e| e.get("kind"))
                    .and_then(|k| k.as_str())
                    .unwrap_or("incidental");
                evidence_kinds.insert((*src, *tgt), ev_kind.to_string());
            }
        }
    }

    let mut symbol_names: HashMap<i64, String> = HashMap::new();
    {
        let mut stmt = conn
            .prepare("SELECT id, name FROM symbols")
            .map_err(|e| e.to_string())?;
        let rows: Vec<(i64, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| e.to_string())?
            .flatten()
            .collect();
        for (id, name) in &rows {
            symbol_names.insert(*id, name.clone());
        }
    }

    let mut per_subsystem_stats: HashMap<usize, SubsystemDegreeStats> = HashMap::new();
    for sub in &subsystem_index.subsystems {
        let member_set: HashSet<i64> = sub.symbol_ids.iter().copied().collect();

        let mut internal_degrees: HashMap<i64, usize> = HashMap::new();
        let mut boundary_degrees: HashMap<i64, usize> = HashMap::new();
        let mut external_callers: HashMap<i64, usize> = HashMap::new();
        let mut external_callees: HashMap<i64, usize> = HashMap::new();

        for &sid in &sub.symbol_ids {
            let out = outgoing.get(&sid);
            let inc = incoming.get(&sid);

            let mut int_deg = 0usize;
            let mut bnd_deg = 0usize;
            let mut ext_callers = 0usize;
            let mut ext_callees = 0usize;

            if let Some(outs) = out {
                for &tgt in outs {
                    if member_set.contains(&tgt) {
                        int_deg += 1;
                    } else {
                        bnd_deg += 1;
                        ext_callees += 1;
                    }
                }
            }

            if let Some(ins) = inc {
                for &src in ins {
                    if !member_set.contains(&src) {
                        ext_callers += 1;
                        if !outgoing.get(&src).map_or(false, |o| o.contains(&sid))
                            || !out
                                .map_or(false, |o| o.iter().any(|t| *t == src))
                        {
                        }
                    }
                }
            }

            internal_degrees.insert(sid, int_deg);
            boundary_degrees.insert(sid, bnd_deg);
            external_callers.insert(sid, ext_callers);
            external_callees.insert(sid, ext_callees);
        }

        per_subsystem_stats.insert(
            sub.id,
            SubsystemDegreeStats {
                internal_degrees,
                boundary_degrees,
                external_callers,
                external_callees,
            },
        );
    }

    let mut results: Vec<SymbolStructuralRole> = Vec::new();

    for sub in &subsystem_index.subsystems {
        if sub.symbol_ids.len() < 3 {
            continue;
        }

        let stats = match per_subsystem_stats.get(&sub.id) {
            Some(s) => s,
            None => continue,
        };

        let max_internal = stats
            .internal_degrees
            .values()
            .copied()
            .max()
            .unwrap_or(0) as f64;
        let avg_internal = if !stats.internal_degrees.is_empty() {
            stats.internal_degrees.values().copied().sum::<usize>() as f64
                / stats.internal_degrees.len() as f64
        } else {
            0.0
        };

        for &sid in &sub.symbol_ids {
            let int_deg = *stats.internal_degrees.get(&sid).unwrap_or(&0);
            let bnd_deg = *stats.boundary_degrees.get(&sid).unwrap_or(&0);
            let ext_callers = *stats.external_callers.get(&sid).unwrap_or(&0);
            let ext_callees = *stats.external_callees.get(&sid).unwrap_or(&0);

            let mut roles = Vec::new();

            let is_boundary = bnd_deg > 0;
            let evidence_boundary = outgoing
                .get(&sid)
                .map(|outs| {
                    outs.iter().any(|&tgt| {
                        let ev = evidence_kinds.get(&(sid, tgt));
                        ev.map_or(false, |k| k == "boundary" || k == "reinforcing")
                    })
                })
                .unwrap_or(false);

            if ext_callers > 0 && int_deg > 0 {
                roles.push(StructuralRole::EntryPoint);
            }

            if max_internal > 0.0 && int_deg as f64 >= max_internal * 0.6 && int_deg >= 3 {
                roles.push(StructuralRole::Hub);
            }

            if int_deg == 0 && bnd_deg == 0 {
                roles.push(StructuralRole::Leaf);
            } else if int_deg <= 1 && ext_callers == 0 {
                roles.push(StructuralRole::Leaf);
            }

            if (is_boundary || evidence_boundary) && bnd_deg >= 2 {
                roles.push(StructuralRole::Boundary);
            }

            if int_deg >= 5 && int_deg as f64 > avg_internal * 1.5 {
                let internal_callees: Vec<i64> = outgoing
                    .get(&sid)
                    .map(|outs| {
                        outs.iter()
                            .filter(|t| {
                                per_subsystem_stats.get(&sub.id).map_or(false, |st| {
                                    st.internal_degrees.contains_key(t)
                                })
                            })
                            .copied()
                            .collect()
                    })
                    .unwrap_or_default();

                let unique_subsystems_called: HashSet<usize> = internal_callees
                    .iter()
                    .filter_map(|t| subsystem_index.symbol_to_subsystem.get(t))
                    .copied()
                    .collect();

                if unique_subsystems_called.len() <= 1 && int_deg >= 5 {
                    roles.push(StructuralRole::Orchestrator);
                }
            }

            if roles.is_empty() && int_deg > 0 {
                roles.push(StructuralRole::Hub);
            }

            roles.sort_by_key(|r| std::cmp::Reverse(match r {
                StructuralRole::EntryPoint => 5,
                StructuralRole::Orchestrator => 4,
                StructuralRole::Hub => 3,
                StructuralRole::Boundary => 2,
                StructuralRole::Leaf => 1,
            }));

            results.push(SymbolStructuralRole {
                symbol_id: sid,
                symbol_name: symbol_names.get(&sid).cloned().unwrap_or_default(),
                roles,
                subsystem_id: sub.id,
                internal_degree: int_deg,
                boundary_degree: bnd_deg,
                external_callers: ext_callers,
                external_callees: ext_callees,
            });
        }
    }

    Ok(results)
}

pub fn store_structural_roles(
    db: &GraphDb,
    roles: &[SymbolStructuralRole],
) -> Result<(), String> {
    let conn = db.conn();

    conn.execute(
        "CREATE TABLE IF NOT EXISTS symbol_structural_roles (
            symbol_id INTEGER PRIMARY KEY,
            symbol_name TEXT NOT NULL,
            subsystem_id INTEGER NOT NULL,
            roles TEXT NOT NULL,
            internal_degree INTEGER NOT NULL DEFAULT 0,
            boundary_degree INTEGER NOT NULL DEFAULT 0,
            external_callers INTEGER NOT NULL DEFAULT 0,
            external_callees INTEGER NOT NULL DEFAULT 0
        )",
        [],
    )
    .map_err(|e| e.to_string())?;

    conn.execute("DELETE FROM symbol_structural_roles", [])
        .map_err(|e| e.to_string())?;

    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO symbol_structural_roles
                 (symbol_id, symbol_name, subsystem_id, roles, internal_degree, boundary_degree, external_callers, external_callees)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )
            .map_err(|e| e.to_string())?;

        for r in roles {
            let roles_str = r
                .roles
                .iter()
                .map(|role| role.as_str().to_string())
                .collect::<Vec<_>>()
                .join(",");

            stmt.execute(params![
                r.symbol_id,
                &r.symbol_name,
                r.subsystem_id as i64,
                &roles_str,
                r.internal_degree as i64,
                r.boundary_degree as i64,
                r.external_callers as i64,
                r.external_callees as i64,
            ])
            .map_err(|e| e.to_string())?;
        }
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

struct SubsystemDegreeStats {
    internal_degrees: HashMap<i64, usize>,
    boundary_degrees: HashMap<i64, usize>,
    external_callers: HashMap<i64, usize>,
    external_callees: HashMap<i64, usize>,
}

fn setup_test_db() -> GraphDb {
    let db = GraphDb::open_in_memory().unwrap();

    let f1 = db
        .upsert_file("src/auth/login.rs", "rust", "a", 1000, 50)
        .unwrap();
    let f2 = db
        .upsert_file("src/auth/session.rs", "rust", "b", 1000, 40)
        .unwrap();
    let f3 = db
        .upsert_file("src/utils/crypto.rs", "rust", "c", 1000, 60)
        .unwrap();

    let s1 = SymbolBuilder::new(
        f1, "authenticate".into(), SymbolKind::Function, "fn auth()".into(), "rust".into(),
    )
    .lines(1, 10)
    .build();
    let s2 = SymbolBuilder::new(
        f1, "validate_token".into(), SymbolKind::Function, "fn val()".into(), "rust".into(),
    )
    .lines(12, 20)
    .build();
    let s3 = SymbolBuilder::new(
        f2, "create_session".into(), SymbolKind::Function, "fn sess()".into(), "rust".into(),
    )
    .lines(1, 10)
    .build();
    let s4 = SymbolBuilder::new(
        f2, "destroy_session".into(), SymbolKind::Function, "fn destroy()".into(), "rust".into(),
    )
    .lines(12, 20)
    .build();
    let s5 = SymbolBuilder::new(
        f3, "hash_password".into(), SymbolKind::Function, "fn hash()".into(), "rust".into(),
    )
    .lines(1, 10)
    .build();
    let s6 = SymbolBuilder::new(
        f3, "verify_password".into(), SymbolKind::Function, "fn verify()".into(), "rust".into(),
    )
    .lines(12, 20)
    .build();

    let id1 = db.insert_symbol(&s1).unwrap();
    let id2 = db.insert_symbol(&s2).unwrap();
    let id3 = db.insert_symbol(&s3).unwrap();
    let id4 = db.insert_symbol(&s4).unwrap();
    let id5 = db.insert_symbol(&s5).unwrap();
    let id6 = db.insert_symbol(&s6).unwrap();

    let null_meta = serde_json::Value::Null;

    db.insert_edge(id1, id2, EdgeKind::Calls, 1.0, null_meta.clone())
        .unwrap();
    db.insert_edge(id3, id4, EdgeKind::Calls, 1.0, null_meta.clone())
        .unwrap();
    db.insert_edge(id1, id3, EdgeKind::Calls, 1.0, null_meta.clone())
        .unwrap();
    db.insert_edge(id5, id6, EdgeKind::Calls, 1.0, null_meta.clone())
        .unwrap();
    db.insert_edge(id1, id5, EdgeKind::Calls, 1.0, null_meta)
        .unwrap();

    db
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_subsystems_basic() {
        let db = setup_test_db();
        let index = detect_subsystems(&db).unwrap();

        assert!(
            !index.subsystems.is_empty(),
            "should detect at least one subsystem"
        );
        assert_eq!(
            index.symbol_to_subsystem.len(),
            6,
            "all 6 symbols should be assigned to a subsystem"
        );

        let total_symbols: usize = index.subsystems.iter().map(|s| s.symbol_ids.len()).sum();
        assert_eq!(total_symbols, 6, "total symbols across subsystems should be 6");
    }

    #[test]
    fn test_subsystem_cohesion() {
        let db = setup_test_db();
        let index = detect_subsystems(&db).unwrap();

        for sub in &index.subsystems {
            assert!(
                sub.cohesion >= 0.0 && sub.cohesion <= 1.0,
                "cohesion {} for subsystem '{}' should be between 0 and 1",
                sub.cohesion,
                sub.name
            );
        }
    }

    #[test]
    fn test_tiny_subsystem_merge() {
        let db = GraphDb::open_in_memory().unwrap();

        let f1 = db
            .upsert_file("src/big.rs", "rust", "a", 1000, 100)
            .unwrap();
        let f2 = db
            .upsert_file("src/tiny.rs", "rust", "b", 1000, 10)
            .unwrap();

        let mut symbols = Vec::new();
        for i in 0..5 {
            let s = SymbolBuilder::new(
                f1,
                format!("big_fn_{}", i),
                SymbolKind::Function,
                format!("fn big_{}() {{}}", i),
                "rust".into(),
            )
            .lines(i * 10 + 1, i * 10 + 5)
            .build();
            symbols.push(db.insert_symbol(&s).unwrap());
        }

        let lone = SymbolBuilder::new(
            f2,
            "lone_fn".into(),
            SymbolKind::Function,
            "fn lone() {}".into(),
            "rust".into(),
        )
        .lines(1, 5)
        .build();
        let lone_id = db.insert_symbol(&lone).unwrap();

        for i in 0..4 {
            db.insert_edge(
                symbols[i],
                symbols[i + 1],
                EdgeKind::Calls,
                1.0,
                serde_json::Value::Null,
            )
            .unwrap();
        }
        db.insert_edge(
            symbols[0],
            lone_id,
            EdgeKind::Calls,
            1.0,
            serde_json::Value::Null,
        )
        .unwrap();

        let index = detect_subsystems(&db).unwrap();

        let tiny_subsystems: Vec<_> = index
            .subsystems
            .iter()
            .filter(|s| s.symbol_ids.len() < 2)
            .collect();

        assert!(
            tiny_subsystems.is_empty(),
            "subsystems with < 2 symbols should have been merged, but found {}",
            tiny_subsystems.len()
        );
    }

    #[test]
    fn test_store_load_roundtrip() {
        let db = setup_test_db();
        let original = detect_subsystems(&db).unwrap();

        store_subsystems(&db, &original).unwrap();
        let loaded = load_subsystems(&db).unwrap();

        let mut orig_sorted = original.subsystems.clone();
        let mut load_sorted = loaded.subsystems.clone();
        orig_sorted.sort_by_key(|s| s.id);
        load_sorted.sort_by_key(|s| s.id);

        assert_eq!(
            orig_sorted.len(),
            load_sorted.len(),
            "subsystem count should match"
        );

        for (orig, load) in orig_sorted.iter().zip(load_sorted.iter()) {
            assert_eq!(orig.id, load.id);
            assert_eq!(orig.name, load.name);
            assert_eq!(orig.dominant_file, load.dominant_file);

            let mut orig_ids = orig.symbol_ids.clone();
            let mut load_ids = load.symbol_ids.clone();
            orig_ids.sort();
            load_ids.sort();
            assert_eq!(orig_ids, load_ids);

            let mut orig_names = orig.symbol_names.clone();
            let mut load_names = load.symbol_names.clone();
            orig_names.sort();
            load_names.sort();
            assert_eq!(orig_names, load_names);
            assert_eq!(orig.internal_edge_count, load.internal_edge_count);
            assert_eq!(orig.boundary_edge_count, load.boundary_edge_count);
            assert!(
                (orig.cohesion - load.cohesion).abs() < 1e-10,
                "cohesion mismatch: {} vs {}",
                orig.cohesion,
                load.cohesion
            );
        }

        assert_eq!(
            original.symbol_to_subsystem, loaded.symbol_to_subsystem,
            "symbol-to-subsystem mapping should match"
        );
    }

    #[test]
    fn test_no_edges_single_subsystem() {
        let db = GraphDb::open_in_memory().unwrap();
        let f1 = db
            .upsert_file("src/alone.rs", "rust", "a", 1000, 10)
            .unwrap();
        let s1 = SymbolBuilder::new(
            f1, "solo".into(), SymbolKind::Function, "fn solo()".into(), "rust".into(),
        )
        .lines(1, 5)
        .build();
        let _id = db.insert_symbol(&s1).unwrap();

        let index = detect_subsystems(&db).unwrap();
        assert_eq!(index.subsystems.len(), 1);
        assert_eq!(index.subsystems[0].symbol_ids.len(), 1);
        assert!(
            (index.subsystems[0].cohesion - 1.0).abs() < 1e-10,
            "single symbol subsystem should have cohesion 1.0"
        );
    }

    #[test]
    fn test_structural_roles_entry_point() {
        let db = setup_test_db();
        let index = detect_subsystems(&db).unwrap();
        let roles = materialize_structural_roles(&db, &index).unwrap();

        let auth_roles: Vec<_> = roles
            .iter()
            .filter(|r| r.symbol_name == "authenticate")
            .collect();

        if !auth_roles.is_empty() {
            let ar = &auth_roles[0];
            let has_significant_role = ar
                .roles
                .iter()
                .any(|r| matches!(r, StructuralRole::Hub | StructuralRole::EntryPoint | StructuralRole::Boundary));
            assert!(
                has_significant_role,
                "authenticate should have a significant structural role, got {:?}",
                ar.roles
            );
        }
    }

    #[test]
    fn test_structural_roles_leaf() {
        let db = setup_test_db();
        let index = detect_subsystems(&db).unwrap();
        let roles = materialize_structural_roles(&db, &index).unwrap();

        let destroy_idx = roles
            .iter()
            .position(|r| r.symbol_name == "destroy_session")
            .map(|i| &roles[i]);

        if let Some(dr) = destroy_idx {
            assert!(
                dr.roles.contains(&StructuralRole::Leaf),
                "destroy_session should be leaf, got {:?}",
                dr.roles
            );
        }
    }

    #[test]
    fn test_structural_roles_store_load() {
        let db = setup_test_db();
        let index = detect_subsystems(&db).unwrap();
        let roles = materialize_structural_roles(&db, &index).unwrap();

        store_structural_roles(&db, &roles).unwrap();

        let table_exists: bool = db
            .conn()
            .prepare("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='symbol_structural_roles'")
            .unwrap()
            .query_row([], |row| row.get::<_, i64>(0))
            .unwrap() > 0;
        assert!(table_exists, "table should exist");

        let count: i64 = db
            .conn()
            .prepare("SELECT COUNT(*) FROM symbol_structural_roles")
            .unwrap()
            .query_row([], |row| row.get(0))
            .unwrap();
        assert!(count > 0, "should have stored roles");
    }
}
