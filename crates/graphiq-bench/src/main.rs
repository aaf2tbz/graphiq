use std::path::Path;

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

fn sym_name(db: &GraphDb, id: i64) -> String {
    db.conn()
        .query_row("SELECT name FROM symbols WHERE id = ?", [id], |row| {
            row.get::<_, String>(0)
        })
        .unwrap_or_default()
}

struct EngineSet<'a> {
    baseline: SearchEngine<'a>,
    sec_engine: SearchEngine<'a>,
    sec_idx: &'a sec::SecIndex,
    sec_inv: &'a sec::SecInvertedIndex,
    db: &'a GraphDb,
}

fn run_searches(es: &EngineSet, query: &str, top_k: usize) -> [Vec<(i64, f64)>; 4] {
    let base: Vec<(i64, f64)> = {
        let r = es.baseline.search(&SearchQuery::new(query).top_k(top_k));
        r.results.iter().map(|r| (r.symbol.id, r.score)).collect()
    };
    let sec_pipe: Vec<(i64, f64)> = {
        let r = es.sec_engine.search(&SearchQuery::new(query).top_k(top_k));
        r.results.iter().map(|r| (r.symbol.id, r.score)).collect()
    };
    let sec_solo = sec::sec_standalone_search(query, es.sec_idx, es.sec_inv, top_k);
    let sec_fusion = sec::sec_fusion_rerank(query, &base, es.sec_idx, es.sec_inv, top_k);
    [base, sec_pipe, sec_solo, sec_fusion]
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max - 3])
    }
}

// ─── NDCG Benchmark ───

fn run_ndcg_benchmark(es: &EngineSet, queries: &[BenchQuery]) {
    println!("\n{}", "=".repeat(60));
    println!("  NDCG@10 BENCHMARK  ({} queries)", queries.len());
    println!("{}", "=".repeat(60));

    let methods = ["Baseline", "SEC Pipe", "SEC Solo", "SEC Fused"];
    let n = queries.len();

    let mut all_ndcg: [Vec<f64>; 4] = Default::default();
    let mut all_hits: [Vec<[bool; 5]>; 4] = Default::default();
    let mut cat_data: std::collections::HashMap<String, [Vec<f64>; 4]> =
        std::collections::HashMap::new();

    for q in queries {
        let ideal = compute_ideal_rels(es.db, q);
        let results = run_searches(es, &q.query, 10);

        for (mi, hits) in results.iter().enumerate() {
            let rels: Vec<f64> = hits
                .iter()
                .map(|(id, _)| q.relevance_of(&sym_name(es.db, *id)) as f64)
                .collect();
            let ndcg = ndcg_at_k(&rels, &ideal, 10);
            all_ndcg[mi].push(ndcg);

            let first_rel = hits
                .iter()
                .position(|(id, _)| q.relevance_of(&sym_name(es.db, *id)) >= 2);
            let h: [bool; 5] = [
                first_rel.map_or(false, |r| r < 1),
                first_rel.map_or(false, |r| r < 3),
                first_rel.map_or(false, |r| r < 5),
                first_rel.map_or(false, |r| r < 10),
                first_rel.is_some(),
            ];
            all_hits[mi].push(h);

            cat_data.entry(q.category.clone()).or_default()[mi].push(ndcg);
        }
    }

    println!("\n--- Overall ---\n");
    println!(
        "{:<15} {:>8} {:>6} {:>6} {:>6} {:>6}",
        "Method", "NDCG@10", "H@1", "H@3", "H@5", "H@10"
    );
    println!("{}", "-".repeat(50));
    for (mi, name) in methods.iter().enumerate() {
        let avg: f64 = all_ndcg[mi].iter().sum::<f64>() / n as f64;
        let h1 = all_hits[mi].iter().filter(|h| h[0]).count();
        let h3 = all_hits[mi].iter().filter(|h| h[1]).count();
        let h5 = all_hits[mi].iter().filter(|h| h[2]).count();
        let h10 = all_hits[mi].iter().filter(|h| h[3]).count();
        println!(
            "{:<15} {:>8.3} {:>6} {:>6} {:>6} {:>6}",
            name, avg, h1, h3, h5, h10
        );
    }

    println!("\n--- By Category ---\n");
    let mut cats: Vec<&String> = cat_data.keys().collect();
    cats.sort();
    println!(
        "{:<20} {:>8} {:>8} {:>8} {:>8}",
        "Category", "Baseline", "SEC Pipe", "SEC Solo", "SEC Fused"
    );
    println!("{}", "-".repeat(56));
    for cat in &cats {
        let d = &cat_data[*cat];
        let avg: Vec<f64> = d
            .iter()
            .map(|v| v.iter().sum::<f64>() / v.len() as f64)
            .collect();
        println!(
            "{:<20} {:>8.3} {:>8.3} {:>8.3} {:>8.3}",
            cat, avg[0], avg[1], avg[2], avg[3]
        );
    }

    println!("\n--- Per-Query ---\n");
    println!(
        "{:<45} {:>8} {:>8} {:>8} {:>8}",
        "Query", "Baseline", "SEC Pipe", "SEC Solo", "SEC Fused"
    );
    println!("{}", "-".repeat(85));
    for (i, q) in queries.iter().enumerate() {
        let b = all_ndcg[0][i];
        let p = all_ndcg[1][i];
        let s = all_ndcg[2][i];
        let f = all_ndcg[3][i];
        println!(
            "{:<45} {:>8.3} {:>8.3} {:>8.3} {:>8.3}",
            truncate(&q.query, 45),
            b,
            p,
            s,
            f
        );
    }
}

// ─── MRR Benchmark ───

fn run_mrr_benchmark(es: &EngineSet, queries: &[BenchQuery]) {
    println!("\n{}", "=".repeat(60));
    println!("  MRR BENCHMARK  ({} queries)", queries.len());
    println!("{}", "=".repeat(60));

    let methods = ["Baseline", "SEC Pipe", "SEC Solo", "SEC Fused"];
    let n = queries.len();

    struct MrrResult {
        rr: f64,
        hit_at: [bool; 5],
        precision_at_5: f64,
        recall_at_5: f64,
        accuracy: bool,
        found_rank: Option<usize>,
    }

    let mut all: [Vec<MrrResult>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];

    for q in queries {
        let results = run_searches(es, &q.query, 10);

        for (mi, hits) in results.iter().enumerate() {
            let expected = q.expected_symbol.as_deref().unwrap_or("");
            let found_rank = hits.iter().position(|(id, _)| {
                let name = sym_name(es.db, *id);
                name == expected || expected.contains(&name) || name.contains(expected)
            });

            let rr = found_rank.map(|r| 1.0 / (r + 1) as f64).unwrap_or(0.0);
            let accuracy = found_rank == Some(0);

            let h: [bool; 5] = [
                found_rank.map_or(false, |r| r < 1),
                found_rank.map_or(false, |r| r < 3),
                found_rank.map_or(false, |r| r < 5),
                found_rank.map_or(false, |r| r < 10),
                found_rank.is_some(),
            ];

            let relevant_in_5 = hits
                .iter()
                .take(5)
                .filter(|(id, _)| {
                    let name = sym_name(es.db, *id);
                    name == expected || expected.contains(&name) || name.contains(expected)
                })
                .count() as f64;
            let precision_at_5 = relevant_in_5 / 5.0_f64.min(hits.len() as f64);
            let recall_at_5 = if found_rank.map_or(false, |r| r < 5) {
                1.0
            } else {
                0.0
            };

            all[mi].push(MrrResult {
                rr,
                hit_at: h,
                precision_at_5,
                recall_at_5,
                accuracy,
                found_rank,
            });
        }
    }

    println!("\n--- Summary ---\n");
    println!(
        "{:<15} {:>6} {:>9} {:>9} {:>9} {:>6} {:>6} {:>6} {:>6} {:>6}",
        "Method", "MRR", "Accuracy", "P@5", "R@5", "H@1", "H@3", "H@5", "H@10", "Miss"
    );
    println!("{}", "-".repeat(90));
    for (mi, name) in methods.iter().enumerate() {
        let mrr: f64 = all[mi].iter().map(|r| r.rr).sum::<f64>() / n as f64;
        let acc: f64 = all[mi].iter().filter(|r| r.accuracy).count() as f64 / n as f64;
        let p5: f64 = all[mi].iter().map(|r| r.precision_at_5).sum::<f64>() / n as f64;
        let r5: f64 = all[mi].iter().map(|r| r.recall_at_5).sum::<f64>() / n as f64;
        let h1 = all[mi].iter().filter(|r| r.hit_at[0]).count();
        let h3 = all[mi].iter().filter(|r| r.hit_at[1]).count();
        let h5 = all[mi].iter().filter(|r| r.hit_at[2]).count();
        let h10 = all[mi].iter().filter(|r| r.hit_at[3]).count();
        let miss = all[mi].iter().filter(|r| !r.hit_at[4]).count();
        println!(
            "{:<15} {:>6.3} {:>9.3} {:>9.3} {:>9.3} {:>6} {:>6} {:>6} {:>6} {:>6}",
            name, mrr, acc, p5, r5, h1, h3, h5, h10, miss
        );
    }

    println!("\n--- Per-Query ---\n");
    println!(
        "{:<45} {:>6} {:>6} {:>6} {:>6}  {:>6} {:>6} {:>6} {:>6}",
        "Query", "B_rr", "P_rr", "S_rr", "F_rr", "B_rnk", "P_rnk", "S_rnk", "F_rnk"
    );
    println!("{}", "-".repeat(105));
    for (i, q) in queries.iter().enumerate() {
        let br = all[0][i].found_rank;
        let pr = all[1][i].found_rank;
        let sr = all[2][i].found_rank;
        let fr = all[3][i].found_rank;
        let fmt = |r: Option<usize>| -> String {
            r.map(|v| format!("{}", v + 1))
                .unwrap_or_else(|| "MISS".into())
        };
        println!(
            "{:<45} {:>6.3} {:>6.3} {:>6.3} {:>6.3}  {:>6} {:>6} {:>6} {:>6}",
            truncate(&q.query, 45),
            all[0][i].rr,
            all[1][i].rr,
            all[2][i].rr,
            all[3][i].rr,
            fmt(br),
            fmt(pr),
            fmt(sr),
            fmt(fr)
        );
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!(
            "usage: graphiq-bench <project-path> [db-path] <ndcg-queries.json> <mrr-queries.json>"
        );
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

    print!("Computing HRR ... ");
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

    print!("Computing SEC ... ");
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

    let sec_inv = sec_index.as_ref().map(|idx| {
        let inv = sec::build_sec_inverted_index(idx);
        inv
    });

    print!("Building evidence index ... ");
    let evidence_index = match evidence::build_evidence_index(&db) {
        Ok(idx) => {
            println!("done");
            Some(idx)
        }
        Err(e) => {
            println!("failed: {e}");
            None
        }
    };

    let sec_index = match sec_index {
        Some(idx) => idx,
        None => {
            eprintln!("SEC index required");
            std::process::exit(1);
        }
    };
    let sec_inv = match sec_inv {
        Some(inv) => inv,
        None => {
            eprintln!("SEC inverted index required");
            std::process::exit(1);
        }
    };

    // Build engines
    let cache_b = HotCache::with_defaults();
    let mut baseline_engine = SearchEngine::new(&db, &cache_b);
    if let Some(ref hrr) = hrr_index {
        baseline_engine = baseline_engine.with_hrr(hrr);
    }
    if let Some(ref ev) = evidence_index {
        baseline_engine = baseline_engine.with_evidence(ev);
    }

    let cache_s = HotCache::with_defaults();
    let mut sec_engine = SearchEngine::new(&db, &cache_s);
    if let Some(ref hrr) = hrr_index {
        sec_engine = sec_engine.with_hrr(hrr);
    }
    if let Some(ref ev) = evidence_index {
        sec_engine = sec_engine.with_evidence(ev);
    }
    sec_engine = sec_engine.with_sec(&sec_index);

    let es = EngineSet {
        baseline: baseline_engine,
        sec_engine,
        sec_idx: &sec_index,
        sec_inv: &sec_inv,
        db: &db,
    };

    // Load query files
    let ndcg_file = args.get(3).map(|s| s.as_str());
    let mrr_file = args.get(4).map(|s| s.as_str());

    if let Some(file) = ndcg_file {
        let content = std::fs::read_to_string(file).unwrap_or_else(|e| {
            eprintln!("error reading NDCG query file: {e}");
            std::process::exit(1);
        });
        let queries: Vec<BenchQuery> = serde_json::from_str(&content).unwrap_or_else(|e| {
            eprintln!("error parsing NDCG query file: {e}");
            std::process::exit(1);
        });
        run_ndcg_benchmark(&es, &queries);
    }

    if let Some(file) = mrr_file {
        let content = std::fs::read_to_string(file).unwrap_or_else(|e| {
            eprintln!("error reading MRR query file: {e}");
            std::process::exit(1);
        });
        let queries: Vec<BenchQuery> = serde_json::from_str(&content).unwrap_or_else(|e| {
            eprintln!("error parsing MRR query file: {e}");
            std::process::exit(1);
        });
        run_mrr_benchmark(&es, &queries);
    }

    if ndcg_file.is_none() && mrr_file.is_none() {
        eprintln!(
            "no query files provided. usage: graphiq-bench <project> [db] <ndcg.json> <mrr.json>"
        );
    }
}
