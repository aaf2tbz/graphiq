use std::path::Path;
use std::time::Instant;

use graphiq_core::cache::HotCache;
use graphiq_core::db::GraphDb;
use graphiq_core::index::Indexer;
use graphiq_core::search::{SearchEngine, SearchQuery};

#[derive(Debug, Clone, serde::Deserialize)]
struct BenchQuery {
    query: String,
    expected_symbol: String,
    category: String,
}

#[derive(Debug)]
struct BenchResult {
    query: String,
    category: String,
    found_rank: Option<usize>,
    hit_at_1: bool,
    hit_at_3: bool,
    hit_at_5: bool,
    hit_at_10: bool,
    latency_us: u128,
    cached_latency_us: u128,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: graphiq-bench <project-path> [db-path] [query-file.json]");
        std::process::exit(1);
    }

    let project_path = Path::new(&args[1]);
    let db_path = args
        .get(2)
        .map(|s| s.as_str())
        .unwrap_or(".graphiq/bench.db");

    if !project_path.exists() {
        eprintln!("project path not found: {}", project_path.display());
        std::process::exit(1);
    }

    let db = match GraphDb::open(Path::new(db_path)) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error opening database: {e}");
            std::process::exit(1);
        }
    };

    println!("=== GraphIQ Benchmark ===\n");

    print!("Indexing {} ... ", project_path.display());
    let indexer = Indexer::new(&db);
    match indexer.index_project(project_path) {
        Ok(stats) => println!(
            "done ({} files, {} symbols)",
            stats.files_indexed, stats.symbols_indexed
        ),
        Err(e) => {
            println!("failed: {e}");
            std::process::exit(1);
        }
    }

    let db_stats = db.stats().unwrap();
    println!(
        "Database: {} files, {} symbols, {} edges\n",
        db_stats.files, db_stats.symbols, db_stats.edges
    );

    let queries = if let Some(query_file) = args.get(3) {
        let content = std::fs::read_to_string(query_file).unwrap_or_else(|e| {
            eprintln!("error reading query file: {e}");
            std::process::exit(1);
        });
        serde_json::from_str(&content).unwrap_or_else(|e| {
            eprintln!("error parsing query file: {e}");
            std::process::exit(1);
        })
    } else {
        build_default_queries()
    };
    let cache = HotCache::with_defaults();
    let engine = SearchEngine::new(&db, &cache);

    println!("Running {} benchmark queries ...\n", queries.len());

    let mut results = Vec::new();

    for q in &queries {
        let start = Instant::now();
        let result = engine.search(&SearchQuery::new(&q.query).top_k(10));
        let cold_latency = start.elapsed().as_micros();

        let start = Instant::now();
        let _ = engine.search(&SearchQuery::new(&q.query).top_k(10));
        let warm_latency = start.elapsed().as_micros();

        let found_rank = result
            .results
            .iter()
            .position(|r| {
                r.symbol.name == q.expected_symbol || r.symbol.name.contains(&q.expected_symbol)
            })
            .map(|p| p + 1);

        results.push(BenchResult {
            query: q.query.clone(),
            category: q.category.clone(),
            hit_at_1: found_rank.map_or(false, |r| r <= 1),
            hit_at_3: found_rank.map_or(false, |r| r <= 3),
            hit_at_5: found_rank.map_or(false, |r| r <= 5),
            hit_at_10: found_rank.map_or(false, |r| r <= 10),
            found_rank,
            latency_us: cold_latency,
            cached_latency_us: warm_latency,
        });
    }

    let total = results.len();
    let hits_1 = results.iter().filter(|r| r.hit_at_1).count();
    let hits_3 = results.iter().filter(|r| r.hit_at_3).count();
    let hits_5 = results.iter().filter(|r| r.hit_at_5).count();
    let hits_10 = results.iter().filter(|r| r.hit_at_10).count();

    let mrr: f64 = results
        .iter()
        .filter_map(|r| r.found_rank.map(|rank| 1.0 / rank as f64))
        .sum::<f64>()
        / total as f64;

    let cold_latencies: Vec<u128> = results.iter().map(|r| r.latency_us).collect();
    let warm_latencies: Vec<u128> = results.iter().map(|r| r.cached_latency_us).collect();

    let mut sorted_cold = cold_latencies.clone();
    sorted_cold.sort();
    let p50_cold = sorted_cold[sorted_cold.len() / 2];
    let p95_idx = ((sorted_cold.len() as f64) * 0.95) as usize;
    let p95_cold = sorted_cold[p95_idx.min(sorted_cold.len() - 1)];

    println!("=== Results ===\n");
    println!("MRR:      {:.3}", mrr);
    println!(
        "Hit@1:    {}/{} ({:.0}%)",
        hits_1,
        total,
        hits_1 as f64 / total as f64 * 100.0
    );
    println!(
        "Hit@3:    {}/{} ({:.0}%)",
        hits_3,
        total,
        hits_3 as f64 / total as f64 * 100.0
    );
    println!(
        "Hit@5:    {}/{} ({:.0}%)",
        hits_5,
        total,
        hits_5 as f64 / total as f64 * 100.0
    );
    println!(
        "Hit@10:   {}/{} ({:.0}%)",
        hits_10,
        total,
        hits_10 as f64 / total as f64 * 100.0
    );
    println!();
    println!(
        "Latency (cold):  p50={:.1}ms  p95={:.1}ms",
        p50_cold as f64 / 1000.0,
        p95_cold as f64 / 1000.0
    );

    let mut sorted_warm = warm_latencies.clone();
    sorted_warm.sort();
    let p50_warm = sorted_warm[sorted_warm.len() / 2];
    println!("Latency (warm):  p50={:.1}ms", p50_warm as f64 / 1000.0);

    println!("\n=== Per-Query Detail ===\n");
    println!(
        "{:<30} {:<15} {:>6} {:>6} {:>8} {:>8}",
        "Query", "Category", "Rank", "Hit@5", "Cold", "Warm"
    );
    println!("{}", "-".repeat(80));

    for r in &results {
        let rank_str = r
            .found_rank
            .map(|rank| rank.to_string())
            .unwrap_or_else(|| "MISS".into());
        println!(
            "{:<30} {:<15} {:>6} {:>6} {:>6.1}ms {:>6.1}ms",
            truncate(&r.query, 30),
            r.category,
            rank_str,
            if r.hit_at_5 { "yes" } else { "no" },
            r.latency_us as f64 / 1000.0,
            r.cached_latency_us as f64 / 1000.0
        );
    }

    println!();
    println!("=== By Category ===\n");
    let categories: Vec<String> = results
        .iter()
        .map(|r| r.category.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    for cat in &categories {
        let cat_results: Vec<_> = results.iter().filter(|r| &r.category == cat).collect();
        let cat_total = cat_results.len();
        let cat_mrr: f64 = cat_results
            .iter()
            .filter_map(|r| r.found_rank.map(|rank| 1.0 / rank as f64))
            .sum::<f64>()
            / cat_total as f64;
        let cat_hit3 = cat_results.iter().filter(|r| r.hit_at_3).count();
        let cat_hit5 = cat_results.iter().filter(|r| r.hit_at_5).count();
        let cat_hit10 = cat_results.iter().filter(|r| r.hit_at_10).count();

        println!(
            "{:<20} MRR={:.3}  Hit@3={}/{} ({:.0}%)  Hit@5={}/{} ({:.0}%)  Hit@10={}/{} ({:.0}%)",
            cat,
            cat_mrr,
            cat_hit3,
            cat_total,
            cat_hit3 as f64 / cat_total as f64 * 100.0,
            cat_hit5,
            cat_total,
            cat_hit5 as f64 / cat_total as f64 * 100.0,
            cat_hit10,
            cat_total,
            cat_hit10 as f64 / cat_total as f64 * 100.0
        );
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max - 3])
    }
}

fn build_default_queries() -> Vec<BenchQuery> {
    vec![
        BenchQuery {
            query: "SymbolKind".into(),
            expected_symbol: "SymbolKind".into(),
            category: "symbol-exact".into(),
        },
        BenchQuery {
            query: "bounded_bfs".into(),
            expected_symbol: "bounded_bfs".into(),
            category: "symbol-exact".into(),
        },
        BenchQuery {
            query: "EdgeKind".into(),
            expected_symbol: "EdgeKind".into(),
            category: "symbol-exact".into(),
        },
        BenchQuery {
            query: "GraphDb".into(),
            expected_symbol: "GraphDb".into(),
            category: "symbol-exact".into(),
        },
        BenchQuery {
            query: "HotCache".into(),
            expected_symbol: "HotCache".into(),
            category: "symbol-exact".into(),
        },
        BenchQuery {
            query: "parse_edge_kind_json".into(),
            expected_symbol: "parse_edge_kind_json".into(),
            category: "symbol-exact".into(),
        },
        BenchQuery {
            query: "SearchQuery".into(),
            expected_symbol: "SearchQuery".into(),
            category: "symbol-exact".into(),
        },
        BenchQuery {
            query: "sym kind".into(),
            expected_symbol: "SymbolKind".into(),
            category: "symbol-partial".into(),
        },
        BenchQuery {
            query: "edge".into(),
            expected_symbol: "Edge".into(),
            category: "symbol-partial".into(),
        },
        BenchQuery {
            query: "cache".into(),
            expected_symbol: "HotCache".into(),
            category: "symbol-partial".into(),
        },
        BenchQuery {
            query: "blast".into(),
            expected_symbol: "blast".into(),
            category: "symbol-partial".into(),
        },
        BenchQuery {
            query: "token".into(),
            expected_symbol: "tokenize".into(),
            category: "symbol-partial".into(),
        },
        BenchQuery {
            query: "rerank".into(),
            expected_symbol: "Reranker".into(),
            category: "symbol-partial".into(),
        },
        BenchQuery {
            query: "bm25 full text search".into(),
            expected_symbol: "search".into(),
            category: "nl-descriptive".into(),
        },
        BenchQuery {
            query: "structural graph expansion".into(),
            expected_symbol: "StructuralExpander".into(),
            category: "nl-descriptive".into(),
        },
        BenchQuery {
            query: "identifier decomposition tokenize".into(),
            expected_symbol: "decompose_identifier".into(),
            category: "nl-descriptive".into(),
        },
        BenchQuery {
            query: "compute blast radius".into(),
            expected_symbol: "compute_blast_radius".into(),
            category: "nl-descriptive".into(),
        },
        BenchQuery {
            query: "insert symbol database".into(),
            expected_symbol: "insert_symbol".into(),
            category: "nl-descriptive".into(),
        },
        BenchQuery {
            query: "how does retrieval ranking work".into(),
            expected_symbol: "Reranker".into(),
            category: "nl-abstract".into(),
        },
        BenchQuery {
            query: "how are symbols indexed from source files".into(),
            expected_symbol: "Indexer".into(),
            category: "nl-abstract".into(),
        },
        BenchQuery {
            query: "what connects callers to callees".into(),
            expected_symbol: "bounded_bfs".into(),
            category: "nl-abstract".into(),
        },
        BenchQuery {
            query: "symbol.rs".into(),
            expected_symbol: "Symbol".into(),
            category: "file-path".into(),
        },
        BenchQuery {
            query: "graph.rs".into(),
            expected_symbol: "TraverseDirection".into(),
            category: "file-path".into(),
        },
        BenchQuery {
            query: "rerank".into(),
            expected_symbol: "Reranker".into(),
            category: "file-path".into(),
        },
        BenchQuery {
            query: "DbError".into(),
            expected_symbol: "DbError".into(),
            category: "error-debug".into(),
        },
        BenchQuery {
            query: "all edge kinds".into(),
            expected_symbol: "EdgeKind".into(),
            category: "cross-cutting".into(),
        },
        BenchQuery {
            query: "all language parsers".into(),
            expected_symbol: "LanguageChunker".into(),
            category: "cross-cutting".into(),
        },
    ]
}
