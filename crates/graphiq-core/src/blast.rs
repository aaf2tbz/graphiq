use crate::db::GraphDb;
use crate::edge::{BlastDirection, BlastEntry, BlastRadius, EdgeKind};
use crate::graph::{bounded_bfs, TraverseDirection};

pub fn compute_blast_radius(
    db: &GraphDb,
    symbol_id: i64,
    max_depth: usize,
    direction: BlastDirection,
    edge_filter: Option<Vec<EdgeKind>>,
) -> Result<BlastRadius, String> {
    let origin = db
        .get_symbol(symbol_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("symbol {} not found", symbol_id))?;

    let origin_file = db
        .get_file_by_id(origin.file_id)
        .map_err(|e| e.to_string())?
        .map(|f| f.path.to_string_lossy().to_string())
        .unwrap_or_default();

    let default_filter = vec![
        EdgeKind::Calls,
        EdgeKind::Imports,
        EdgeKind::References,
        EdgeKind::Contains,
    ];
    let filter = edge_filter.unwrap_or(default_filter);

    let forward_filter: Vec<EdgeKind> = filter
        .iter()
        .filter(|k| **k != EdgeKind::Tests && **k != EdgeKind::Implements)
        .cloned()
        .collect();

    let backward_filter = filter.clone();

    let mut forward_entries = Vec::new();
    let mut backward_entries = Vec::new();

    if direction == BlastDirection::Forward || direction == BlastDirection::Both {
        let raw = bounded_bfs(
            db,
            &[symbol_id],
            TraverseDirection::Outgoing,
            &forward_filter,
            max_depth,
        );
        forward_entries = raw
            .into_iter()
            .filter_map(|(sid, dist, kinds)| entry_from_bfs(db, sid, dist, kinds))
            .collect();
    }

    if direction == BlastDirection::Backward || direction == BlastDirection::Both {
        let raw = bounded_bfs(
            db,
            &[symbol_id],
            TraverseDirection::Incoming,
            &backward_filter,
            max_depth,
        );
        backward_entries = raw
            .into_iter()
            .filter_map(|(sid, dist, kinds)| entry_from_bfs(db, sid, dist, kinds))
            .collect();
    }

    Ok(BlastRadius {
        origin_name: origin.name,
        origin_kind: origin.kind.as_str().to_string(),
        origin_file,
        forward: forward_entries,
        backward: backward_entries,
        max_depth,
    })
}

fn entry_from_bfs(
    db: &GraphDb,
    symbol_id: i64,
    distance: usize,
    edge_kinds: Vec<EdgeKind>,
) -> Option<BlastEntry> {
    let sym = db.get_symbol(symbol_id).ok()??;
    let file_path = db
        .get_file_by_id(sym.file_id)
        .ok()
        .flatten()
        .map(|f| f.path.to_string_lossy().to_string())
        .unwrap_or_default();

    Some(BlastEntry {
        symbol_id: sym.id,
        symbol_name: sym.name,
        symbol_kind: sym.kind.as_str().to_string(),
        file_path,
        distance,
        path: edge_kinds,
    })
}

pub fn format_blast_report(radius: &BlastRadius) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Blast Radius: {} ({})",
        radius.origin_name, radius.origin_kind
    ));
    if !radius.origin_file.is_empty() {
        out.push_str(&format!(" @ {}", radius.origin_file));
    }
    out.push('\n');

    out.push_str("├── Forward (affects):\n");
    if radius.forward.is_empty() {
        out.push_str("│   (none)\n");
    } else {
        for entry in &radius.forward {
            let kinds: Vec<&str> = entry.path.iter().map(|k| k.as_str()).collect();
            out.push_str(&format!(
                "│   ├── [{}] {}::{} ({})\n",
                entry.distance,
                entry.file_path,
                entry.symbol_name,
                kinds.join(" → ")
            ));
        }
    }

    out.push_str("├── Backward (depends on):\n");
    if radius.backward.is_empty() {
        out.push_str("│   (none)\n");
    } else {
        for entry in &radius.backward {
            let kinds: Vec<&str> = entry.path.iter().map(|k| k.as_str()).collect();
            out.push_str(&format!(
                "│   ├── [{}] {}::{} ({})\n",
                entry.distance,
                entry.file_path,
                entry.symbol_name,
                kinds.join(" ← ")
            ));
        }
    }

    out.push_str(&format!(
        "└── Summary: {} forward, {} backward, depth {}",
        radius.forward_count(),
        radius.backward_count(),
        radius.max_depth,
    ));

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol::{SymbolBuilder, SymbolKind};

    fn setup_db() -> GraphDb {
        let db = GraphDb::open_in_memory().unwrap();
        let fid = db
            .upsert_file("src/app.ts", "typescript", "abc", 1000, 100)
            .unwrap();

        let s1 = SymbolBuilder::new(
            fid,
            "RateLimiter".into(),
            SymbolKind::Class,
            "class RateLimiter {}".into(),
            "typescript".into(),
        )
        .lines(1, 20)
        .build();
        let s2 = SymbolBuilder::new(
            fid,
            "handle".into(),
            SymbolKind::Method,
            "fn handle()".into(),
            "typescript".into(),
        )
        .lines(5, 10)
        .build();
        let s3 = SymbolBuilder::new(
            fid,
            "checkLimit".into(),
            SymbolKind::Function,
            "fn checkLimit()".into(),
            "typescript".into(),
        )
        .lines(22, 30)
        .build();
        let s4 = SymbolBuilder::new(
            fid,
            "server".into(),
            SymbolKind::Function,
            "fn server()".into(),
            "typescript".into(),
        )
        .lines(32, 40)
        .build();

        let id1 = db.insert_symbol(&s1).unwrap();
        let id2 = db.insert_symbol(&s2).unwrap();
        let id3 = db.insert_symbol(&s3).unwrap();
        let id4 = db.insert_symbol(&s4).unwrap();

        db.insert_edge(id1, id2, EdgeKind::Contains, 0.9, serde_json::Value::Null)
            .unwrap();
        db.insert_edge(id2, id3, EdgeKind::Calls, 1.0, serde_json::Value::Null)
            .unwrap();
        db.insert_edge(id4, id2, EdgeKind::Calls, 1.0, serde_json::Value::Null)
            .unwrap();

        db
    }

    #[test]
    fn test_blast_forward() {
        let db = setup_db();
        let radius = compute_blast_radius(&db, 1, 3, BlastDirection::Forward, None).unwrap();
        assert!(radius.forward_count() >= 1);
        assert!(radius.backward_count() == 0);
    }

    #[test]
    fn test_blast_backward() {
        let db = setup_db();
        let radius = compute_blast_radius(&db, 3, 3, BlastDirection::Backward, None).unwrap();
        assert!(radius.backward_count() >= 1);
        assert!(radius.forward_count() == 0);
    }

    #[test]
    fn test_blast_both() {
        let db = setup_db();
        let radius = compute_blast_radius(&db, 2, 3, BlastDirection::Both, None).unwrap();
        assert!(radius.forward_count() + radius.backward_count() >= 2);
    }

    #[test]
    fn test_blast_depth_limit() {
        let db = setup_db();
        let radius = compute_blast_radius(&db, 1, 1, BlastDirection::Forward, None).unwrap();
        for entry in &radius.forward {
            assert!(entry.distance <= 1);
        }
    }

    #[test]
    fn test_blast_missing_symbol() {
        let db = GraphDb::open_in_memory().unwrap();
        let result = compute_blast_radius(&db, 999, 3, BlastDirection::Both, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_format_blast_report() {
        let db = setup_db();
        let radius = compute_blast_radius(&db, 2, 3, BlastDirection::Both, None).unwrap();
        let report = format_blast_report(&radius);
        assert!(report.contains("Blast Radius: handle"));
        assert!(report.contains("Summary:"));
    }
}
