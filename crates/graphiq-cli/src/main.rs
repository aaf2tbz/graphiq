use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "graphiq",
    about = "Code intelligence with structural retrieval"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Index {
        #[arg(value_name = "PATH")]
        path: PathBuf,
        #[arg(long, default_value = ".graphiq/graphiq.db")]
        db: PathBuf,
        #[cfg(feature = "embed")]
        #[arg(long)]
        embed: bool,
    },
    Search {
        query: String,
        #[arg(long, default_value = ".graphiq/graphiq.db")]
        db: PathBuf,
        #[arg(short, long, default_value_t = 10)]
        top: usize,
        #[arg(long)]
        debug: bool,
        #[arg(long)]
        file: Option<String>,
        #[arg(long)]
        blast: bool,
        #[arg(short, long, default_value_t = 3)]
        depth: usize,
    },
    Blast {
        symbol: String,
        #[arg(long, default_value = ".graphiq/graphiq.db")]
        db: PathBuf,
        #[arg(short, long, default_value_t = 3)]
        depth: usize,
        #[arg(long, default_value = "both")]
        direction: String,
    },
    Status {
        #[arg(long, default_value = ".graphiq/graphiq.db")]
        db: PathBuf,
    },
    Reindex {
        #[arg(value_name = "PATH")]
        path: PathBuf,
        #[arg(long, default_value = ".graphiq/graphiq.db")]
        db: PathBuf,
    },
    Lsa {
        #[arg(long, default_value = ".graphiq/graphiq.db")]
        db: PathBuf,
    },
    Spectral {
        #[arg(long, default_value = ".graphiq/graphiq.db")]
        db: PathBuf,
    },
    Subsystems {
        #[arg(long, default_value = ".graphiq/graphiq.db")]
        db: PathBuf,
        #[arg(long)]
        roles: bool,
    },
    Roles {
        #[arg(long, default_value = ".graphiq/graphiq.db")]
        db: PathBuf,
        #[arg(long)]
        subsystem: Option<usize>,
        #[arg(short, long, default_value_t = 30)]
        top: usize,
    },
    Demo,
    Setup {
        #[arg(long, value_name = "PATH")]
        project: Option<PathBuf>,
        #[arg(long)]
        skip_index: bool,
    },
    #[cfg(feature = "embed")]
    EmbedTest {
        text: Option<String>,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        #[cfg(not(feature = "embed"))]
        Commands::Index { path, db, .. } => cmd_index(&path, &db, false),
        #[cfg(feature = "embed")]
        Commands::Index {
            path, db, embed, ..
        } => cmd_index(&path, &db, embed),

        Commands::Search {
            query,
            db,
            top,
            debug,
            file,
            blast,
            depth,
        } => cmd_search(&query, &db, top, debug, file.as_deref(), blast, depth),
        Commands::Blast {
            symbol,
            db,
            depth,
            direction,
        } => cmd_blast(&symbol, &db, depth, &direction),
        Commands::Status { db } => cmd_status(&db),
        Commands::Reindex { path, db } => cmd_reindex(&path, &db),
        Commands::Lsa { db } => cmd_lsa(&db),
        Commands::Spectral { db } => cmd_spectral(&db),
        Commands::Subsystems { db, roles } => cmd_subsystems(&db, roles),
        Commands::Roles { db, subsystem, top } => cmd_roles(&db, subsystem, top),
        Commands::Demo => cmd_demo(),
        Commands::Setup {
            project,
            skip_index,
        } => cmd_setup(project.as_deref(), skip_index),
        #[cfg(feature = "embed")]
        Commands::EmbedTest { text } => cmd_embed_test(text.as_deref().unwrap_or("hello world")),
    }
}

fn cmd_index(path: &std::path::Path, db_path: &std::path::Path, do_embed: bool) {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }

    let db = match graphiq_core::db::GraphDb::open(db_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error opening database: {e}");
            std::process::exit(1);
        }
    };

    print!("Indexing {} ... ", path.display());
    let indexer = graphiq_core::index::Indexer::new(&db);
    match indexer.index_project(path) {
        Ok(stats) => {
            println!("done");
            println!(
                "  Files: {}  Symbols: {}  Imports: {}  Calls: {}  Edges: {}",
                stats.files_indexed,
                stats.symbols_indexed,
                stats.imports_extracted,
                stats.calls_extracted,
                stats.edges_inserted
            );
        }
        Err(e) => {
            println!("failed");
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }

    if do_embed {
        #[cfg(feature = "embed")]
        {
            eprintln!("Embedding symbols ...");
            match indexer.embed_symbols(None) {
                Ok(count) => eprintln!("  done ({} symbols embedded)", count),
                Err(e) => {
                    println!("failed");
                    eprintln!("embed error: {e}");
                }
            }
        }
        #[cfg(not(feature = "embed"))]
        {
            eprintln!("embed feature not enabled — rebuild with --features embed");
        }
    }
}

fn cmd_search(
    query: &str,
    db_path: &std::path::Path,
    top_k: usize,
    debug: bool,
    file_filter: Option<&str>,
    with_blast: bool,
    blast_depth: usize,
) {
    if !db_path.exists() {
        eprintln!("database not found: {}", db_path.display());
        eprintln!("run `graphiq index <path>` first");
        std::process::exit(1);
    }

    let db = match graphiq_core::db::GraphDb::open(db_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error opening database: {e}");
            std::process::exit(1);
        }
    };

    let cache = graphiq_core::cache::HotCache::with_defaults();
    cache.prewarm(&db, 200);

    let goober = graphiq_core::cruncher::build_cruncher_index(&db).ok().map(|ci| {
        let hi = graphiq_core::cruncher::build_holo_index(&db, &ci);
        (ci, hi)
    });

    let mut engine = graphiq_core::search::SearchEngine::new(&db, &cache);
    if let Some((ref ci, ref hi)) = goober {
        engine = engine.with_goober(ci, hi);
    }

    let mut q = graphiq_core::search::SearchQuery::new(query)
        .top_k(top_k)
        .debug(debug);

    if let Some(f) = file_filter {
        q = q.file_filter(f);
    }
    if with_blast {
        q = q.with_blast(blast_depth);
    }

    let result = engine.search(&q);

    if result.from_cache {
        eprintln!("(cached)");
    }

    for (i, scored) in result.results.iter().enumerate() {
        let sym = &scored.symbol;
        let file = scored.file_path.as_deref().unwrap_or("?");
        let kind = sym.kind.as_str();

        println!(
            "#{:<3} {:.3}  {}:{}  {}::{}",
            i + 1,
            scored.score,
            file,
            sym.line_start,
            kind,
            sym.name,
        );

        if let Some(ref sig) = sym.signature {
            let short = sig.lines().next().unwrap_or("");
            if short.len() > 100 {
                println!("     {}", &short[..100]);
            } else {
                println!("     {}", short);
            }
        }

        if debug {
            if let Some(ref bd) = scored.breakdown {
                println!(
                    "     layer2={:.3}  path_w={:.2}  diversity={:.2}",
                    bd.layer2_score, bd.path_weight, bd.diversity_dampen
                );
                print!("     heuristics:");
                for (name, val) in &bd.heuristics {
                    print!(" {}={:.2}", name, val);
                }
                println!();
            }
        }
    }

    if result.results.is_empty() {
        println!("No results for \"{}\"", query);
    }

    if let Some(ref blast) = result.blast_radius {
        println!();
        println!("{}", graphiq_core::blast::format_blast_report(blast));
    }
}

fn cmd_blast(symbol_name: &str, db_path: &std::path::Path, depth: usize, direction: &str) {
    if !db_path.exists() {
        eprintln!("database not found: {}", db_path.display());
        std::process::exit(1);
    }

    let db = match graphiq_core::db::GraphDb::open(db_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error opening database: {e}");
            std::process::exit(1);
        }
    };

    let candidates = db.symbols_by_name(symbol_name).unwrap_or_default();
    let sym = match candidates.first() {
        Some(s) => s,
        None => {
            eprintln!("symbol not found: {}", symbol_name);
            std::process::exit(1);
        }
    };

    if candidates.len() > 1 {
        eprintln!(
            "Found {} symbols named '{}', using first (id={})",
            candidates.len(),
            symbol_name,
            sym.id
        );
    }

    let dir = match direction {
        "forward" | "f" => graphiq_core::edge::BlastDirection::Forward,
        "backward" | "b" => graphiq_core::edge::BlastDirection::Backward,
        _ => graphiq_core::edge::BlastDirection::Both,
    };

    match graphiq_core::blast::compute_blast_radius(&db, sym.id, depth, dir, None) {
        Ok(radius) => println!("{}", graphiq_core::blast::format_blast_report(&radius)),
        Err(e) => {
            eprintln!("error computing blast radius: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_status(db_path: &std::path::Path) {
    if !db_path.exists() {
        eprintln!("database not found: {}", db_path.display());
        std::process::exit(1);
    }

    match graphiq_core::db::GraphDb::open(db_path) {
        Ok(gdb) => {
            let stats = gdb.stats().unwrap();
            println!("GraphIQ Status");
            println!("  Schema:      v{}", stats.schema_version);
            println!("  Files:       {}", stats.files);
            println!("  Symbols:     {}", stats.symbols);
            println!("  Edges:       {}", stats.edges);
            println!("  File Edges:  {}", stats.file_edges);
            println!(
                "  DB Size:     {}",
                human_bytes(std::fs::metadata(db_path).map(|m| m.len()).unwrap_or(0))
            );
        }
        Err(e) => {
            eprintln!("error opening database: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_reindex(path: &std::path::Path, db_path: &std::path::Path) {
    if !db_path.exists() {
        eprintln!("database not found: {}", db_path.display());
        eprintln!("run `graphiq index` first to create the database");
        std::process::exit(1);
    }

    let db = match graphiq_core::db::GraphDb::open(db_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error opening database: {e}");
            std::process::exit(1);
        }
    };

    print!("Reindexing {} ... ", path.display());
    let indexer = graphiq_core::index::Indexer::new(&db);
    match indexer.index_project(path) {
        Ok(stats) => {
            println!("done");
            println!(
                "  Files: {}  Symbols: {}  Imports: {}  Calls: {}  Edges: {}",
                stats.files_indexed,
                stats.symbols_indexed,
                stats.imports_extracted,
                stats.calls_extracted,
                stats.edges_inserted
            );
        }
        Err(e) => {
            println!("failed");
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_lsa(db_path: &std::path::Path) {
    if !db_path.exists() {
        eprintln!("database not found: {}", db_path.display());
        eprintln!("run `graphiq index` first to create the database");
        std::process::exit(1);
    }

    let db = match graphiq_core::db::GraphDb::open(db_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error opening database: {e}");
            std::process::exit(1);
        }
    };

    eprintln!("Computing LSA (anisotropic hypersphere)...");
    let lsa = match graphiq_core::lsa::compute_lsa(&db) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("LSA computation failed: {e}");
            std::process::exit(1);
        }
    };

    eprintln!("Storing LSA vectors...");
    match graphiq_core::lsa::store_lsa_vectors(&db, &lsa.symbol_ids, &lsa.symbol_vecs) {
        Ok(n) => eprintln!("  {} symbol vectors stored", n),
        Err(e) => eprintln!("  vector store failed: {e}"),
    }

    match graphiq_core::lsa::store_lsa_basis(&db, &lsa.term_basis, &lsa.term_index, &lsa.term_idf) {
        Ok(()) => eprintln!("  {} term basis vectors stored", lsa.term_index.len()),
        Err(e) => eprintln!("  basis store failed: {e}"),
    }

    match graphiq_core::lsa::store_lsa_sigma(&db, &lsa.singular_values) {
        Ok(()) => eprintln!("  {} singular values stored", lsa.singular_values.len()),
        Err(e) => eprintln!("  sigma store failed: {e}"),
    }

    match graphiq_core::lsa::store_anisotropy_weights(&db, &lsa.anisotropy_weights) {
        Ok(()) => eprintln!("  {} anisotropy weights stored", lsa.anisotropy_weights.len()),
        Err(e) => eprintln!("  anisotropy store failed: {e}"),
    }

    eprintln!("LSA done.");
}

fn cmd_spectral(db_path: &std::path::Path) {
    if !db_path.exists() {
        eprintln!("database not found: {}", db_path.display());
        eprintln!("run `graphiq index` first to create the database");
        std::process::exit(1);
    }

    let db = match graphiq_core::db::GraphDb::open(db_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error opening database: {e}");
            std::process::exit(1);
        }
    };

    eprintln!("Computing spectral embedding (k={})...", graphiq_core::spectral::SPECTRAL_DIM);
    let index = match graphiq_core::spectral::compute_spectral(&db) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("spectral computation failed: {e}");
            std::process::exit(1);
        }
    };

    eprintln!("Storing spectral coords and eigenvalues...");
    match graphiq_core::spectral::store_spectral_coords(
        &db,
        &index.symbol_ids,
        &index.symbol_coords,
        &index.eigenvalues,
        index.lambda_max,
    ) {
        Ok(n) => eprintln!("  {} symbol coords stored", n),
        Err(e) => eprintln!("  store failed: {e}"),
    }

    eprintln!("  {} eigenvectors, lambda_max = {:.6}", index.eigenvalues.len(), index.lambda_max);
    eprintln!("Spectral done.");
}

fn cmd_subsystems(db_path: &std::path::Path, compute_roles: bool) {
    if !db_path.exists() {
        eprintln!("database not found: {}", db_path.display());
        std::process::exit(1);
    }

    let db = match graphiq_core::db::GraphDb::open(db_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error opening database: {e}");
            std::process::exit(1);
        }
    };

    eprintln!("Detecting subsystems...");
    let index = match graphiq_core::subsystems::detect_subsystems(&db) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("subsystem detection failed: {e}");
            std::process::exit(1);
        }
    };

    eprintln!("Storing subsystems...");
    if let Err(e) = graphiq_core::subsystems::store_subsystems(&db, &index) {
        eprintln!("store failed: {e}");
    }

    if compute_roles {
        eprintln!("Materializing structural roles...");
        let roles = match graphiq_core::subsystems::materialize_structural_roles(&db, &index) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("role materialization failed: {e}");
                std::process::exit(1);
            }
        };
        eprintln!("Storing structural roles ({} symbols)...", roles.len());
        if let Err(e) = graphiq_core::subsystems::store_structural_roles(&db, &roles) {
            eprintln!("role store failed: {e}");
        }
    }

    let mut sorted: Vec<&graphiq_core::subsystems::Subsystem> = index.subsystems.iter().collect();
    sorted.sort_by(|a, b| b.cohesion.partial_cmp(&a.cohesion).unwrap());

    println!("\n=== Subsystems ({}) ===\n", index.subsystems.len());
    println!("{:<40} {:>6} {:>10} {:>10} {:>8}", "Name", "Symbols", "Internal", "Boundary", "Cohesion");
    println!("{}", "-".repeat(78));
    for s in sorted.iter().take(30) {
        println!(
            "{:<40} {:>6} {:>10} {:>10} {:>8.2}",
            s.name, s.symbol_ids.len(), s.internal_edge_count, s.boundary_edge_count, s.cohesion
        );
    }
}

fn cmd_roles(db_path: &std::path::Path, subsystem_filter: Option<usize>, top: usize) {
    use graphiq_core::subsystems::StructuralRole;

    if !db_path.exists() {
        eprintln!("database not found: {}", db_path.display());
        std::process::exit(1);
    }

    let db = match graphiq_core::db::GraphDb::open(db_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error opening database: {e}");
            std::process::exit(1);
        }
    };

    let table_exists: bool = db
        .conn()
        .prepare("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='symbol_structural_roles'")
        .unwrap()
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap() > 0;

    if !table_exists {
        eprintln!("No structural roles found. Run `graphiq subsystems --roles` first.");
        std::process::exit(1);
    }

    let query = if let Some(sub_id) = subsystem_filter {
        format!("SELECT symbol_name, roles, subsystem_id, internal_degree, boundary_degree, external_callers, external_callees FROM symbol_structural_roles WHERE subsystem_id = {} ORDER BY external_callers DESC, internal_degree DESC LIMIT {}", sub_id, top)
    } else {
        format!("SELECT symbol_name, roles, subsystem_id, internal_degree, boundary_degree, external_callers, external_callees FROM symbol_structural_roles ORDER BY external_callers DESC, internal_degree DESC LIMIT {}", top)
    };

    let conn = db.conn();
    let mut stmt = conn.prepare(&query).unwrap();
    let rows: Vec<(String, String, i64, i64, i64, i64, i64)> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?, row.get(1)?, row.get(2)?,
                row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?,
            ))
        })
        .unwrap()
        .flatten()
        .collect();

    println!("\n=== Structural Roles (top {}) ===\n", rows.len().min(top));
    println!("{:<45} {:<30} {:>8} {:>6} {:>6} {:>5} {:>5}", "Symbol", "Roles", "Subsystem", "IntDeg", "BndDeg", "ExtIn", "ExtOut");
    println!("{}", "-".repeat(112));

    for (name, roles_str, sub_id, int_deg, bnd_deg, ext_in, ext_out) in &rows {
        let role_icons: Vec<String> = roles_str
            .split(',')
            .filter_map(|r| match r.trim() {
                "entry_point" => Some("EP".to_string()),
                "orchestrator" => Some("ORC".to_string()),
                "hub" => Some("HUB".to_string()),
                "boundary" => Some("BND".to_string()),
                "leaf" => Some("LEAF".to_string()),
                _ => None,
            })
            .collect();
        println!(
            "{:<45} {:<30} {:>8} {:>6} {:>6} {:>5} {:>5}",
            name,
            role_icons.join(", "),
            sub_id,
            int_deg,
            bnd_deg,
            ext_in,
            ext_out,
        );
    }
}

fn cmd_setup(project: Option<&std::path::Path>, skip_index: bool) {
    use serde_json::{json, Value};

    fn pretty(v: &Value) -> String {
        serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
    }

    println!("╭──────────────────────────────────────────────╮");
    println!("│            GraphIQ Setup                      │");
    println!("╰──────────────────────────────────────────────╯");
    println!();

    let project_path = match project {
        Some(p) => {
            let resolved = if p.is_absolute() {
                p.to_path_buf()
            } else {
                match std::env::current_dir() {
                    Ok(cwd) => cwd.join(p),
                    Err(_) => p.to_path_buf(),
                }
            };
            let resolved = resolved.canonicalize().unwrap_or(resolved);
            if !resolved.join(".git").exists() {
                eprintln!("  warning: {} is not a git repository", resolved.display());
            }
            resolved
        }
        None => match std::env::current_dir() {
            Ok(cwd) => {
                let mut candidate = cwd.as_path().canonicalize().unwrap_or_else(|_| cwd.clone());
                loop {
                    if candidate.join(".git").exists() {
                        break candidate;
                    }
                    if !candidate.pop() {
                        break cwd;
                    }
                }
            }
            Err(_) => {
                eprintln!("  error: cannot determine current directory");
                std::process::exit(1);
            }
        },
    };

    println!("  Project: {}", project_path.display());
    println!();

    let mut configured: Vec<String> = Vec::new();
    let graphiq_bin = which_graphiq();

    let claude_config =
        dirs::config_dir().map(|d| d.join("Claude").join("claude_desktop_config.json"));

    if let Some(ref config_path) = claude_config {
        if config_path.exists() || config_path.parent().map_or(false, |p| p.exists()) {
            let project_str = project_path.display().to_string();
            let entry = json!({
                "command": "graphiq-mcp",
                "args": [project_str]
            });

            let (config, written) = if config_path.exists() {
                match std::fs::read_to_string(config_path) {
                    Ok(content) => {
                        let mut parsed: Value = serde_json::from_str(&content).unwrap_or(json!({}));
                        let servers = parsed
                            .as_object_mut()
                            .unwrap()
                            .entry("mcpServers")
                            .or_insert_with(|| json!({}))
                            .as_object_mut()
                            .unwrap();
                        let already = servers
                            .get("graphiq")
                            .and_then(|v| v.get("args"))
                            .and_then(|a| a.as_array())
                            .and_then(|arr| arr.first())
                            .and_then(|v| v.as_str())
                            .map_or(false, |s| s == project_str);
                        if already {
                            servers.insert("graphiq".into(), entry);
                            (pretty(&parsed), false)
                        } else {
                            servers.insert("graphiq".into(), entry);
                            (pretty(&parsed), true)
                        }
                    }
                    Err(_) => {
                        let obj = json!({"mcpServers": {"graphiq": entry}});
                        (pretty(&obj), true)
                    }
                }
            } else {
                let obj = json!({"mcpServers": {"graphiq": entry}});
                (pretty(&obj), true)
            };

            if let Some(parent) = config_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            match std::fs::write(config_path, &config) {
                Ok(()) => {
                    let status = if written { "configured" } else { "updated" };
                    println!("  Claude Desktop: {} {}", status, config_path.display());
                    configured.push("Claude Desktop".to_string());
                }
                Err(e) => {
                    eprintln!("  Claude Desktop: failed to write config: {e}");
                }
            }
        }
    }

    let opencode_config =
        dirs::home_dir().map(|d| d.join(".config").join("opencode").join("opencode.json"));

    if let Some(ref config_path) = opencode_config {
        let project_str = project_path.display().to_string();
        let entry = json!({
            "type": "local",
            "command": ["graphiq-mcp", project_str],
            "enabled": true
        });

        let (config, written) = if config_path.exists() {
            match std::fs::read_to_string(config_path) {
                Ok(content) => {
                    let mut parsed: Value = serde_json::from_str(&content).unwrap_or(json!({}));
                    let mcp = parsed
                        .as_object_mut()
                        .unwrap()
                        .entry("mcp")
                        .or_insert_with(|| json!({}))
                        .as_object_mut()
                        .unwrap();
                    let already = mcp
                        .get("graphiq")
                        .and_then(|v| v.get("command"))
                        .and_then(|a| a.as_array())
                        .and_then(|arr| arr.get(1))
                        .and_then(|v| v.as_str())
                        .map_or(false, |s| s == project_str);
                    mcp.insert("graphiq".into(), entry);
                    (pretty(&parsed), !already)
                }
                Err(_) => {
                    let obj = json!({"mcp": {"graphiq": entry}});
                    (pretty(&obj), true)
                }
            }
        } else {
            let obj = json!({"mcp": {"graphiq": entry}});
            (pretty(&obj), true)
        };

        if let Some(parent) = config_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::write(config_path, &config) {
            Ok(()) => {
                let status = if written { "configured" } else { "updated" };
                println!("  opencode:      {} {}", status, config_path.display());
                configured.push("opencode".to_string());
            }
            Err(e) => {
                eprintln!("  opencode:      failed to write config: {e}");
            }
        }
    }

    let codex_config = dirs::home_dir().map(|d| d.join(".codex").join("config.toml"));

    if let Some(ref config_path) = codex_config {
        let project_str = project_path.display().to_string();

        let (content, written) = if config_path.exists() {
            match std::fs::read_to_string(config_path) {
                Ok(existing) => {
                    let already = existing.contains("[mcp_servers.graphiq]")
                        && existing.contains(&project_str);
                    if already {
                        (existing, false)
                    } else {
                        let mut cleaned = existing;
                        let section = format!(
                            "\n[mcp_servers.graphiq]\ncommand = \"graphiq-mcp\"\nargs = [\"{}\"]\nenabled = true\n",
                            project_str
                        );
                        cleaned.push_str(&section);
                        (cleaned, true)
                    }
                }
                Err(e) => {
                    eprintln!("  Codex:         failed to read config: {e}");
                    return;
                }
            }
        } else {
            let section = format!(
                "[mcp_servers.graphiq]\ncommand = \"graphiq-mcp\"\nargs = [\"{}\"]\nenabled = true\n",
                project_str
            );
            (section, true)
        };

        match std::fs::write(config_path, &content) {
            Ok(()) => {
                let status = if written { "configured" } else { "updated" };
                println!("  Codex:         {} {}", status, config_path.display());
                configured.push("Codex".to_string());
            }
            Err(e) => {
                eprintln!("  Codex:         failed to write config: {e}");
            }
        }
    }

    let hermes_config = dirs::home_dir().map(|d| d.join(".hermes").join("config.yaml"));

    if let Some(ref config_path) = hermes_config {
        let project_str = project_path.display().to_string();

        let (content, written) = if config_path.exists() {
            match std::fs::read_to_string(config_path) {
                Ok(existing) => {
                    let has_graphiq =
                        existing.contains("mcp_servers:") && existing.contains("graphiq:");
                    if has_graphiq {
                        let updated = regex::Regex::new(
                            r"(?m)^(mcp_servers:\n(\s+graphiq:.*?)(?=\n\n|\n[a-z_]+:|\z))"
                        )
                        .map(|re| {
                            let replacement = format!(
                                "mcp_servers:\n  graphiq:\n    command: graphiq-mcp\n    args:\n      - {}\
                                \n    enabled: true",
                                project_str
                            );
                            re.replace(&existing, replacement.as_str()).to_string()
                        })
                        .unwrap_or_else(|_| existing.clone());
                        (updated, false)
                    } else {
                        let section = format!(
                            "\nmcp_servers:\n  graphiq:\n    command: graphiq-mcp\n    args:\n      - {}\n    enabled: true\n",
                            project_str
                        );
                        let mut out = existing;
                        out.push_str(&section);
                        (out, true)
                    }
                }
                Err(e) => {
                    eprintln!("  Hermes:        failed to read config: {e}");
                    return;
                }
            }
        } else {
            let section = format!(
                "mcp_servers:\n  graphiq:\n    command: graphiq-mcp\n    args:\n      - {}\n    enabled: true\n",
                project_str
            );
            (section, true)
        };

        match std::fs::write(config_path, &content) {
            Ok(()) => {
                let status = if written { "configured" } else { "updated" };
                println!("  Hermes:        {} {}", status, config_path.display());
                configured.push("Hermes".to_string());
            }
            Err(e) => {
                eprintln!("  Hermes:        failed to write config: {e}");
            }
        }
    }

    if configured.is_empty() {
        println!("  No harness configs found to update.");
        println!("  You can manually configure graphiq-mcp as an MCP server:");
        println!("    graphiq-mcp {}", project_path.display());
    }

    println!();

    if !skip_index {
        let db_path = project_path.join(".graphiq").join("graphiq.db");
        if let Some(parent) = db_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        if db_path.exists() {
            let _ = std::fs::remove_file(&db_path);
        }

        let db = match graphiq_core::db::GraphDb::open(&db_path) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("  error opening database: {e}");
                std::process::exit(1);
            }
        };

        print!("  Indexing {} ... ", project_path.display());
        let indexer = graphiq_core::index::Indexer::new(&db);
        match indexer.index_project(&project_path) {
            Ok(stats) => {
                println!("done");
                println!(
                    "    {} files, {} symbols, {} edges",
                    stats.files_indexed, stats.symbols_indexed, stats.edges_inserted
                );
            }
            Err(e) => {
                println!("failed");
                eprintln!("  index error: {e}");
            }
        }
    } else {
        println!("  Skipping index (--skip-index)");
    }

    println!();
    println!("── Ready ──");
    println!();

    if !configured.is_empty() {
        println!("  GraphIQ is configured for: {}", configured.join(", "));
        println!("  Restart your harness(es) to pick up the new MCP server.");
    }

    println!();
    println!("  Try it:");
    println!(
        "    graphiq search \"rate limit middleware\" --db {}/.graphiq/graphiq.db",
        project_path.display()
    );
    println!(
        "    graphiq blast RateLimiter --db {}/.graphiq/graphiq.db",
        project_path.display()
    );
    println!("    graphiq demo");

    if let Some(ref bin_path) = graphiq_bin {
        println!();
        println!("  MCP server: {} <project>", bin_path.display());
        println!("  Installed at: {}", bin_path.display());
    }

    println!();
}

fn which_graphiq() -> Option<PathBuf> {
    let graphiq_mcp = std::env::current_exe().ok()?;
    let bin_name = graphiq_mcp.file_name()?.to_str()?.to_string();
    if bin_name == "graphiq" {
        let mut p = graphiq_mcp.clone();
        p.set_file_name("graphiq-mcp");
        if p.exists() {
            return Some(p);
        }
        if let Some(parent) = graphiq_mcp.parent() {
            let alt = parent.join("graphiq-mcp");
            if alt.exists() {
                return Some(alt);
            }
        }
    }
    None
}

fn cmd_demo() {
    use std::time::Instant;

    let tmp = std::env::temp_dir().join("graphiq-demo");
    if tmp.exists() {
        let _ = std::fs::remove_dir_all(&tmp);
    }
    let _ = std::fs::create_dir_all(&tmp.join("src"));
    let _ = std::fs::create_dir_all(&tmp.join("tests"));
    let _ = std::fs::create_dir_all(&tmp.join("src/main/java/com/demo"));
    let _ = std::fs::create_dir_all(&tmp.join("lib"));

    std::fs::write(
        tmp.join("src/lib.rs"),
        r#"pub mod auth;
pub mod middleware;
pub mod routes;
pub mod db;

pub struct AppConfig {
    pub host: String,
    pub port: u16,
    pub database_url: String,
}

impl AppConfig {
    pub fn from_env() -> Self {
        Self {
            host: std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".into()),
            port: std::env::var("PORT").unwrap_or_else(|_| "8080".to_string()).parse().unwrap_or(8080),
            database_url: std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:app.db".into()),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.port == 0 {
            return Err("port must be non-zero".into());
        }
        if self.database_url.is_empty() {
            return Err("database_url is required".into());
        }
        Ok(())
    }
}
"#,
    )
    .unwrap();

    std::fs::write(
        tmp.join("src/auth.rs"),
        r#"use crate::db::DatabasePool;

pub struct AuthService {
    pool: DatabasePool,
    token_ttl: u64,
}

#[derive(Debug)]
pub struct AuthError {
    pub kind: AuthErrorKind,
    pub message: String,
}

#[derive(Debug)]
pub enum AuthErrorKind {
    InvalidToken,
    ExpiredToken,
    MissingCredentials,
    RateLimited,
}

impl AuthService {
    pub fn new(pool: DatabasePool) -> Self {
        Self {
            pool,
            token_ttl: 3600,
        }
    }

    pub fn authenticate(&self, username: &str, password: &str) -> Result<String, AuthError> {
        if username.is_empty() || password.is_empty() {
            return Err(AuthError {
                kind: AuthErrorKind::MissingCredentials,
                message: "username and password are required".into(),
            });
        }

        let user = self.pool.find_user(username)
            .ok_or_else(|| AuthError {
                kind: AuthErrorKind::InvalidToken,
                message: format!("user not found: {}", username),
            })?;

        if !verify_password(password, &user.password_hash) {
            return Err(AuthError {
                kind: AuthErrorKind::InvalidToken,
                message: "invalid password".into(),
            });
        }

        Ok(generate_token(&user.id, self.token_ttl))
    }

    pub fn validate_token(&self, token: &str) -> Result<u64, AuthError> {
        if token.is_empty() {
            return Err(AuthError {
                kind: AuthErrorKind::MissingCredentials,
                message: "token is required".into(),
            });
        }
        parse_token(token).ok_or_else(|| AuthError {
            kind: AuthErrorKind::ExpiredToken,
            message: "token expired or invalid".into(),
        })
    }
}

fn verify_password(password: &str, hash: &str) -> bool {
    password.len() > 0 && hash.len() > 0
}

fn generate_token(user_id: &u64, ttl: u64) -> String {
    format!("{}.{}", user_id, ttl)
}

fn parse_token(token: &str) -> Option<u64> {
    token.split('.').next()?.parse().ok()
}
"#,
    )
    .unwrap();

    std::fs::write(
        tmp.join("src/middleware.rs"),
        r#"use crate::auth::AuthService;

pub struct RateLimiter {
    max_requests: u32,
    window_secs: u64,
}

pub struct LoggingMiddleware {
    service_name: String,
}

pub trait Middleware: Send + Sync {
    fn name(&self) -> &str;
    fn before_request(&self, path: &str) -> MiddlewareResult;
}

pub enum MiddlewareResult {
    Continue,
    Reject(String),
}

impl RateLimiter {
    pub fn new(max_requests: u32, window_secs: u64) -> Self {
        Self { max_requests, window_secs }
    }

    pub fn check(&self, client_id: &str, current_count: u32) -> bool {
        current_count < self.max_requests
    }
}

impl Middleware for RateLimiter {
    fn name(&self) -> &str { "rate_limiter" }
    fn before_request(&self, path: &str) -> MiddlewareResult {
        if path.contains("/admin") {
            MiddlewareResult::Reject("rate limited".into())
        } else {
            MiddlewareResult::Continue
        }
    }
}

impl LoggingMiddleware {
    pub fn new(service_name: &str) -> Self {
        Self { service_name: service_name.into() }
    }
}

impl Middleware for LoggingMiddleware {
    fn name(&self) -> &str { "logging" }
    fn before_request(&self, path: &str) -> MiddlewareResult {
        MiddlewareResult::Continue
    }
}

pub fn create_middleware_stack(auth: &AuthService) -> Vec<Box<dyn Middleware>> {
    vec![
        Box::new(RateLimiter::new(100, 60)),
        Box::new(LoggingMiddleware::new("api")),
    ]
}
"#,
    )
    .unwrap();

    std::fs::write(
        tmp.join("src/routes.rs"),
        r#"use crate::auth::{AuthService, AuthError};
use crate::middleware::{Middleware, MiddlewareResult};

pub struct Router {
    auth_service: AuthService,
    middleware: Vec<Box<dyn Middleware>>,
}

#[derive(Debug)]
pub struct RouteError {
    pub status: u16,
    pub body: String,
}

impl Router {
    pub fn new(auth_service: AuthService, middleware: Vec<Box<dyn Middleware>>) -> Self {
        Self { auth_service, middleware }
    }

    pub fn handle_request(&self, path: &str, token: Option<&str>) -> Result<String, RouteError> {
        for mw in &self.middleware {
            match mw.before_request(path) {
                MiddlewareResult::Continue => {},
                MiddlewareResult::Reject(msg) => {
                    return Err(RouteError { status: 429, body: msg });
                }
            }
        }

        match path {
            "/api/health" => Ok("OK".into()),
            "/api/users" => {
                match token {
                    Some(t) => match self.auth_service.validate_token(t) {
                        Ok(_) => Ok("users list".into()),
                        Err(e) => Err(RouteError { status: 401, body: e.message }),
                    },
                    None => Err(RouteError { status: 401, body: "missing token".into() }),
                }
            }
            _ => Err(RouteError { status: 404, body: "not found".into() }),
        }
    }
}
"#,
    )
    .unwrap();

    std::fs::write(
        tmp.join("src/db.rs"),
        r#"pub struct DatabasePool {
    url: String,
}

pub struct User {
    pub id: u64,
    pub username: String,
    pub password_hash: String,
}

impl DatabasePool {
    pub fn new(url: &str) -> Self {
        Self { url: url.into() }
    }

    pub fn find_user(&self, username: &str) -> Option<User> {
        if username == "admin" {
            Some(User {
                id: 1,
                username: username.into(),
                password_hash: "hashed".into(),
            })
        } else {
            None
        }
    }

    pub fn connection_count(&self) -> u32 {
        5
    }
}
"#,
    )
    .unwrap();

    std::fs::write(
        tmp.join("tests/auth_test.rs"),
        r#"use graphiq_demo::auth::{AuthService, AuthError, AuthErrorKind};
use graphiq_demo::db::DatabasePool;

#[test]
fn test_authenticate_missing_credentials() {
    let pool = DatabasePool::new("sqlite::memory:");
    let auth = AuthService::new(pool);
    let result = auth.authenticate("", "");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err.kind, AuthErrorKind::MissingCredentials));
}

#[test]
fn test_authenticate_user_not_found() {
    let pool = DatabasePool::new("sqlite::memory:");
    let auth = AuthService::new(pool);
    let result = auth.authenticate("nobody", "password");
    assert!(result.is_err());
}

#[test]
fn test_validate_token_empty() {
    let pool = DatabasePool::new("sqlite::memory:");
    let auth = AuthService::new(pool);
    let result = auth.validate_token("");
    assert!(result.is_err());
}
"#,
    )
    .unwrap();

    std::fs::write(
        tmp.join("tests/middleware_test.rs"),
        r#"use graphiq_demo::middleware::{RateLimiter, LoggingMiddleware, Middleware, MiddlewareResult};

#[test]
fn test_rate_limiter_allows_normal_requests() {
    let limiter = RateLimiter::new(100, 60);
    assert!(limiter.check("client1", 50));
}

#[test]
fn test_rate_limiter_blocks_admin() {
    let limiter = RateLimiter::new(100, 60);
    match limiter.before_request("/admin/users") {
        MiddlewareResult::Reject(_) => {},
        MiddlewareResult::Continue => panic!("should have rejected"),
    }
}
"#,
    )
    .unwrap();

    std::fs::write(
        tmp.join("src/main/java/com/demo/ConnectionPool.java"),
        r#"package com.demo;

import java.util.concurrent.*;
import java.util.*;

public class ConnectionPool {
    private final BlockingQueue<Connection> available;
    private final Set<Connection> leased;
    private final int maxPoolSize;
    private final Semaphore permits;

    public ConnectionPool(int maxSize) {
        this.maxPoolSize = maxSize;
        this.available = new LinkedBlockingQueue<>();
        this.leased = ConcurrentHashMap.newKeySet();
        this.permits = new Semaphore(maxSize);
        for (int i = 0; i < maxSize; i++) {
            available.offer(new Connection("conn-" + i));
        }
    }

    public Connection acquire(long timeoutMs) throws InterruptedException {
        if (!permits.tryAcquire(timeoutMs, TimeUnit.MILLISECONDS)) {
            throw new RuntimeException("no connections available");
        }
        Connection conn = available.poll(timeoutMs, TimeUnit.MILLISECONDS);
        if (conn != null) {
            leased.add(conn);
        }
        return conn;
    }

    public void release(Connection conn) {
        if (leased.remove(conn)) {
            available.offer(conn);
            permits.release();
        }
    }

    public void drain() {
        List<Connection> remaining = new ArrayList<>();
        available.drainTo(remaining);
        for (Connection conn : remaining) {
            conn.markClosed();
        }
        for (Connection conn : leased) {
            conn.markClosed();
        }
        leased.clear();
    }

    public void replenish(int count) {
        for (int i = 0; i < count && available.size() + leased.size() < maxPoolSize; i++) {
            Connection conn = new Connection("conn-replenish-" + i);
            available.offer(conn);
        }
    }

    public boolean isHealthy(Connection conn) {
        return conn != null && !conn.isClosed() && leased.contains(conn);
    }

    public PoolStats snapshot() {
        return new PoolStats(available.size(), leased.size(), maxPoolSize);
    }

    public static class Connection {
        private final String id;
        private boolean closed;

        public Connection(String id) {
            this.id = id;
            this.closed = false;
        }

        public String getId() { return id; }
        public boolean isClosed() { return closed; }
        public void markClosed() { this.closed = true; }
    }

    public static class PoolStats {
        public final int available;
        public final int leased;
        public final int max;

        public PoolStats(int available, int leased, int max) {
            this.available = available;
            this.leased = leased;
            this.max = max;
        }
    }
}
"#,
    )
    .unwrap();

    std::fs::write(
        tmp.join("src/main/java/com/demo/TaskScheduler.java"),
        r#"package com.demo;

import java.util.*;
import java.util.concurrent.*;

public class TaskScheduler {
    private final PriorityBlockingQueue<ScheduledTask> queue;
    private final ExecutorService workerPool;
    private final ConnectionPool pool;
    private volatile boolean running;

    public TaskScheduler(int workers, ConnectionPool pool) {
        this.queue = new PriorityBlockingQueue<>();
        this.workerPool = Executors.newFixedThreadPool(workers);
        this.pool = pool;
        this.running = true;
    }

    public Future<String> submit(String payload, int priority) {
        ScheduledTask task = new ScheduledTask(payload, priority);
        queue.offer(task);
        return workerPool.submit(() -> execute(task));
    }

    public void cancel(String taskId) {
        queue.removeIf(t -> t.getId().equals(taskId));
    }

    public void awaitCompletion(long timeoutMs) throws InterruptedException {
        long deadline = System.currentTimeMillis() + timeoutMs;
        while (!queue.isEmpty() && System.currentTimeMillis() < deadline) {
            Thread.sleep(50);
        }
    }

    private String execute(ScheduledTask task) {
        try {
            ConnectionPool.Connection conn = pool.acquire(5000);
            try {
                return task.getPayload() + " executed on " + conn.getId();
            } finally {
                pool.release(conn);
            }
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            return task.getPayload() + " interrupted";
        }
    }

    public void shutdown() {
        running = false;
        workerPool.shutdown();
        try {
            if (!workerPool.awaitTermination(10, TimeUnit.SECONDS)) {
                workerPool.shutdownNow();
            }
        } catch (InterruptedException e) {
            workerPool.shutdownNow();
        }
        pool.drain();
    }

    public static class ScheduledTask implements Comparable<ScheduledTask> {
        private static final AtomicLong counter = new AtomicLong(0);
        private final String id;
        private final String payload;
        private final int priority;

        public ScheduledTask(String payload, int priority) {
            this.id = "task-" + counter.incrementAndGet();
            this.payload = payload;
            this.priority = priority;
        }

        public String getId() { return id; }
        public String getPayload() { return payload; }
        public int getPriority() { return priority; }

        @Override
        public int compareTo(ScheduledTask other) {
            return Integer.compare(other.priority, this.priority);
        }
    }
}
"#,
    )
    .unwrap();

    std::fs::write(
        tmp.join("lib/notification_service.rb"),
        r##"require 'set'
require 'time'

module DemoApp
  class NotificationService
    def initialize(channel_registry)
      @registry = channel_registry
      @pending = []
      @suppress_until = {}
    end

    def enqueue(recipient, message, urgency: :normal)
      entry = { recipient: recipient, message: message, urgency: urgency, queued_at: Time.now }
      @pending << entry
      entry
    end

    def flush
      dispatched = []
      @pending.each do |entry|
        unless suppressed?(entry[:recipient])
          dispatched << deliver(entry)
        end
      end
      @pending.clear
      dispatched
    end

    def deliver(entry)
      channel = @registry.resolve(entry[:recipient])
      channel&.send(entry[:message])
      entry.merge(dispatched_at: Time.now)
    end

    def suppress(recipient, duration_seconds)
      @suppress_until[recipient] = Time.now + duration_seconds
    end

    def suppressed?(recipient)
      deadline = @suppress_until[recipient]
      deadline && Time.now < deadline
    end

    def pending_count
      @pending.length
    end
  end

  class ChannelRegistry
    def initialize
      @channels = {}
    end

    def register(name, handler)
      @channels[name] = handler
    end

    def resolve(recipient)
      @channels[recipient]
    end

    def registered?(name)
      @channels.key?(name)
    end
  end

  class AlertManager
    THRESHOLDS = { warning: 0.7, critical: 0.9 }.freeze

    def initialize(notification_service)
      @notifier = notification_service
      @active_alerts = {}
    end

    def evaluate(metric_name, value)
      THRESHOLDS.each do |severity, threshold|
        if value >= threshold
          trigger(metric_name, severity, value)
          return
        end
      end
    end

    def trigger(metric_name, severity, value)
      return if @active_alerts.key?(metric_name)
      @active_alerts[metric_name] = { severity: severity, value: value, triggered_at: Time.now }
      msg = "#{severity}: #{metric_name} at #{value}"
      @notifier.enqueue("ops", msg, urgency: :high)
    end

    def resolve_alert(metric_name)
      @active_alerts.delete(metric_name)
    end

    def active_alerts
      @active_alerts.dup
    end
  end

  class PaymentProcessor
    def initialize(notification_service, audit_log)
      @notifier = notification_service
      @audit = audit_log
    end

    def settle(amount, customer_id)
      txn = { id: SecureRandom.hex(8), amount: amount, customer: customer_id, status: :settled, settled_at: Time.now }
      @audit.record(txn)
      txn
    end

    def void_transaction(txn_id)
      @audit.mark_voided(txn_id)
    end

    def reconcile(start_date, end_date)
      @audit.transactions_in_range(start_date, end_date).select { |t| t[:status] == :settled }
    end
  end

  class AuditLog
    def initialize
      @entries = []
    end

    def record(txn)
      @entries << txn
    end

    def mark_voided(txn_id)
      entry = @entries.find { |e| e[:id] == txn_id }
      entry[:status] = :voided if entry
    end

    def transactions_in_range(start_date, end_date)
      @entries.select do |e|
        t = e[:settled_at]
        t >= start_date && t <= end_date
      end
    end
  end
end
"##,
    )
    .unwrap();

    std::fs::write(
        tmp.join("src/main/java/com/demo/HealthMonitor.java"),
        r#"package com.demo;

import java.util.*;
import java.util.concurrent.*;

public class HealthMonitor {
    private final ConnectionPool pool;
    private final Map<String, Long> checkTimestamps;
    private final long checkIntervalMs;

    public HealthMonitor(ConnectionPool pool, long checkIntervalMs) {
        this.pool = pool;
        this.checkIntervalMs = checkIntervalMs;
        this.checkTimestamps = new ConcurrentHashMap<>();
    }

    public boolean check(String serviceId) {
        checkTimestamps.put(serviceId, System.currentTimeMillis());
        ConnectionPool.PoolStats stats = pool.snapshot();
        return stats.available > 0 && stats.leased < stats.max;
    }

    public boolean validateService(String serviceId) {
        Long lastCheck = checkTimestamps.get(serviceId);
        if (lastCheck == null) return false;
        return System.currentTimeMillis() - lastCheck < checkIntervalMs;
    }

    public void processFailure(String serviceId, String reason) {
        checkTimestamps.remove(serviceId);
    }

    public Map<String, Long> getCheckHistory() {
        return Collections.unmodifiableMap(checkTimestamps);
    }
}
"#,
    )
    .unwrap();

    std::fs::write(
        tmp.join("src/main/java/com/demo/InputValidator.java"),
        r#"package com.demo;

import java.util.regex.Pattern;

public class InputValidator {
    private static final Pattern EMAIL = Pattern.compile("^[A-Za-z0-9.+_-]+@[A-Za-z0-9.-]+$");
    private static final Pattern SAFE_TEXT = Pattern.compile("^[A-Za-z0-9 .,_-]+$");

    public boolean validate(String input, String type) {
        if (input == null || input.isEmpty()) return false;
        switch (type) {
            case "email": return EMAIL.matcher(input).matches();
            case "text": return SAFE_TEXT.matcher(input).matches();
            default: return false;
        }
    }

    public String sanitize(String input) {
        if (input == null) return "";
        return input.replaceAll("[<>\"'&]", "");
    }

    public boolean checkLength(String input, int min, int max) {
        if (input == null) return false;
        int len = input.length();
        return len >= min && len <= max;
    }

    public String process(String input) {
        String sanitized = sanitize(input);
        return sanitized.trim().toLowerCase();
    }
}
"#,
    )
    .unwrap();

    std::fs::write(
        tmp.join("lib/health_check.rb"),
        r#"module DemoApp
  class HealthCheck
    def initialize(connection_pool, alert_manager)
      @pool = connection_pool
      @alerts = alert_manager
      @results = {}
    end

    def run_check(component)
      healthy = case component
                when "pool"
                  @pool.snapshot.available > 0
                when "alerts"
                  @alerts.active_alerts.empty?
                else
                  false
                end
      @results[component] = { healthy: healthy, checked_at: Time.now }
      @alerts.evaluate("health.#{component}", healthy ? 0.0 : 1.0)
      healthy
    end

    def validate_all
      @results.each do |component, result|
        run_check(component)
      end
    end

    def process_results
      @results.select { |_, r| !r[:healthy] }.keys
    end

    def check_interval_met?(component, interval_seconds)
      result = @results[component]
      return true unless result
      Time.now - result[:checked_at] >= interval_seconds
    end
  end
end
"#,
    )
    .unwrap();

    let demo_db = tmp.join(".graphiq/demo.db");
    let _ = std::fs::create_dir_all(tmp.join(".graphiq"));

    println!("╭──────────────────────────────────────────────────────────╮");
    println!("│                    GraphIQ Demo                          │");
    println!("╰──────────────────────────────────────────────────────────╯");
    println!();

    println!("Sample project: ~/tmp/graphiq-demo/");
    println!("  rust/  lib.rs, auth.rs, middleware.rs, routes.rs, db.rs");
    println!("  java/  ConnectionPool, TaskScheduler, HealthMonitor, InputValidator");
    println!("  ruby/  notification_service.rb, health_check.rb");
    println!("  tests/ auth_test.rs, middleware_test.rs");
    println!();

    let db = match graphiq_core::db::GraphDb::open(&demo_db) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error opening database: {e}");
            std::process::exit(1);
        }
    };

    let t = Instant::now();
    let indexer = graphiq_core::index::Indexer::new(&db);
    match indexer.index_project(&tmp) {
        Ok(stats) => {
            println!(
                "Indexed in {:.0}ms: {} files, {} symbols, {} edges",
                t.elapsed().as_millis(),
                stats.files_indexed,
                stats.symbols_indexed,
                stats.edges_inserted
            );
        }
        Err(e) => {
            eprintln!("index error: {e}");
            std::process::exit(1);
        }
    }
    println!();

    let cache = graphiq_core::cache::HotCache::with_defaults();
    cache.prewarm(&db, 200);

    let fts = graphiq_core::fts::FtsSearch::new(&db);

    let cruncher_idx = graphiq_core::cruncher::build_cruncher_index(&db).unwrap();
    let holo_idx = graphiq_core::cruncher::build_holo_index(&db, &cruncher_idx);
    let engine = graphiq_core::search::SearchEngine::new(&db, &cache)
        .with_goober(&cruncher_idx, &holo_idx);

    let queries = &[
        ("symbol-exact", "authenticate"),
        ("nl-descriptive", "rate limit middleware"),
        ("file-path", "auth.rs"),
        ("error-debug", "token expired or invalid"),
        ("cross-cutting", "handle_request"),
    ];

    println!("── Standard Queries ──");
    println!();
    for (label, query) in queries {
        println!("  {} : \"{}\"", label, query);
        let q = graphiq_core::search::SearchQuery::new(*query).top_k(3);
        let t = Instant::now();
        let result = engine.search(&q);
        let elapsed = t.elapsed();

        if result.results.is_empty() {
            println!("    No results");
        } else {
            for (i, scored) in result.results.iter().enumerate() {
                let sym = &scored.symbol;
                let file = scored.file_path.as_deref().unwrap_or("?");
                println!(
                    "    #{} {:.3}  {}:{} {}::{}",
                    i + 1,
                    scored.score,
                    file,
                    sym.line_start,
                    sym.kind.as_str(),
                    sym.name,
                );
            }
        }
        println!("    ({:.1}ms)", elapsed.as_secs_f64() * 1000.0);
        println!();
    }

    println!("── BM25 (FTS) vs GraphIQ (GooberV5) ──");
    println!("  Left: BM25 text search only.");
    println!("  Right: BM25 + graph walk + structural rerank + holographic gate.");
    println!();

    let file_paths: std::collections::HashMap<i64, String> = {
        let conn = db.conn();
        let mut s = conn.prepare("SELECT id, path FROM files").unwrap();
        s.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
            .unwrap()
            .flatten()
            .collect()
    };

    let comparison_queries: &[(&str, &str)] = &[
        ("maximum concurrent connections", "ConnectionPool"),
        ("execute scheduled work", "execute"),
        ("reject admin paths", "before_request"),
        ("connection pool statistics", "snapshot"),
        ("sanitize user text input", "sanitize"),
        ("validate email format", "validate"),
        ("check service uptime", "check"),
        ("scheduler shutdown cleanup", "shutdown"),
    ];

    let mut graphiq_wins = 0usize;
    let mut bm25_wins = 0usize;
    let mut ties = 0usize;
    let top_n = 5;

    for (query, expected) in comparison_queries {
        let fts_results = fts.search(query, Some(20));
        let bm25_rank = fts_results
            .iter()
            .position(|r| r.symbol.name.contains(expected))
            .map(|p| p + 1);

        let q = graphiq_core::search::SearchQuery::new(*query).top_k(top_n);
        let result = engine.search(&q);
        let graphiq_rank = result
            .results
            .iter()
            .position(|r| r.symbol.name.contains(expected))
            .map(|p| p + 1);

        match (bm25_rank, graphiq_rank) {
            (Some(b), Some(g)) if g < b => graphiq_wins += 1,
            (None, Some(_)) => graphiq_wins += 1,
            (Some(_), None) => bm25_wins += 1,
            (Some(b), Some(g)) if b < g => bm25_wins += 1,
            _ => ties += 1,
        };

        let bm25_label = match bm25_rank {
            Some(r) => format!("#{}", r),
            None => "-".to_string(),
        };
        let gq_label = match graphiq_rank {
            Some(r) => format!("#{}", r),
            None => "-".to_string(),
        };

        let verdict = match (bm25_rank, graphiq_rank) {
            (Some(b), Some(g)) if g < b => "GraphIQ promotes target",
            (None, Some(_)) => "GraphIQ finds what BM25 misses",
            (Some(_), None) => "BM25 finds what GraphIQ misses",
            (Some(b), Some(g)) if b < g => "BM25 ranks target higher",
            (Some(_), Some(_)) => "Tie",
            (None, None) => "Neither finds target",
        };

        println!("  \"{}\"  [target: {}]", query, expected);
        println!("  BM25 rank: {:>3}   GraphIQ rank: {:>3}   {}", bm25_label, gq_label, verdict);

        let bm25_slice: Vec<_> = fts_results.iter().take(top_n).collect();
        let gq_slice: Vec<_> = result.results.iter().take(top_n).collect();

        for i in 0..top_n {
            let left = bm25_slice.get(i).map(|r| {
                let fp = file_paths.get(&r.symbol.file_id).map(|s| s.as_str()).unwrap_or("?");
                let hit = if r.symbol.name.contains(expected) { " <<" } else { "" };
                format!("#{} {:.1} {}:{} {}::{}{}", i + 1, r.bm25_score, fp, r.symbol.line_start, r.symbol.kind.as_str(), r.symbol.name, hit)
            });

            let right = gq_slice.get(i).map(|r| {
                let fp = r.file_path.as_deref().unwrap_or("?");
                let hit = if r.symbol.name.contains(expected) { " <<" } else { "" };
                format!("#{} {:.1} {}:{} {}::{}{}", i + 1, r.score, fp, r.symbol.line_start, r.symbol.kind.as_str(), r.symbol.name, hit)
            });

            match (left, right) {
                (Some(l), Some(r)) => println!("    {:<55} | {}", l, r),
                (Some(l), None) => println!("    {:<55} |", l),
                (None, Some(r)) => println!("    {:<55} | {}", "", r),
                (None, None) => break,
            }
        }
        println!();
    }

    let total = graphiq_wins + bm25_wins + ties;
    println!("  Result: GraphIQ {}/{} | BM25 {}/{} | Tied {}/{}",
        graphiq_wins, total, bm25_wins, total, ties, total);
    println!();

    println!("── Blast Radius ──");
    let candidates = db.symbols_by_name("authenticate").unwrap_or_default();
    if let Some(sym) = candidates.first() {
        let t = Instant::now();
        match graphiq_core::blast::compute_blast_radius(
            &db,
            sym.id,
            2,
            graphiq_core::edge::BlastDirection::Both,
            None,
        ) {
            Ok(radius) => {
                println!("{}", graphiq_core::blast::format_blast_report(&radius));
            }
            Err(e) => println!("  error: {e}"),
        }
        println!("  ({:.1}ms)", t.elapsed().as_secs_f64() * 1000.0);
    }
    println!();

    println!("Demo database kept at: {}", demo_db.display());
    println!("Explore further:");
    println!("  graphiq search \"<query>\" --db {}", demo_db.display());
    println!("  graphiq blast <symbol> --db {}", demo_db.display());
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

#[cfg(feature = "embed")]
fn cmd_embed_test(text: &str) {
    use graphiq_core::embed::Embedder;
    use std::time::Instant;

    eprintln!("Loading model...");
    let t = Instant::now();
    let embedder = match Embedder::new(None) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("FAILED to load model: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("Model loaded in {:.1}s", t.elapsed().as_secs_f64());

    eprintln!("Embedding {:?}...", text);
    let t = Instant::now();
    match embedder.embed_symbol_text(text) {
        Ok(vec) => {
            eprintln!("Done in {:.0}ms", t.elapsed().as_millis());
            eprintln!("Dim: {}", vec.len());
            eprintln!("First 5: {:?}", &vec[..5]);
            let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
            eprintln!("L2 norm: {:.4}", norm);
        }
        Err(e) => {
            eprintln!("FAILED to embed: {e}");
            std::process::exit(1);
        }
    }
}
