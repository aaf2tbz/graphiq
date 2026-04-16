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
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Index { path, db } => cmd_index(&path, &db),
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
    }
}

fn cmd_index(path: &std::path::Path, db_path: &std::path::Path) {
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
    let engine = graphiq_core::search::SearchEngine::new(&db, &cache);

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
        let vis = sym.visibility.as_str();
        let kind = sym.kind.as_str();

        print!(
            "#{:>2} {:8.3}  {} {}::{}",
            i + 1,
            scored.score,
            vis,
            file,
            sym.name
        );
        if !matches!(kind, "function" | "method" | "class") {
            print!(" [{}]", kind);
        }
        if let Some(ref sig) = sym.signature {
            let short = sig.lines().next().unwrap_or("");
            if short.len() > 80 {
                print!("  {}", &short[..80]);
            } else {
                print!("  {}", short);
            }
        }
        println!();

        if debug {
            if let Some(ref bd) = scored.breakdown {
                println!(
                    "          layer2={:.3}  path_w={:.2}  diversity={:.2}",
                    bd.layer2_score, bd.path_weight, bd.diversity_dampen
                );
                print!("          heuristics:");
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
