use std::io::{BufRead, Write};
use std::path::PathBuf;

use serde_json::{json, Value};

fn main() {
    let db_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| ".graphiq/graphiq.db".into());
    let db_path = PathBuf::from(&db_path);

    let db = match graphiq_core::db::GraphDb::open(&db_path) {
        Ok(d) => d,
        Err(e) => {
            let _ = send_error(-1, &format!("failed to open database: {e}"));
            std::process::exit(1);
        }
    };

    let cache = graphiq_core::cache::HotCache::with_defaults();
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };

        let msg: Value = match serde_json::from_str(&line) {
            Ok(m) => m,
            Err(_) => continue,
        };

        let id = msg.get("id").and_then(|v| v.as_i64()).unwrap_or(-1);

        let method = match msg.get("method").and_then(|v| v.as_str()) {
            Some(m) => m,
            None => {
                let _ = send_error(id, "missing method");
                continue;
            }
        };

        let result = match method {
            "initialize" => handle_initialize(),
            "initialized" => continue,
            "tools/list" => handle_tools_list(),
            "tools/call" => {
                let params = msg.get("params").cloned().unwrap_or(json!({}));
                handle_tool_call(&db, &cache, params)
            }
            "shutdown" => break,
            _ => json!({}),
        };

        let response = json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        });

        let _ = writeln!(stdout, "{}", serde_json::to_string(&response).unwrap());
        let _ = stdout.flush();
    }
}

fn send_error(id: i64, message: &str) -> serde_json::Result<()> {
    let resp = json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": -32603, "message": message }
    });
    println!("{}", serde_json::to_string(&resp)?);
    Ok(())
}

fn handle_initialize() -> Value {
    json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {
            "tools": { "listChanged": false }
        },
        "serverInfo": {
            "name": "graphiq",
            "version": "0.1.0"
        }
    })
}

fn handle_tools_list() -> Value {
    json!({
        "tools": [
            {
                "name": "search",
                "description": "Search the indexed codebase for symbols matching a query. Returns ranked results with file paths, symbol kinds, and scores.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Search query — symbol name, natural language description, or file path fragment" },
                        "top_k": { "type": "integer", "description": "Max results to return (default: 10)", "default": 10 },
                        "file_filter": { "type": "string", "description": "Optional file path substring filter" }
                    },
                    "required": ["query"]
                }
            },
            {
                "name": "blast",
                "description": "Compute blast radius for a symbol — what it affects (forward) and what it depends on (backward). Useful for understanding change impact.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "symbol": { "type": "string", "description": "Symbol name to analyze" },
                        "depth": { "type": "integer", "description": "Max traversal depth (default: 3)", "default": 3 },
                        "direction": { "type": "string", "enum": ["forward", "backward", "both"], "description": "Blast direction (default: both)", "default": "both" }
                    },
                    "required": ["symbol"]
                }
            },
            {
                "name": "context",
                "description": "Get full source context for a symbol — its source code, signature, and structural neighborhood (callers, callees, contained members).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "symbol": { "type": "string", "description": "Symbol name to get context for" }
                    },
                    "required": ["symbol"]
                }
            },
            {
                "name": "status",
                "description": "Get indexing status — file count, symbol count, edge count, database size.",
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
    let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");

    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

    match tool_name {
        "search" => tool_search(db, cache, arguments),
        "blast" => tool_blast(db, arguments),
        "context" => tool_context(db, cache, arguments),
        "status" => tool_status(db),
        _ => json!({
            "content": [{ "type": "text", "text": format!("unknown tool: {}", tool_name) }],
            "isError": true
        }),
    }
}

fn tool_search(
    db: &graphiq_core::db::GraphDb,
    cache: &graphiq_core::cache::HotCache,
    args: Value,
) -> Value {
    let query = match args.get("query").and_then(|v| v.as_str()) {
        Some(q) => q,
        None => {
            return json!({
                "content": [{ "type": "text", "text": "missing required parameter: query" }],
                "isError": true
            });
        }
    };

    let top_k = args.get("top_k").and_then(|v| v.as_u64()).unwrap_or(10) as usize;
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
            "#{} [{:.2}] {}::{} ({}:{}, {}L) [{}]",
            i + 1,
            scored.score,
            file,
            sym.name,
            sym.line_start,
            sym.kind.as_str(),
            line_count,
            sym.visibility.as_str(),
        ));
        if let Some(ref sig) = sym.signature {
            let short = sig.lines().next().unwrap_or("");
            lines.push(format!("  {}", short));
        }
    }

    if result.results.is_empty() {
        lines.push("No results found.".into());
    }

    json!({
        "content": [{ "type": "text", "text": lines.join("\n") }]
    })
}

fn tool_blast(db: &graphiq_core::db::GraphDb, args: Value) -> Value {
    let symbol_name = match args.get("symbol").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return json!({
                "content": [{ "type": "text", "text": "missing required parameter: symbol" }],
                "isError": true
            });
        }
    };

    let depth = args.get("depth").and_then(|v| v.as_u64()).unwrap_or(3) as usize;
    let direction_str = args
        .get("direction")
        .and_then(|v| v.as_str())
        .unwrap_or("both");

    let candidates = match db.symbols_by_name(symbol_name) {
        Ok(c) => c,
        Err(e) => {
            return json!({
                "content": [{ "type": "text", "text": format!("db error: {e}") }],
                "isError": true
            });
        }
    };

    let sym = match candidates.first() {
        Some(s) => s,
        None => {
            return json!({
                "content": [{ "type": "text", "text": format!("symbol not found: {}", symbol_name) }],
                "isError": true
            });
        }
    };

    let direction = match direction_str {
        "forward" | "f" => graphiq_core::edge::BlastDirection::Forward,
        "backward" | "b" => graphiq_core::edge::BlastDirection::Backward,
        _ => graphiq_core::edge::BlastDirection::Both,
    };

    match graphiq_core::blast::compute_blast_radius(db, sym.id, depth, direction, None) {
        Ok(radius) => {
            let report = graphiq_core::blast::format_blast_report(&radius);
            json!({
                "content": [{ "type": "text", "text": report }]
            })
        }
        Err(e) => json!({
            "content": [{ "type": "text", "text": format!("error: {e}") }],
            "isError": true
        }),
    }
}

fn tool_context(
    db: &graphiq_core::db::GraphDb,
    cache: &graphiq_core::cache::HotCache,
    args: Value,
) -> Value {
    let symbol_name = match args.get("symbol").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return json!({
                "content": [{ "type": "text", "text": "missing required parameter: symbol" }],
                "isError": true
            });
        }
    };

    let candidates = match db.symbols_by_name(symbol_name) {
        Ok(c) => c,
        Err(e) => {
            return json!({
                "content": [{ "type": "text", "text": format!("db error: {e}") }],
                "isError": true
            });
        }
    };

    let sym = match candidates.first() {
        Some(s) => s,
        None => {
            return json!({
                "content": [{ "type": "text", "text": format!("symbol not found: {}", symbol_name) }],
                "isError": true
            });
        }
    };

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

    json!({
        "content": [{ "type": "text", "text": lines.join("\n") }]
    })
}

fn tool_status(db: &graphiq_core::db::GraphDb) -> Value {
    match db.stats() {
        Ok(stats) => {
            let text = format!(
                "GraphIQ Status\n  Schema: v{}\n  Files: {}\n  Symbols: {}\n  Edges: {}\n  File Edges: {}",
                stats.schema_version, stats.files, stats.symbols, stats.edges, stats.file_edges
            );
            json!({
                "content": [{ "type": "text", "text": text }]
            })
        }
        Err(e) => json!({
            "content": [{ "type": "text", "text": format!("error: {e}") }],
            "isError": true
        }),
    }
}
