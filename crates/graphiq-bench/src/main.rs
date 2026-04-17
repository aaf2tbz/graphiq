use std::path::Path;
use std::time::Instant;

use graphiq_core::cache::HotCache;
use graphiq_core::db::GraphDb;
use graphiq_core::index::Indexer;
use graphiq_core::lsa;
use graphiq_core::search::{SearchEngine, SearchQuery};

#[derive(Debug, Clone, serde::Deserialize)]
struct BenchQuery {
    query: String,
    category: String,
    #[serde(default)]
    expected_symbol: Option<String>,
    #[serde(default)]
    relevance: std::collections::HashMap<String, u32>,
}

impl BenchQuery {
    fn relevance_of(&self, symbol_name: &str) -> u32 {
        if let Some(rel) = self.relevance.get(symbol_name) {
            return *rel;
        }
        if let Some(exp) = &self.expected_symbol {
            if symbol_name == exp {
                return 3;
            }
        }
        0
    }

    fn max_relevance(&self) -> u32 {
        let from_map = self.relevance.values().copied().max().unwrap_or(0);
        let from_expected = if self.expected_symbol.is_some() { 3 } else { 0 };
        from_map.max(from_expected)
    }

    fn has_relevance(&self) -> bool {
        !self.relevance.is_empty() || self.expected_symbol.is_some()
    }
}

fn dcg_at_k(relevances: &[f64], k: usize) -> f64 {
    relevances
        .iter()
        .take(k)
        .enumerate()
        .map(|(i, rel)| {
            if i == 0 {
                *rel
            } else {
                *rel / ((i + 1) as f64).log2()
            }
        })
        .sum()
}

fn ndcg_at_k(results: &[f64], ideal: &[f64], k: usize) -> f64 {
    let dcg = dcg_at_k(results, k);
    let idcg = dcg_at_k(ideal, k);
    if idcg == 0.0 {
        0.0
    } else {
        dcg / idcg
    }
}

#[derive(Debug)]
struct BenchResult {
    query: String,
    category: String,
    ndcg: f64,
    best_relevance: u32,
    best_rank: Option<usize>,
    hit_at_1: bool,
    hit_at_3: bool,
    hit_at_5: bool,
    hit_at_10: bool,
    latency_us: u128,
    warm_latency_us: u128,
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

    #[cfg(feature = "embed")]
    {
        print!("Embedding symbols ... ");
        let indexer = Indexer::new(&db);
        match indexer.embed_symbols(None) {
            Ok(count) => println!("done ({} symbols embedded)", count),
            Err(e) => println!("embed failed: {e}"),
        }
        let embed_count = db.embedding_count().unwrap_or(0);
        println!("Embeddings: {} vectors stored\n", embed_count);
    }

    print!("Computing LSA ... ");
    let lsa_index = match lsa::compute_lsa(&db) {
        Ok(idx) => {
            let n_syms = idx.symbol_ids.len();
            let n_terms = idx.term_index.len();
            let dim = idx.symbol_vecs.first().map(|v| v.len()).unwrap_or(0);
            let sym_id_to_idx: std::collections::HashMap<i64, usize> = idx
                .symbol_ids
                .iter()
                .enumerate()
                .map(|(i, &id)| (id, i))
                .collect();

            match lsa::store_lsa_vectors(&db, &idx.symbol_ids, &idx.symbol_vecs) {
                Ok(c) => eprintln!("  stored {} latent vectors", c),
                Err(e) => eprintln!("  store failed: {e}"),
            }
            match lsa::store_lsa_basis(&db, &idx.term_basis, &idx.term_index) {
                Ok(()) => {}
                Err(e) => eprintln!("  basis store failed: {e}"),
            }

            println!("done ({} terms × {} symbols, dim={})", n_terms, n_syms, dim);
            Some((idx, sym_id_to_idx))
        }
        Err(e) => {
            println!("failed: {e}");
            None
        }
    };

    let queries: Vec<BenchQuery> = if let Some(query_file) = args.get(3) {
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

        let result_rels: Vec<f64> = result
            .results
            .iter()
            .map(|r| q.relevance_of(&r.symbol.name) as f64)
            .collect();

        let ideal_rels = compute_ideal_rels(&db, q);

        let ndcg = ndcg_at_k(&result_rels, &ideal_rels, 10);

        let best_relevance = result
            .results
            .iter()
            .map(|r| q.relevance_of(&r.symbol.name))
            .max()
            .unwrap_or(0);

        let best_rank = result
            .results
            .iter()
            .position(|r| q.relevance_of(&r.symbol.name) > 0)
            .map(|p| p + 1);

        let hit_at_1 = best_rank.map_or(false, |r| r <= 1) && best_relevance >= 2;
        let hit_at_3 = best_rank.map_or(false, |r| r <= 3) && best_relevance >= 2;
        let hit_at_5 = best_rank.map_or(false, |r| r <= 5) && best_relevance >= 1;
        let hit_at_10 = best_rank.map_or(false, |r| r <= 10) && best_relevance >= 1;

        results.push(BenchResult {
            query: q.query.clone(),
            category: q.category.clone(),
            ndcg,
            best_relevance,
            best_rank,
            hit_at_1,
            hit_at_3,
            hit_at_5,
            hit_at_10,
            latency_us: cold_latency,
            warm_latency_us: warm_latency,
        });
    }

    if let Some((ref lsa_idx, ref sym_map)) = lsa_index {
        println!("\n=== Pure LSA Evaluation (angular distance on hypersphere) ===\n");
        let conn = db.conn();
        let mut lsa_results: Vec<BenchResult> = Vec::new();

        for q in &queries {
            let query_vec = lsa::project_query(
                &q.query,
                &lsa_idx.term_index,
                &lsa_idx.term_basis,
                lsa_idx.term_index.len(),
            );

            let mut scored: Vec<(usize, f64)> = lsa_idx
                .symbol_vecs
                .iter()
                .enumerate()
                .map(|(i, v)| (i, lsa::angular_distance(&query_vec, v)))
                .collect();
            scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
            scored.truncate(10);

            let result_rels: Vec<f64> = scored
                .iter()
                .map(|(idx, _)| {
                    let sym_id = lsa_idx.symbol_ids.get(*idx).copied().unwrap_or(0);
                    let name: String = conn
                        .query_row("SELECT name FROM symbols WHERE id = ?", [sym_id], |row| {
                            row.get(0)
                        })
                        .unwrap_or_default();
                    q.relevance_of(&name) as f64
                })
                .collect();

            let ideal_rels = compute_ideal_rels(&db, q);
            let ndcg = ndcg_at_k(&result_rels, &ideal_rels, 10);

            let best_relevance = scored
                .iter()
                .map(|(idx, _)| {
                    let sym_id = lsa_idx.symbol_ids.get(*idx).copied().unwrap_or(0);
                    let name: String = conn
                        .query_row("SELECT name FROM symbols WHERE id = ?", [sym_id], |row| {
                            row.get(0)
                        })
                        .unwrap_or_default();
                    q.relevance_of(&name)
                })
                .max()
                .unwrap_or(0);

            let best_rank = scored
                .iter()
                .position(|(idx, _)| {
                    let sym_id = lsa_idx.symbol_ids.get(*idx).copied().unwrap_or(0);
                    let name: String = conn
                        .query_row("SELECT name FROM symbols WHERE id = ?", [sym_id], |row| {
                            row.get(0)
                        })
                        .unwrap_or_default();
                    q.relevance_of(&name) > 0
                })
                .map(|p| p + 1);

            let hit_at_1 = best_rank.map_or(false, |r| r <= 1) && best_relevance >= 2;
            let hit_at_3 = best_rank.map_or(false, |r| r <= 3) && best_relevance >= 2;
            let hit_at_5 = best_rank.map_or(false, |r| r <= 5) && best_relevance >= 1;
            let hit_at_10 = best_rank.map_or(false, |r| r <= 10) && best_relevance >= 1;

            lsa_results.push(BenchResult {
                query: q.query.clone(),
                category: q.category.clone(),
                ndcg,
                best_relevance,
                best_rank,
                hit_at_1,
                hit_at_3,
                hit_at_5,
                hit_at_10,
                latency_us: 0,
                warm_latency_us: 0,
            });
        }

        let total = lsa_results.len();
        let avg_ndcg: f64 = lsa_results.iter().map(|r| r.ndcg).sum::<f64>() / total as f64;
        let hits_1 = lsa_results.iter().filter(|r| r.hit_at_1).count();
        let hits_3 = lsa_results.iter().filter(|r| r.hit_at_3).count();
        let hits_10 = lsa_results.iter().filter(|r| r.hit_at_10).count();

        println!("LSA NDCG@10: {:.3}", avg_ndcg);
        println!(
            "LSA Hit@1: {}/{} ({:.0}%)",
            hits_1,
            total,
            hits_1 as f64 / total as f64 * 100.0
        );
        println!(
            "LSA Hit@3: {}/{} ({:.0}%)",
            hits_3,
            total,
            hits_3 as f64 / total as f64 * 100.0
        );
        println!(
            "LSA Hit@10: {}/{} ({:.0}%)",
            hits_10,
            total,
            hits_10 as f64 / total as f64 * 100.0
        );

        println!("\n--- Per-Query LSA vs BM25 ---\n");
        println!(
            "{:<30} {:<15} {:>8} {:>8} {:>6} {:>6}",
            "Query", "Category", "LSA", "BM25", "L@1", "B@1"
        );
        println!("{}", "-".repeat(100));
        for (bm25, lsa) in results.iter().zip(lsa_results.iter()) {
            println!(
                "{:<30} {:<15} {:>8.3} {:>8.3} {:>6} {:>6}",
                truncate(&bm25.query, 30),
                bm25.category,
                lsa.ndcg,
                bm25.ndcg,
                if lsa.hit_at_1 { "Y" } else { "N" },
                if bm25.hit_at_1 { "Y" } else { "N" },
            );
        }
    }

    let total = results.len();
    let avg_ndcg: f64 = results.iter().map(|r| r.ndcg).sum::<f64>() / total as f64;
    let hits_1 = results.iter().filter(|r| r.hit_at_1).count();
    let hits_3 = results.iter().filter(|r| r.hit_at_3).count();
    let hits_5 = results.iter().filter(|r| r.hit_at_5).count();
    let hits_10 = results.iter().filter(|r| r.hit_at_10).count();

    let cold_latencies: Vec<u128> = results.iter().map(|r| r.latency_us).collect();
    let mut sorted_cold = cold_latencies;
    sorted_cold.sort();
    let p50_cold = sorted_cold[sorted_cold.len() / 2];
    let p95_idx = ((sorted_cold.len() as f64) * 0.95) as usize;
    let p95_cold = sorted_cold[p95_idx.min(sorted_cold.len() - 1)];

    println!("=== Results ===\n");
    println!("NDCG@10:  {:.3}", avg_ndcg);
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

    let warm_latencies: Vec<u128> = results.iter().map(|r| r.warm_latency_us).collect();
    let mut sorted_warm = warm_latencies;
    sorted_warm.sort();
    let p50_warm = sorted_warm[sorted_warm.len() / 2];
    println!("Latency (warm):  p50={:.1}ms", p50_warm as f64 / 1000.0);

    println!("\n=== Per-Query Detail ===\n");
    println!(
        "{:<30} {:<15} {:>6} {:>6} {:>8} {:>8} {:>8}",
        "Query", "Category", "NDCG", "Best", "Rank", "Cold", "Warm"
    );
    println!("{}", "-".repeat(95));

    for r in &results {
        let rank_str = r
            .best_rank
            .map(|rank| format!("{}", rank))
            .unwrap_or_else(|| "MISS".into());
        let best_str = if r.best_relevance >= 3 {
            "###".to_string()
        } else if r.best_relevance >= 2 {
            "##".to_string()
        } else if r.best_relevance >= 1 {
            "#".to_string()
        } else {
            "-".to_string()
        };
        println!(
            "{:<30} {:<15} {:>6.2} {:>6} {:>6} {:>6.1}ms {:>6.1}ms",
            truncate(&r.query, 30),
            r.category,
            r.ndcg,
            best_str,
            rank_str,
            r.latency_us as f64 / 1000.0,
            r.warm_latency_us as f64 / 1000.0
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
        let cat_ndcg: f64 = cat_results.iter().map(|r| r.ndcg).sum::<f64>() / cat_total as f64;
        let cat_hit1 = cat_results.iter().filter(|r| r.hit_at_1).count();
        let cat_hit3 = cat_results.iter().filter(|r| r.hit_at_3).count();
        let cat_hit5 = cat_results.iter().filter(|r| r.hit_at_5).count();
        let cat_hit10 = cat_results.iter().filter(|r| r.hit_at_10).count();

        println!(
            "{:<20} NDCG={:.3}  H@1={}/{}  H@3={}/{}  H@5={}/{}  H@10={}/{}",
            cat,
            cat_ndcg,
            cat_hit1,
            cat_total,
            cat_hit3,
            cat_total,
            cat_hit5,
            cat_total,
            cat_hit10,
            cat_total,
        );
    }
}

fn compute_ideal_rels(db: &GraphDb, q: &BenchQuery) -> Vec<f64> {
    let conn = db.conn();
    let mut ideal: Vec<f64> = Vec::new();

    if !q.relevance.is_empty() {
        for (name, grade) in &q.relevance {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM symbols WHERE name = ?",
                    [&name],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            for _ in 0..count {
                ideal.push(*grade as f64);
            }
        }
    } else if let Some(exp) = &q.expected_symbol {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM symbols WHERE name = ?",
                [&exp],
                |row| row.get(0),
            )
            .unwrap_or(0);
        for _ in 0..count.max(1) {
            ideal.push(3.0);
        }
    }

    ideal.sort_by(|a, b| b.partial_cmp(a).unwrap());
    ideal
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max - 3])
    }
}

fn build_default_queries() -> Vec<BenchQuery> {
    let pairs = vec![
        ("SymbolKind", "SymbolKind", "symbol-exact"),
        ("bounded_bfs", "bounded_bfs", "symbol-exact"),
        ("EdgeKind", "EdgeKind", "symbol-exact"),
        ("GraphDb", "GraphDb", "symbol-exact"),
        ("HotCache", "HotCache", "symbol-exact"),
        (
            "parse_edge_kind_json",
            "parse_edge_kind_json",
            "symbol-exact",
        ),
        ("SearchQuery", "SearchQuery", "symbol-exact"),
        ("sym kind", "SymbolKind", "symbol-partial"),
        ("edge", "Edge", "symbol-partial"),
        ("cache", "HotCache", "symbol-partial"),
        ("blast", "blast", "symbol-partial"),
        ("token", "tokenize", "symbol-partial"),
        ("rerank", "Reranker", "symbol-partial"),
        ("bm25 full text search", "search", "nl-descriptive"),
        (
            "structural graph expansion",
            "StructuralExpander",
            "nl-descriptive",
        ),
        (
            "identifier decomposition tokenize",
            "decompose_identifier",
            "nl-descriptive",
        ),
        (
            "compute blast radius",
            "compute_blast_radius",
            "nl-descriptive",
        ),
        ("insert symbol database", "insert_symbol", "nl-descriptive"),
        ("how does retrieval ranking work", "Reranker", "nl-abstract"),
        (
            "how are symbols indexed from source files",
            "Indexer",
            "nl-abstract",
        ),
        (
            "what connects callers to callees",
            "bounded_bfs",
            "nl-abstract",
        ),
        ("symbol.rs", "Symbol", "file-path"),
        ("graph.rs", "TraverseDirection", "file-path"),
        ("rerank", "Reranker", "file-path"),
        ("DbError", "DbError", "error-debug"),
        ("all edge kinds", "EdgeKind", "cross-cutting"),
        ("all language parsers", "LanguageChunker", "cross-cutting"),
    ];

    pairs
        .into_iter()
        .map(|(query, expected, category)| BenchQuery {
            query: query.into(),
            category: category.into(),
            expected_symbol: Some(expected.into()),
            relevance: std::collections::HashMap::new(),
        })
        .collect()
}
