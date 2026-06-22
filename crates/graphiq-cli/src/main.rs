use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "graphiq",
    about = "Code intelligence with structural retrieval",
    version
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
        #[arg(long)]
        force_reindex: bool,
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
    Impact {
        #[arg(long, value_name = "PATH", default_value = ".")]
        project: PathBuf,
        #[arg(long, default_value = ".graphiq/graphiq.db")]
        db: PathBuf,
        #[arg(long)]
        base: Option<String>,
        #[arg(long)]
        head: Option<String>,
        #[arg(short, long, default_value_t = 2)]
        depth: usize,
        #[arg(short, long, default_value_t = 30)]
        top: usize,
        #[arg(long)]
        json: bool,
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
    Clear {
        #[arg(long, default_value = ".graphiq/graphiq.db")]
        db: PathBuf,
        #[arg(short, long, help = "Skip the confirmation prompt")]
        yes: bool,
    },
    Sync {
        #[arg(long, value_name = "PATH")]
        project: Option<PathBuf>,
        #[arg(
            long,
            help = "Limit the displayed report to harnesses matching this substring"
        )]
        harness: Option<String>,
        #[arg(
            long,
            help = "Re-apply graphiq config to any discovered-but-unconfigured harness, then re-verify"
        )]
        apply: bool,
    },
    Discover {
        #[arg(long, help = "Output machine-readable JSON instead of a table")]
        json: bool,
    },
    Git {
        #[arg(long, value_name = "PATH", default_value = ".")]
        project: PathBuf,
        #[arg(long, help = "Number of recent commits to show", default_value_t = 10)]
        commits: usize,
        #[arg(long, help = "Output machine-readable JSON")]
        json: bool,
    },
    Projects {
        #[arg(long, help = "Output machine-readable JSON")]
        json: bool,
        #[arg(long, help = "Forget a project by path (or 'all')")]
        forget: Option<String>,
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
        #[arg(long, conflicts_with = "persistent")]
        ephemeral: bool,
        #[arg(long, conflicts_with = "ephemeral")]
        persistent: bool,
        #[arg(long)]
        harness: Option<String>,
        #[arg(long)]
        dry_run: bool,
    },
    Doctor {
        #[arg(long, default_value = ".graphiq/graphiq.db")]
        db: PathBuf,
    },
    UpgradeIndex {
        #[arg(long, default_value = ".graphiq/graphiq.db")]
        db: PathBuf,
    },
    Constants {
        #[arg(long, default_value = ".graphiq/graphiq.db")]
        db: PathBuf,
        query: Option<String>,
        #[arg(short, long, default_value_t = 20)]
        top: usize,
    },
    DeepGraph {
        #[arg(long, default_value = ".graphiq/graphiq.db")]
        db: PathBuf,
    },
    Briefing {
        #[arg(long, default_value = ".graphiq/graphiq.db")]
        db: PathBuf,
        #[arg(long)]
        compact: bool,
    },
    Context {
        symbol: String,
        #[arg(long, default_value = ".graphiq/graphiq.db")]
        db: PathBuf,
    },
    DeadCode {
        #[arg(long, default_value = ".graphiq/graphiq.db")]
        db: PathBuf,
    },
    Update {
        #[arg(
            long,
            value_name = "DIR",
            default_value = "/usr/local/bin",
            help = "Installation directory for binaries"
        )]
        install_dir: Option<String>,
        #[arg(short, long, help = "Skip confirmation prompts")]
        yes: bool,
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
        Commands::Index {
            path,
            db,
            force_reindex,
            ..
        } => cmd_index(&path, &db, false, force_reindex),
        #[cfg(feature = "embed")]
        Commands::Index {
            path,
            db,
            embed,
            force_reindex,
            ..
        } => cmd_index(&path, &db, embed, force_reindex),

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
        Commands::Impact {
            project,
            db,
            base,
            head,
            depth,
            top,
            json,
        } => cmd_impact(
            &project,
            &db,
            base.as_deref(),
            head.as_deref(),
            depth,
            top,
            json,
        ),
        Commands::Status { db } => cmd_status(&db),
        Commands::Reindex { path, db } => cmd_reindex(&path, &db),
        Commands::Clear { db, yes } => cmd_clear(&db, yes),
        Commands::Sync {
            project,
            harness,
            apply,
        } => cmd_sync(project.as_deref(), harness.as_deref(), apply),
        Commands::Discover { json } => cmd_discover(json),
        Commands::Git {
            project,
            commits,
            json,
        } => cmd_git(&project, commits, json),
        Commands::Projects { json, forget } => cmd_projects(json, forget.as_deref()),
        Commands::Subsystems { db, roles } => cmd_subsystems(&db, roles),
        Commands::Roles { db, subsystem, top } => cmd_roles(&db, subsystem, top),
        Commands::Demo => cmd_demo(),
        Commands::Setup {
            project,
            skip_index,
            ephemeral,
            persistent,
            harness,
            dry_run,
        } => cmd_setup(
            project.as_deref(),
            skip_index,
            ephemeral,
            persistent,
            harness.as_deref(),
            dry_run,
        ),
        Commands::Doctor { db } => cmd_doctor(&db),
        Commands::UpgradeIndex { db } => cmd_upgrade_index(&db),
        Commands::Constants { db, query, top } => cmd_constants(&db, query.as_deref(), top),
        Commands::DeepGraph { db } => cmd_deep_graph(&db),
        Commands::Briefing { db, compact } => cmd_briefing(&db, compact),
        Commands::Context { symbol, db } => cmd_context(&symbol, &db),
        Commands::DeadCode { db } => cmd_dead_code(&db),
        Commands::Update { install_dir, yes } => cmd_update(install_dir.as_deref(), yes),
        #[cfg(feature = "embed")]
        Commands::EmbedTest { text } => cmd_embed_test(text.as_deref().unwrap_or("hello world")),
    }
}

fn resolve_db(project_path: &std::path::Path, db_arg: &std::path::Path) -> PathBuf {
    if let Ok(val) = std::env::var("GRAPHIQ_DB") {
        if !val.is_empty() {
            let p = PathBuf::from(&val);
            if p.is_absolute() {
                return p;
            }
            return std::env::current_dir().map(|cwd| cwd.join(&p)).unwrap_or(p);
        }
    }
    let computed = project_path.join(".graphiq").join("graphiq.db");
    if db_arg == std::path::Path::new(".graphiq/graphiq.db") {
        computed
    } else {
        db_arg.to_path_buf()
    }
}

fn cmd_index(
    path: &std::path::Path,
    db_path: &std::path::Path,
    do_embed: bool,
    force_reindex: bool,
) {
    let db_path = resolve_db(path, db_path);

    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }

    if force_reindex && db_path.exists() {
        println!(
            "Force reindex: removing existing database {}",
            db_path.display()
        );
        let wal = db_path.with_extension("db-wal");
        let shm = db_path.with_extension("db-shm");
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(&wal);
        let _ = std::fs::remove_file(&shm);
    }

    let db = open_db_or_exit(&db_path);

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

    let manifest = graphiq_core::manifest::build_manifest_all_ready(&db);
    let db_dir = db_path.parent().unwrap_or(std::path::Path::new("."));
    if let Err(e) = graphiq_core::manifest::write_manifest(db_dir, &manifest) {
        eprintln!("  warning: failed to write manifest: {e}");
    }

    // Record this project in the persistent multi-project registry so
    // `graphiq projects` can list it and the user/agent can return to it without
    // re-indexing. Best-effort: never let a registry write fail the index.
    let _ = record_indexed_project(path, &db);

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

    let db = open_db_or_exit(db_path);

    let db_dir = db_path.parent().unwrap_or(std::path::Path::new("."));
    if let Ok(Some(manifest)) = graphiq_core::manifest::read_manifest(db_dir) {
        let fresh = graphiq_core::manifest::check_artifact_freshness(&db, &manifest);
        let stale: Vec<(&str, graphiq_core::manifest::ArtifactStatus)> =
            vec![("cruncher", fresh.cruncher)]
                .into_iter()
                .filter(|(_, s)| *s != graphiq_core::manifest::ArtifactStatus::Ready)
                .collect();

        if !stale.is_empty() && debug {
            eprintln!("manifest: {} artifact(s) not ready:", stale.len());
            for (name, status) in &stale {
                eprintln!("  {}: {}", name, status);
            }
            eprintln!(
                "  run `graphiq upgrade-index --db {}` to rebuild",
                db_path.display()
            );
        }
    }

    let cache = graphiq_core::cache::HotCache::with_defaults();
    cache.prewarm(&db, 200);

    let cruncher = graphiq_core::cruncher::build_cruncher_index(&db).ok();

    let mut engine = graphiq_core::search::SearchEngine::new(&db, &cache);
    if let Some(ref ci) = cruncher {
        engine = engine.with_cruncher(ci);
    }

    if debug {
        eprintln!("search mode: {}", engine.active_mode());
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

    if debug {
        eprintln!("query family: {}", result.query_family);
        eprintln!("search mode: {}", result.search_mode);
    }

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
            if let Some(trace) = result.traces.get(&scored.symbol.id) {
                eprintln!("{}", trace.format_debug(&scored.symbol.name));
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

    let db = open_db_or_exit(db_path);

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

fn cmd_impact(
    project_path: &std::path::Path,
    db_path: &std::path::Path,
    base: Option<&str>,
    head: Option<&str>,
    depth: usize,
    top: usize,
    json: bool,
) {
    let project = project_path
        .canonicalize()
        .unwrap_or_else(|_| project_path.to_path_buf());
    let db_path = resolve_db(&project, db_path);

    if !db_path.exists() {
        eprintln!("database not found: {}", db_path.display());
        eprintln!("run `graphiq index {}` first", project.display());
        std::process::exit(1);
    }

    let db = open_db_or_exit(&db_path);
    let source = match base {
        Some(base) => graphiq_core::impact::ChangeSource::BaseRef {
            base: base.to_string(),
            head: head.unwrap_or("HEAD").to_string(),
        },
        None => graphiq_core::impact::ChangeSource::WorkingTree,
    };
    let options = graphiq_core::impact::ImpactOptions {
        project_root: project,
        db_path: Some(db_path),
        source,
        depth: depth.min(10),
        top: top.min(200),
    };

    match graphiq_core::impact::analyze_git_impact(&db, options) {
        Ok(report) if json => match serde_json::to_string_pretty(&report) {
            Ok(text) => println!("{text}"),
            Err(e) => {
                eprintln!("error serializing impact report: {e}");
                std::process::exit(1);
            }
        },
        Ok(report) => println!("{}", graphiq_core::impact::format_impact_report(&report)),
        Err(e) => {
            eprintln!("impact analysis failed: {e}");
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

            let db_dir = db_path.parent().unwrap_or(std::path::Path::new("."));
            if let Ok(Some(manifest)) = graphiq_core::manifest::read_manifest(db_dir) {
                let fresh = graphiq_core::manifest::check_artifact_freshness(&gdb, &manifest);
                println!();
                println!("  Manifest (v{}):", manifest.schema_version);
                println!("    Indexed at:  {}", manifest.indexed_at);
                println!("    Artifacts:");
                println!("      fts:          {}", fresh.fts);
                println!("      cruncher:     {}", fresh.cruncher);
                let mode = graphiq_core::manifest::Manifest::compute_active_mode(&fresh);
                println!("    Active mode: {}", mode);
                if mode != graphiq_core::search::SearchMode::GraphWalk {
                    let reasons =
                        graphiq_core::manifest::Manifest::compute_downgrade_reasons(&fresh);
                    if !reasons.is_empty() {
                        println!("    Downgrade reasons:");
                        for r in &reasons {
                            println!("      - {}", r);
                        }
                    }
                }
            } else {
                println!();
                println!("  Manifest: not found (run `graphiq index` or `graphiq upgrade-index`)");
            }
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

    let db = open_db_or_exit(db_path);

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

    let manifest = graphiq_core::manifest::build_manifest_all_ready(&db);
    let db_dir = db_path.parent().unwrap_or(std::path::Path::new("."));
    if let Err(e) = graphiq_core::manifest::write_manifest(db_dir, &manifest) {
        eprintln!("  warning: failed to write manifest: {e}");
    }
}

/// Remove an existing GraphIQ index and create a fresh empty one.
///
/// Deletes the SQLite database (and its WAL/SHM sidecars) plus the cached
/// `cruncher.bin.zst`, then opens a brand-new empty database so the project is
/// ready for a clean reindex. Existing indexed data is discarded; the on-disk
/// layout (`.graphiq/`) is preserved.
fn cmd_clear(db_path: &std::path::Path, yes: bool) {
    let db_dir = db_path.parent().unwrap_or(std::path::Path::new("."));

    // Gather everything we consider part of "the index" so the report is honest.
    let sidecars = [
        db_path.to_path_buf(),
        db_path.with_extension("db-wal"),
        db_path.with_extension("db-shm"),
        db_dir.join("cruncher.bin.zst"),
        db_dir.join("manifest.json"),
    ];

    let existing: Vec<_> = sidecars.iter().filter(|p| p.exists()).collect();

    if existing.is_empty() {
        // Nothing to clear — make sure there is a fresh empty DB so the command
        // is idempotent and the project is ready to index.
        if let Some(parent) = db_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match graphiq_core::db::GraphDb::open(db_path) {
            Ok(_) => {
                println!("No existing index found at {}.", db_path.display());
                println!("Created a fresh empty index.");
                return;
            }
            Err(e) => {
                eprintln!("error creating fresh database: {e}");
                std::process::exit(1);
            }
        }
    }

    if !yes
        && !confirm(&format!(
            "Clear the GraphIQ index at {}? This cannot be undone.",
            db_path.display()
        ))
    {
        println!("aborted.");
        return;
    }

    let mut removed = 0usize;
    for path in &existing {
        match std::fs::remove_file(path) {
            Ok(()) => {
                println!("  removed {}", path.display());
                removed += 1;
            }
            Err(e) => eprintln!("  warning: could not remove {}: {e}", path.display()),
        }
    }

    // Create a fresh empty database (open() initializes the schema).
    if let Some(parent) = db_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match graphiq_core::db::GraphDb::open(db_path) {
        Ok(_) => {
            println!(
                "Cleared index at {} (removed {removed} file(s)).",
                db_path.display()
            );
            println!("Fresh empty index ready. Run `graphiq index <path>` to rebuild.");
        }
        Err(e) => {
            eprintln!("removed old index but failed to create fresh database: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_subsystems(db_path: &std::path::Path, compute_roles: bool) {
    if !db_path.exists() {
        eprintln!("database not found: {}", db_path.display());
        std::process::exit(1);
    }

    let db = open_db_or_exit(db_path);

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
    println!(
        "{:<40} {:>6} {:>10} {:>10} {:>8}",
        "Name", "Symbols", "Internal", "Boundary", "Cohesion"
    );
    println!("{}", "-".repeat(78));
    for s in sorted.iter().take(30) {
        println!(
            "{:<40} {:>6} {:>10} {:>10} {:>8.2}",
            s.name,
            s.symbol_ids.len(),
            s.internal_edge_count,
            s.boundary_edge_count,
            s.cohesion
        );
    }
}

fn cmd_roles(db_path: &std::path::Path, subsystem_filter: Option<usize>, top: usize) {
    if !db_path.exists() {
        eprintln!("database not found: {}", db_path.display());
        std::process::exit(1);
    }

    let db = open_db_or_exit(db_path);

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

    use rusqlite::params;

    let conn = db.conn();
    let rows: Vec<(String, String, i64, i64, i64, i64, i64)> = if let Some(sub_id) =
        subsystem_filter
    {
        let mut stmt = conn
            .prepare(
                "SELECT symbol_name, roles, subsystem_id, internal_degree, boundary_degree, external_callers, external_callees
                 FROM symbol_structural_roles
                 WHERE subsystem_id = ?
                 ORDER BY external_callers DESC, internal_degree DESC
                 LIMIT ?",
            )
            .unwrap();
        stmt.query_map(params![sub_id as i64, top as i64], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
            ))
        })
        .unwrap()
        .flatten()
        .collect()
    } else {
        let mut stmt = conn
            .prepare(
                "SELECT symbol_name, roles, subsystem_id, internal_degree, boundary_degree, external_callers, external_callees
                 FROM symbol_structural_roles
                 ORDER BY external_callers DESC, internal_degree DESC
                 LIMIT ?",
            )
            .unwrap();
        stmt.query_map(params![top as i64], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
            ))
        })
        .unwrap()
        .flatten()
        .collect()
    };

    println!("\n=== Structural Roles (top {}) ===\n", rows.len().min(top));
    println!(
        "{:<45} {:<30} {:>8} {:>6} {:>6} {:>5} {:>5}",
        "Symbol", "Roles", "Subsystem", "IntDeg", "BndDeg", "ExtIn", "ExtOut"
    );
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

fn cmd_doctor(db_path: &std::path::Path) {
    if !db_path.exists() {
        eprintln!("database not found: {}", db_path.display());
        eprintln!("run `graphiq index <path>` first");
        std::process::exit(1);
    }

    let db = open_db_or_exit(db_path);

    let stats = match db.stats() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error reading stats: {e}");
            std::process::exit(1);
        }
    };

    println!("GraphIQ Doctor");
    println!("  Database: {}", db_path.display());
    println!(
        "  Files: {}  Symbols: {}  Edges: {}",
        stats.files, stats.symbols, stats.edges
    );
    println!();

    let db_dir = db_path.parent().unwrap_or(std::path::Path::new("."));

    let manifest = match graphiq_core::manifest::read_manifest(db_dir) {
        Ok(Some(m)) => m,
        Ok(None) => {
            println!("  Manifest: MISSING");
            println!("    No manifest.json found. Run `graphiq upgrade-index` to create one.");
            println!();

            println!("  Artifact status (probing):");
            let cruncher_ok = graphiq_core::cruncher::build_cruncher_index(&db).is_ok();

            println!("    fts:          ready ({} symbols in FTS)", stats.symbols);
            println!(
                "    cruncher:     {}",
                if cruncher_ok { "ready" } else { "missing" }
            );
            return;
        }
        Err(e) => {
            eprintln!("  error reading manifest: {e}");
            return;
        }
    };

    let fresh = graphiq_core::manifest::check_artifact_freshness(&db, &manifest);
    let mode = graphiq_core::manifest::Manifest::compute_active_mode(&fresh);

    println!(
        "  Manifest (v{}, indexed at {})",
        manifest.schema_version, manifest.indexed_at
    );
    println!();

    println!("  Artifact health:");
    let all_artifacts: Vec<(&str, graphiq_core::manifest::ArtifactStatus)> =
        vec![("fts", fresh.fts), ("cruncher", fresh.cruncher)];

    let mut stale_count = 0;
    let mut missing_count = 0;
    for (name, status) in &all_artifacts {
        let icon = match status {
            graphiq_core::manifest::ArtifactStatus::Ready => "OK",
            graphiq_core::manifest::ArtifactStatus::Stale => "STALE",
            graphiq_core::manifest::ArtifactStatus::Missing => "MISSING",
        };
        println!("    {:14} {}", format!("{}:", name), icon);
        match status {
            graphiq_core::manifest::ArtifactStatus::Stale => stale_count += 1,
            graphiq_core::manifest::ArtifactStatus::Missing => missing_count += 1,
            graphiq_core::manifest::ArtifactStatus::Ready => {}
        }
    }

    println!();
    println!("  Active search mode: {}", mode);

    if mode != graphiq_core::search::SearchMode::GraphWalk {
        let reasons = graphiq_core::manifest::Manifest::compute_downgrade_reasons(&fresh);
        if !reasons.is_empty() {
            println!("  Downgrade reasons:");
            for r in &reasons {
                println!("    - {}", r);
            }
        }
    }

    println!();
    if stale_count > 0 || missing_count > 0 {
        println!(
            "  DIAGNOSIS: {} stale, {} missing artifacts",
            stale_count, missing_count
        );
        println!(
            "  FIX: run `graphiq upgrade-index --db {}`",
            db_path.display()
        );
    } else {
        println!("  DIAGNOSIS: all artifacts healthy");
    }

    println!();
    print!("  GPU: ");
    if std::env::consts::OS == "macos" {
        #[cfg(feature = "gpu")]
        {
            match graphiq_core::gpu_compute::GpuContext::new() {
                Some(_) => println!("Metal (initialized OK)"),
                None => println!("Metal (init failed — will use CPU)"),
            }
        }
        #[cfg(not(feature = "gpu"))]
        {
            println!("Metal (built-in, GPU build not enabled)");
        }
    } else if vulkan_available() {
        #[cfg(feature = "gpu")]
        {
            match graphiq_core::gpu_compute::GpuContext::new() {
                Some(_) => println!("Vulkan (initialized OK)"),
                None => println!("Vulkan loader found but GPU init failed — will use CPU"),
            }
        }
        #[cfg(not(feature = "gpu"))]
        {
            println!("Vulkan loader found (GPU build not enabled)");
        }
    } else {
        println!("MISSING — install libvulkan1 for GPU acceleration");
    }
}

fn cmd_upgrade_index(db_path: &std::path::Path) {
    if !db_path.exists() {
        eprintln!("database not found: {}", db_path.display());
        eprintln!("run `graphiq index <path>` first");
        std::process::exit(1);
    }

    let db = open_db_or_exit(db_path);

    let db_dir = db_path.parent().unwrap_or(std::path::Path::new("."));
    let existing = graphiq_core::manifest::read_manifest(db_dir).ok().flatten();
    let needs_rebuild = existing.as_ref().map_or(true, |m| {
        let fresh = graphiq_core::manifest::check_artifact_freshness(&db, m);
        let all: Vec<_> = vec![fresh.cruncher];
        all.iter()
            .any(|s| *s != graphiq_core::manifest::ArtifactStatus::Ready)
    });

    if !needs_rebuild {
        if let Some(m) = &existing {
            let fresh = graphiq_core::manifest::check_artifact_freshness(&db, m);
            if fresh.cruncher == graphiq_core::manifest::ArtifactStatus::Ready {
                println!("All artifacts are fresh. No rebuild needed.");
                return;
            }
        }
    }

    println!("Rebuilding stale/missing artifacts...");

    let mut rebuilt = Vec::new();

    if let Ok(_ci) = graphiq_core::cruncher::build_cruncher_index(&db) {
        rebuilt.push("cruncher");
    } else {
        eprintln!("  warning: cruncher build failed");
    }

    let manifest = graphiq_core::manifest::build_manifest_all_ready(&db);
    if let Err(e) = graphiq_core::manifest::write_manifest(db_dir, &manifest) {
        eprintln!("  warning: failed to write manifest: {e}");
    }

    println!("  rebuilt: {}", rebuilt.join(", "));
    println!("  active mode: {}", manifest.active_search_mode);
    println!("Done.");
}

fn cmd_constants(db_path: &std::path::Path, query: Option<&str>, top: usize) {
    if !db_path.exists() {
        eprintln!("database not found: {}", db_path.display());
        eprintln!("run `graphiq index <path>` first");
        std::process::exit(1);
    }

    let db = open_db_or_exit(db_path);

    let entries = match graphiq_core::numeric_bridges::query_constants(&db, query, top) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("query failed: {e}");
            std::process::exit(1);
        }
    };

    if entries.is_empty() {
        println!("No numeric bridges found.");
        return;
    }

    println!(
        "{:<12} {:<30} {:>6}  {}",
        "LITERAL", "NAMED", "COUNT", "USAGE SITES"
    );
    println!("{}", "-".repeat(90));
    for entry in &entries {
        let named = entry.named.as_deref().unwrap_or("—");
        let sites: Vec<String> = entry
            .symbols
            .iter()
            .map(|s| {
                let file = s.file.rsplit('/').next().unwrap_or(&s.file);
                format!("{}:{}:{}", file, s.line, s.name)
            })
            .collect();
        println!(
            "{:<12} {:<30} {:>6}  {}",
            entry.literal,
            named,
            entry.count,
            sites.join(", ")
        );
    }
}

fn open_db_or_exit(db_path: &std::path::Path) -> graphiq_core::db::GraphDb {
    match graphiq_core::db::GraphDb::open(db_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error opening database: {e}");
            eprintln!("hint: try deleting {} and re-indexing", db_path.display());
            std::process::exit(1);
        }
    }
}

fn cmd_deep_graph(db_path: &std::path::Path) {
    let db = open_db_or_exit(db_path);
    let stats = graphiq_core::deep_graph::compute_deep_graph_edges(&db).expect("compute");
    println!(
        "deep graph: {} type-flow, {} error-type, {} data-shape edges",
        stats.type_flow_edges, stats.error_type_edges, stats.data_shape_edges
    );
    let src_stats =
        graphiq_core::deep_graph::compute_source_graph_edges(&db).expect("compute source");
    println!(
        "source graph: {} string-literal, {} comment-ref edges",
        src_stats.string_literal_edges, src_stats.comment_ref_edges
    );
}

fn cmd_briefing(db_path: &std::path::Path, compact: bool) {
    let db = open_db_or_exit(db_path);
    let result = if compact {
        graphiq_core::briefing::generate_briefing_compact(&db)
    } else {
        graphiq_core::briefing::generate_briefing(&db)
    };
    match result {
        Ok(text) => println!("{}", text),
        Err(e) => eprintln!("error: {e}"),
    }
}

struct DepCheck {
    name: &'static str,
    cmd: &'static str,
    args: &'static [&'static str],
    install_cmd_macos: &'static str,
    install_cmd_linux: &'static str,
    alt_hint: &'static str,
    required: bool,
}

fn cmd_exists(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
        || std::process::Command::new("command")
            .args(["-v", cmd])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
}

fn confirm(prompt: &str) -> bool {
    use std::io::{self, Write};
    print!("{} [y/N] ", prompt);
    let _ = io::stdout().flush();
    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_err() {
        return false;
    }
    matches!(input.trim().to_lowercase().as_str(), "y" | "yes")
}

fn check_build_dependencies() {
    let is_macos = std::env::consts::OS == "macos";

    let deps = [
        DepCheck {
            name: "C compiler (cc/gcc/clang)",
            cmd: "cc",
            args: &["--version"],
            install_cmd_macos: "xcode-select --install",
            install_cmd_linux: "sudo apt install -y build-essential",
            alt_hint: "Linux: sudo dnf install gcc, sudo pacman -S gcc",
            required: true,
        },
        DepCheck {
            name: "Rust toolchain",
            cmd: "rustc",
            args: &["--version"],
            install_cmd_macos: "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh",
            install_cmd_linux: "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh",
            alt_hint: "",
            required: false,
        },
        DepCheck {
            name: "pkg-config",
            cmd: "pkg-config",
            args: &["--version"],
            install_cmd_macos: "brew install pkg-config",
            install_cmd_linux: "sudo apt install -y pkg-config",
            alt_hint: "Linux: sudo dnf install pkgconfig",
            required: !is_macos,
        },
        DepCheck {
            name: "cmake",
            cmd: "cmake",
            args: &["--version"],
            install_cmd_macos: "brew install cmake",
            install_cmd_linux: "sudo apt install -y cmake",
            alt_hint: "needed for embed features only; Linux: sudo dnf install cmake, sudo pacman -S cmake",
            required: false,
        },
    ];

    if !is_macos {
        if vulkan_available() {
            println!("  ✓ Vulkan loader — GPU acceleration available");
        } else {
            println!("  ⚠ Vulkan loader not found — GPU acceleration disabled");
            println!("    Install: sudo apt install -y libvulkan1");
            println!("    Or:      sudo dnf install vulkan-loader, sudo pacman -S vulkan-driver");
        }
    }

    let mut missing_required: Vec<&DepCheck> = Vec::new();
    let mut missing_optional: Vec<&DepCheck> = Vec::new();

    for dep in &deps {
        if cmd_exists(dep.cmd) {
            if let Ok(output) = std::process::Command::new(dep.cmd).args(dep.args).output() {
                if output.status.success() {
                    let ver = String::from_utf8_lossy(&output.stdout);
                    let first_line = ver.lines().next().unwrap_or("");
                    let short = if first_line.len() > 60 {
                        format!("{}...", &first_line[..57])
                    } else {
                        first_line.to_string()
                    };
                    println!("  ✓ {} — {}", dep.name, short);
                    continue;
                }
            }
        }

        if dep.required {
            missing_required.push(dep);
        } else {
            missing_optional.push(dep);
        }
    }

    let install_cmd = |dep: &DepCheck| -> &'static str {
        if is_macos {
            dep.install_cmd_macos
        } else {
            dep.install_cmd_linux
        }
    };

    if !missing_required.is_empty() {
        println!();
        eprintln!("  Missing required dependencies:");
        for dep in &missing_required {
            eprintln!("    {} — install: {}", dep.name, install_cmd(dep));
            if !dep.alt_hint.is_empty() {
                eprintln!("      {}", dep.alt_hint);
            }
        }
        println!();
        if confirm("Install missing dependencies now?") {
            for dep in &missing_required {
                let cmd = install_cmd(dep);
                println!("  Running: {}", cmd);
                if cmd.contains('|') || cmd.contains("curl") {
                    let _ = std::process::Command::new("sh").args(["-c", cmd]).status();
                } else {
                    let parts: Vec<&str> = cmd.split_whitespace().collect();
                    if !parts.is_empty() {
                        let _ = std::process::Command::new(parts[0])
                            .args(&parts[1..])
                            .status();
                    }
                }
            }
            println!();
        } else {
            eprintln!(
                "  Cannot continue without required dependencies. Install them and re-run setup."
            );
            std::process::exit(1);
        }
    }

    if !missing_optional.is_empty() {
        println!();
        println!("  Optional dependencies not found:");
        for dep in &missing_optional {
            println!("    {} — install: {}", dep.name, install_cmd(dep));
            if !dep.alt_hint.is_empty() {
                println!("      {}", dep.alt_hint);
            }
        }
        println!();
        if confirm("Install optional dependencies now?") {
            for dep in &missing_optional {
                let cmd = install_cmd(dep);
                println!("  Installing {}...", dep.name);
                if cmd.contains('|') || cmd.contains("curl") {
                    let _ = std::process::Command::new("sh").args(["-c", cmd]).status();
                } else {
                    let parts: Vec<&str> = cmd.split_whitespace().collect();
                    if !parts.is_empty() {
                        let _ = std::process::Command::new(parts[0])
                            .args(&parts[1..])
                            .status();
                    }
                }
            }
            println!();
        }
    }
}

/// Strip JSONC comments (`// line` and `/* block */`) from text so it can be
/// parsed by a strict JSON parser (serde_json). String literals are preserved —
/// a `//` or `/*` inside a string value (e.g. a URL) is NOT treated as a
/// comment. Multi-byte UTF-8 content is preserved by working on `char`s.
/// This lets setup read+preserve the user's existing opencode config
/// (commonly written as JSONC) instead of falling back to `{}` and destroying
/// it on parse failure.
fn strip_jsonc_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.char_indices().peekable();
    let mut in_string = false;

    while let Some((_, c)) = chars.next() {
        if in_string {
            out.push(c);
            if c == '\\' {
                // Escape: copy the next char verbatim so an escaped quote (\")
                // doesn't terminate the string.
                if let Some((_, escaped)) = chars.next() {
                    out.push(escaped);
                }
                continue;
            }
            if c == '"' {
                in_string = false;
            }
            continue;
        }

        // Not in a string.
        if c == '"' {
            in_string = true;
            out.push('"');
            continue;
        }

        if c == '/' {
            match chars.peek() {
                Some((_, '/')) => {
                    // Line comment: consume the '/' and everything to end of
                    // line. Keep the newline so line numbers stay sane.
                    chars.next(); // consume second '/'
                    while let Some((_, cc)) = chars.next() {
                        if cc == '\n' {
                            out.push('\n');
                            break;
                        }
                    }
                    continue;
                }
                Some((_, '*')) => {
                    // Block comment: consume to '*/'. Emit a newline so adjacent
                    // tokens stay separated (e.g. "a",/*x*/"b" -> "a",\n"b").
                    chars.next(); // consume '*'
                    let mut closed = false;
                    while let Some((_, cc)) = chars.next() {
                        if cc == '*' {
                            if let Some((_, '/')) = chars.peek() {
                                chars.next(); // consume '/'
                                closed = true;
                                break;
                            }
                        }
                    }
                    let _ = closed;
                    out.push('\n');
                    continue;
                }
                _ => {}
            }
        }

        out.push(c);
    }

    out
}

/// Parse JSONC config text into a serde_json::Value, stripping comments first.
/// Returns `None` (instead of an empty object) when parsing fails, so callers
/// can refuse to overwrite unreadable config rather than silently destroying it.
fn parse_jsonc_config(content: &str) -> Option<serde_json::Value> {
    let stripped = strip_jsonc_comments(content);
    serde_json::from_str(&stripped).ok()
}

/// Decide which opencode config file to read/write.
///
/// opencode reads both `opencode.jsonc` (canonical, modern) and `opencode.json`
/// (legacy) and merges them. To avoid creating a confusing second file and to
/// correctly detect an existing graphiq entry, we prefer the file that already
/// exists (`.jsonc` first), and create `.jsonc` when neither exists.
fn opencode_config_path(home: &std::path::Path) -> std::path::PathBuf {
    let dir = home.join(".config").join("opencode");
    let jsonc = dir.join("opencode.jsonc");
    let json = dir.join("opencode.json");
    if jsonc.exists() {
        jsonc
    } else if json.exists() {
        json
    } else {
        jsonc
    }
}

// ─── graphiq sync: verify harness attach + reconcile graphiq storage ─────────
//
// `graphiq sync` reports the TRUE attach state of every supported harness
// (is graphiq wired into the config setup targets?), confirms `graphiq-mcp` is
// reachable, and writes a graphiq-owned registry recording what it found. It is
// a read-only health check + storage reconcile; to (re)apply a config, run
// `graphiq setup`. This directly addresses the plan's "does not reliably attach"
// and "synced back with graphiq storage" requirements.

/// Minimal JSONC `//` line-comment + `/* */` block-comment stripper for the read
/// path (opencode configs are JSONC). String literals are preserved, and it is
/// char-based so multi-byte UTF-8 round-trips intact.
fn sync_strip_jsonc(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.char_indices().peekable();
    let mut in_string = false;
    while let Some((_, c)) = chars.next() {
        if in_string {
            out.push(c);
            if c == '\\' {
                if let Some((_, e)) = chars.next() {
                    out.push(e);
                }
                continue;
            }
            if c == '"' {
                in_string = false;
            }
            continue;
        }
        if c == '"' {
            in_string = true;
            out.push('"');
            continue;
        }
        if c == '/' {
            match chars.peek() {
                Some((_, '/')) => {
                    chars.next();
                    while let Some((_, cc)) = chars.next() {
                        if cc == '\n' {
                            out.push('\n');
                            break;
                        }
                    }
                    continue;
                }
                Some((_, '*')) => {
                    chars.next();
                    while let Some((_, cc)) = chars.next() {
                        if cc == '*' && matches!(chars.peek(), Some((_, '/'))) {
                            chars.next();
                            break;
                        }
                    }
                    out.push('\n');
                    continue;
                }
                _ => {}
            }
        }
        out.push(c);
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttachShape {
    /// top-level `mcpServers.graphiq` (Claude Desktop, Claude Code, Cursor, Windsurf).
    McpServers,
    /// top-level `mcp.graphiq`, JSONC (OpenCode).
    Mcp,
    /// TOML `[mcp_servers.graphiq]` (Codex).
    CodexToml,
    /// pi does not use MCP; attach is the graphiq-pi extension file existing in
    /// ~/.pi/agent/extensions/.
    PiExtension,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AttachState {
    /// Config exists and has a graphiq MCP entry.
    Connected,
    /// Config exists but has no graphiq entry.
    NotConfigured,
    /// Config exists but could not be parsed.
    ParseFailed,
}

/// Pure: given a harness config's text and its shape, decide whether graphiq is
/// wired in. Robust to JSONC comments for the JSON shapes; the TOML shape matches
/// the `[mcp_servers.graphiq]` section header with a terminator so it does not
/// false-positive on `[mcp_servers.graphiqfoo]`.
fn detect_attach(content: &str, shape: AttachShape) -> AttachState {
    match shape {
        AttachShape::McpServers | AttachShape::Mcp => {
            let key = match shape {
                AttachShape::McpServers => "mcpServers",
                _ => "mcp",
            };
            let stripped = sync_strip_jsonc(content);
            match serde_json::from_str::<serde_json::Value>(&stripped) {
                Ok(v) => {
                    let has = v.get(key).and_then(|m| m.get("graphiq")).is_some();
                    if has {
                        AttachState::Connected
                    } else {
                        AttachState::NotConfigured
                    }
                }
                Err(_) => AttachState::ParseFailed,
            }
        }
        AttachShape::CodexToml => {
            // Match the section header with a terminator (] or .) so
            // [mcp_servers.graphiqfoo] does not false-positive.
            let has = content.lines().any(|l| {
                let t = l.trim();
                t == "[mcp_servers.graphiq]"
                    || t.starts_with("[mcp_servers.graphiq.")
                    || t.starts_with("[mcp_servers.graphiq ")
            });
            if has {
                AttachState::Connected
            } else {
                AttachState::NotConfigured
            }
        }
        AttachShape::PiExtension => {
            // pi attach = the graphiq-pi extension file exists (content isn't a
            // config we parse; it's the extension source). The `content` here is
            // the file's text — connected if it looks like the graphiq extension.
            if content.contains("GraphIQ extension for pi") || content.contains("graphiq_search") {
                AttachState::Connected
            } else {
                AttachState::NotConfigured
            }
        }
    }
}

/// One harness attach target: display name, config path, and config shape.
struct HarnessTarget {
    name: &'static str,
    path: PathBuf,
    shape: AttachShape,
}

/// Resolve the config-file targets for all supported harnesses. Paths mirror
/// what `setup` writes so sync reads the same files setup configures (this is
/// what makes sync an accurate check, not a parallel source of truth).
fn harness_targets(home: &std::path::Path, project: &std::path::Path) -> Vec<HarnessTarget> {
    let mut out = Vec::new();
    if let Some(claude_desktop) =
        dirs::config_dir().map(|d| d.join("Claude").join("claude_desktop_config.json"))
    {
        out.push(HarnessTarget {
            name: "claude-desktop",
            path: claude_desktop,
            shape: AttachShape::McpServers,
        });
    }
    out.push(HarnessTarget {
        name: "claude-code",
        path: project.join(".claude").join(".mcp.json"),
        shape: AttachShape::McpServers,
    });
    // OpenCode: same resolution setup uses (prefer existing .jsonc, else .json).
    let oc_dir = home.join(".config").join("opencode");
    let oc_path = {
        let jsonc = oc_dir.join("opencode.jsonc");
        if jsonc.exists() {
            jsonc
        } else {
            oc_dir.join("opencode.json")
        }
    };
    out.push(HarnessTarget {
        name: "opencode",
        path: oc_path,
        shape: AttachShape::Mcp,
    });
    out.push(HarnessTarget {
        name: "codex",
        path: home.join(".codex").join("config.toml"),
        shape: AttachShape::CodexToml,
    });
    out.push(HarnessTarget {
        name: "cursor",
        path: project.join(".cursor").join("mcp.json"),
        shape: AttachShape::McpServers,
    });
    out.push(HarnessTarget {
        name: "windsurf",
        path: project.join(".windsurf").join("mcp.json"),
        shape: AttachShape::McpServers,
    });
    // pi: attach is the extension file in ~/.pi/agent/extensions/.
    out.push(HarnessTarget {
        name: "pi",
        path: home
            .join(".pi")
            .join("agent")
            .join("extensions")
            .join("graphiq-pi.ts"),
        shape: AttachShape::PiExtension,
    });
    out
}

/// Resolve the graphiq-mcp binary: sibling of the running `graphiq` exe, then
/// PATH lookup. Returns the path used (for the registry) or None.
fn resolve_graphiq_mcp() -> Option<PathBuf> {
    if let Some(sibling) = which_graphiq() {
        return Some(sibling);
    }
    // Fallback: PATH lookup. `which` is Unix; on Windows `where` is the tool.
    let probe = if cfg!(windows) { "where" } else { "which" };
    std::process::Command::new(probe)
        .arg("graphiq-mcp")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| {
            let line = s.lines().next()?.trim().to_string();
            if line.is_empty() {
                None
            } else {
                Some(PathBuf::from(line))
            }
        })
}

/// Graphiq-owned registry location ("graphiq storage").
fn graphiq_registry_path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("graphiq").join("registry.json"))
}

fn cmd_sync(project: Option<&std::path::Path>, harness_filter: Option<&str>, apply: bool) {
    use serde_json::{json, Value};

    let project_path = match project {
        Some(p) => p.to_path_buf(),
        None => match std::env::current_dir() {
            Ok(cwd) => cwd.canonicalize().unwrap_or(cwd),
            Err(_) => {
                eprintln!("error: cannot determine current directory");
                std::process::exit(1);
            }
        },
    };
    let home = dirs::home_dir().unwrap_or_else(|| std::path::Path::new(".").to_path_buf());

    println!("╭──────────────────────────────────────────────╮");
    println!("│            GraphIQ Sync                      │");
    println!("╰──────────────────────────────────────────────╯");
    println!("  Project: {}", project_path.display());

    // 1. Is graphiq-mcp reachable? This is the #1 attach failure.
    let mcp = resolve_graphiq_mcp();
    match &mcp {
        Some(p) => println!("  graphiq-mcp:  {}", p.display()),
        None => {
            eprintln!("  warning: graphiq-mcp is not reachable (not on PATH, not beside graphiq).");
            eprintln!("  Harnesses cannot attach without it. Reinstall graphiq, then re-run sync.");
            // Continue anyway: per-harness state is still useful diagnostics
            // when debugging a broken attach, and the registry records it.
        }
    }

    let filter = harness_filter.map(|f| f.to_lowercase());

    // Scan all harness targets once. Returns (connected, not_configured,
    // registry entries, printable rows) for the display + registry. Kept as a
    // closure so `--apply` can re-run setup and re-scan without duplicating the
    // ~50-line probe loop.
    //
    // The registry ALWAYS records every target's state, regardless of the
    // display filter — a focused report must not erase the source-of-truth
    // record for the other harnesses. The filter only narrows the printed rows.
    let scan = |home: &std::path::Path,
                project_path: &std::path::Path|
     -> (Vec<String>, Vec<String>, Vec<Value>, Vec<(String, String)>) {
        let targets = harness_targets(home, project_path);
        let mut connected: Vec<String> = Vec::new();
        let mut not_configured: Vec<String> = Vec::new();
        let mut registry_harnesses: Vec<Value> = Vec::new();
        let mut rows: Vec<(String, String)> = Vec::new();
        for t in &targets {
            let (state_char, state_label, detail) = if !t.path.exists() {
                (
                    "—",
                    "absent",
                    format!("{} (not configured)", t.path.display()),
                )
            } else {
                match std::fs::read_to_string(&t.path) {
                    Ok(content) => match detect_attach(&content, t.shape) {
                        AttachState::Connected => ("✓", "connected", t.path.display().to_string()),
                        AttachState::NotConfigured => {
                            ("✗", "missing", t.path.display().to_string())
                        }
                        AttachState::ParseFailed => ("✗", "unparsed", t.path.display().to_string()),
                    },
                    Err(_) => ("✗", "unreadable", t.path.display().to_string()),
                }
            };
            if state_label == "connected" {
                connected.push(t.name.to_string());
            } else if state_label != "absent" {
                not_configured.push(t.name.to_string());
            }
            registry_harnesses.push(json!({
                "name": t.name,
                "state": state_label,
                "configPath": t.path.display().to_string(),
            }));
            rows.push((
                t.name.to_string(),
                format!(
                    "  {:<16} {:<10} {}",
                    t.name,
                    format!("{} {}", state_char, state_label),
                    detail
                ),
            ));
        }
        (connected, not_configured, registry_harnesses, rows)
    };

    let (mut connected, mut not_configured, mut registry_harnesses, mut rows) =
        scan(&home, &project_path);

    // --apply: reconcile. The plan wants `graphiq sync` to re-apply the harness
    // configuration. For any discovered-but-unconfigured harness, re-run the
    // fully-tested `setup` writer (idempotent), then re-scan to confirm.
    if apply && !not_configured.is_empty() {
        println!();
        println!("  Re-applying graphiq config to missing harnesses via setup...");
        // setup is idempotent and scoped to installed harnesses; --skip-index
        // keeps it fast (sync never indexes).
        let setup_status = std::process::Command::new("graphiq")
            .arg("setup")
            .arg("--project")
            .arg(&project_path)
            .arg("--skip-index")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::inherit())
            .status();
        match setup_status {
            Ok(s) if s.success() => {
                let (c, nc, rh, r) = scan(&home, &project_path);
                connected = c;
                not_configured = nc;
                registry_harnesses = rh;
                rows = r;
                println!("  re-applied; re-scanning.");
            }
            Ok(s) => {
                eprintln!(
                    "  warning: `graphiq setup` exited non-zero ({:?}); not re-scanning.",
                    s.code()
                );
            }
            Err(e) => {
                eprintln!("  warning: could not run `graphiq setup`: {e}");
            }
        }
    }

    println!();
    println!("  {:<16} {:<10} {}", "harness", "state", "config");
    println!("  {:-<16} {:-<10} {:-<40}", "", "", "");
    for (name, row) in &rows {
        if let Some(ref f) = filter {
            if !name.to_lowercase().contains(f) {
                continue;
            }
        }
        println!("{}", row);
    }

    println!();
    if connected.is_empty() {
        println!("  No harnesses have graphiq wired in yet.");
        println!("  Run: graphiq setup --project {}", project_path.display());
    } else {
        println!("  connected: {}", connected.join(", "));
        if !not_configured.is_empty() {
            println!("  missing:   {}", not_configured.join(", "));
            println!(
                "  To wire them in: graphiq setup --project {}",
                project_path.display()
            );
        }
    }

    // 2. Reconcile graphiq storage (registry).
    if let Some(reg_path) = graphiq_registry_path() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let registry = json!({
            "version": env!("CARGO_PKG_VERSION"),
            "mcpBinary": mcp.as_ref().map(|p| p.display().to_string()),
            "projectRoot": project_path.display().to_string(),
            "syncedAt": now,
            "harnesses": registry_harnesses,
        });
        if let Some(parent) = reg_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::write(
            &reg_path,
            serde_json::to_string_pretty(&registry).unwrap_or_default(),
        ) {
            Ok(()) => println!("  registry:    {}", reg_path.display()),
            Err(e) => eprintln!("  warning: could not write registry: {e}"),
        }
    }
}

// ─── graphiq discover: top-down scan for installed agent harnesses ───────────
//
// `graphiq discover` inspects the user's computer to reliably find which agent
// harnesses are installed (codex, forge, pi, opencode, claude, cursor, hermes,
// windsurf, gemini, aider, openclaw). This is the plan's "top-down file search to
// reliably find the users harness installs". It checks three signal types per
// harness — config directory, binary on PATH, and macOS .app bundle — so it's
// robust to any single install method. `setup` can use it to scope which
// harnesses to configure instead of blindly writing configs that are ignored.

/// How a harness was detected. A harness may match more than one; we report the
/// strongest evidence first (config dir is the most reliable long-lived signal).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DetectReason {
    ConfigDir,
    BinaryOnPath,
    MacosApp,
}

/// The install signature for one harness: every place graphiq knows to look.
/// Kept data-only so detection is a pure function of (signature, probes).
struct HarnessSignature {
    name: &'static str,
    /// Human-readable label.
    label: &'static str,
    /// Config/config-directory path, relative to $HOME (e.g. ".codex").
    /// Empty = no config-dir signal.
    config_dir: &'static str,
    /// Binary name expected on PATH (e.g. "codex"). Empty = no PATH signal.
    binary: &'static str,
    /// macOS app bundle name under /Applications (e.g. "Cursor.app").
    /// Empty = no macOS-app signal.
    macos_app: &'static str,
}

/// One detected harness and why.
#[derive(Debug, Clone)]
struct DetectedHarness {
    name: String,
    label: String,
    reason: DetectReason,
    /// The concrete path/evidence that matched (for display + JSON).
    evidence: String,
}

/// The catalog of harnesses graphiq knows how to discover and configure.
/// Mirrors the set `setup` handles so discover and setup agree.
fn harness_catalog() -> Vec<HarnessSignature> {
    vec![
        HarnessSignature {
            name: "claude-desktop",
            label: "Claude Desktop",
            config_dir: "Library/Application Support/Claude",
            binary: "",
            macos_app: "Claude.app",
        },
        HarnessSignature {
            name: "claude-code",
            label: "Claude Code",
            config_dir: ".claude",
            binary: "claude",
            macos_app: "",
        },
        HarnessSignature {
            name: "codex",
            label: "Codex CLI",
            config_dir: ".codex",
            binary: "codex",
            macos_app: "",
        },
        HarnessSignature {
            name: "opencode",
            label: "OpenCode",
            config_dir: ".config/opencode",
            binary: "opencode",
            macos_app: "",
        },
        HarnessSignature {
            name: "pi",
            label: "Pi",
            config_dir: ".pi/agent",
            binary: "pi",
            macos_app: "",
        },
        HarnessSignature {
            name: "cursor",
            label: "Cursor",
            config_dir: ".cursor",
            binary: "",
            macos_app: "Cursor.app",
        },
        HarnessSignature {
            name: "hermes",
            label: "Hermes Agent",
            config_dir: ".hermes",
            binary: "hermes",
            macos_app: "",
        },
        HarnessSignature {
            name: "windsurf",
            label: "Windsurf",
            config_dir: ".windsurf",
            binary: "",
            macos_app: "Windsurf.app",
        },
        HarnessSignature {
            name: "gemini",
            label: "Gemini CLI",
            config_dir: ".gemini",
            binary: "gemini",
            macos_app: "",
        },
        HarnessSignature {
            name: "aider",
            label: "Aider",
            config_dir: ".aider.conf.yml",
            binary: "aider",
            macos_app: "",
        },
        HarnessSignature {
            name: "forge",
            label: "Forge",
            config_dir: ".forge",
            binary: "forge-code",
            macos_app: "Forge.app",
        },
        HarnessSignature {
            name: "openclaw",
            label: "OpenClaw",
            config_dir: ".openclaw",
            binary: "openclaw",
            macos_app: "",
        },
    ]
}

/// Pure detection: given a signature and three probe results (does the config
/// dir exist, is the binary on PATH, does the macOS app exist), return the
/// strongest reason found, or None. Order matters: ConfigDir is the most stable
/// signal (a binary may be renamed/uninstalled but the config persists), then
/// BinaryOnPath, then MacosApp.
fn detect_signature(
    sig: &HarnessSignature,
    config_exists: bool,
    config_path: &str,
    binary_on_path: bool,
    binary_path: &str,
    app_exists: bool,
    app_path: &str,
) -> Option<DetectedHarness> {
    if config_exists && !sig.config_dir.is_empty() {
        return Some(DetectedHarness {
            name: sig.name.to_string(),
            label: sig.label.to_string(),
            reason: DetectReason::ConfigDir,
            evidence: config_path.to_string(),
        });
    }
    if binary_on_path && !sig.binary.is_empty() {
        return Some(DetectedHarness {
            name: sig.name.to_string(),
            label: sig.label.to_string(),
            reason: DetectReason::BinaryOnPath,
            evidence: binary_path.to_string(),
        });
    }
    if app_exists && !sig.macos_app.is_empty() {
        return Some(DetectedHarness {
            name: sig.name.to_string(),
            label: sig.label.to_string(),
            reason: DetectReason::MacosApp,
            evidence: app_path.to_string(),
        });
    }
    None
}

/// Resolve the full path for a harness's config-dir signal, given a home dir.
/// Returns None if the signature has no config-dir signal.
fn config_dir_path(home: &std::path::Path, sig: &HarnessSignature) -> Option<PathBuf> {
    if sig.config_dir.is_empty() {
        None
    } else {
        Some(home.join(sig.config_dir))
    }
}

/// Check whether `binary` is on PATH. Returns the resolved path if found.
/// Uses `which` on Unix and `where` on Windows so it works cross-platform.
fn binary_on_path(binary: &str) -> Option<PathBuf> {
    if binary.is_empty() {
        return None;
    }
    let probe = if cfg!(windows) { "where" } else { "which" };
    std::process::Command::new(probe)
        .arg(binary)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| {
            let line = s.lines().next()?.trim().to_string();
            if line.is_empty() {
                None
            } else {
                Some(PathBuf::from(line))
            }
        })
}

/// Run a full discovery scan against the real filesystem. Returns one entry per
/// installed harness (strongest signal only), sorted by name for stable output.
fn run_discovery(home: &std::path::Path) -> Vec<DetectedHarness> {
    let mut found = Vec::new();
    for sig in harness_catalog() {
        let (config_exists, config_path) = match config_dir_path(home, &sig) {
            Some(p) => (p.exists(), p.display().to_string()),
            None => (false, String::new()),
        };
        let (binary_on_path_flag, binary_path) = match binary_on_path(sig.binary) {
            Some(p) => (true, p.display().to_string()),
            None => (false, String::new()),
        };
        // macOS app bundles only exist on macOS; skip the probe on other OSes.
        let (app_exists, app_path) = if !sig.macos_app.is_empty() && cfg!(target_os = "macos") {
            let p = PathBuf::from("/Applications").join(sig.macos_app);
            (p.exists(), p.display().to_string())
        } else {
            (false, String::new())
        };
        if let Some(d) = detect_signature(
            &sig,
            config_exists,
            &config_path,
            binary_on_path_flag,
            &binary_path,
            app_exists,
            &app_path,
        ) {
            found.push(d);
        }
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found
}

fn cmd_discover(json: bool) {
    use serde_json::json;

    let home = dirs::home_dir().unwrap_or_else(|| std::path::Path::new(".").to_path_buf());
    let found = run_discovery(&home);

    if json {
        // Machine-readable: stable for scripts and for `graphiq sync`/setup to consume.
        let entries: Vec<_> = found
            .iter()
            .map(|d| {
                json!({
                    "name": d.name,
                    "label": d.label,
                    "reason": match d.reason {
                        DetectReason::ConfigDir => "config-dir",
                        DetectReason::BinaryOnPath => "binary",
                        DetectReason::MacosApp => "macos-app",
                    },
                    "evidence": d.evidence,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "harnesses": entries, "count": entries.len() }))
                .unwrap_or_default()
        );
        return;
    }

    println!("╭──────────────────────────────────────────────╮");
    println!("│            GraphIQ Discover                  │");
    println!("╰──────────────────────────────────────────────╯");
    if found.is_empty() {
        println!("  No supported harnesses detected.");
        return;
    }
    println!("  Detected {} installed harness(es):\n", found.len());
    println!("  {:<16} {:<12} {}", "harness", "via", "evidence");
    println!("  {:-<16} {:-<12} {:-<40}", "", "", "");
    for d in &found {
        let via = match d.reason {
            DetectReason::ConfigDir => "config-dir",
            DetectReason::BinaryOnPath => "binary",
            DetectReason::MacosApp => "macos-app",
        };
        println!("  {:<16} {:<12} {}", d.name, via, d.evidence);
    }
    println!();
    println!("  To configure graphiq for all of these:");
    println!("    graphiq setup --project <path>");
    println!("  Then verify with:");
    println!("    graphiq sync");
}

// ─── graphiq git: current project scope for agents ───────────────────────────
//
// The plan wants graphiq to "index every pushed commit, and sync it back with
// the local harness so search results are accurate and up-to-date with the
// current project scope". In practice an agent needs to KNOW the scope first —
// branch, clean/dirty state, ahead/behind, recent commits, active changes —
// then re-index as needed. `graphiq git` surfaces exactly that from git itself.
// It is read-only (never mutates the repo) and works in any git project.

/// A recent commit in the project history.
#[derive(Debug, Clone)]
struct RecentCommit {
    sha: String,
    subject: String,
    author: String,
    date: String,
}

/// Run git in a project dir and return its UTF-8 stdout, or an error message.
/// Kept in the CLI (not core) so the pure parsers below can be unit-tested
/// without a real repo: they take the already-fetched string output.
fn git_in(project: &std::path::Path, args: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(project)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!("git {:?} failed", args)
        } else {
            stderr
        });
    }
    String::from_utf8(output.stdout).map_err(|e| format!("git output was not valid UTF-8: {e}"))
}

/// Parse `git status -b --porcelain` branch line into branch name + ahead/behind.
/// Pure: takes the porcelain output string.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct BranchStatus {
    branch: String,
    ahead: usize,
    behind: usize,
    /// true if HEAD is detached (no branch checked out).
    detached: bool,
}

fn parse_branch_status(status_b_output: &str) -> BranchStatus {
    let first = status_b_output.lines().next().unwrap_or("").trim();
    let line = first.strip_prefix("## ").unwrap_or(first);
    let mut bs = BranchStatus::default();
    if line.starts_with("HEAD (no branch)") {
        bs.detached = true;
        bs.branch = "(detached)".to_string();
    } else if line.starts_with("No commits yet on ") {
        bs.branch = line.trim_start_matches("No commits yet on ").to_string();
    } else {
        let branch_end = line.find("...").unwrap_or_else(|| line.len());
        bs.branch = line[..branch_end]
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_string();
    }
    if let Some(open) = line.rfind('[') {
        if let Some(close) = line[open..].find(']') {
            let bracket = &line[open + 1..open + close];
            for tok in bracket.split(',') {
                let tok = tok.trim();
                if let Some(rest) = tok.strip_prefix("ahead ") {
                    bs.ahead = rest.parse().unwrap_or(0);
                } else if let Some(rest) = tok.strip_prefix("behind ") {
                    bs.behind = rest.parse().unwrap_or(0);
                }
            }
        }
    }
    bs
}

/// Count working-tree changes from `git status --porcelain` output. Pure.
/// Returns (staged, unstaged, untracked) counts.
fn count_changes(porcelain_output: &str) -> (usize, usize, usize) {
    let mut staged = 0usize;
    let mut unstaged = 0usize;
    let mut untracked = 0usize;
    for line in porcelain_output.lines() {
        if line.len() < 2 {
            continue;
        }
        let x = line.as_bytes()[0];
        let y = line.as_bytes()[1];
        if x == b'?' && y == b'?' {
            untracked += 1;
            continue;
        }
        if x != b' ' && x != b'?' {
            staged += 1;
        }
        if y != b' ' && y != b'?' {
            unstaged += 1;
        }
    }
    (staged, unstaged, untracked)
}

/// Parse `git log --format=%H%x00%s%x00%an%x00%ad --date=short` output into
/// commits. Fields are NUL-separated. Pure.
fn parse_commits(log_output: &str, limit: usize) -> Vec<RecentCommit> {
    log_output
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('\u{0}').collect();
            if parts.len() < 4 {
                return None;
            }
            Some(RecentCommit {
                sha: parts[0].to_string(),
                subject: parts[1].to_string(),
                author: parts[2].to_string(),
                date: parts[3].to_string(),
            })
        })
        .take(limit)
        .collect()
}

/// Summarize working-tree state as a single human word. Pure.
fn tree_state_label(staged: usize, unstaged: usize, untracked: usize) -> &'static str {
    if staged + unstaged + untracked == 0 {
        "clean"
    } else {
        "dirty"
    }
}

fn cmd_git(project: &std::path::Path, commits: usize, json: bool) {
    use serde_json::json;

    if git_in(project, &["rev-parse", "--is-inside-work-tree"]).is_err() {
        eprintln!("error: {} is not inside a git work tree", project.display());
        std::process::exit(1);
    }

    let remote_url = git_in(project, &["remote", "get-url", "origin"])
        .ok()
        .map(|s| s.trim().to_string());
    let status_b = git_in(project, &["status", "-b", "--porcelain"]).unwrap_or_default();
    let porcelain = git_in(project, &["status", "--porcelain"]).unwrap_or_default();
    let head_sha = git_in(project, &["rev-parse", "--short", "HEAD"])
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let log = git_in(
        project,
        &[
            "log",
            "--format=%H%x00%s%x00%an%x00%ad",
            "--date=short",
            &format!("-n{}", commits.max(1)),
        ],
    )
    .unwrap_or_default();

    let branch = parse_branch_status(&status_b);
    let (staged, unstaged, untracked) = count_changes(&porcelain);
    let recent = parse_commits(&log, commits.max(1));
    let state = tree_state_label(staged, unstaged, untracked);

    if json {
        let commits_json: Vec<_> = recent
            .iter()
            .map(|c| {
                json!({ "sha": c.sha, "subject": c.subject, "author": c.author, "date": c.date })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "project": project.display().to_string(),
                "remote": remote_url,
                "branch": branch.branch,
                "detached": branch.detached,
                "head": head_sha,
                "ahead": branch.ahead,
                "behind": branch.behind,
                "treeState": state,
                "changes": { "staged": staged, "unstaged": unstaged, "untracked": untracked },
                "commits": commits_json,
            }))
            .unwrap_or_default()
        );
        return;
    }

    println!("╭──────────────────────────────────────────────╮");
    println!("│            GraphIQ Git Scope                 │");
    println!("╰──────────────────────────────────────────────╯");
    println!("  Project: {}", project.display());
    if let Some(r) = &remote_url {
        println!("  Remote:  {}", r);
    }
    if branch.detached {
        println!("  HEAD:    {} (detached)", head_sha);
    } else {
        let ab = match (branch.ahead, branch.behind) {
            (0, 0) => String::new(),
            (a, 0) => format!(" (ahead {a})"),
            (0, b) => format!(" (behind {b})"),
            (a, b) => format!(" (ahead {a}, behind {b})"),
        };
        println!("  Branch:  {}{}", branch.branch, ab);
        println!("  HEAD:    {}", head_sha);
    }
    println!("  Tree:    {}", state);
    if staged + unstaged + untracked > 0 {
        println!(
            "  Changes: {} staged, {} unstaged, {} untracked",
            staged, unstaged, untracked
        );
    }
    if !recent.is_empty() {
        println!();
        println!("  Recent commits:");
        for c in &recent {
            let short_sha = &c.sha[..c.sha.len().min(7)];
            let subj = if c.subject.len() > 64 {
                format!("{}…", &c.subject[..62])
            } else {
                c.subject.clone()
            };
            println!("    {} {} — {} ({})", short_sha, subj, c.author, c.date);
        }
    }
    println!();
    println!("  Keep search in sync with the current scope:");
    println!("    graphiq index {} --force-reindex", project.display());
}

// ─── graphiq projects: multi-project tracking across sessions ────────────────
//
// The plan: graphiq must "move around reliably during each session, with each
// user project... retain existing information for when a user calls back to
// that project again in a new session". Project-local .graphiq/graphiq.db
// already retains each index; this command adds the missing piece — a central,
// persistent registry of every project graphiq has indexed, so agents and users
// can list them, see which still have a fresh index, and return to one without
// re-indexing from scratch.

/// One tracked project in the registry.
#[derive(Debug, Clone)]
struct TrackedProject {
    path: String,
    /// ISO-8601 timestamp of when it was last indexed (or first seen).
    last_indexed_at: String,
    /// File-count snapshot at last index time (0 if unknown).
    files: u64,
    /// Symbol-count snapshot at last index time.
    symbols: u64,
}

/// Pure: parse the projects registry JSON into a list. Tolerates missing/
/// malformed files (returns empty). Sorted by path for stable output.
fn parse_projects_registry(content: &str) -> Vec<TrackedProject> {
    let v: serde_json::Value = match serde_json::from_str(content) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    if let Some(arr) = v.get("projects").and_then(|p| p.as_array()) {
        for entry in arr {
            let path = entry
                .get("path")
                .and_then(|p| p.as_str())
                .unwrap_or("")
                .to_string();
            if path.is_empty() {
                continue;
            }
            out.push(TrackedProject {
                path,
                last_indexed_at: entry
                    .get("lastIndexedAt")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string(),
                files: entry.get("files").and_then(|f| f.as_u64()).unwrap_or(0),
                symbols: entry.get("symbols").and_then(|s| s.as_u64()).unwrap_or(0),
            });
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// Pure: upsert a project into a registry list (replace if same path, else add).
/// Returns the new list sorted by path.
fn upsert_project(mut projects: Vec<TrackedProject>, p: TrackedProject) -> Vec<TrackedProject> {
    if let Some(existing) = projects.iter_mut().find(|t| t.path == p.path) {
        *existing = p;
    } else {
        projects.push(p);
    }
    projects.sort_by(|a, b| a.path.cmp(&b.path));
    projects
}

/// Pure: remove a project by exact path. Returns the filtered list.
fn forget_project(projects: Vec<TrackedProject>, path: &str) -> Vec<TrackedProject> {
    projects.into_iter().filter(|t| t.path != path).collect()
}

/// Pure: serialize the registry list to pretty JSON.
fn serialize_projects_registry(projects: &[TrackedProject]) -> String {
    use serde_json::json;
    let entries: Vec<_> = projects
        .iter()
        .map(|p| {
            json!({
                "path": p.path,
                "lastIndexedAt": p.last_indexed_at,
                "files": p.files,
                "symbols": p.symbols,
            })
        })
        .collect();
    serde_json::to_string_pretty(&json!({ "projects": entries }))
        .unwrap_or_else(|_| "{\"projects\":[]}".to_string())
}

/// Location of the persistent multi-project registry.
fn projects_registry_path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("graphiq").join("projects.json"))
}

/// Load the current registry from disk (empty if absent/malformed).
fn load_projects_registry() -> Vec<TrackedProject> {
    match projects_registry_path().and_then(|p| std::fs::read_to_string(p).ok()) {
        Some(content) => parse_projects_registry(&content),
        None => Vec::new(),
    }
}

/// Does a project still have a usable on-disk index?
fn project_index_health(project_path: &std::path::Path) -> &'static str {
    let db = project_path.join(".graphiq").join("graphiq.db");
    if !db.exists() {
        return "no-index";
    }
    // Try a quick symbol count to confirm it's not empty/corrupt.
    if let Ok(conn) = rusqlite::Connection::open(&db) {
        if let Ok(n) = conn.query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get::<_, i64>(0)) {
            return if n > 0 { "ready" } else { "empty" };
        }
    }
    "ready" // file exists but we couldn't probe; assume ready
}

/// Record (or refresh) a project in the persistent registry after a successful
/// index. Best-effort: any failure is swallowed so it never breaks indexing.
/// Captures a snapshot of files/symbols so `graphiq projects` can show at a
/// glance whether an index is populated.
fn record_indexed_project(path: &std::path::Path, db: &graphiq_core::db::GraphDb) {
    let Some(reg_path) = projects_registry_path() else {
        return;
    };
    let canonical = match path.canonicalize() {
        Ok(p) => p.display().to_string(),
        Err(_) => path.display().to_string(),
    };
    let (files, symbols) = match db.stats() {
        Ok(s) => (s.files as u64, s.symbols as u64),
        Err(_) => (0, 0),
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let iso = format_unix_ts_iso(now);
    let entry = TrackedProject {
        path: canonical,
        last_indexed_at: iso,
        files,
        symbols,
    };
    let updated = upsert_project(load_projects_registry(), entry);
    if let Some(parent) = reg_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&reg_path, serialize_projects_registry(&updated));
}

/// Format a unix timestamp as an ISO-8601 UTC string (no deps). Pure.
fn format_unix_ts_iso(secs: u64) -> String {
    let days_since_epoch = (secs / 86400) as i64;
    let sod = (secs % 86400) as u64;
    let h = sod / 3600;
    let m = (sod % 3600) / 60;
    let s = sod % 60;
    // Civil date from days-since-epoch (Howard Hinnant's algorithm).
    let z = days_since_epoch + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = if month <= 2 { y + 1 } else { y };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, d, h, m, s
    )
}

fn cmd_projects(json: bool, forget: Option<&str>) {
    let reg_path = match projects_registry_path() {
        Some(p) => p,
        None => {
            eprintln!("error: cannot determine data directory for the projects registry");
            std::process::exit(1);
        }
    };

    // --forget <path|all>
    if let Some(target) = forget {
        let mut projects = load_projects_registry();
        let before = projects.len();
        if target == "all" {
            projects.clear();
        } else {
            let resolved = std::path::Path::new(target)
                .canonicalize()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| target.to_string());
            projects = forget_project(projects, &resolved);
        }
        if let Some(parent) = reg_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&reg_path, serialize_projects_registry(&projects)).ok();
        println!(
            "Forgot {} project(s). {} tracked remaining.",
            before - projects.len(),
            projects.len()
        );
        return;
    }

    let projects = load_projects_registry();
    if projects.is_empty() {
        if json {
            println!("{}", serialize_projects_registry(&[]));
        } else {
            println!("No tracked projects yet.");
            println!("Index one with: graphiq index /path/to/project");
        }
        return;
    }

    if json {
        println!("{}", serialize_projects_registry(&projects));
        return;
    }

    println!("╭──────────────────────────────────────────────╮");
    println!("│            GraphIQ Projects                  │");
    println!("╰──────────────────────────────────────────────╯");
    println!("  {} tracked project(s):\n", projects.len());
    println!(
        "  {:<48} {:<8} {:<10} {}",
        "project", "health", "symbols", "last indexed"
    );
    println!("  {:-<48} {:-<8} {:-<10} {:-<20}", "", "", "", "");
    for p in &projects {
        let path = std::path::Path::new(&p.path);
        let health = if path.exists() {
            project_index_health(path)
        } else {
            "gone"
        };
        let display_path = if p.path.len() > 46 {
            format!("…{}", &p.path[p.path.len().saturating_sub(45)..])
        } else {
            p.path.clone()
        };
        println!(
            "  {:<48} {:<8} {:<10} {}",
            display_path, health, p.symbols, p.last_indexed_at
        );
    }
    println!();
    println!("  Return to a project: cd <path> && graphiq search <query>");
    println!("  Forget one:          graphiq projects --forget <path>");
}

fn cmd_setup(
    project: Option<&std::path::Path>,
    skip_index: bool,
    ephemeral: bool,
    persistent: bool,
    harness_filter: Option<&str>,
    dry_run: bool,
) {
    use serde_json::{json, Value};

    let ephemeral = !persistent || ephemeral;

    fn pretty(v: &Value) -> String {
        serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
    }

    fn write_mcp_json_config(
        config_path: &std::path::Path,
        project_str: &str,
        ephemeral: bool,
        label: &str,
        top_level_key: &str,
    ) -> Result<bool, String> {
        let mut args = vec![project_str.to_string()];
        if ephemeral {
            args.push("--ephemeral".to_string());
        }
        let entry = json!({
            "command": "graphiq-mcp",
            "args": args
        });

        let (config, written) = if config_path.exists() {
            match std::fs::read_to_string(config_path) {
                Ok(content) => {
                    let mut parsed: Value = serde_json::from_str(&content).unwrap_or(json!({}));
                    let servers = parsed
                        .as_object_mut()
                        .unwrap()
                        .entry(top_level_key)
                        .or_insert_with(|| json!({}))
                        .as_object_mut()
                        .unwrap();
                    let already = servers.contains_key("graphiq");
                    servers.insert("graphiq".into(), entry);
                    (pretty(&parsed), !already)
                }
                Err(_) => {
                    let obj = json!({ top_level_key: { "graphiq": entry } });
                    (pretty(&obj), true)
                }
            }
        } else {
            let obj = json!({ top_level_key: { "graphiq": entry } });
            (pretty(&obj), true)
        };

        if let Some(parent) = config_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(config_path, &config)
            .map_err(|e| format!("{}: failed to write config: {e}", label))?;
        Ok(written)
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
    if ephemeral {
        println!("  Index storage: temp MCP database (--persistent opts into .graphiq)");
    } else {
        println!("  Index storage: project .graphiq database");
    }
    println!();

    let graphiq_bin = which_graphiq();
    if graphiq_bin.is_none() {
        let found = std::process::Command::new("which")
            .arg("graphiq-mcp")
            .output()
            .ok()
            .filter(|o| o.status.success());
        if found.is_none() {
            eprintln!("  error: graphiq-mcp not found on PATH.");
            eprintln!("  Install with: cargo install --path . --bin graphiq-mcp");
            eprintln!("  Or: curl -fsSL https://raw.githubusercontent.com/aaf2tbz/graphiq/main/install.sh | bash");
            eprintln!("  Then re-run setup.");
            std::process::exit(1);
        }
    }

    check_build_dependencies();

    if dry_run {
        println!("  [dry-run] No files will be written.");
        match harness_filter {
            Some(harness) => println!("  [dry-run] Would configure harnesses matching: {harness}"),
            None => println!("  [dry-run] Would configure all supported harnesses"),
        }
        if ephemeral {
            println!("  [dry-run] Would use temp-backed MCP indexes");
            println!("  [dry-run] Would skip upfront project index");
        } else {
            println!(
                "  [dry-run] Would create {}",
                project_path.join(".graphiq").display()
            );
            println!("  [dry-run] Would rebuild the project-local GraphIQ index");
        }
        println!();
        println!("── Ready ──");
        println!();
        return;
    }

    let mut configured: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut failed: Vec<String> = Vec::new();

    let harness_filter_lower = harness_filter.map(|h| h.to_lowercase());

    let should_configure = |name: &str| -> bool {
        match &harness_filter_lower {
            Some(f) => name.to_lowercase().contains(f),
            None => true,
        }
    };

    let _binary_on_path = |name: &str| -> bool {
        std::process::Command::new("which")
            .arg(name)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    };

    // Claude Desktop
    if should_configure("claude-desktop") {
        let claude_config =
            dirs::config_dir().map(|d| d.join("Claude").join("claude_desktop_config.json"));

        if let Some(ref config_path) = claude_config {
            if config_path.exists() || config_path.parent().map_or(false, |p| p.exists()) {
                let project_str = project_path.display().to_string();
                match write_mcp_json_config(
                    config_path,
                    &project_str,
                    ephemeral,
                    "Claude Desktop",
                    "mcpServers",
                ) {
                    Ok(written) => {
                        let status = if written { "configured" } else { "updated" };
                        println!("  Claude Desktop: {} {}", status, config_path.display());
                        configured.push("Claude Desktop".to_string());
                    }
                    Err(e) => {
                        eprintln!("  {}", e);
                        failed.push("Claude Desktop".to_string());
                    }
                }
            }
        }
    } else {
        skipped.push("Claude Desktop".to_string());
    }

    // Claude Code
    if should_configure("claude-code") {
        let claude_code_config = project_path.join(".claude").join(".mcp.json");
        let project_str = project_path.display().to_string();
        match write_mcp_json_config(
            &claude_code_config,
            &project_str,
            ephemeral,
            "Claude Code",
            "mcpServers",
        ) {
            Ok(written) => {
                let status = if written { "configured" } else { "updated" };
                println!(
                    "  Claude Code:   {} {}",
                    status,
                    claude_code_config.display()
                );
                configured.push("Claude Code".to_string());
            }
            Err(e) => {
                eprintln!("  {}", e);
                failed.push("Claude Code".to_string());
            }
        }
    } else {
        skipped.push("Claude Code".to_string());
    }

    // OpenCode
    if should_configure("opencode") {
        // opencode reads opencode.jsonc (canonical) and opencode.json (legacy)
        // and merges them. Write to whichever already exists (prefer .jsonc) so
        // we don't spawn a second file, and parse JSONC safely so a commented
        // config is preserved rather than destroyed.
        let opencode_config = dirs::home_dir()
            .map(|h| opencode_config_path(&h))
            .and_then(|p| {
                // Only proceed if the opencode config directory itself exists —
                // creating a brand-new opencode.jsonc in a missing dir would
                // imply opencode is installed when it may not be.
                if p.parent().map_or(false, |d| d.exists()) {
                    Some(p)
                } else {
                    None
                }
            });

        if let Some(ref config_path) = opencode_config {
            // Wrapped in a labeled block so recoverable opencode errors
            // (unreadable/unparseable config) skip ONLY this harness via
            // `break 'opencode` — they must not abort the whole setup run.
            'opencode: {
                let project_str = project_path.display().to_string();
                let mut cmd = vec!["graphiq-mcp".to_string(), project_str.clone()];
                if ephemeral {
                    cmd.push("--ephemeral".to_string());
                }
                let entry = json!({
                    "type": "local",
                    "command": cmd,
                    "enabled": true
                });

                // Read existing config. If it exists but can't be parsed (even after
                // JSONC stripping), DO NOT fall back to {} — that would overwrite
                // and destroy the user's config. Instead, treat it as a hard error.
                let mut parsed: Value = if config_path.exists() {
                    match std::fs::read_to_string(config_path) {
                        Ok(content) => match parse_jsonc_config(&content) {
                            Some(v) => v,
                            None => {
                                eprintln!(
                                    "  opencode:      failed to parse existing config at {}",
                                    config_path.display()
                                );
                                eprintln!("  leaving it untouched; fix or back up the file and re-run setup.");
                                failed.push("opencode".to_string());
                                break 'opencode;
                            }
                        },
                        Err(e) => {
                            eprintln!("  opencode:      failed to read config: {e}");
                            failed.push("opencode".to_string());
                            break 'opencode;
                        }
                    }
                } else {
                    json!({})
                };

                // Ensure `parsed` is an object so we can insert the mcp key.
                if !parsed.is_object() {
                    eprintln!(
                    "  opencode:      existing config at {} is not a JSON object; leaving it untouched",
                    config_path.display()
                );
                    failed.push("opencode".to_string());
                    break 'opencode;
                }

                // Get or create the `mcp` object. If `mcp` exists but isn't an
                // object (e.g. "mcp": false), refuse to overwrite rather than panic.
                let mcp_obj = parsed
                    .as_object_mut()
                    .unwrap()
                    .entry("mcp")
                    .or_insert_with(|| json!({}));
                if !mcp_obj.is_object() {
                    eprintln!(
                    "  opencode:      existing `mcp` key in {} is not an object; leaving it untouched",
                    config_path.display()
                );
                    failed.push("opencode".to_string());
                    break 'opencode;
                }
                let mcp = mcp_obj.as_object_mut().unwrap();
                let already = mcp
                    .get("graphiq")
                    .and_then(|v| v.get("command"))
                    .and_then(|a| a.as_array())
                    .and_then(|arr| arr.get(1))
                    .and_then(|v| v.as_str())
                    .map_or(false, |s| s == project_str);
                mcp.insert("graphiq".into(), entry);
                let written = !already;
                let config = pretty(&parsed);

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
                        failed.push("opencode".to_string());
                    }
                }
            } // end 'opencode labeled block
        } else {
            skipped.push("opencode".to_string());
        }
    } else {
        skipped.push("opencode".to_string());
    }

    // Codex CLI
    if should_configure("codex") {
        let codex_config = dirs::home_dir().map(|d| d.join(".codex").join("config.toml"));

        if let Some(ref config_path) = codex_config {
            // Labeled block so a codex read failure skips ONLY codex, not the
            // rest of cmd_setup (Hermes/Cursor/Windsurf/Gemini/Aider/index).
            'codex: {
                let project_str = project_path.display().to_string();
                let args_suffix = if ephemeral { ", \"--ephemeral\"" } else { "" };

                let (content, written) = if config_path.exists() {
                    match std::fs::read_to_string(config_path) {
                        Ok(existing) => {
                            // Detect an existing graphiq section EXACTLY (with a
                            // terminator) so [mcp_servers.graphiqfoo] doesn't count.
                            let has_graphiq = existing.lines().any(|l| {
                                let t = l.trim();
                                t == "[mcp_servers.graphiq]"
                                    || t.starts_with("[mcp_servers.graphiq.")
                                    || t.starts_with("[mcp_servers.graphiq ")
                            });
                            let same_project = existing.contains(&project_str);
                            if has_graphiq && same_project {
                                (existing, false)
                            } else {
                                let mut cleaned = existing;
                                let section = format!(
                            "\n[mcp_servers.graphiq]\ncommand = \"graphiq-mcp\"\nargs = [\"{}\"{}]\nenabled = true\n",
                            project_str, args_suffix
                        );
                                cleaned.push_str(&section);
                                (cleaned, true)
                            }
                        }
                        Err(e) => {
                            eprintln!("  Codex:         failed to read config: {e}");
                            failed.push("Codex".to_string());
                            break 'codex;
                        }
                    }
                } else {
                    let section = format!(
                "[mcp_servers.graphiq]\ncommand = \"graphiq-mcp\"\nargs = [\"{}\"{}]\nenabled = true\n",
                project_str, args_suffix
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
                        failed.push("Codex".to_string());
                    }
                }
            } // end 'codex labeled block
        }
    } else {
        skipped.push("Codex".to_string());
    }

    // Hermes
    if should_configure("hermes") {
        let hermes_config = dirs::home_dir().map(|d| d.join(".hermes").join("config.yaml"));

        if let Some(ref config_path) = hermes_config {
            let project_str = project_path.display().to_string();
            let ephemeral_line = if ephemeral {
                "\n      - --ephemeral"
            } else {
                ""
            };

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
                                {ephemeral_line}\n    enabled: true",
                                project_str
                            );
                            re.replace(&existing, replacement.as_str()).to_string()
                        })
                        .unwrap_or_else(|_| existing.clone());
                            (updated, false)
                        } else {
                            let section = format!(
                            "\nmcp_servers:\n  graphiq:\n    command: graphiq-mcp\n    args:\n      - {}{ephemeral_line}\n    enabled: true\n",
                            project_str
                        );
                            let mut out = existing;
                            out.push_str(&section);
                            (out, true)
                        }
                    }
                    Err(e) => {
                        eprintln!("  Hermes:        failed to read config: {e}");
                        failed.push("Hermes".to_string());
                        return;
                    }
                }
            } else {
                let section = format!(
                "mcp_servers:\n  graphiq:\n    command: graphiq-mcp\n    args:\n      - {}{ephemeral_line}\n    enabled: true\n",
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
                    failed.push("Hermes".to_string());
                }
            }
        }
    } else {
        skipped.push("Hermes".to_string());
    }

    // Cursor
    if should_configure("cursor") {
        let cursor_config = project_path.join(".cursor").join("mcp.json");
        let project_str = project_path.display().to_string();
        match write_mcp_json_config(
            &cursor_config,
            &project_str,
            ephemeral,
            "Cursor",
            "mcpServers",
        ) {
            Ok(written) => {
                let status = if written { "configured" } else { "updated" };
                println!("  Cursor:        {} {}", status, cursor_config.display());
                configured.push("Cursor".to_string());
            }
            Err(e) => {
                eprintln!("  {}", e);
                failed.push("Cursor".to_string());
            }
        }
    } else {
        skipped.push("Cursor".to_string());
    }

    // Windsurf
    if should_configure("windsurf") {
        let windsurf_config = project_path.join(".windsurf").join("mcp.json");
        let project_str = project_path.display().to_string();
        match write_mcp_json_config(
            &windsurf_config,
            &project_str,
            ephemeral,
            "Windsurf",
            "mcpServers",
        ) {
            Ok(written) => {
                let status = if written { "configured" } else { "updated" };
                println!("  Windsurf:      {} {}", status, windsurf_config.display());
                configured.push("Windsurf".to_string());
            }
            Err(e) => {
                eprintln!("  {}", e);
                failed.push("Windsurf".to_string());
            }
        }
    } else {
        skipped.push("Windsurf".to_string());
    }

    // Gemini CLI
    if should_configure("gemini") {
        let gemini_config = dirs::home_dir().map(|d| d.join(".gemini").join("settings.json"));
        if let Some(ref config_path) = gemini_config {
            let project_str = project_path.display().to_string();
            let mut cmd = vec!["graphiq-mcp".to_string(), project_str.clone()];
            if ephemeral {
                cmd.push("--ephemeral".to_string());
            }
            let entry = json!({
                "mcpServers": {
                    "graphiq": {
                        "command": cmd[0],
                        "args": cmd[1..].to_vec()
                    }
                }
            });

            if let Some(parent) = config_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }

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
                        let already = servers.contains_key("graphiq");
                        servers.insert("graphiq".into(), entry["mcpServers"]["graphiq"].clone());
                        (pretty(&parsed), !already)
                    }
                    Err(_) => (pretty(&entry), true),
                }
            } else {
                (pretty(&entry), true)
            };

            match std::fs::write(config_path, &config) {
                Ok(()) => {
                    let status = if written { "configured" } else { "updated" };
                    println!("  Gemini CLI:    {} {}", status, config_path.display());
                    configured.push("Gemini CLI".to_string());
                }
                Err(e) => {
                    eprintln!("  Gemini CLI:    failed to write config: {e}");
                    failed.push("Gemini CLI".to_string());
                }
            }
        }
    } else {
        skipped.push("Gemini CLI".to_string());
    }

    // Aider
    if should_configure("aider") {
        let aider_config = project_path.join(".aider.conf.yml");
        let instructions = format!(
            "# GraphIQ MCP server is available at: graphiq-mcp {}\n# Add to your MCP configuration to enable code intelligence.\n",
            project_path.display()
        );

        if !aider_config.exists() {
            match std::fs::write(&aider_config, &instructions) {
                Ok(()) => {
                    println!("  Aider:         wrote {}", aider_config.display());
                    configured.push("Aider".to_string());
                }
                Err(e) => {
                    eprintln!("  Aider:         failed to write config: {e}");
                    failed.push("Aider".to_string());
                }
            }
        } else {
            println!(
                "  Aider:         already configured ({})",
                aider_config.display()
            );
            configured.push("Aider".to_string());
        }
    } else {
        skipped.push("Aider".to_string());
    }

    // Pi — pi does not support MCP; it integrates via a pi extension (the same
    // approach Signet uses). Install the graphiq-pi extension into
    // ~/.pi/agent/extensions/ so pi auto-discovers it and exposes the graphiq
    // tools + /graphiq command. The extension shells out to this `graphiq` CLI.
    if should_configure("pi") {
        'pi: {
            let home_dir =
                dirs::home_dir().unwrap_or_else(|| std::path::Path::new(".").to_path_buf());
            let pi_ext_dir = home_dir.join(".pi").join("agent").join("extensions");
            // Only install where pi is actually present (the extensions dir exists).
            if !pi_ext_dir.exists() {
                skipped.push("pi".to_string());
                break 'pi;
            }
            // Find the extension source bundled with this graphiq install.
            let exe = std::env::current_exe().ok();
            let candidates: Vec<PathBuf> = [
                exe.as_ref().map(|e| {
                    e.parent()
                        .unwrap_or(std::path::Path::new("."))
                        .join("integrations")
                        .join("pi")
                        .join("graphiq-pi.ts")
                }),
                exe.as_ref().map(|e| {
                    e.parent()
                        .unwrap_or(std::path::Path::new("."))
                        .join("..")
                        .join("share")
                        .join("graphiq")
                        .join("graphiq-pi.ts")
                }),
                Some(PathBuf::from("/usr/local/share/graphiq/graphiq-pi.ts")),
                Some(PathBuf::from("/opt/homebrew/share/graphiq/graphiq-pi.ts")),
            ]
            .into_iter()
            .flatten()
            .collect();
            let src = candidates.iter().find(|p| p.exists());
            let dest = pi_ext_dir.join("graphiq-pi.ts");
            match src {
                Some(src_path) => {
                    if let Some(parent) = dest.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    match std::fs::copy(src_path, &dest) {
                        Ok(_) => {
                            println!("  Pi:            installed extension {}", dest.display());
                            configured.push("Pi".to_string());
                        }
                        Err(e) => {
                            eprintln!("  Pi:            failed to install extension: {e}");
                            failed.push("Pi".to_string());
                        }
                    }
                }
                None => {
                    // Development convenience: if running from the graphiq repo, the
                    // source is at repo integrations/pi/graphiq-pi.ts.
                    let dev_src = PathBuf::from("integrations/pi/graphiq-pi.ts");
                    if dev_src.exists() {
                        if let Some(parent) = dest.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        match std::fs::copy(&dev_src, &dest) {
                            Ok(_) => {
                                println!("  Pi:            installed extension {}", dest.display());
                                configured.push("Pi".to_string());
                            }
                            Err(e) => {
                                eprintln!("  Pi:            failed to install extension: {e}");
                                failed.push("Pi".to_string());
                            }
                        }
                    } else {
                        eprintln!(
                            "  Pi:            graphiq-pi.ts not found beside the graphiq binary;"
                        );
                        eprintln!("                 reinstall graphiq to bundle the pi extension.");
                        failed.push("Pi".to_string());
                    }
                }
            }
        }
    } else {
        skipped.push("pi".to_string());
    }

    if configured.is_empty() && skipped.is_empty() {
        println!("  No harness configs found to update.");
        println!("  You can manually configure graphiq-mcp as an MCP server:");
        println!("    graphiq-mcp {}", project_path.display());
    }

    println!();

    let graphiq_dir = project_path.join(".graphiq");
    if !ephemeral && !dry_run {
        let _ = std::fs::create_dir_all(&graphiq_dir);
        write_agents_md(&graphiq_dir);
    } else if dry_run && !ephemeral {
        println!("  [dry-run] Would create {}", graphiq_dir.display());
    } else if dry_run {
        println!("  [dry-run] Would configure temp-backed MCP servers");
    }

    if !skip_index && !ephemeral && !dry_run {
        let db_path = graphiq_dir.join("graphiq.db");

        if db_path.exists() {
            let _ = std::fs::remove_file(&db_path);
        }

        let db = open_db_or_exit(&db_path);

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
    } else if !skip_index && ephemeral && !dry_run {
        println!("  Skipping upfront project index (temp MCP servers warm on launch)");
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
    if !skipped.is_empty() {
        println!("  Skipped (use --harness <name>): {}", skipped.join(", "));
    }
    if !failed.is_empty() {
        eprintln!("  Failed: {}", failed.join(", "));
    }

    println!();
    println!("  Try it:");
    if ephemeral {
        println!("    Restart your harness and call the GraphIQ MCP tools.");
        println!(
            "    graphiq-mcp {} --ephemeral --watch",
            project_path.display()
        );
        println!("    graphiq impact --project {}", project_path.display());
    } else {
        println!(
            "    graphiq search \"rate limit middleware\" --db {}/.graphiq/graphiq.db",
            project_path.display()
        );
        println!(
            "    graphiq blast RateLimiter --db {}/.graphiq/graphiq.db",
            project_path.display()
        );
        println!("    graphiq impact --project {}", project_path.display());
        println!(
            "    graphiq doctor --db {}/.graphiq/graphiq.db",
            project_path.display()
        );
    }
    println!("    graphiq demo");

    if let Some(ref bin_path) = graphiq_bin {
        println!();
        if ephemeral {
            println!("  MCP server: {} <project> --ephemeral", bin_path.display());
        } else {
            println!("  MCP server: {} <project>", bin_path.display());
        }
        println!("  Installed at: {}", bin_path.display());
    }

    println!();
}

fn write_agents_md(graphiq_dir: &std::path::Path) {
    let content = include_str!("../AGENTS.md.template");
    let agents_path = graphiq_dir.join("AGENTS.md");
    if let Err(e) = std::fs::write(&agents_path, content) {
        eprintln!("  warning: failed to write AGENTS.md: {e}");
    } else {
        println!("  wrote {}", agents_path.display());
    }
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

fn cmd_context(symbol_name: &str, db_path: &std::path::Path) {
    if !db_path.exists() {
        eprintln!("database not found: {}", db_path.display());
        eprintln!("run `graphiq index <path>` first");
        std::process::exit(1);
    }

    let db = open_db_or_exit(db_path);

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

    let cache = graphiq_core::cache::HotCache::with_defaults();
    cache.prewarm(&db, 200);
    let neighborhood = cache.load_neighborhood(&db, sym.id);

    println!("=== {} ({}) ===", sym.name, sym.kind.as_str());

    if let Some(ref sig) = sym.signature {
        println!("Signature: {}", sig);
    }
    println!("Location: line {}-{}", sym.line_start, sym.line_end);
    println!();
    println!("Source:");
    println!("{}", sym.source);

    if let Some(n) = neighborhood {
        if !n.callers.is_empty() {
            println!();
            println!("Called by:");
            for (caller, _) in &n.callers {
                println!("  - {}", caller.name);
            }
        }
        if !n.callees.is_empty() {
            println!();
            println!("Calls:");
            for (callee, _) in &n.callees {
                println!("  - {}", callee.name);
            }
        }
        if !n.members.is_empty() {
            println!();
            println!("Contains:");
            for member in &n.members {
                println!("  - {} ({})", member.name, member.kind.as_str());
            }
        }
        if let Some(ref container) = n.container {
            println!();
            println!("Contained in: {}", container.name);
        }
        if !n.parents.is_empty() {
            println!();
            println!("Extends/Implements:");
            for parent in &n.parents {
                println!("  - {}", parent.name);
            }
        }
        if !n.tests.is_empty() {
            println!();
            println!("Tested by:");
            for test in &n.tests {
                println!("  - {}", test.name);
            }
        }
    }
}

fn cmd_dead_code(db_path: &std::path::Path) {
    if !db_path.exists() {
        eprintln!("database not found: {}", db_path.display());
        eprintln!("run `graphiq index <path>` first");
        std::process::exit(1);
    }

    let db = open_db_or_exit(db_path);

    match graphiq_core::dead_code::detect_dead_code(&db) {
        Ok(result) => {
            if result.files.is_empty() {
                println!("No dead code detected.");
                return;
            }
            println!(
                "Dead Code: {} symbols, ~{} LOC",
                result.total_dead_symbols, result.estimated_dead_loc
            );
            println!();
            for file in &result.files {
                println!(
                    "  {} ({} dead, ~{} LOC)",
                    file.path,
                    file.dead_symbols.len(),
                    file.dead_loc
                );
                for name in &file.dead_symbols {
                    println!("    - {}", name);
                }
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
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

    let db = open_db_or_exit(&demo_db);

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
    let engine = graphiq_core::search::SearchEngine::new(&db, &cache).with_cruncher(&cruncher_idx);

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

    println!("── BM25 (FTS) vs GraphIQ ──");
    println!("  Left: BM25 text search only.");
    println!("  Right: BM25 + graph walk + structural rerank.");
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
        println!(
            "  BM25 rank: {:>3}   GraphIQ rank: {:>3}   {}",
            bm25_label, gq_label, verdict
        );

        let bm25_slice: Vec<_> = fts_results.iter().take(top_n).collect();
        let gq_slice: Vec<_> = result.results.iter().take(top_n).collect();

        for i in 0..top_n {
            let left = bm25_slice.get(i).map(|r| {
                let fp = file_paths
                    .get(&r.symbol.file_id)
                    .map(|s| s.as_str())
                    .unwrap_or("?");
                let hit = if r.symbol.name.contains(expected) {
                    " <<"
                } else {
                    ""
                };
                format!(
                    "#{} {:.1} {}:{} {}::{}{}",
                    i + 1,
                    r.bm25_score,
                    fp,
                    r.symbol.line_start,
                    r.symbol.kind.as_str(),
                    r.symbol.name,
                    hit
                )
            });

            let right = gq_slice.get(i).map(|r| {
                let fp = r.file_path.as_deref().unwrap_or("?");
                let hit = if r.symbol.name.contains(expected) {
                    " <<"
                } else {
                    ""
                };
                format!(
                    "#{} {:.1} {}:{} {}::{}{}",
                    i + 1,
                    r.score,
                    fp,
                    r.symbol.line_start,
                    r.symbol.kind.as_str(),
                    r.symbol.name,
                    hit
                )
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
    println!(
        "  Result: GraphIQ {}/{} | BM25 {}/{} | Tied {}/{}",
        graphiq_wins, total, bm25_wins, total, ties, total
    );
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

fn vulkan_available() -> bool {
    std::path::Path::new("/usr/lib/x86_64-linux-gnu/libvulkan.so.1").exists()
        || std::path::Path::new("/usr/lib/aarch64-linux-gnu/libvulkan.so.1").exists()
        || std::process::Command::new("ldconfig")
            .args(["-p"])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("libvulkan.so.1"))
            .unwrap_or(false)
}

fn check_gpu_runtime() {
    if std::env::consts::OS == "macos" {
        println!("  GPU: Metal available (built-in)");
    } else if vulkan_available() {
        println!("  GPU: Vulkan loader found");
    } else {
        eprintln!("  GPU: Vulkan loader not found — GPU acceleration disabled");
        eprintln!("    Install: sudo apt install -y libvulkan1");
        eprintln!("    Or:      sudo dnf install vulkan-loader");
    }
}

fn cmd_update(install_dir: Option<&str>, yes: bool) {
    let current_bin = which_graphiq().unwrap_or_else(|| {
        let exe = std::env::current_exe().unwrap_or_else(|_| {
            eprintln!("  error: cannot determine current executable");
            std::process::exit(1);
        });
        eprintln!(
            "  warning: graphiq not found on PATH, using {}",
            exe.display()
        );
        exe
    });

    let current_dir = current_bin.parent().map(PathBuf::from);
    let install_dir = install_dir
        .map(PathBuf::from)
        .or(current_dir)
        .unwrap_or_else(|| PathBuf::from("/usr/local/bin"));

    let current_version = std::process::Command::new(&current_bin)
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unknown".to_string());

    println!("  Current version: {}", current_version);
    println!("  Checking for updates...");

    let client = reqwest_or_curl();
    let latest_version = match fetch_latest_version(&client) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("  error: failed to check for updates: {e}");
            std::process::exit(1);
        }
    };

    if latest_version == current_version || format!("v{}", current_version) == latest_version {
        println!("  Already up to date ({}).", latest_version);
        return;
    }

    let tag = if latest_version.starts_with('v') {
        latest_version.clone()
    } else {
        format!("v{}", latest_version)
    };

    println!("  Update available: {} → {}", current_version, tag);
    println!("  Installing to:   {}", install_dir.display());

    let platform = detect_platform();
    let archive = format!("graphiq-{}.tar.gz", platform);
    let url = format!(
        "https://github.com/aaf2tbz/graphiq/releases/download/{}/{}",
        tag, archive
    );

    let tmpdir = tempfile::tempdir().unwrap_or_else(|e| {
        eprintln!("  error: failed to create temp dir: {e}");
        std::process::exit(1);
    });
    let archive_path = tmpdir.path().join(&archive);

    print!("  Downloading {}... ", archive);
    let download_status = std::process::Command::new("curl")
        .args(["-fsSL", "--max-time", "120", "--retry", "3", &url, "-o"])
        .arg(&archive_path)
        .status();

    match download_status {
        Ok(s) if s.success() => println!("done"),
        Ok(s) => {
            println!("failed (exit {})", s.code().unwrap_or(-1));
            eprintln!("  error: download failed. Check your connection.");
            std::process::exit(1);
        }
        Err(e) => {
            println!("failed");
            eprintln!("  error: {}", e);
            std::process::exit(1);
        }
    }

    match fetch_release_digest(&tag, &archive)
        .and_then(|expected| sha256_file(&archive_path).map(|actual| (expected, actual)))
    {
        Ok((expected, actual)) if expected == actual => {
            println!("  ✓ checksum verified");
        }
        Ok((expected, actual)) => {
            eprintln!("  error: SHA256 checksum mismatch");
            eprintln!("    expected: {expected}");
            eprintln!("    actual:   {actual}");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("  error: checksum verification failed: {e}");
            std::process::exit(1);
        }
    }

    println!("  Extracting...");
    let extract_status = std::process::Command::new("tar")
        .args(["xzf", archive_path.to_str().unwrap_or("")])
        .current_dir(tmpdir.path())
        .status();

    if let Err(e) = extract_status {
        eprintln!("  error: extraction failed: {e}");
        std::process::exit(1);
    }

    let need_sudo = if install_dir.exists() {
        !can_write_to_dir(&install_dir)
    } else {
        install_dir
            .parent()
            .map(|p| !can_write_to_dir(p))
            .unwrap_or(true)
    };

    let mut installed = 0;
    for bin in &["graphiq", "graphiq-mcp", "graphiq-bench"] {
        let src = tmpdir.path().join(bin);
        if !src.exists() {
            continue;
        }
        let dst = install_dir.join(bin);

        if !install_dir.exists() {
            if need_sudo {
                let status = std::process::Command::new("sudo")
                    .args(["mkdir", "-p"])
                    .arg(&install_dir)
                    .status();
                if !matches!(status, Ok(s) if s.success()) {
                    eprintln!("  error: failed to create {}", install_dir.display());
                    std::process::exit(1);
                }
            } else {
                std::fs::create_dir_all(&install_dir).unwrap_or_else(|e| {
                    eprintln!("  error: failed to create {}: {e}", install_dir.display());
                    std::process::exit(1);
                });
            }
        }

        match install_binary(&src, &dst, need_sudo) {
            Ok(()) => {
                println!("  ✓ {} → {}", bin, dst.display());
                installed += 1;
            }
            Err(e) => eprintln!("  ✗ {}: {}", bin, e),
        }
    }

    if installed == 0 {
        eprintln!("  error: no binaries were installed");
        std::process::exit(1);
    }

    let new_version = std::process::Command::new(install_dir.join("graphiq"))
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unknown".to_string());

    println!("  Updated to {}.", new_version);

    if let Some(path_bin) = which_graphiq() {
        if path_bin != install_dir.join("graphiq") {
            eprintln!(
                "  warning: {} shadows the updated binary at {}",
                path_bin.display(),
                install_dir.join("graphiq").display()
            );
            eprintln!(
                "  Put {} earlier in PATH or remove the old binary manually.",
                install_dir.display()
            );
        }
    } else {
        eprintln!("  warning: {} is not on PATH", install_dir.display());
    }

    check_gpu_runtime();

    let mcp_running = std::process::Command::new("pgrep")
        .args(["-x", "graphiq-mcp"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if mcp_running {
        println!();
        if yes {
            restart_mcp();
        } else {
            println!("  GraphIQ MCP server is running.");
            println!("  Restart graphiq? [y/N] ");
            if confirm("") {
                restart_mcp();
            } else {
                println!("  Skipped restart. Run `graphiq update` again or restart manually.");
            }
        }
    }

    println!();
    println!("  Done.");
}

fn restart_mcp() {
    println!("  Restarting graphiq-mcp...");
    let kill_ok = std::process::Command::new("pkill")
        .args(["-x", "graphiq-mcp"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if kill_ok {
        std::thread::sleep(std::time::Duration::from_millis(500));
        println!("  ✓ graphiq-mcp stopped.");
        println!("  Your harness will auto-reconnect on next request.");
    } else {
        println!("  graphiq-mcp was not running or already stopped.");
    }
}

fn fetch_release_digest(tag: &str, asset_name: &str) -> Result<String, String> {
    let url = format!("https://api.github.com/repos/aaf2tbz/graphiq/releases/tags/{tag}");
    let output = std::process::Command::new("curl")
        .args(["-fsSL", "--max-time", "15", &url])
        .output()
        .map_err(|e| format!("curl failed: {e}"))?;

    if !output.status.success() {
        return Err(format!("failed to fetch release metadata for {tag}"));
    }

    let body = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| format!("invalid release metadata: {e}"))?;
    let assets = parsed
        .get("assets")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "release metadata has no assets array".to_string())?;

    for asset in assets {
        let name = asset.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if name != asset_name {
            continue;
        }
        let digest = asset.get("digest").and_then(|v| v.as_str()).unwrap_or("");
        let digest = digest.strip_prefix("sha256:").unwrap_or(digest).trim();
        if digest.is_empty() {
            return Err(format!(
                "{asset_name} has no sha256 digest in release metadata"
            ));
        }
        return Ok(digest.to_string());
    }

    Err(format!(
        "{asset_name} not found in release metadata for {tag}"
    ))
}

fn sha256_file(path: &std::path::Path) -> Result<String, String> {
    let mut command = if cmd_exists("sha256sum") {
        let mut cmd = std::process::Command::new("sha256sum");
        cmd.arg(path);
        cmd
    } else if cmd_exists("shasum") {
        let mut cmd = std::process::Command::new("shasum");
        cmd.args(["-a", "256"]).arg(path);
        cmd
    } else {
        return Err("no sha256sum or shasum found".to_string());
    };

    let output = command
        .output()
        .map_err(|e| format!("failed to compute sha256: {e}"))?;
    if !output.status.success() {
        return Err("sha256 command failed".to_string());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .split_whitespace()
        .next()
        .map(|s| s.to_string())
        .ok_or_else(|| "sha256 command produced no digest".to_string())
}

fn install_binary(
    src: &std::path::Path,
    dst: &std::path::Path,
    need_sudo: bool,
) -> Result<(), String> {
    let tmp = dst.with_file_name(format!(
        "{}.tmp.{}",
        dst.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("graphiq-bin"),
        std::process::id()
    ));

    if need_sudo {
        let copy = std::process::Command::new("sudo")
            .arg("cp")
            .arg(src)
            .arg(&tmp)
            .status()
            .map_err(|e| format!("sudo cp failed: {e}"))?;
        if !copy.success() {
            return Err("sudo cp failed".to_string());
        }

        let chmod = std::process::Command::new("sudo")
            .args(["chmod", "755"])
            .arg(&tmp)
            .status()
            .map_err(|e| format!("sudo chmod failed: {e}"))?;
        if !chmod.success() {
            let _ = std::process::Command::new("sudo")
                .arg("rm")
                .arg(&tmp)
                .status();
            return Err("sudo chmod failed".to_string());
        }

        let mv = std::process::Command::new("sudo")
            .arg("mv")
            .arg(&tmp)
            .arg(dst)
            .status()
            .map_err(|e| format!("sudo mv failed: {e}"))?;
        if !mv.success() {
            let _ = std::process::Command::new("sudo")
                .arg("rm")
                .arg(&tmp)
                .status();
            return Err("sudo mv failed".to_string());
        }
        return Ok(());
    }

    std::fs::copy(src, &tmp).map_err(|e| format!("copy failed: {e}"))?;
    std::fs::set_permissions(&tmp, std::os::unix::fs::PermissionsExt::from_mode(0o755))
        .map_err(|e| format!("chmod failed: {e}"))?;
    std::fs::rename(&tmp, dst).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("rename failed: {e}")
    })?;
    Ok(())
}

fn fetch_latest_version(_client: &str) -> Result<String, String> {
    let output = std::process::Command::new("curl")
        .args([
            "-fsSL",
            "--max-time",
            "15",
            "https://api.github.com/repos/aaf2tbz/graphiq/releases/latest",
        ])
        .output()
        .map_err(|e| format!("curl failed: {e}"))?;

    if !output.status.success() {
        return Err("failed to fetch latest release info".into());
    }

    let body = String::from_utf8_lossy(&output.stdout);
    for line in body.lines() {
        if line.contains("\"tag_name\"") {
            let version = line
                .split(':')
                .nth(1)
                .unwrap_or("")
                .trim()
                .trim_matches(',')
                .trim_matches('"')
                .trim()
                .to_string();
            if !version.is_empty() {
                return Ok(version);
            }
        }
    }

    Err("could not parse tag_name from release response".into())
}

fn reqwest_or_curl() -> String {
    "curl".to_string()
}

fn can_write_to_dir(dir: &std::path::Path) -> bool {
    if !dir.exists() {
        return false;
    }
    let test_file = dir.join(".graphiq_write_test");
    let can_write = std::fs::write(&test_file, b"t").is_ok();
    let _ = std::fs::remove_file(&test_file);
    can_write
}

fn detect_platform() -> String {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    match (os, arch) {
        ("macos", "aarch64") => "aarch64-apple-darwin".to_string(),
        ("macos", "x86_64") => "x86_64-apple-darwin".to_string(),
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu".to_string(),
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu".to_string(),
        _ => format!("{}-{}", arch, os),
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn strip_jsonc_line_and_block_comments() {
        let input = r#"{
  // a line comment
  "name": "value", /* block comment */
  "n": 1
}"#;
        let out = strip_jsonc_comments(input);
        // Comments gone, valid JSON, values preserved.
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["name"], "value");
        assert_eq!(v["n"], 1);
    }

    #[test]
    fn strip_jsonc_preserves_urls_and_strings() {
        // `//` inside a string (a URL) must NOT be treated as a comment.
        let input = r#"{
  "$schema": "https://opencode.ai/config.json",
  "url": "http://example.com/path"
}"#;
        let out = strip_jsonc_comments(input);
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["$schema"], "https://opencode.ai/config.json");
        assert_eq!(v["url"], "http://example.com/path");
    }

    #[test]
    fn strip_jsonc_preserves_escaped_quotes() {
        // Escaped quote inside a string must not terminate it.
        let input = r#"{ "msg": "she said \"hi\" // not a comment" }"#;
        let out = strip_jsonc_comments(input);
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["msg"], "she said \"hi\" // not a comment");
    }

    #[test]
    fn strip_jsonc_block_comment_separates_tokens() {
        let input = r#"{ "a": 1,/*x*/"b": 2 }"#;
        let out = strip_jsonc_comments(input);
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["a"], 1);
        assert_eq!(v["b"], 2);
    }

    #[test]
    fn strip_jsonc_unterminated_block_comment() {
        // Should not panic; produces parseable (incomplete) output but no crash.
        let input = "{ \"a\": 1 /* never closed";
        let _ = strip_jsonc_comments(input);
    }

    #[test]
    fn strip_jsonc_preserves_utf8() {
        // Regression: the byte-casting implementation mangled multi-byte chars.
        // Non-ASCII content (names, descriptions, emojis) must round-trip intact.
        let input = "{ \"name\": \"café\", \"emoji\": \"🚀\", // 注释\n \"ok\": true }";
        let out = strip_jsonc_comments(input);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["name"], "café");
        assert_eq!(v["emoji"], "🚀");
        assert_eq!(v["ok"], true);
    }

    #[test]
    fn parse_jsonc_config_returns_none_on_invalid() {
        assert!(parse_jsonc_config("{ not valid json }").is_none());
        assert!(parse_jsonc_config("").is_none());
        assert!(parse_jsonc_config(r#"{ "ok": true } // trailing"#).is_some());
    }

    #[test]
    fn opencode_config_path_prefers_existing_jsonc() {
        let tmp = std::env::temp_dir().join(format!(
            "gq-test-jsonc-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let dir = tmp.join(".config").join("opencode");
        std::fs::create_dir_all(&dir).unwrap();

        // Neither exists -> default to .jsonc (modern canonical).
        let p = opencode_config_path(&tmp);
        assert!(p.ends_with("opencode.jsonc"));

        // Only .json exists -> use it (legacy).
        std::fs::write(dir.join("opencode.json"), "{}").unwrap();
        let p = opencode_config_path(&tmp);
        assert!(p.ends_with("opencode.json"));

        // Both exist -> prefer .jsonc.
        std::fs::write(dir.join("opencode.jsonc"), "{}").unwrap();
        let p = opencode_config_path(&tmp);
        assert!(p.ends_with("opencode.jsonc"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ── graphiq sync tests ──────────────────────────────────────────────

    #[test]
    fn sync_detect_attach_mcp_servers_json() {
        let connected = r#"{"mcpServers":{"graphiq":{"command":"graphiq-mcp"}}}"#;
        assert_eq!(
            detect_attach(connected, AttachShape::McpServers),
            AttachState::Connected
        );
        let other = r#"{"mcpServers":{"x":{"command":"y"}}}"#;
        assert_eq!(
            detect_attach(other, AttachShape::McpServers),
            AttachState::NotConfigured
        );
    }

    #[test]
    fn sync_detect_attach_opencode_jsonc_with_comments() {
        let jsonc = r#"{
  // opencode
  "$schema": "https://opencode.ai/config.json",
  "mcp": { "graphiq": { "command": ["graphiq-mcp", "."] } }
}"#;
        assert_eq!(
            detect_attach(jsonc, AttachShape::Mcp),
            AttachState::Connected
        );
        let none = r#"{ /* fine */ "mcp": { "other": {} } }"#;
        assert_eq!(
            detect_attach(none, AttachShape::Mcp),
            AttachState::NotConfigured
        );
    }

    #[test]
    fn sync_detect_attach_codex_toml_exact_and_terminator() {
        // exact section header -> connected
        let connected = "[mcp_servers.graphiq]\ncommand = \"graphiq-mcp\"\n";
        assert_eq!(
            detect_attach(connected, AttachShape::CodexToml),
            AttachState::Connected
        );
        // a different server -> not configured
        let other = "[mcp_servers.something]\ncommand = \"x\"\n";
        assert_eq!(
            detect_attach(other, AttachShape::CodexToml),
            AttachState::NotConfigured
        );
        // REGRESSION: a server named graphiqfoo must NOT match graphiq.
        let lookalike = "[mcp_servers.graphiqfoo]\ncommand = \"x\"\n";
        assert_eq!(
            detect_attach(lookalike, AttachShape::CodexToml),
            AttachState::NotConfigured
        );
    }

    #[test]
    fn sync_detect_attach_parse_failed_for_garbage() {
        assert_eq!(
            detect_attach("{ totally broken {{{", AttachShape::McpServers),
            AttachState::ParseFailed
        );
    }

    #[test]
    fn sync_strip_jsonc_keeps_strings_and_utf8() {
        let input = "{ \"url\": \"https://x.io/a\" /* c */ , \"n\": \"café\" // line\n }";
        let out = sync_strip_jsonc(input);
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["url"], "https://x.io/a");
        assert_eq!(v["n"], "café");
    }

    #[test]
    fn sync_harness_targets_covers_known_harnesses() {
        let home = std::path::Path::new("/tmp/fakehome");
        let project = std::path::Path::new("/tmp/fakeproj");
        let targets = harness_targets(home, project);
        let names: Vec<&str> = targets.iter().map(|t| t.name).collect();
        for expected in [
            "claude-code",
            "opencode",
            "codex",
            "cursor",
            "windsurf",
            "pi",
        ] {
            assert!(
                names.contains(&expected),
                "missing harness target: {expected}"
            );
        }
    }

    #[test]
    fn sync_detect_attach_pi_extension() {
        let installed = "/** GraphIQ extension for pi */\nexport default function(){}\npi.registerTool({name:\"graphiq_search\"})";
        assert_eq!(
            detect_attach(installed, AttachShape::PiExtension),
            AttachState::Connected
        );
        // A different extension file is not a graphiq attach.
        let other = "// some other pi extension";
        assert_eq!(
            detect_attach(other, AttachShape::PiExtension),
            AttachState::NotConfigured
        );
    }

    // ── graphiq discover tests ───────────────────────────────────────────

    #[test]
    fn detect_signature_prefers_config_dir_then_binary_then_app() {
        let sig = HarnessSignature {
            name: "codex",
            label: "Codex CLI",
            config_dir: ".codex",
            binary: "codex",
            macos_app: "",
        };
        // Only config dir present -> ConfigDir wins.
        let d = detect_signature(&sig, true, "/h/.codex", false, "", false, "").unwrap();
        assert_eq!(d.reason, DetectReason::ConfigDir);
        assert_eq!(d.evidence, "/h/.codex");
        // All three present -> ConfigDir still wins (most stable signal).
        let d =
            detect_signature(&sig, true, "/h/.codex", true, "/usr/bin/codex", false, "").unwrap();
        assert_eq!(d.reason, DetectReason::ConfigDir);
        // No config dir, binary on PATH -> BinaryOnPath.
        let d = detect_signature(&sig, false, "", true, "/usr/bin/codex", false, "").unwrap();
        assert_eq!(d.reason, DetectReason::BinaryOnPath);
        // Nothing -> None.
        assert!(detect_signature(&sig, false, "", false, "", false, "").is_none());
    }

    #[test]
    fn detect_signature_macos_app_fallback() {
        // A harness with only an app-bundle signal (e.g. a Cursor.app install
        // where the config dir hasn't been created yet).
        let sig = HarnessSignature {
            name: "cursor",
            label: "Cursor",
            config_dir: ".cursor",
            binary: "",
            macos_app: "Cursor.app",
        };
        let d =
            detect_signature(&sig, false, "", false, "", true, "/Applications/Cursor.app").unwrap();
        assert_eq!(d.reason, DetectReason::MacosApp);
        assert_eq!(d.evidence, "/Applications/Cursor.app");
    }

    #[test]
    fn detect_signature_empty_binary_never_reports_binary() {
        // claude-desktop has no PATH binary; an empty binary field must never
        // produce a BinaryOnPath result even if the probe would otherwise match.
        let sig = HarnessSignature {
            name: "claude-desktop",
            label: "Claude Desktop",
            config_dir: "",
            binary: "",
            macos_app: "Claude.app",
        };
        // No config dir, no app, and no binary signal -> None.
        assert!(detect_signature(&sig, false, "", true, "/x/claude", false, "").is_none());
    }

    #[test]
    fn harness_catalog_is_complete_and_unique() {
        let cat = harness_catalog();
        let names: Vec<&str> = cat.iter().map(|s| s.name).collect();
        // Every harness setup handles must be discoverable.
        for expected in [
            "claude-desktop",
            "claude-code",
            "codex",
            "opencode",
            "pi",
            "cursor",
            "hermes",
            "windsurf",
            "gemini",
            "aider",
            "forge",
            "openclaw",
        ] {
            assert!(names.contains(&expected), "catalog missing: {expected}");
        }
        // Names are unique (no accidental duplicates that would double-report).
        let mut sorted = names.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            names.len(),
            "duplicate harness names in catalog"
        );
        // Each harness has at least one signal (otherwise it can never be detected).
        for s in &cat {
            assert!(
                !s.config_dir.is_empty() || !s.binary.is_empty() || !s.macos_app.is_empty(),
                "{} has no detection signal",
                s.name
            );
        }
    }

    #[test]
    fn config_dir_path_resolves_against_home() {
        let sig = HarnessSignature {
            name: "x",
            label: "X",
            config_dir: ".codex",
            binary: "",
            macos_app: "",
        };
        assert_eq!(
            config_dir_path(std::path::Path::new("/home/u"), &sig),
            Some(PathBuf::from("/home/u/.codex"))
        );
        // Empty config_dir -> None.
        let sig2 = HarnessSignature {
            name: "x",
            label: "X",
            config_dir: "",
            binary: "",
            macos_app: "",
        };
        assert_eq!(config_dir_path(std::path::Path::new("/h"), &sig2), None);
    }

    // ── graphiq git tests (pure parsers) ─────────────────────────────────

    #[test]
    fn git_parse_branch_status_normal() {
        let s = parse_branch_status("## main...origin/main");
        assert_eq!(s.branch, "main");
        assert_eq!((s.ahead, s.behind), (0, 0));
        assert!(!s.detached);
    }

    #[test]
    fn git_parse_branch_status_ahead_behind() {
        let s = parse_branch_status("## feat/x...origin/feat/x [ahead 2, behind 1]");
        assert_eq!(s.branch, "feat/x");
        assert_eq!((s.ahead, s.behind), (2, 1));
    }

    #[test]
    fn git_parse_branch_status_detached_and_nocommits() {
        let s = parse_branch_status("## HEAD (no branch)");
        assert!(s.detached);
        assert_eq!(s.branch, "(detached)");
        let s2 = parse_branch_status("## No commits yet on main");
        assert_eq!(s2.branch, "main");
    }

    #[test]
    fn git_count_changes_classifies_staged_unstaged_untracked() {
        // XY porcelain: X=index, Y=worktree. ?? = untracked.
        let p = "M  modified-staged-only\n M modified-worktree-only\nMM modified-both\n?? untracked\nA  added-staged\n";
        let (staged, unstaged, untracked) = count_changes(p);
        // staged (X != ' '/'?'): M, M(both), A => 3
        assert_eq!(staged, 3);
        // unstaged (Y != ' '/'?'): M(worktree-only), M(both) => 2
        assert_eq!(unstaged, 2);
        // untracked: ??
        assert_eq!(untracked, 1);
    }

    #[test]
    fn git_count_changes_empty_is_clean() {
        assert_eq!(count_changes(""), (0, 0, 0));
    }

    #[test]
    fn git_parse_commits_nul_separated() {
        // %H%x00%s%x00%an%x00%ad
        let log = "abc123\u{0}feat: add x\u{0}Alex\u{0}2026-06-22\ndef456\u{0}fix: y\u{0}Sam\u{0}2026-06-21\n";
        let c = parse_commits(log, 10);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].sha, "abc123");
        assert_eq!(c[0].subject, "feat: add x");
        assert_eq!(c[0].author, "Alex");
        assert_eq!(c[1].sha, "def456");
    }

    #[test]
    fn git_parse_commits_respects_limit() {
        let log = "a\u{0}s\u{0}n\u{0}d\nb\u{0}s\u{0}n\u{0}d\nc\u{0}s\u{0}n\u{0}d\n";
        assert_eq!(parse_commits(log, 2).len(), 2);
    }

    #[test]
    fn git_tree_state_label() {
        assert_eq!(tree_state_label(0, 0, 0), "clean");
        assert_eq!(tree_state_label(1, 0, 0), "dirty");
        assert_eq!(tree_state_label(0, 5, 0), "dirty");
        assert_eq!(tree_state_label(0, 0, 3), "dirty");
    }

    // ── graphiq projects tests (pure helpers) ────────────────────────────

    #[test]
    fn projects_parse_registry_roundtrip() {
        let json = r#"{"projects":[
          {"path":"/a","lastIndexedAt":"2026-06-22T01:00:00Z","files":10,"symbols":100},
          {"path":"/b","lastIndexedAt":"2026-06-22T02:00:00Z","files":5,"symbols":50}
        ]}"#;
        let p = parse_projects_registry(json);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].path, "/a"); // sorted
        assert_eq!(p[0].symbols, 100);
    }

    #[test]
    fn projects_parse_tolerates_garbage() {
        assert!(parse_projects_registry("").is_empty());
        assert!(parse_projects_registry("not json").is_empty());
        assert!(parse_projects_registry("{}").is_empty());
    }

    #[test]
    fn projects_upsert_replaces_same_path() {
        let base = vec![TrackedProject {
            path: "/a".into(),
            last_indexed_at: "old".into(),
            files: 1,
            symbols: 1,
        }];
        let updated = upsert_project(
            base,
            TrackedProject {
                path: "/a".into(),
                last_indexed_at: "new".into(),
                files: 2,
                symbols: 2,
            },
        );
        assert_eq!(updated.len(), 1);
        assert_eq!(updated[0].last_indexed_at, "new");
        assert_eq!(updated[0].symbols, 2);
    }

    #[test]
    fn projects_upsert_adds_new_sorted() {
        let base = vec![TrackedProject {
            path: "/b".into(),
            last_indexed_at: "".into(),
            files: 0,
            symbols: 0,
        }];
        let updated = upsert_project(
            base,
            TrackedProject {
                path: "/a".into(),
                last_indexed_at: "".into(),
                files: 0,
                symbols: 0,
            },
        );
        assert_eq!(updated.len(), 2);
        assert_eq!(updated[0].path, "/a"); // sorted
    }

    #[test]
    fn projects_forget_removes_by_path() {
        let p = vec![
            TrackedProject {
                path: "/a".into(),
                last_indexed_at: "".into(),
                files: 0,
                symbols: 0,
            },
            TrackedProject {
                path: "/b".into(),
                last_indexed_at: "".into(),
                files: 0,
                symbols: 0,
            },
        ];
        let out = forget_project(p, "/a");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].path, "/b");
    }

    #[test]
    fn projects_serialize_is_valid_json() {
        let p = vec![TrackedProject {
            path: "/x".into(),
            last_indexed_at: "2026-01-01T00:00:00Z".into(),
            files: 3,
            symbols: 9,
        }];
        let s = serialize_projects_registry(&p);
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["projects"][0]["path"], "/x");
        assert_eq!(v["projects"][0]["symbols"], 9);
    }

    #[test]
    fn format_unix_ts_iso_matches_known_epoch() {
        // Verified via Python datetime: 1782153600 == 2026-06-22T18:40:00Z.
        assert_eq!(format_unix_ts_iso(1782153600), "2026-06-22T18:40:00Z");
        // Unix epoch itself.
        assert_eq!(format_unix_ts_iso(0), "1970-01-01T00:00:00Z");
    }
}
