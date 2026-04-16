use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde_json::{json, Value};

const SERVER_NAME: &str = "graphiq";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
const PROTOCOL_VERSION: &str = "2024-11-05";

fn main() {
    let db_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| ".graphiq/graphiq.db".into());
    let db_path = PathBuf::from(&db_path);

    let db = match graphiq_core::db::GraphDb::open(&db_path) {
        Ok(d) => d,
        Err(e) => {
            log_err(&format!("failed to open database: {e}"));
            send_error(-1, -32603, &format!("failed to open database: {e}"));
            std::process::exit(1);
        }
    };

    let cache = graphiq_core::cache::HotCache::with_defaults();
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

        if let Err(e) = handle_message(&msg, &db, &cache, &running, &mut stdout) {
            log_err(&format!("handler error: {e}"));
        }
    }
}

fn handle_message(
    msg: &Value,
    db: &graphiq_core::db::GraphDb,
    cache: &graphiq_core::cache::HotCache,
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
            let result = json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {
                    "tools": { "listChanged": false }
                },
                "serverInfo": {
                    "name": SERVER_NAME,
                    "version": SERVER_VERSION
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
            let result = handle_tool_call(db, cache, params);
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
                "description": "Get indexing status — file count, symbol count, edge count, and database size.",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            }
        ]
    })
}

fn handle_tool_call(
    db: &graphiq_core::db::GraphDb,
    cache: &graphiq_core::cache::HotCache,
    params: Value,
) -> Value {
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
        "search" => tool_search(db, cache, arguments),
        "blast" => tool_blast(db, arguments),
        "context" => tool_context(db, cache, arguments),
        "status" => tool_status(db),
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

    let engine = graphiq_core::search::SearchEngine::new(db, cache);
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

fn tool_status(db: &graphiq_core::db::GraphDb) -> Value {
    match db.stats() {
        Ok(stats) => {
            let db_path = std::env::args()
                .nth(1)
                .unwrap_or_else(|| ".graphiq/graphiq.db".into());
            let size = std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);
            let text = format!(
                "GraphIQ v{}\n  Files: {}\n  Symbols: {}\n  Edges: {}\n  File Edges: {}\n  DB Size: {}",
                SERVER_VERSION,
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
