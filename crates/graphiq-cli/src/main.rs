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
    Demo,
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
        Commands::Demo => cmd_demo(),
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

fn cmd_demo() {
    use std::time::Instant;

    let tmp = std::env::temp_dir().join("graphiq-demo");
    if tmp.exists() {
        let _ = std::fs::remove_dir_all(&tmp);
    }
    let _ = std::fs::create_dir_all(&tmp.join("src"));
    let _ = std::fs::create_dir_all(&tmp.join("tests"));

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

    let demo_db = tmp.join(".graphiq/demo.db");
    let _ = std::fs::create_dir_all(tmp.join(".graphiq"));

    println!("╭──────────────────────────────────────────────╮");
    println!("│              GraphIQ Demo                     │");
    println!("╰──────────────────────────────────────────────╯");
    println!();

    println!("Sample project: ~/tmp/graphiq-demo/");
    println!("  src/lib.rs, auth.rs, middleware.rs, routes.rs, db.rs");
    println!("  tests/auth_test.rs, middleware_test.rs");
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
    let engine = graphiq_core::search::SearchEngine::new(&db, &cache);

    let queries = &[
        ("symbol-exact", "authenticate"),
        ("nl-descriptive", "rate limit middleware"),
        ("file-path", "auth.rs"),
        ("error-debug", "token expired or invalid"),
        ("cross-cutting", "handle_request"),
    ];

    for (label, query) in queries {
        println!("── {} ──", label);
        let q = graphiq_core::search::SearchQuery::new(*query).top_k(3);
        let t = Instant::now();
        let result = engine.search(&q);
        let elapsed = t.elapsed();

        if result.results.is_empty() {
            println!("  No results for \"{}\"", query);
        } else {
            for (i, scored) in result.results.iter().enumerate() {
                let sym = &scored.symbol;
                let file = scored.file_path.as_deref().unwrap_or("?");
                println!(
                    "  #{} {:.3} {}:{}  {}::{}",
                    i + 1,
                    scored.score,
                    file,
                    sym.line_start,
                    sym.kind.as_str(),
                    sym.name,
                );
            }
        }
        println!("  ({:.1}ms)", elapsed.as_secs_f64() * 1000.0);
        println!();
    }

    println!("── blast radius ──");
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
