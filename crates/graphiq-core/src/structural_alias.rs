//! Structural aliases — disambiguate collision-prone symbol names.
//!
//! When multiple symbols share the same name (e.g., `new`, `build`, `process`),
//! this module builds ambiguity fingerprints from edge mix, signature types,
//! neighborhood context, container context, and behavioral patterns. Symbols
//! with identical fingerprints are aliases — searching for one should find all.
//!
//! The fingerprint is stored as `alias_text` in the CruncherIndex so that
//! graph walk scoring can boost alias matches when a generic name appears
//! in the right structural context.
//!
//! Entry point: [`compute_structural_aliases`] — builds collision sets and
//! writes alias text to the database.

use std::collections::HashMap;

use crate::db::GraphDb;

const COLLISION_THRESHOLD: usize = 3;
const MAX_ALIAS_TOKENS: usize = 15;

pub struct AmbiguityFingerprint {
    pub edge_mix: String,
    pub sig_types: String,
    pub neighborhood_sig: String,
    pub container_context: String,
    pub behavioral_context: String,
    pub alias_text: String,
}

pub struct AliasStats {
    pub collision_sets: usize,
    pub symbols_aliased: usize,
}

fn extract_type_tokens(signature: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut depth: i32 = 0;

    for ch in signature.chars() {
        match ch {
            '<' | '(' | '[' => {
                depth += 1;
                if !current.trim().is_empty() {
                    tokens.push(current.trim().to_string());
                }
                current.clear();
            }
            '>' | ')' | ']' | ',' => {
                depth = depth.saturating_sub(1);
                if !current.trim().is_empty() {
                    tokens.push(current.trim().to_string());
                }
                current.clear();
            }
            ':' | '-' => {
                if !current.trim().is_empty() {
                    tokens.push(current.trim().to_string());
                }
                current.clear();
            }
            ' ' | '\t' | '\n' => {
                if depth > 0 && !current.trim().is_empty() {
                    tokens.push(current.trim().to_string());
                }
                current.clear();
            }
            _ => {
                current.push(ch);
            }
        }
    }
    if !current.trim().is_empty() {
        tokens.push(current.trim().to_string());
    }

    let keywords: &[&str] = &[
        "fn", "function", "async", "await", "pub", "private", "protected",
        "static", "const", "mut", "let", "var", "self", "Self", "impl",
        "where", "return", "void", "never", "unknown", "any",
        "true", "false", "null", "undefined",
    ];

    tokens
        .into_iter()
        .filter(|t| t.len() >= 2 && !keywords.contains(&t.as_str()))
        .filter(|t| !t.chars().all(|c| c.is_ascii_digit()))
        .map(|t| crate::tokenize::decompose_identifier(&t))
        .flat_map(|t| t.split_whitespace().map(|s| s.to_lowercase()).collect::<Vec<_>>())
        .filter(|t| t.len() >= 2)
        .collect()
}

fn compute_edge_mix(
    outgoing: &[(String, String)],
    incoming: &[(String, String)],
) -> String {
    let mut kind_counts: HashMap<String, usize> = HashMap::new();

    for (kind, _) in outgoing {
        *kind_counts.entry(format!("out_{}", kind)).or_insert(0) += 1;
    }
    for (kind, _) in incoming {
        *kind_counts.entry(format!("in_{}", kind)).or_insert(0) += 1;
    }

    let mut sorted: Vec<(String, usize)> = kind_counts.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));

    let mut parts = Vec::new();
    for (label, count) in sorted.iter().take(4) {
        let short = label
            .replace("out_calls", "caller-of")
            .replace("in_calls", "called-by")
            .replace("out_contains", "contains")
            .replace("in_contains", "contained-in")
            .replace("out_references", "refs")
            .replace("in_references", "refd-by")
            .replace("out_implements", "implements")
            .replace("in_implements", "impld-by")
            .replace("out_extends", "extends")
            .replace("in_extends", "extended-by")
            .replace("out_imports", "imports")
            .replace("in_imports", "imported-by")
            .replace("out_tests", "tests")
            .replace("out_shares_type", "shares-type")
            .replace("out_shares_error_type", "shares-err-type")
            .replace("out_shares_data_shape", "shares-data");
        let tag = if *count > 3 {
            format!("{}-heavy", short)
        } else if *count > 1 {
            format!("{}-x{}", short, count)
        } else {
            short
        };
        parts.push(tag);
    }

    parts.join(" ")
}

fn compute_neighborhood_signature(
    symbol_id: i64,
    out_by_id: &HashMap<i64, Vec<(String, String)>>,
    in_by_id: &HashMap<i64, Vec<(String, String)>>,
    name_to_decomposed: &HashMap<String, String>,
    global_term_doc_freq: &HashMap<String, usize>,
    total_symbols: usize,
) -> String {
    let mut neighbor_terms: HashMap<String, f64> = HashMap::new();
    let df_threshold = (total_symbols as f64 * 0.3) as usize;

    let collect_from = |edges: &[(String, String)], terms: &mut HashMap<String, f64>| {
        for (_, neighbor_name) in edges.iter().take(12) {
            if let Some(decomp) = name_to_decomposed.get(neighbor_name) {
                for word in decomp.split_whitespace() {
                    let wl = word.to_lowercase();
                    if wl.len() >= 3 {
                        let df = global_term_doc_freq.get(&wl).copied().unwrap_or(0);
                        if df < df_threshold {
                            let idf = (1.0 + total_symbols as f64 / (df as f64 + 1.0)).ln();
                            *terms.entry(wl).or_insert(0.0) += idf;
                        }
                    }
                }
            }
        }
    };

    if let Some(outgoing) = out_by_id.get(&symbol_id) {
        collect_from(outgoing, &mut neighbor_terms);
    }
    if let Some(incoming) = in_by_id.get(&symbol_id) {
        collect_from(incoming, &mut neighbor_terms);
    }

    let mut sorted: Vec<(String, f64)> = neighbor_terms.into_iter().collect();
    sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    sorted.truncate(MAX_ALIAS_TOKENS);

    sorted.into_iter().map(|(t, _)| t).collect::<Vec<_>>().join(" ")
}

fn compute_container_context(
    db: &GraphDb,
    symbol_id: i64,
    file_path: Option<&str>,
) -> String {
    let mut parts = Vec::new();

    if let Some(path) = file_path {
        let segments: Vec<&str> = path
            .split('/')
            .filter(|s| !s.is_empty() && *s != "src" && *s != "lib" && *s != "crate")
            .collect();
        let meaningful = segments.iter().rev().take(3).cloned().collect::<Vec<_>>();
        let mut rev = meaningful;
        rev.reverse();
        for seg in &rev {
            let decomp = crate::tokenize::decompose_identifier(seg);
            let stem = seg.rsplit_once('.').map(|(n, _)| n).unwrap_or(seg);
            let decomp2 = crate::tokenize::decompose_identifier(stem);
            if !decomp.is_empty() {
                parts.push(decomp.clone());
            }
            if decomp2 != decomp {
                parts.push(decomp2);
            }
        }
    }

    if let Ok(Some((_, container_name))) = db.container_for(symbol_id) {
        let decomp = crate::tokenize::decompose_identifier(&container_name);
        parts.push(format!("in-{}", decomp.replace(' ', "-")));
        let container_lower = container_name.to_lowercase();
        let domain_tags: &[(&str, &str)] = &[
            ("runtime", "runtime-context"),
            ("io", "io-context"),
            ("net", "network-context"),
            ("http", "http-context"),
            ("fs", "filesystem-context"),
            ("channel", "channel-context"),
            ("stream", "stream-context"),
            ("sync", "sync-context"),
            ("park", "parking-context"),
            ("worker", "worker-context"),
            ("task", "task-context"),
            ("process", "process-context"),
            ("time", "timer-context"),
            ("signal", "signal-context"),
            ("buffer", "buffer-context"),
            ("codec", "codec-context"),
            ("frame", "frame-context"),
            ("buf", "buffer-context"),
            ("read", "reader-context"),
            ("write", "writer-context"),
            ("poll", "poll-context"),
            ("future", "future-context"),
            ("async", "async-context"),
            ("block", "blocking-context"),
            ("sem", "semaphore-context"),
            ("lock", "lock-context"),
            ("mutex", "mutex-context"),
            ("queue", "queue-context"),
            ("sched", "scheduler-context"),
        ];
        for (pattern, tag) in domain_tags {
            if container_lower.contains(pattern) {
                parts.push(tag.to_string());
            }
        }
    }

    parts.into_iter().take(10).collect::<Vec<_>>().join(" ")
}

fn infer_behavioral_context(
    name: &str,
    signature: Option<&str>,
    source: Option<&str>,
    outgoing: &[(String, String)],
    incoming: &[(String, String)],
) -> String {
    let name_lower = name.to_lowercase();
    let sig_lower = signature.unwrap_or("").to_lowercase();
    let src_lower = source.unwrap_or("").to_lowercase();

    let mut tags = Vec::new();

    let callee_names: Vec<String> = outgoing
        .iter()
        .filter(|(k, _)| k == "calls")
        .map(|(_, n)| n.to_lowercase())
        .take(8)
        .collect();

    if sig_lower.contains("async") || sig_lower.contains("future")
        || sig_lower.contains("promise")
    {
        tags.push("async-op");
    }
    if sig_lower.contains("pin") || src_lower.contains("pin!")
        || src_lower.contains("pin_project")
    {
        tags.push("pinned");
    }
    if sig_lower.contains("poll") || name_lower.starts_with("poll_") {
        if callee_names.iter().any(|n| n.contains("park") || n.contains("waker")) {
            tags.push("parking-poll");
        } else if callee_names.iter().any(|n| n.contains("read") || n.contains("write") || n.contains("flush")) {
            tags.push("io-poll");
        } else if callee_names.iter().any(|n| n.contains("stream") || n.contains("next")) {
            tags.push("stream-poll");
        } else if callee_names.iter().any(|n| n.contains("reserve") || n.contains("acquire")) {
            tags.push("resource-poll");
        } else if callee_names.iter().any(|n| n.contains("complete") || n.contains("close")) {
            tags.push("completion-poll");
        } else {
            tags.push("task-poll");
        }
    }
    if name_lower.contains("read") || name_lower.contains("recv") {
        if sig_lower.contains("buf") || callee_names.iter().any(|n| n.contains("buf")) {
            tags.push("buffered-read");
        } else {
            tags.push("raw-read");
        }
    }
    if name_lower.contains("write") || name_lower.contains("send") {
        if sig_lower.contains("vectored") {
            tags.push("scatter-write");
        } else if callee_names.iter().any(|n| n.contains("flush")) {
            tags.push("buffered-write");
        } else {
            tags.push("raw-write");
        }
    }
    if name_lower.contains("flush") {
        tags.push("drain-buffer");
    }
    if name_lower.contains("spawn") || name_lower.contains("launch") {
        tags.push("task-spawn");
    }
    if name_lower.contains("block") {
        tags.push("blocking-wait");
    }
    if name_lower.contains("park") {
        tags.push("thread-park");
    }
    if name_lower.contains("shutdown") {
        if callee_names.iter().any(|n| n.contains("close") || n.contains("drop")) {
            tags.push("graceful-shutdown");
        } else {
            tags.push("shutdown");
        }
    }
    if name_lower.contains("transition") {
        if callee_names.iter().any(|n| n.contains("park")) {
            tags.push("state-transition-parking");
        } else if callee_names.iter().any(|n| n.contains("search") || n.contains("work")) {
            tags.push("state-transition-search");
        } else if callee_names.iter().any(|n| n.contains("complete") || n.contains("finish")) {
            tags.push("state-transition-completion");
        } else if callee_names.iter().any(|n| n.contains("shutdown")) {
            tags.push("state-transition-shutdown");
        } else {
            tags.push("state-transition");
        }
    }
    if name_lower.contains("join") {
        if name_lower.contains("next") {
            tags.push("iterative-join");
        } else {
            tags.push("task-join");
        }
    }
    if name_lower.contains("cancel") {
        tags.push("cancellation");
    }
    if name_lower.contains("reclaim") || name_lower.contains("free") || name_lower.contains("dealloc") {
        tags.push("memory-reclaim");
    }
    if name_lower.contains("merge") {
        if callee_names.iter().any(|n| n.contains("stream")) {
            tags.push("stream-merge");
        } else {
            tags.push("merge");
        }
    }

    if incoming.iter().any(|(k, _)| k == "contains") {
        tags.push("method");
    }

    tags.into_iter().take(6).collect::<Vec<_>>().join(" ")
}

pub fn compute_structural_aliases(db: &GraphDb) -> Result<AliasStats, Box<dyn std::error::Error>> {
    let conn = db.conn();

    let total_symbols: usize = conn
        .query_row("SELECT COUNT(*) FROM symbols", [], |row| row.get::<_, i64>(0))?
        as usize;

    let mut collision_map: HashMap<String, Vec<i64>> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT id, LOWER(name) as lname FROM symbols ORDER BY id"
        )?;
        let rows: Vec<(i64, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .flatten()
            .collect();
        for (id, lname) in rows {
            collision_map.entry(lname).or_default().push(id);
        }
    }

    let collision_sets: Vec<&Vec<i64>> = collision_map
        .values()
        .filter(|ids| ids.len() >= COLLISION_THRESHOLD)
        .collect();

    if collision_sets.is_empty() {
        return Ok(AliasStats {
            collision_sets: 0,
            symbols_aliased: 0,
        });
    }

    let mut out_by_id: HashMap<i64, Vec<(String, String)>> = HashMap::new();
    for (source_id, kind, target_name) in db.outgoing_edges_grouped()? {
        out_by_id
            .entry(source_id)
            .or_default()
            .push((kind, target_name));
    }

    let mut in_by_id: HashMap<i64, Vec<(String, String)>> = HashMap::new();
    for (target_id, kind, source_name) in db.incoming_edges_grouped()? {
        in_by_id
            .entry(target_id)
            .or_default()
            .push((kind, source_name));
    }

    let mut name_to_decomposed: HashMap<String, String> = HashMap::new();
    let mut symbol_info: HashMap<i64, SymbolInfo> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT id, name, name_decomposed, signature, source, file_id \
             FROM symbols"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(SymbolInfo {
                id: row.get(0)?,
                name: row.get(1)?,
                name_decomposed: row.get(2)?,
                signature: row.get::<_, Option<String>>(3)?,
                source: row.get::<_, Option<String>>(4)?,
                file_id: row.get(5)?,
            })
        })?;
        for row in rows {
            let info = row?;
            name_to_decomposed.insert(info.name.clone(), info.name_decomposed.clone());
            symbol_info.insert(info.id, info);
        }
    }

    let mut global_term_doc_freq: HashMap<String, usize> = HashMap::new();
    for (_, decomp) in &name_to_decomposed {
        let mut seen = std::collections::HashSet::new();
        for word in decomp.split_whitespace() {
            let wl = word.to_lowercase();
            if wl.len() >= 3 && seen.insert(wl.clone()) {
                *global_term_doc_freq.entry(wl).or_insert(0) += 1;
            }
        }
    }

    let mut file_paths: HashMap<i64, String> = HashMap::new();
    {
        let mut stmt = conn.prepare("SELECT id, path FROM files")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (fid, path) = row?;
            file_paths.insert(fid, path);
        }
    }

    let mut symbols_aliased = 0usize;

    for ids in &collision_sets {
        let mut alias_data: Vec<(i64, AmbiguityFingerprint)> = Vec::new();

        for &id in *ids {
            let info = match symbol_info.get(&id) {
                Some(i) => i,
                None => continue,
            };

            let outgoing = out_by_id.get(&id).cloned().unwrap_or_default();
            let incoming = in_by_id.get(&id).cloned().unwrap_or_default();

            let edge_mix = compute_edge_mix(&outgoing, &incoming);

            let sig_str = info.signature.as_deref().unwrap_or("");
            let type_tokens = extract_type_tokens(sig_str);
            let sig_types = type_tokens.into_iter().take(8).collect::<Vec<_>>().join(" ");

            let neighborhood_sig = compute_neighborhood_signature(
                id,
                &out_by_id,
                &in_by_id,
                &name_to_decomposed,
                &global_term_doc_freq,
                total_symbols,
            );

            let fp = file_paths.get(&info.file_id).map(|s| s.as_str());
            let container_context = compute_container_context(db, id, fp);

            let behavioral_context = infer_behavioral_context(
                &info.name,
                info.signature.as_deref(),
                info.source.as_deref(),
                &outgoing,
                &incoming,
            );

            let mut alias_parts = Vec::new();
            if !edge_mix.is_empty() {
                alias_parts.push(format!("struct:{}", edge_mix));
            }
            if !sig_types.is_empty() {
                alias_parts.push(format!("types:{}", sig_types));
            }
            if !neighborhood_sig.is_empty() {
                alias_parts.push(format!("nbr:{}", neighborhood_sig));
            }
            if !container_context.is_empty() {
                alias_parts.push(format!("ctx:{}", container_context));
            }
            if !behavioral_context.is_empty() {
                alias_parts.push(format!("op:{}", behavioral_context));
            }

            let alias_text = alias_parts.join(". ");

            alias_data.push((id, AmbiguityFingerprint {
                edge_mix,
                sig_types,
                neighborhood_sig,
                container_context,
                behavioral_context,
                alias_text,
            }));
        }

        for (id, fp) in &alias_data {
            let existing_hints: String = conn
                .query_row(
                    "SELECT search_hints FROM symbols WHERE id = ?1",
                    rusqlite::params![id],
                    |row| row.get(0),
                )
                .unwrap_or_default();

            if fp.alias_text.is_empty() {
                continue;
            }

            let alias_hint = format!("alias:{}", fp.alias_text);

            let updated = if existing_hints.is_empty() {
                alias_hint
            } else {
                format!("{}. {}", existing_hints, alias_hint)
            };

            db.update_search_hints(*id, &updated)?;
            symbols_aliased += 1;
        }
    }

    Ok(AliasStats {
        collision_sets: collision_sets.len(),
        symbols_aliased,
    })
}

struct SymbolInfo {
    id: i64,
    name: String,
    name_decomposed: String,
    signature: Option<String>,
    source: Option<String>,
    file_id: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::GraphDb;
    use crate::symbol::{SymbolBuilder, SymbolKind};

    fn build_collision_db() -> GraphDb {
        let db = GraphDb::open_in_memory().unwrap();
        let fid = db
            .upsert_file("src/runtime/worker.rs", "rust", "abc", 1000, 100)
            .unwrap();
        let fid2 = db
            .upsert_file("src/io/buffered_reader.rs", "rust", "def", 1000, 100)
            .unwrap();
        let fid3 = db
            .upsert_file("src/sync/semaphore.rs", "rust", "ghi", 1000, 100)
            .unwrap();

        let poll_worker = SymbolBuilder::new(
            fid, "poll".into(), SymbolKind::Method, 
            "fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<()>".into(),
            "rust".into(),
        ).lines(1, 10)
            .signature("fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()>")
            .build();

        let poll_reader = SymbolBuilder::new(
            fid2, "poll".into(), SymbolKind::Method,
            "fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Ready>".into(),
            "rust".into(),
        ).lines(11, 20)
            .signature("fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Ready<()>>")
            .build();

        let poll_sem = SymbolBuilder::new(
            fid3, "poll".into(), SymbolKind::Method,
            "fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Permit>".into(),
            "rust".into(),
        ).lines(21, 30)
            .signature("fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Permit>")
            .build();

        let worker_id = db.insert_symbol(&poll_worker).unwrap();
        let reader_id = db.insert_symbol(&poll_reader).unwrap();
        let sem_id = db.insert_symbol(&poll_sem).unwrap();

        let park_sym = SymbolBuilder::new(
            fid, "park".into(), SymbolKind::Function, "fn park()".into(), "rust".into(),
        ).lines(31, 35).build();
        let read_buf_sym = SymbolBuilder::new(
            fid2, "read_buffer".into(), SymbolKind::Function, "fn read_buffer()".into(), "rust".into(),
        ).lines(36, 40).build();
        let acquire_sym = SymbolBuilder::new(
            fid3, "acquire".into(), SymbolKind::Function, "fn acquire()".into(), "rust".into(),
        ).lines(41, 45).build();

        let park_id = db.insert_symbol(&park_sym).unwrap();
        let read_buf_id = db.insert_symbol(&read_buf_sym).unwrap();
        let acquire_id = db.insert_symbol(&acquire_sym).unwrap();

        use crate::edge::EdgeKind;
        db.insert_edge(worker_id, park_id, EdgeKind::Calls, 1.0, serde_json::Value::Null).unwrap();
        db.insert_edge(reader_id, read_buf_id, EdgeKind::Calls, 1.0, serde_json::Value::Null).unwrap();
        db.insert_edge(sem_id, acquire_id, EdgeKind::Calls, 1.0, serde_json::Value::Null).unwrap();

        db
    }

    #[test]
    fn test_compute_aliases_basic() {
        let db = build_collision_db();
        let stats = compute_structural_aliases(&db).unwrap();
        assert!(stats.collision_sets >= 1);
        assert!(stats.symbols_aliased >= 3);
    }

    #[test]
    fn test_alias_text_written_to_hints() {
        let db = build_collision_db();
        compute_structural_aliases(&db).unwrap();
        
        let conn = db.conn();
        let hints: Vec<String> = conn
            .prepare("SELECT search_hints FROM symbols WHERE name = 'poll'")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .flatten()
            .collect();

        assert_eq!(hints.len(), 3);
        for h in &hints {
            assert!(h.contains("alias:"), "expected alias in hints, got: {}", h);
        }

        let has_io = hints.iter().any(|h| h.contains("io-poll") || h.contains("buffered-read"));
        let has_parking = hints.iter().any(|h| h.contains("parking-poll") || h.contains("parking"));
        let has_resource = hints.iter().any(|h| h.contains("resource-poll") || h.contains("semaphore"));
        assert!(has_io || has_parking || has_resource, "expected distinct behavioral tags across poll symbols");
    }

    #[test]
    fn test_no_aliases_for_unique_names() {
        let db = GraphDb::open_in_memory().unwrap();
        let fid = db.upsert_file("src/app.rs", "rust", "abc", 100, 10).unwrap();
        
        let unique_sym = SymbolBuilder::new(
            fid, "very_unique_name_xyz".into(), SymbolKind::Function,
            "fn very_unique_name_xyz()".into(), "rust".into(),
        ).lines(1, 5).build();
        db.insert_symbol(&unique_sym).unwrap();

        let stats = compute_structural_aliases(&db).unwrap();
        assert_eq!(stats.collision_sets, 0);
        assert_eq!(stats.symbols_aliased, 0);
    }

    #[test]
    fn test_extract_type_tokens() {
        let tokens = extract_type_tokens("fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Ready<()>>");
        let token_str = tokens.join(" ");
        assert!(token_str.contains("poll"));
        assert!(token_str.contains("ready"));
        assert!(token_str.contains("context"));
        assert!(token_str.contains("pin"));
    }

    #[test]
    fn test_edge_mix_distinction() {
        let out_io = vec![
            ("calls".into(), "read_buf".into()),
            ("calls".into(), "flush".into()),
        ];
        let in_io = vec![
            ("calls".into(), "process".into()),
        ];

        let out_sched = vec![
            ("calls".into(), "spawn".into()),
            ("calls".into(), "park".into()),
            ("contains".into(), "inner".into()),
        ];
        let in_sched: Vec<(String, String)> = vec![];

        let mix_io = compute_edge_mix(&out_io, &in_io);
        let mix_sched = compute_edge_mix(&out_sched, &in_sched);

        assert_ne!(mix_io, mix_sched, "different edge patterns should produce different mixes");
        assert!(mix_io.contains("caller-of"));
        assert!(mix_sched.contains("contains") || mix_sched.contains("caller-of"));
    }
}
