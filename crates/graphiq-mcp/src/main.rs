use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

const SERVER_NAME: &str = "graphiq";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
const PROTOCOL_VERSION: &str = "2024-11-05";

struct ServerState {
    project_root: PathBuf,
    db_path: PathBuf,
    db: graphiq_core::db::GraphDb,
    cache: graphiq_core::cache::HotCache,
    cruncher_index: Option<graphiq_core::cruncher::CruncherIndex>,
    holo_index: Option<graphiq_core::cruncher::HoloIndex>,
}

fn resolve_project_root(raw: &str) -> PathBuf {
    let mut path = PathBuf::from(raw);

    if path.exists() && path.is_file() && path.extension().map_or(false, |e| e == "db") {
        if let Some(parent) = path.parent() {
            if parent.file_name().map_or(false, |n| n == ".graphiq") {
                if let Some(project) = parent.parent() {
                    path = project.to_path_buf();
                }
            }
        }
    }

    let resolved = if path.is_absolute() {
        path
    } else {
        match std::env::current_dir() {
            Ok(cwd) => cwd.join(&path),
            Err(_) => path,
        }
    };

    let resolved = resolved.canonicalize().unwrap_or(resolved);

    if !resolved.exists() {
        return resolved;
    }

    let mut candidate = resolved.clone();
    loop {
        if candidate.join(".git").exists() {
            log_err(&format!("detected git root: {}", candidate.display()));
            return candidate;
        }
        if !candidate.pop() {
            break;
        }
    }

    log_err(&format!("no git root found, using: {}", resolved.display()));
    resolved
}

fn resolve_db_path(project_root: &Path) -> PathBuf {
    project_root.join(".graphiq").join("graphiq.db")
}

fn ensure_indexed(state: &mut ServerState) -> Result<(), String> {
    let stats = state
        .db
        .stats()
        .map_err(|e| format!("failed to read stats: {e}"))?;

    if stats.files == 0 {
        log_err("database is empty — use the 'index' tool to index, or run 'graphiq setup'");
    }

    state.cache.prewarm(&state.db, 200);

    if state.cruncher_index.is_none() && stats.files > 0 {
        if let Ok(ci) = graphiq_core::cruncher::build_cruncher_index(&state.db) {
            let hi = graphiq_core::cruncher::build_holo_index(&state.db, &ci);
            state.cruncher_index = Some(ci);
            state.holo_index = Some(hi);
            log_err("goober v5 index built");
        }
    }

    Ok(())
}

fn do_index(state: &mut ServerState) -> Result<String, String> {
    let db_path = &state.db_path;
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
    }

    let indexer = graphiq_core::index::Indexer::new(&state.db);
    let result = indexer
        .index_project(&state.project_root)
        .map_err(|e| format!("index failed: {e}"))?;

    state.cache = graphiq_core::cache::HotCache::with_defaults();
    state.cache.prewarm(&state.db, 200);

    if let Ok(ci) = graphiq_core::cruncher::build_cruncher_index(&state.db) {
        let hi = graphiq_core::cruncher::build_holo_index(&state.db, &ci);
        state.cruncher_index = Some(ci);
        state.holo_index = Some(hi);
        log_err("goober v5 index rebuilt");
    }

    let msg = format!(
        "Indexed {} in {} files ({} symbols, {} edges)",
        state.project_root.display(),
        result.files_indexed,
        result.symbols_indexed,
        result.edges_inserted,
    );
    log_err(&msg);
    Ok(msg)
}

fn main() {
    let raw_arg = std::env::args().nth(1).unwrap_or_else(|| ".".into());

    let project_root = resolve_project_root(&raw_arg);
    let db_path = resolve_db_path(&project_root);

    log_err(&format!("project root: {}", project_root.display()));
    log_err(&format!("db path: {}", db_path.display()));

    if let Some(parent) = db_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let db = match graphiq_core::db::GraphDb::open(&db_path) {
        Ok(d) => d,
        Err(e) => {
            let msg = format!("failed to open database {}: {e}", db_path.display());
            log_err(&msg);
            send_error(-1, -32603, &msg);
            std::process::exit(1);
        }
    };

    let cache = graphiq_core::cache::HotCache::with_defaults();

    let (cruncher_index, holo_index) = match graphiq_core::cruncher::build_cruncher_index(&db) {
        Ok(ci) => {
            let hi = graphiq_core::cruncher::build_holo_index(&db, &ci);
            log_err("goober v5 index built");
            (Some(ci), Some(hi))
        }
        Err(e) => {
            log_err(&format!("cruncher build failed (falling back to FTS): {e}"));
            (None, None)
        }
    };

    let mut state = ServerState {
        project_root: project_root.clone(),
        db_path: db_path.clone(),
        db,
        cache,
        cruncher_index,
        holo_index,
    };

    if let Err(e) = ensure_indexed(&mut state) {
        log_err(&format!("auto-index failed: {e}"));
    }

    log_err("ready");

    let state = Arc::new(Mutex::new(state));
    let running = Arc::new(AtomicBool::new(true));

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();

    for line in stdin.lock().lines() {
        if !running.load(Ordering::Relaxed) {
            break;
        }

        let line = match line {
            Ok(l) => l,
            Err(e) => {
                log_err(&format!("stdin read error: {e}"));
                break;
            }
        };

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let msg: Value = match serde_json::from_str(line) {
            Ok(m) => m,
            Err(e) => {
                send_error(-1, -32700, &format!("parse error: {e}"));
                continue;
            }
        };

        if let Err(e) = handle_message(&msg, &state, &running, &mut stdout) {
            log_err(&format!("handler error: {e}"));
        }
    }
}

fn handle_message(
    msg: &Value,
    state: &Arc<Mutex<ServerState>>,
    running: &Arc<AtomicBool>,
    out: &mut impl Write,
) -> Result<(), String> {
    let id = msg.get("id").and_then(|v| v.as_i64()).unwrap_or(-1);

    if msg.get("method").is_none() && msg.get("result").is_none() && msg.get("error").is_none() {
        send_error(
            id,
            -32600,
            "invalid request: missing method, result, or error",
        );
        return Ok(());
    }

    let is_notification = msg.get("id").is_none();

    let method = match msg.get("method").and_then(|v| v.as_str()) {
        Some(m) => m,
        None => return Ok(()),
    };

    match method {
        "initialize" => {
            let s = state.lock().map_err(|e| e.to_string())?;
            let result = json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {
                    "tools": { "listChanged": false }
                },
                "serverInfo": {
                    "name": SERVER_NAME,
                    "version": SERVER_VERSION
                },
                "_meta": {
                    "projectRoot": s.project_root.display().to_string(),
                    "dbPath": s.db_path.display().to_string()
                }
            });
            send_response(id, &result, out)?;
        }
        "initialized" => {}
        "ping" => {
            send_response(id, &json!({}), out)?;
        }
        "notifications/cancelled" => {
            let req_id = msg
                .get("params")
                .and_then(|p| p.get("requestId"))
                .and_then(|v| v.as_i64());
            if let Some(rid) = req_id {
                log_err(&format!("notification: request {} cancelled", rid));
            }
        }
        "tools/list" => {
            send_response(id, &tools_list(), out)?;
        }
        "tools/call" => {
            let params = msg.get("params").cloned().unwrap_or(json!({}));
            let result = handle_tool_call(state, params);
            send_response(id, &result, out)?;
        }
        "shutdown" => {
            send_response(id, &json!(null), out)?;
            running.store(false, Ordering::Relaxed);
        }
        _ => {
            if !is_notification {
                send_error(id, -32601, &format!("method not found: {method}"));
            }
        }
    }

    Ok(())
}

fn send_response(id: i64, result: &Value, out: &mut impl Write) -> Result<(), String> {
    let resp = json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    });
    let serialized =
        serde_json::to_string(&resp).map_err(|e| format!("serialize response: {e}"))?;
    writeln!(out, "{}", serialized).map_err(|e| format!("write response: {e}"))?;
    out.flush().map_err(|e| format!("flush: {e}"))?;
    Ok(())
}

fn send_error(id: i64, code: i64, message: &str) {
    let resp = json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    });
    if let Ok(s) = serde_json::to_string(&resp) {
        let stderr = std::io::stderr();
        let mut out = stderr.lock();
        let _ = writeln!(out, "{}", s);
        let _ = out.flush();
    }
}

fn log_err(msg: &str) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let stderr = std::io::stderr();
    let mut out = stderr.lock();
    let _ = writeln!(out, "[graphiq-mcp {}] {}", ts, msg);
    let _ = out.flush();
}

fn tools_list() -> Value {
    json!({
        "tools": [
            {
                "name": "search",
                "description": "Search the indexed codebase for symbols matching a query. Returns ranked results with file paths, line numbers, symbol kinds, signatures, and source previews.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search query — symbol name, natural language description, file path fragment, or error message"
                        },
                        "top_k": {
                            "type": "integer",
                            "description": "Max results to return (default: 10, max: 50)",
                            "default": 10
                        },
                        "file_filter": {
                            "type": "string",
                            "description": "Optional file path substring to restrict search scope"
                        }
                    },
                    "required": ["query"]
                }
            },
            {
                "name": "blast",
                "description": "Compute blast radius for a symbol — what it affects (forward) and what depends on it (backward). Useful for understanding change impact before modifying code.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "symbol": {
                            "type": "string",
                            "description": "Symbol name to analyze"
                        },
                        "depth": {
                            "type": "integer",
                            "description": "Max traversal depth (default: 3, max: 10)",
                            "default": 3
                        },
                        "direction": {
                            "type": "string",
                            "enum": ["forward", "backward", "both"],
                            "description": "Blast direction (default: both)",
                            "default": "both"
                        }
                    },
                    "required": ["symbol"]
                }
            },
            {
                "name": "context",
                "description": "Get full source context for a symbol — source code, signature, file location, and structural neighborhood (callers, callees, contained members, parents, tests).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "symbol": {
                            "type": "string",
                            "description": "Symbol name to get context for"
                        }
                    },
                    "required": ["symbol"]
                }
            },
            {
                "name": "status",
                "description": "Get indexing status — project root, file count, symbol count, edge count, and database size.",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            },
            {
                "name": "index",
                "description": "(Re)index the project. Call this after significant code changes to update the symbol database. Auto-called on first use if the database is empty.",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            },
            {
                "name": "explain",
                "description": "Explain a symbol's structural role — its evidence-bearing edges, subsystem membership, and how it fits into the graph. Reveals what the graph knows about this symbol.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "symbol": {
                            "type": "string",
                            "description": "Symbol name to explain"
                        }
                    },
                    "required": ["symbol"]
                }
            },
            {
                "name": "topology",
                "description": "Describe the structural topology around a region — motifs, boundary-defining symbols, and evidence clusters. Shows how the graph is wired.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "symbol": {
                            "type": "string",
                            "description": "Symbol name or file path to center the topology on"
                        },
                        "depth": {
                            "type": "integer",
                            "description": "Max traversal depth (default: 2)",
                            "default": 2
                        }
                    },
                    "required": ["symbol"]
                }
            },
            {
                "name": "why",
                "description": "Explain why a search result ranked where it did — the evidence chain, edge types, and structural signals that caused it to appear.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "The search query that produced the result"
                        },
                        "symbol": {
                            "type": "string",
                            "description": "The symbol name from the result to explain"
                        }
                    },
                    "required": ["query", "symbol"]
                }
            },
            {
                "name": "interrogate",
                "description": "Ask a structural question about the codebase. Answers questions about subsystems, boundaries, entry points, error flow, and architectural patterns. Not a symbol search — a structural interrogation.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "question": {
                            "type": "string",
                            "description": "Structural question about the codebase, e.g. 'What are the main subsystems?', 'Where are the entry points?', 'What handles errors?'"
                        },
                        "subsystem": {
                            "type": "string",
                            "description": "Optional: focus on a specific subsystem name"
                        }
                    },
                    "required": ["question"]
                }
            }
        ]
    })
}

fn handle_tool_call(state: &Arc<Mutex<ServerState>>, params: Value) -> Value {
    let tool_name = match params.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => {
            return tool_error("missing tool name in request");
        }
    };

    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

    if !arguments.is_object() {
        return tool_error("arguments must be a JSON object");
    }

    match tool_name {
        "search" => {
            let s = match state.lock() {
                Ok(s) => s,
                Err(e) => return tool_error(&format!("lock error: {e}")),
            };
            tool_search(&s.db, &s.cache, s.cruncher_index.as_ref(), s.holo_index.as_ref(), arguments)
        }
        "blast" => {
            let s = match state.lock() {
                Ok(s) => s,
                Err(e) => return tool_error(&format!("lock error: {e}")),
            };
            tool_blast(&s.db, arguments)
        }
        "context" => {
            let s = match state.lock() {
                Ok(s) => s,
                Err(e) => return tool_error(&format!("lock error: {e}")),
            };
            tool_context(&s.db, &s.cache, arguments)
        }
        "status" => {
            let s = match state.lock() {
                Ok(s) => s,
                Err(e) => return tool_error(&format!("lock error: {e}")),
            };
            tool_status(&s)
        }
        "index" => {
            let mut s = match state.lock() {
                Ok(s) => s,
                Err(e) => return tool_error(&format!("lock error: {e}")),
            };
            match do_index(&mut s) {
                Ok(msg) => tool_ok(msg),
                Err(e) => tool_error(&e),
            }
        }
        "explain" => {
            let s = match state.lock() {
                Ok(s) => s,
                Err(e) => return tool_error(&format!("lock error: {e}")),
            };
            tool_explain(&s.db, arguments)
        }
        "topology" => {
            let s = match state.lock() {
                Ok(s) => s,
                Err(e) => return tool_error(&format!("lock error: {e}")),
            };
            tool_topology(&s.db, arguments)
        }
        "why" => {
            let s = match state.lock() {
                Ok(s) => s,
                Err(e) => return tool_error(&format!("lock error: {e}")),
            };
            tool_why(&s.db, &s.cache, s.cruncher_index.as_ref(), s.holo_index.as_ref(), arguments)
        }
        "interrogate" => {
            let s = match state.lock() {
                Ok(s) => s,
                Err(e) => return tool_error(&format!("lock error: {e}")),
            };
            tool_interrogate(&s.db, arguments)
        }
        _ => tool_error(&format!("unknown tool: {tool_name}")),
    }
}

fn tool_error(message: &str) -> Value {
    json!({
        "content": [{ "type": "text", "text": message }],
        "isError": true
    })
}

fn tool_ok(text: String) -> Value {
    json!({
        "content": [{ "type": "text", "text": text }]
    })
}

fn tool_search(
    db: &graphiq_core::db::GraphDb,
    cache: &graphiq_core::cache::HotCache,
    cruncher_index: Option<&graphiq_core::cruncher::CruncherIndex>,
    holo_index: Option<&graphiq_core::cruncher::HoloIndex>,
    args: Value,
) -> Value {
    let query = match args.get("query").and_then(|v| v.as_str()) {
        Some(q) => q,
        None => return tool_error("missing required parameter: query"),
    };

    if query.trim().is_empty() {
        return tool_error("query must not be empty");
    }

    let top_k = args
        .get("top_k")
        .and_then(|v| v.as_u64())
        .unwrap_or(10)
        .min(50) as usize;
    let file_filter = args.get("file_filter").and_then(|v| v.as_str());

    let mut engine = graphiq_core::search::SearchEngine::new(db, cache);
    if let (Some(ci), Some(hi)) = (cruncher_index, holo_index) {
        engine = engine.with_goober(ci, hi);
    }
    let mut q = graphiq_core::search::SearchQuery::new(query).top_k(top_k);
    if let Some(f) = file_filter {
        q = q.file_filter(f);
    }

    let result = engine.search(&q);

    let mut lines = Vec::new();
    lines.push(format!(
        "Search: \"{}\" ({} results)",
        query,
        result.results.len()
    ));
    lines.push(String::new());

    for (i, scored) in result.results.iter().enumerate() {
        let sym = &scored.symbol;
        let file = scored.file_path.as_deref().unwrap_or("?");
        let line_count = sym.line_end.saturating_sub(sym.line_start);
        lines.push(format!(
            "#{} [{:.2}] {}:{}  {}::{} ({}L)",
            i + 1,
            scored.score,
            file,
            sym.line_start,
            sym.kind.as_str(),
            sym.name,
            line_count,
        ));
        if let Some(ref sig) = sym.signature {
            let short = sig.lines().next().unwrap_or("");
            lines.push(format!("  {}", short));
        }
        let source_lines: Vec<&str> = sym.source.lines().take(3).collect();
        if !source_lines.is_empty() {
            let preview = source_lines.join("\n    ");
            if preview.len() > 200 {
                lines.push(format!("    {}...", &preview[..200]));
            } else {
                lines.push(format!("    {}", preview));
            }
        }
    }

    if result.results.is_empty() {
        lines.push("No results found.".into());
    }

    tool_ok(lines.join("\n"))
}

fn tool_blast(db: &graphiq_core::db::GraphDb, args: Value) -> Value {
    let symbol_name = match args.get("symbol").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return tool_error("missing required parameter: symbol"),
    };

    if symbol_name.trim().is_empty() {
        return tool_error("symbol must not be empty");
    }

    let depth = args
        .get("depth")
        .and_then(|v| v.as_u64())
        .unwrap_or(3)
        .min(10) as usize;
    let direction_str = args
        .get("direction")
        .and_then(|v| v.as_str())
        .unwrap_or("both");

    let candidates = match db.symbols_by_name(symbol_name) {
        Ok(c) => c,
        Err(e) => return tool_error(&format!("database error: {e}")),
    };

    let sym = match candidates.first() {
        Some(s) => s,
        None => return tool_error(&format!("symbol not found: {symbol_name}")),
    };

    if candidates.len() > 1 {
        let names: Vec<String> = candidates
            .iter()
            .take(5)
            .map(|s| format!("  {}::{} ({})", s.kind.as_str(), s.name, s.file_id))
            .collect();
        log_err(&format!(
            "blast: {} matches for '{}', using first:\n{}",
            candidates.len(),
            symbol_name,
            names.join("\n")
        ));
    }

    let direction = match direction_str {
        "forward" | "f" => graphiq_core::edge::BlastDirection::Forward,
        "backward" | "b" => graphiq_core::edge::BlastDirection::Backward,
        _ => graphiq_core::edge::BlastDirection::Both,
    };

    match graphiq_core::blast::compute_blast_radius(db, sym.id, depth, direction, None) {
        Ok(radius) => tool_ok(graphiq_core::blast::format_blast_report(&radius)),
        Err(e) => tool_error(&format!("blast computation failed: {e}")),
    }
}

fn tool_context(
    db: &graphiq_core::db::GraphDb,
    cache: &graphiq_core::cache::HotCache,
    args: Value,
) -> Value {
    let symbol_name = match args.get("symbol").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return tool_error("missing required parameter: symbol"),
    };

    if symbol_name.trim().is_empty() {
        return tool_error("symbol must not be empty");
    }

    let candidates = match db.symbols_by_name(symbol_name) {
        Ok(c) => c,
        Err(e) => return tool_error(&format!("database error: {e}")),
    };

    let sym = match candidates.first() {
        Some(s) => s,
        None => return tool_error(&format!("symbol not found: {symbol_name}")),
    };

    if candidates.len() > 1 {
        log_err(&format!(
            "context: {} matches for '{}', using first (id={})",
            candidates.len(),
            symbol_name,
            sym.id
        ));
    }

    let neighborhood = cache.load_neighborhood(db, sym.id);

    let mut lines = Vec::new();
    lines.push(format!("=== {} ({}) ===", sym.name, sym.kind.as_str()));

    if let Some(ref sig) = sym.signature {
        lines.push(format!("Signature: {}", sig));
    }
    lines.push(format!(
        "Location: line {}-{}",
        sym.line_start, sym.line_end
    ));
    lines.push(String::new());
    lines.push("Source:".into());
    lines.push(sym.source.clone());

    if let Some(n) = neighborhood {
        if !n.callers.is_empty() {
            lines.push(String::new());
            lines.push("Called by:".into());
            for (caller, _) in &n.callers {
                lines.push(format!("  - {}", caller.name));
            }
        }
        if !n.callees.is_empty() {
            lines.push(String::new());
            lines.push("Calls:".into());
            for (callee, _) in &n.callees {
                lines.push(format!("  - {}", callee.name));
            }
        }
        if !n.members.is_empty() {
            lines.push(String::new());
            lines.push("Contains:".into());
            for member in &n.members {
                lines.push(format!("  - {} ({})", member.name, member.kind.as_str()));
            }
        }
        if let Some(ref container) = n.container {
            lines.push(String::new());
            lines.push(format!("Contained in: {}", container.name));
        }
        if !n.parents.is_empty() {
            lines.push(String::new());
            lines.push("Extends/Implements:".into());
            for parent in &n.parents {
                lines.push(format!("  - {}", parent.name));
            }
        }
        if !n.tests.is_empty() {
            lines.push(String::new());
            lines.push("Tested by:".into());
            for test in &n.tests {
                lines.push(format!("  - {}", test.name));
            }
        }
    }

    tool_ok(lines.join("\n"))
}

fn tool_status(state: &ServerState) -> Value {
    match state.db.stats() {
        Ok(stats) => {
            let size = std::fs::metadata(&state.db_path)
                .map(|m| m.len())
                .unwrap_or(0);
            let text = format!(
                "GraphIQ v{}\n  Project:  {}\n  Files: {}\n  Symbols: {}\n  Edges: {}\n  File Edges: {}\n  DB: {}",
                SERVER_VERSION,
                state.project_root.display(),
                stats.files,
                stats.symbols,
                stats.edges,
                stats.file_edges,
                human_bytes(size),
            );
            tool_ok(text)
        }
        Err(e) => tool_error(&format!("database error: {e}")),
    }
}

fn tool_explain(db: &graphiq_core::db::GraphDb, args: Value) -> Value {
    let symbol_name = match args.get("symbol").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return tool_error("missing required parameter: symbol"),
    };

    let candidates = match db.symbols_by_name(symbol_name) {
        Ok(c) => c,
        Err(e) => return tool_error(&format!("database error: {e}")),
    };

    let sym = match candidates.first() {
        Some(s) => s,
        None => return tool_error(&format!("symbol not found: {symbol_name}")),
    };

    let mut lines = Vec::new();
    lines.push(format!("=== {} ({}) ===", sym.name, sym.kind.as_str()));

    if let Some(ref sig) = sym.signature {
        lines.push(format!("Signature: {}", sig));
    }

    let outgoing = db.edges_from(sym.id).unwrap_or_default();
    let incoming = db.edges_to(sym.id).unwrap_or_default();

    let mut direct_count = 0usize;
    let mut boundary_count = 0usize;
    let mut reinforcing_count = 0usize;
    let mut structural_count = 0usize;
    let mut incidental_count = 0usize;

    let mut evidence_lines = Vec::new();
    for edge in &outgoing {
        let ev = parse_evidence_kind(&edge.metadata);
        count_evidence(ev, &mut direct_count, &mut boundary_count, &mut reinforcing_count, &mut structural_count, &mut incidental_count);
        if ev != "incidental" {
            if let Some(t) = db.get_symbol(edge.target_id).ok().flatten() {
                evidence_lines.push(format!("  -> [{}] {} ({}) via {}", ev, t.name, t.kind.as_str(), edge.kind.as_str()));
            }
        }
    }
    for edge in &incoming {
        let ev = parse_evidence_kind(&edge.metadata);
        count_evidence(ev, &mut direct_count, &mut boundary_count, &mut reinforcing_count, &mut structural_count, &mut incidental_count);
        if ev != "incidental" {
            if let Some(s) = db.get_symbol(edge.source_id).ok().flatten() {
                evidence_lines.push(format!("  <- [{}] {} ({}) via {}", ev, s.name, s.kind.as_str(), edge.kind.as_str()));
            }
        }
    }

    let total_edges = outgoing.len() + incoming.len();
    lines.push(format!("\nEvidence profile ({} edges):", total_edges));
    lines.push(format!("  direct: {} | boundary: {} | reinforcing: {} | structural: {} | incidental: {}",
        direct_count, boundary_count, reinforcing_count, structural_count, incidental_count));

    if !evidence_lines.is_empty() {
        lines.push("\nEvidence-bearing edges:".into());
        for el in evidence_lines.iter().take(20) {
            lines.push(el.clone());
        }
        if evidence_lines.len() > 20 {
            lines.push(format!("  ... and {} more", evidence_lines.len() - 20));
        }
    }

    let out_call_count = outgoing.iter().filter(|e| e.kind == graphiq_core::edge::EdgeKind::Calls).count();
    let in_call_count = incoming.iter().filter(|e| e.kind == graphiq_core::edge::EdgeKind::Calls).count();

    if out_call_count >= 5 && in_call_count <= 2 {
        lines.push("\nStructural role: orchestrator (many outgoing calls, few incoming)".into());
    } else if in_call_count >= 5 && out_call_count <= 2 {
        lines.push("\nStructural role: sink / leaf (many incoming calls, few outgoing)".into());
    } else if out_call_count > 0 && in_call_count > 0 {
        lines.push("\nStructural role: connector (bidirectional call flow)".into());
    }

    let cross_module = outgoing.iter().chain(incoming.iter())
        .filter(|e| e.metadata.to_string().contains("\"cross_module\":true"))
        .count();
    if cross_module > 0 {
        lines.push(format!("\nCross-module connections: {} edges cross module boundaries", cross_module));
    }

    tool_ok(lines.join("\n"))
}

fn count_evidence(ev: &str, d: &mut usize, b: &mut usize, r: &mut usize, s: &mut usize, i: &mut usize) {
    match ev {
        "direct" => *d += 1,
        "boundary" => *b += 1,
        "reinforcing" => *r += 1,
        "structural" => *s += 1,
        "incidental" => *i += 1,
        _ => {}
    }
}

fn tool_topology(db: &graphiq_core::db::GraphDb, args: Value) -> Value {
    let symbol_name = match args.get("symbol").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return tool_error("missing required parameter: symbol"),
    };

    let depth = args.get("depth").and_then(|v| v.as_u64()).unwrap_or(2).min(5) as usize;

    let candidates = match db.symbols_by_name(symbol_name) {
        Ok(c) => c,
        Err(e) => return tool_error(&format!("database error: {e}")),
    };

    let sym = match candidates.first() {
        Some(s) => s,
        None => return tool_error(&format!("symbol not found: {symbol_name}")),
    };

    let mut lines = Vec::new();
    lines.push(format!("=== Topology around {} ===", sym.name));

    let mut visited: std::collections::HashSet<i64> = std::collections::HashSet::new();
    let mut queue: std::collections::VecDeque<(i64, usize)> = std::collections::VecDeque::new();
    visited.insert(sym.id);
    queue.push_back((sym.id, 0));

    let mut boundary_symbols: Vec<(String, String, String)> = Vec::new();
    let mut hub_symbols: Vec<(String, usize)> = Vec::new();
    let mut evidence_summary: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    while let Some((sid, d)) = queue.pop_front() {
        if d >= depth { continue; }

        let out = db.edges_from(sid).unwrap_or_default();
        let inc = db.edges_to(sid).unwrap_or_default();
        let out_count = out.len();

        let sym_name = if sid == sym.id {
            sym.name.clone()
        } else {
            match db.get_symbol(sid) {
                Ok(Some(s)) => s.name.clone(),
                _ => continue,
            }
        };

        if out_count >= 5 {
            hub_symbols.push((sym_name.clone(), out_count));
        }

        for edge in &out {
            let ev = parse_evidence_kind(&edge.metadata);
            *evidence_summary.entry(ev.to_string()).or_insert(0) += 1;
            if ev == "boundary" {
                if let Some(t) = db.get_symbol(edge.target_id).ok().flatten() {
                    boundary_symbols.push((t.name, t.kind.as_str().to_string(), edge.kind.as_str().to_string()));
                }
            }
        }
        for edge in &inc {
            let ev = parse_evidence_kind(&edge.metadata);
            *evidence_summary.entry(ev.to_string()).or_insert(0) += 1;
        }

        for edge in &out {
            if visited.insert(edge.target_id) {
                queue.push_back((edge.target_id, d + 1));
            }
        }
        for edge in &inc {
            if visited.insert(edge.source_id) {
                queue.push_back((edge.source_id, d + 1));
            }
        }
    }

    lines.push(format!("\nRegion size: {} symbols (depth={})", visited.len(), depth));

    let total_ev: usize = evidence_summary.values().sum();
    if total_ev > 0 {
        lines.push("\nEvidence distribution:".into());
        for (kind, count) in &evidence_summary {
            lines.push(format!("  {}: {} ({:.0}%)", kind, count, *count as f64 / total_ev as f64 * 100.0));
        }
    }

    if !boundary_symbols.is_empty() {
        lines.push("\nBoundary-defining edges:".into());
        for (name, kind, edge_kind) in boundary_symbols.iter().take(15) {
            lines.push(format!("  {} ({}) via {}", name, kind, edge_kind));
        }
    }

    if !hub_symbols.is_empty() {
        lines.push("\nHub symbols (high out-degree):".into());
        hub_symbols.sort_by(|a, b| b.1.cmp(&a.1));
        for (name, count) in hub_symbols.iter().take(10) {
            lines.push(format!("  {} ({} outgoing)", name, count));
        }
    }

    tool_ok(lines.join("\n"))
}

fn tool_why(
    db: &graphiq_core::db::GraphDb,
    cache: &graphiq_core::cache::HotCache,
    cruncher_index: Option<&graphiq_core::cruncher::CruncherIndex>,
    holo_index: Option<&graphiq_core::cruncher::HoloIndex>,
    args: Value,
) -> Value {
    let query = match args.get("query").and_then(|v| v.as_str()) {
        Some(q) => q,
        None => return tool_error("missing required parameter: query"),
    };
    let symbol_name = match args.get("symbol").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return tool_error("missing required parameter: symbol"),
    };

    let mut engine = graphiq_core::search::SearchEngine::new(db, cache);
    if let (Some(ci), Some(hi)) = (cruncher_index, holo_index) {
        engine = engine.with_goober(ci, hi);
    }
    let q = graphiq_core::search::SearchQuery::new(query).top_k(20);
    let result = engine.search(&q);

    let scored = match result.results.iter().find(|r| r.symbol.name == symbol_name) {
        Some(s) => s,
        None => return tool_error(&format!("'{}' not found in search results for '{}'", symbol_name, query)),
    };

    let sym = &scored.symbol;
    let mut lines = Vec::new();
    lines.push(format!("=== Why did '{}' rank for '{}'? ===", sym.name, query));
    lines.push(format!("Score: {:.4}", scored.score));

    let rank = result.results.iter().position(|r| r.symbol.name == symbol_name).unwrap_or(0);
    if rank < 5 {
        lines.push(format!("Rank: #{} (BM25 seed region)", rank + 1));
    } else {
        lines.push(format!("Rank: #{} (structural expansion)", rank + 1));
    }

    let query_lower = query.to_lowercase();
    let query_terms: Vec<String> = query_lower.split_whitespace().filter(|w| w.len() >= 3).map(|s| s.to_string()).collect();

    let neighborhood = cache.load_neighborhood(db, sym.id);

    if let Some(ref n) = neighborhood {
        let matching_callers: Vec<String> = n.callers.iter()
            .filter(|(s, _)| query_terms.iter().any(|t| s.name.to_lowercase().contains(t.as_str())))
            .map(|(s, _)| s.name.clone())
            .collect();
        if !matching_callers.is_empty() {
            lines.push("\nCalling symbols matching query:".into());
            for name in matching_callers.iter().take(5) {
                lines.push(format!("  <- {} (calls this symbol)", name));
            }
        }

        let matching_callees: Vec<String> = n.callees.iter()
            .filter(|(s, _)| query_terms.iter().any(|t| s.name.to_lowercase().contains(t.as_str())))
            .map(|(s, _)| s.name.clone())
            .collect();
        if !matching_callees.is_empty() {
            lines.push("\nCalled symbols matching query:".into());
            for name in matching_callees.iter().take(5) {
                lines.push(format!("  -> {} (called by this symbol)", name));
            }
        }
    }

    let outgoing = db.edges_from(sym.id).unwrap_or_default();
    let incoming = db.edges_to(sym.id).unwrap_or_default();

    let mut evidence_chain: Vec<String> = Vec::new();
    for edge in &outgoing {
        let ev = parse_evidence_kind(&edge.metadata);
        if ev != "incidental" {
            if let Some(t) = db.get_symbol(edge.target_id).ok().flatten() {
                evidence_chain.push(format!("  -> [{}] {} ({})", ev, t.name, edge.kind.as_str()));
            }
        }
    }
    for edge in &incoming {
        let ev = parse_evidence_kind(&edge.metadata);
        if ev != "incidental" {
            if let Some(s) = db.get_symbol(edge.source_id).ok().flatten() {
                evidence_chain.push(format!("  <- [{}] {} ({})", ev, s.name, edge.kind.as_str()));
            }
        }
    }

    if !evidence_chain.is_empty() {
        lines.push("\nEvidence chain:".into());
        for ec in evidence_chain.iter().take(15) {
            lines.push(ec.clone());
        }
    }

    tool_ok(lines.join("\n"))
}

fn tool_interrogate(db: &graphiq_core::db::GraphDb, args: Value) -> Value {
    let question = match args.get("question").and_then(|v| v.as_str()) {
        Some(q) => q,
        None => return tool_error("missing 'question' parameter"),
    };

    let focus_subsystem = args.get("subsystem").and_then(|v| v.as_str());

    let subsystems = match graphiq_core::subsystems::load_subsystems(db) {
        Ok(s) if !s.subsystems.is_empty() => s,
        _ => {
            match graphiq_core::subsystems::detect_subsystems(db) {
                Ok(s) => s,
                Err(e) => return tool_error(&format!("subsystem detection failed: {e}")),
            }
        }
    };

    let roles_available = db
        .conn()
        .prepare("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='symbol_structural_roles'")
        .ok()
        .and_then(|mut s| s.query_row([], |r| r.get::<_, i64>(0)).ok())
        .map_or(false, |c| c > 0);

    let mut ss: Vec<&graphiq_core::subsystems::Subsystem> = subsystems.subsystems.iter().collect();
    if let Some(focus) = focus_subsystem {
        let focus_lower = focus.to_lowercase();
        let filtered: Vec<&graphiq_core::subsystems::Subsystem> = ss
            .iter()
            .filter(|s| s.name.to_lowercase().contains(&focus_lower))
            .cloned()
            .collect();
        if !filtered.is_empty() {
            ss = filtered;
        }
    }

    ss.sort_by(|a, b| b.symbol_ids.len().cmp(&a.symbol_ids.len()));
    let top = ss.iter().take(15).collect::<Vec<_>>();

    let lower = question.to_lowercase();
    let mut lines: Vec<String> = Vec::new();

    if lower.contains("subsystem") || lower.contains("module") || lower.contains("component") || lower.contains("architecture") {
        let active: Vec<_> = subsystems.subsystems.iter()
            .filter(|s| s.internal_edge_count > 0)
            .collect();
        let active_top: Vec<_> = active.iter().take(20).collect();

        lines.push(format!("Active subsystems ({} with internal edges, {} total):", active.len(), subsystems.subsystems.len()));
        lines.push("".into());
        for s in &active_top {
            let sample: Vec<String> = s.symbol_names.iter().take(5).cloned().collect();
            lines.push(format!(
                "{}: {} symbols, {} internal, {} boundary (cohesion: {:.2})",
                s.name, s.symbol_ids.len(), s.internal_edge_count, s.boundary_edge_count, s.cohesion
            ));
            if !sample.is_empty() {
                lines.push(format!("  key symbols: {}", sample.join(", ")));
            }
        }
        if active.len() > 20 {
            lines.push(format!("  ... and {} more", active.len() - 20));
        }
    }

    if lower.contains("entry point") || lower.contains("entrypoint") || lower.contains("main") || lower.contains("start") {
        if roles_available {
            let conn = db.conn();
            let mut stmt = conn.prepare(
                "SELECT symbol_name, roles, subsystem_id, external_callers, internal_degree
                 FROM symbol_structural_roles
                 WHERE roles LIKE '%entry_point%'
                 ORDER BY external_callers DESC
                 LIMIT 25"
            ).unwrap();
            let rows: Vec<(String, String, i64, i64, i64)> = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)))
                .ok()
                .map(|r| r.flatten().collect())
                .unwrap_or_default();

            lines.push(format!("\nEntry points ({} found, by external caller count):", rows.len()));
            for (name, roles, sub_id, ext_callers, int_deg) in &rows {
                let sub_name = subsystems.subsystems.iter()
                    .find(|s| s.id == *sub_id as usize)
                    .map(|s| s.name.as_str())
                    .unwrap_or("?");
                lines.push(format!("  {} [{}] ({} callers, {} internal calls)", name, sub_name, ext_callers, int_deg));
            }
        } else {
            lines.push("\nRun `graphiq subsystems --roles` to enable entry point detection.".into());
        }
    }

    if lower.contains("error") || lower.contains("fail") || lower.contains("fault") {
        if roles_available {
            let conn = db.conn();
            let mut stmt = conn.prepare(
                "SELECT symbol_name, roles, subsystem_id, internal_degree, boundary_degree
                 FROM symbol_structural_roles
                 WHERE roles LIKE '%boundary%'
                 AND (symbol_name LIKE '%error%' OR symbol_name LIKE '%fail%' OR symbol_name LIKE '%handle%' OR symbol_name LIKE '%catch%' OR symbol_name LIKE '%recover%')
                 ORDER BY boundary_degree DESC
                 LIMIT 25"
            ).unwrap();
            let rows: Vec<(String, String, i64, i64, i64)> = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)))
                .ok()
                .map(|r| r.flatten().collect())
                .unwrap_or_default();

            lines.push(format!("\nError boundary symbols ({} found):", rows.len()));
            for (name, roles, sub_id, int_deg, bnd_deg) in &rows {
                let sub_name = subsystems.subsystems.iter()
                    .find(|s| s.id == *sub_id as usize)
                    .map(|s| s.name.as_str())
                    .unwrap_or("?");
                lines.push(format!("  {} [{}] (roles: {}, {} internal, {} boundary)", name, sub_name, roles, int_deg, bnd_deg));
            }
        }
    }

    if lower.contains("boundary") || lower.contains("boundar") || lower.contains("interface") {
        if roles_available {
            let conn = db.conn();
            let mut stmt = conn.prepare(
                "SELECT symbol_name, subsystem_id, boundary_degree, internal_degree
                 FROM symbol_structural_roles
                 WHERE roles LIKE '%boundary%'
                 ORDER BY boundary_degree DESC
                 LIMIT 25"
            ).unwrap();
            let rows: Vec<(String, i64, i64, i64)> = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
                .ok()
                .map(|r| r.flatten().collect())
                .unwrap_or_default();

            lines.push(format!("\nBoundary symbols ({} with boundary role):", rows.len()));
            for (name, sub_id, bnd_deg, int_deg) in &rows {
                let sub_name = subsystems.subsystems.iter()
                    .find(|s| s.id == *sub_id as usize)
                    .map(|s| s.name.as_str())
                    .unwrap_or("?");
                lines.push(format!("  {} [{}] ({} boundary edges, {} internal)", name, sub_name, bnd_deg, int_deg));
            }
        }
    }

    if lower.contains("role") || lower.contains("structural role") {
        if roles_available {
            let conn = db.conn();
            let role_types = [
                ("entry_point", "Entry points"),
                ("orchestrator", "Orchestrators"),
                ("hub", "Hubs"),
                ("boundary", "Boundary symbols"),
                ("leaf", "Leaves"),
            ];

            for (role_key, role_label) in &role_types {
                let sql = format!(
                    "SELECT COUNT(*) FROM symbol_structural_roles WHERE roles LIKE '%{}%'",
                    role_key
                );
                let count: i64 = conn.prepare(&sql).ok()
                    .and_then(|mut s| s.query_row([], |r| r.get(0)).ok())
                    .unwrap_or(0);
                if count > 0 {
                    lines.push(format!("  {}: {}", role_label, count));
                }
            }
        } else {
            lines.push("\nRun `graphiq subsystems --roles` to enable structural role analysis.".into());
        }
    }

    if lower.contains("orchestrat") || lower.contains("orchestrator") {
        if roles_available {
            let conn = db.conn();
            let mut stmt = conn.prepare(
                "SELECT symbol_name, subsystem_id, internal_degree, external_callers
                 FROM symbol_structural_roles
                 WHERE roles LIKE '%orchestrator%'
                 ORDER BY internal_degree DESC
                 LIMIT 20"
            ).unwrap();
            let rows: Vec<(String, i64, i64, i64)> = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
                .ok()
                .map(|r| r.flatten().collect())
                .unwrap_or_default();

            lines.push(format!("\nOrchestrators ({} found):", rows.len()));
            for (name, sub_id, int_deg, ext_callers) in &rows {
                let sub_name = subsystems.subsystems.iter()
                    .find(|s| s.id == *sub_id as usize)
                    .map(|s| s.name.as_str())
                    .unwrap_or("?");
                lines.push(format!("  {} [{}] ({} internal calls, {} external callers)", name, sub_name, int_deg, ext_callers));
            }
        }
    }

    if lower.contains("cohesion") || lower.contains("coupling") || lower.contains("tight") || lower.contains("loose") {
        let mut sorted: Vec<_> = subsystems.subsystems.iter()
            .filter(|s| s.internal_edge_count > 0)
            .collect();
        sorted.sort_by(|a, b| b.cohesion.partial_cmp(&a.cohesion).unwrap());

        lines.push("\nSubsystem cohesion ranking (highest first):".into());
        for s in sorted.iter().take(10) {
            lines.push(format!("  {:.2} - {} ({} symbols, {} internal, {} boundary)",
                s.cohesion, s.name, s.symbol_ids.len(), s.internal_edge_count, s.boundary_edge_count));
        }
        lines.push("\nLowest cohesion:".into());
        for s in sorted.iter().rev().take(5) {
            lines.push(format!("  {:.2} - {} ({} symbols, {} internal, {} boundary)",
                s.cohesion, s.name, s.symbol_ids.len(), s.internal_edge_count, s.boundary_edge_count));
        }
    }

    if lower.contains("convention") || lower.contains("pattern") || lower.contains("contradiction") {
        if roles_available {
            let conn = db.conn();
            let mut stmt = conn.prepare(
                "SELECT subsystem_id, roles, COUNT(*) as cnt
                 FROM symbol_structural_roles
                 WHERE roles LIKE '%entry_point%' OR roles LIKE '%orchestrator%'
                 GROUP BY subsystem_id
                 ORDER BY cnt DESC
                 LIMIT 10"
            ).unwrap();
            let rows: Vec<(i64, String, i64)> = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                .ok()
                .map(|r| r.flatten().collect())
                .unwrap_or_default();

            lines.push("\nSubsystem leadership (entry points + orchestrators per subsystem):".into());
            for (sub_id, roles, cnt) in &rows {
                let sub_name = subsystems.subsystems.iter()
                    .find(|s| s.id == *sub_id as usize)
                    .map(|s| s.name.as_str())
                    .unwrap_or("?");
                lines.push(format!("  {} [{}]: {} leaders", sub_name, roles, cnt));
            }
        }
    }

    if lines.len() <= 1 || (lines.len() == 2 && lines[1].is_empty()) {
        lines.push(format!("No specific structural pattern matched for: {}", question));
        lines.push("Try: subsystems, entry points, error boundaries, roles, boundary, orchestrators, cohesion, or convention analysis.".into());
    }

    tool_ok(lines.join("\n"))
}

fn parse_evidence_kind(meta: &serde_json::Value) -> &'static str {
    if let Some(kind) = meta.get("evidence").and_then(|e| e.get("kind")).and_then(|k| k.as_str()) {
        match kind {
            "direct" => "direct",
            "boundary" => "boundary",
            "reinforcing" => "reinforcing",
            "structural" => "structural",
            "incidental" => "incidental",
            _ => "unknown",
        }
    } else {
        "unknown"
    }
}

fn human_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}
