use std::path::Path;
use std::time::Instant;

use graphiq_core::cache::HotCache;
use graphiq_core::db::GraphDb;
use graphiq_core::evidence;
use graphiq_core::index::Indexer;
use graphiq_core::search::{SearchEngine, SearchQuery};
use graphiq_core::sec;

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
}

fn eval_hits(hits: &[(i64, f64)], q: &BenchQuery, db: &GraphDb) -> BenchResult {
    let conn = db.conn();
    let result_rels: Vec<f64> = hits
        .iter()
        .map(|(sym_id, _)| {
            let name: String = conn
                .query_row("SELECT name FROM symbols WHERE id = ?", [*sym_id], |row| {
                    row.get(0)
                })
                .unwrap_or_default();
            q.relevance_of(&name) as f64
        })
        .collect();
    let ideal_rels = compute_ideal_rels(db, q);
    let ndcg = ndcg_at_k(&result_rels, &ideal_rels, 10);
    let best_relevance = hits
        .iter()
        .map(|(sym_id, _)| {
            conn.query_row("SELECT name FROM symbols WHERE id = ?", [*sym_id], |row| {
                row.get::<_, String>(0)
            })
            .unwrap_or_default()
        })
        .map(|name| q.relevance_of(&name))
        .max()
        .unwrap_or(0);
    let best_rank = hits
        .iter()
        .position(|(sym_id, _)| {
            let name: String = conn
                .query_row("SELECT name FROM symbols WHERE id = ?", [*sym_id], |row| {
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
    BenchResult {
        query: q.query.clone(),
        category: q.category.clone(),
        ndcg,
        best_relevance,
        best_rank,
        hit_at_1,
        hit_at_3,
        hit_at_5,
        hit_at_10,
    }
}

fn print_summary(label: &str, res: &[BenchResult]) {
    let total = res.len();
    let ndcg: f64 = res.iter().map(|r| r.ndcg).sum::<f64>() / total as f64;
    let h1 = res.iter().filter(|r| r.hit_at_1).count();
    let h3 = res.iter().filter(|r| r.hit_at_3).count();
    let h5 = res.iter().filter(|r| r.hit_at_5).count();
    let h10 = res.iter().filter(|r| r.hit_at_10).count();
    println!(
        "{} NDCG@10: {:.3}  Hit@1: {}/{}  Hit@3: {}/{}  Hit@5: {}/{}  Hit@10: {}/{}",
        label, ndcg, h1, total, h3, total, h5, total, h10, total
    );
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

    println!("=== GraphIQ SEC Benchmark ===\n");

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

    print!("Computing HRR (holographic) ... ");
    let hrr_index = match graphiq_core::hrr::compute_hrr(&db) {
        Ok(idx) => {
            println!("done ({} symbols)", idx.symbol_ids.len());
            Some(idx)
        }
        Err(e) => {
            println!("failed: {e}");
            None
        }
    };

    print!("Computing SEC (structural evidence convolution) ... ");
    let sec_index = match sec::build_sec_index(&db) {
        Ok(idx) => {
            println!("done ({} symbols)", idx.symbol_ids.len());
            Some(idx)
        }
        Err(e) => {
            println!("failed: {e}");
            None
        }
    };

    print!("Building evidence index ... ");
    let evidence_index = match evidence::build_evidence_index(&db) {
        Ok(idx) => {
            println!("done ({} symbols)", idx.symbol_ids.len());
            Some(idx)
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

    // --- Baseline engine: BM25+REFT+evidence+HRR (no SEC) ---
    let cache_baseline = HotCache::with_defaults();
    let mut baseline_engine = SearchEngine::new(&db, &cache_baseline);
    if let Some(ref hrr_idx) = hrr_index {
        baseline_engine = baseline_engine.with_hrr(hrr_idx);
    }
    if let Some(ref ev_idx) = evidence_index {
        baseline_engine = baseline_engine.with_evidence(ev_idx);
    }
    let baseline_engine = baseline_engine;

    // --- SEC engine: baseline + SEC reranking ---
    let cache_sec = HotCache::with_defaults();
    let mut sec_engine = SearchEngine::new(&db, &cache_sec);
    if let Some(ref hrr_idx) = hrr_index {
        sec_engine = sec_engine.with_hrr(hrr_idx);
    }
    if let Some(ref ev_idx) = evidence_index {
        sec_engine = sec_engine.with_evidence(ev_idx);
    }
    if let Some(ref sec_idx) = sec_index {
        sec_engine = sec_engine.with_sec(sec_idx);
    }
    let sec_engine = sec_engine;

    println!("Running {} queries ...\n", queries.len());

    // --- Baseline results ---
    let baseline_results: Vec<BenchResult> = queries
        .iter()
        .map(|q| {
            let result = baseline_engine.search(&SearchQuery::new(&q.query).top_k(10));
            let hits: Vec<(i64, f64)> = result
                .results
                .iter()
                .map(|r| (r.symbol.id, r.score))
                .collect();
            eval_hits(&hits, q, &db)
        })
        .collect();

    // --- SEC Pipeline results ---
    let sec_pipeline_results: Vec<BenchResult> = queries
        .iter()
        .map(|q| {
            let result = sec_engine.search(&SearchQuery::new(&q.query).top_k(10));
            let hits: Vec<(i64, f64)> = result
                .results
                .iter()
                .map(|r| (r.symbol.id, r.score))
                .collect();
            eval_hits(&hits, q, &db)
        })
        .collect();

    // --- SEC Pure results ---
    let sec_pure_results: Vec<BenchResult> = if let Some(ref sec_idx) = sec_index {
        queries
            .iter()
            .map(|q| eval_hits(&sec::sec_search(&q.query, sec_idx, 10), q, &db))
            .collect()
    } else {
        Vec::new()
    };

    // --- SEC Rerank on baseline candidates ---
    let sec_rerank_results: Vec<BenchResult> = if let Some(ref sec_idx) = sec_index {
        queries
            .iter()
            .map(|q| {
                let result = baseline_engine.search(&SearchQuery::new(&q.query).top_k(50));
                let cids: Vec<i64> = result.results.iter().map(|r| r.symbol.id).collect();
                let cscores: Vec<f64> = result.results.iter().map(|r| r.score).collect();
                let reranked = sec::sec_rerank(&q.query, &cids, &cscores, sec_idx, 10);
                eval_hits(&reranked, q, &db)
            })
            .collect()
    } else {
        Vec::new()
    };

    // --- Print summaries ---
    println!("=== Summary ===\n");
    print_summary("Baseline (BM25+REFT+HRR) ", &baseline_results);
    print_summary("SEC Pipeline (Base+SEC)  ", &sec_pipeline_results);
    if !sec_pure_results.is_empty() {
        print_summary("SEC Pure (no BM25)       ", &sec_pure_results);
    }
    if !sec_rerank_results.is_empty() {
        print_summary("SEC Rerank (on Base@50)  ", &sec_rerank_results);
    }

    // --- Per-query comparison ---
    println!("\n=== Per-Query: Baseline vs SEC Pipeline vs SEC Pure vs SEC Rerank ===\n");
    println!(
        "{:<40} {:>8} {:>8} {:>8} {:>8}  {}",
        "Query", "Base", "Pipe", "Pure", "ReRnk", "Winner"
    );
    println!("{}", "-".repeat(110));

    for (i, q) in queries.iter().enumerate() {
        let b = baseline_results.get(i);
        let p = sec_pipeline_results.get(i);
        let sp = sec_pure_results.get(i);
        let sr = sec_rerank_results.get(i);

        let b_ndcg = b.map(|r| r.ndcg).unwrap_or(0.0);
        let p_ndcg = p.map(|r| r.ndcg).unwrap_or(0.0);
        let sp_ndcg = sp.map(|r| r.ndcg).unwrap_or(0.0);
        let sr_ndcg = sr.map(|r| r.ndcg).unwrap_or(0.0);

        let best = b_ndcg.max(p_ndcg).max(sp_ndcg).max(sr_ndcg);
        let winner = if sr_ndcg == best && sr.is_some() {
            "SR"
        } else if sp_ndcg == best && sp.is_some() {
            "SP"
        } else if p_ndcg == best && p.is_some() {
            "P"
        } else {
            "B"
        };

        let b_rank = b
            .and_then(|r| r.best_rank)
            .map(|r| format!("{}", r))
            .unwrap_or_else(|| "MISS".into());
        let sr_rank = sr
            .and_then(|r| r.best_rank)
            .map(|r| format!("{}", r))
            .unwrap_or_else(|| "MISS".into());

        println!(
            "{:<40} {:>8.3} {:>8.3} {:>8.3} {:>8.3}  {} (B:{} SR:{})",
            truncate(&q.query, 40),
            b_ndcg,
            p_ndcg,
            sp_ndcg,
            sr_ndcg,
            winner,
            b_rank,
            sr_rank
        );
    }

    // --- SEC Debug ---
    if let Some(ref sec_idx) = sec_index {
        println!("\n=== SEC Debug (channel scores) ===\n");
        for q in &queries {
            let result = baseline_engine.search(&SearchQuery::new(&q.query).top_k(50));
            let cids: Vec<i64> = result
                .results
                .iter()
                .take(50)
                .map(|r| r.symbol.id)
                .collect();
            let cscores: Vec<f64> = result.results.iter().take(50).map(|r| r.score).collect();
            let debug_results = sec::sec_rerank_debug(&q.query, &cids, &cscores, sec_idx, 5);
            println!("Q: {}", q.query);
            for (id, score, cs) in &debug_results {
                let name = sec_idx
                    .id_to_idx
                    .get(id)
                    .map(|&i| sec_idx.symbol_names[i].as_str())
                    .unwrap_or("?");
                let channels_hitting = [
                    cs.ch_self > 0.01,
                    cs.ch_calls_out > 0.01,
                    cs.ch_calls_in > 0.01,
                    cs.ch_calls_out_2hop > 0.01,
                    cs.ch_calls_in_2hop > 0.01,
                    cs.ch_type_ret > 0.01,
                    cs.ch_file_path > 0.01,
                ]
                .iter()
                .filter(|&&x| x)
                .count();
                println!(
                    "  {:45} s={:6.1} self={:.2} out={:.2} in={:.2} out2={:.2} in2={:.2} typ={:.2} path={:.2} [{}ch]",
                    truncate(name, 45), score,
                    cs.ch_self, cs.ch_calls_out, cs.ch_calls_in,
                    cs.ch_calls_out_2hop, cs.ch_calls_in_2hop,
                    cs.ch_type_ret, cs.ch_file_path,
                    channels_hitting
                );
            }
            println!();
        }
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
