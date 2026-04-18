use std::path::Path;

use graphiq_core::cruncher;
use graphiq_core::db::GraphDb;
use graphiq_core::fts::FtsSearch;

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
        .map(|(rank, rel)| rel / ((rank + 2) as f64).log2())
        .sum()
}

fn ndcg_at_k(results: &[f64], ideal: &[f64], k: usize) -> f64 {
    let dcg = dcg_at_k(results, k);
    let idcg = dcg_at_k(ideal, k);
    if idcg == 0.0 { 0.0 } else { dcg / idcg }
}

fn compute_ideal_rels(db: &GraphDb, q: &BenchQuery) -> Vec<f64> {
    let conn = db.conn();
    let mut ideal: Vec<f64> = Vec::new();
    if !q.relevance.is_empty() {
        for (name, grade) in &q.relevance {
            let count: i64 = conn
                .query_row("SELECT COUNT(*) FROM symbols WHERE name = ?", [&name], |row| row.get(0))
                .unwrap_or(0);
            for _ in 0..count {
                ideal.push(*grade as f64);
            }
        }
    } else if let Some(exp) = &q.expected_symbol {
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM symbols WHERE name = ?", [&exp], |row| row.get(0))
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

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max - 3])
    }
}

fn run_searches(
    fts: &FtsSearch,
    ci: &cruncher::CruncherIndex,
    query: &str,
    top_k: usize,
) -> [Vec<(i64, f64)>; 3] {
    let bm25: Vec<(i64, f64)> = fts
        .search(query, Some(top_k))
        .into_iter()
        .map(|r| (r.symbol.id, r.bm25_score))
        .collect();

    let cr_solo = cruncher::cruncher_search_standalone(query, ci, top_k);
    let cr_fused = cruncher::cruncher_search(query, ci, &bm25, top_k);

    [bm25, cr_solo, cr_fused]
}

fn run_ndcg_benchmark(
    db: &GraphDb,
    fts: &FtsSearch,
    ci: &cruncher::CruncherIndex,
    queries: &[BenchQuery],
) {
    println!("\n{}", "=".repeat(60));
    println!("  NDCG@10 BENCHMARK  ({} queries)", queries.len());
    println!("{}", "=".repeat(60));

    let methods = ["BM25", "CR Solo", "CR Fused"];
    let n = queries.len();
    let mut all_ndcg: [Vec<f64>; 3] = Default::default();
    let mut all_hits: [Vec<[bool; 5]>; 3] = Default::default();
    let mut cat_data: std::collections::HashMap<String, [Vec<f64>; 3]> =
        std::collections::HashMap::new();

    for q in queries {
        let ideal = compute_ideal_rels(db, q);
        let results = run_searches(fts, ci, &q.query, 10);

        for (mi, hits) in results.iter().enumerate() {
            let rels: Vec<f64> = hits
                .iter()
                .map(|(id, _)| q.relevance_of(&sym_name(db, *id)) as f64)
                .collect();
            let ndcg = ndcg_at_k(&rels, &ideal, 10);
            all_ndcg[mi].push(ndcg);

            let first_rel = hits
                .iter()
                .position(|(id, _)| q.relevance_of(&sym_name(db, *id)) >= 2);
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
        "{:<12} {:>8} {:>6} {:>6} {:>6} {:>6}",
        "Method", "NDCG@10", "H@1", "H@3", "H@5", "H@10"
    );
    println!("{}", "-".repeat(46));
    for (mi, name) in methods.iter().enumerate() {
        let avg: f64 = all_ndcg[mi].iter().sum::<f64>() / n as f64;
        let h1 = all_hits[mi].iter().filter(|h| h[0]).count();
        let h3 = all_hits[mi].iter().filter(|h| h[1]).count();
        let h5 = all_hits[mi].iter().filter(|h| h[2]).count();
        let h10 = all_hits[mi].iter().filter(|h| h[3]).count();
        println!(
            "{:<12} {:>8.3} {:>6} {:>6} {:>6} {:>6}",
            name, avg, h1, h3, h5, h10
        );
    }

    println!("\n--- By Category ---\n");
    let mut cats: Vec<&String> = cat_data.keys().collect();
    cats.sort();
    println!("{:<20} {:>8} {:>8} {:>8}", "Category", "BM25", "CR Solo", "CR Fused");
    println!("{}", "-".repeat(50));
    for cat in &cats {
        let d = &cat_data[*cat];
        let avg: Vec<f64> = d.iter().map(|v| v.iter().sum::<f64>() / v.len() as f64).collect();
        println!("{:<20} {:>8.3} {:>8.3} {:>8.3}", cat, avg[0], avg[1], avg[2]);
    }

    println!("\n--- Per-Query ---\n");
    println!("{:<40} {:>8} {:>8} {:>8}", "Query", "BM25", "CR Solo", "CR Fused");
    println!("{}", "-".repeat(70));
    for (i, q) in queries.iter().enumerate() {
        println!(
            "{:<40} {:>8.3} {:>8.3} {:>8.3}",
            truncate(&q.query, 40), all_ndcg[0][i], all_ndcg[1][i], all_ndcg[2][i]
        );
    }
}

fn run_mrr_benchmark(
    db: &GraphDb,
    fts: &FtsSearch,
    ci: &cruncher::CruncherIndex,
    queries: &[BenchQuery],
) {
    println!("\n{}", "=".repeat(60));
    println!("  MRR BENCHMARK  ({} queries)", queries.len());
    println!("{}", "=".repeat(60));

    let methods = ["BM25", "CR Solo", "CR Fused"];
    let n = queries.len();

    struct MrrResult {
        rr: f64,
        hit_at: [bool; 5],
        accuracy: bool,
        found_rank: Option<usize>,
    }

    let mut all: [Vec<MrrResult>; 3] = [Vec::new(), Vec::new(), Vec::new()];

    for q in queries {
        let results = run_searches(fts, ci, &q.query, 10);

        for (mi, hits) in results.iter().enumerate() {
            let expected = q.expected_symbol.as_deref().unwrap_or("");
            let found_rank = hits.iter().position(|(id, _)| {
                let name = sym_name(db, *id);
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

            all[mi].push(MrrResult { rr, hit_at: h, accuracy, found_rank });
        }
    }

    println!("\n--- Summary ---\n");
    println!(
        "{:<12} {:>6} {:>9} {:>6} {:>6} {:>6} {:>6} {:>6}",
        "Method", "MRR", "Accuracy", "H@1", "H@3", "H@5", "H@10", "Miss"
    );
    println!("{}", "-".repeat(68));
    for (mi, name) in methods.iter().enumerate() {
        let mrr: f64 = all[mi].iter().map(|r| r.rr).sum::<f64>() / n as f64;
        let acc: f64 = all[mi].iter().filter(|r| r.accuracy).count() as f64 / n as f64;
        let h1 = all[mi].iter().filter(|r| r.hit_at[0]).count();
        let h3 = all[mi].iter().filter(|r| r.hit_at[1]).count();
        let h5 = all[mi].iter().filter(|r| r.hit_at[2]).count();
        let h10 = all[mi].iter().filter(|r| r.hit_at[3]).count();
        let miss = all[mi].iter().filter(|r| !r.hit_at[4]).count();
        println!(
            "{:<12} {:>6.3} {:>9.3} {:>6} {:>6} {:>6} {:>6} {:>6}",
            name, mrr, acc, h1, h3, h5, h10, miss
        );
    }

    println!("\n--- Per-Query ---\n");
    println!(
        "{:<40} {:>6} {:>6} {:>6}  {:>6} {:>6} {:>6}",
        "Query", "B_rr", "S_rr", "F_rr", "B_rnk", "S_rnk", "F_rnk"
    );
    println!("{}", "-".repeat(78));
    for (i, q) in queries.iter().enumerate() {
        let fmt = |r: Option<usize>| -> String {
            r.map(|v| format!("{}", v + 1)).unwrap_or_else(|| "MISS".into())
        };
        println!(
            "{:<40} {:>6.3} {:>6.3} {:>6.3}  {:>6} {:>6} {:>6}",
            truncate(&q.query, 40),
            all[0][i].rr, all[1][i].rr, all[2][i].rr,
            fmt(all[0][i].found_rank), fmt(all[1][i].found_rank), fmt(all[2][i].found_rank)
        );
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: graphiq-bench <db-path> <ndcg-queries.json> <mrr-queries.json>");
        std::process::exit(1);
    }

    let db_path = &args[1];

    let db = match GraphDb::open(Path::new(db_path)) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error opening database: {e}");
            std::process::exit(1);
        }
    };

    let stats = db.stats().unwrap();
    println!("=== Cruncher Benchmark ===\n");
    println!(
        "Database: {} files, {} symbols, {} edges\n",
        stats.files, stats.symbols, stats.edges
    );

    let fts = FtsSearch::new(&db);

    let ci = match cruncher::build_cruncher_index(&db) {
        Ok(idx) => idx,
        Err(e) => {
            eprintln!("cruncher build failed: {e}");
            std::process::exit(1);
        }
    };

    let ndcg_file = args.get(2).map(|s| s.as_str());
    let mrr_file = args.get(3).map(|s| s.as_str());

    if let Some(file) = ndcg_file {
        let content = std::fs::read_to_string(file).unwrap_or_else(|e| {
            eprintln!("error reading NDCG query file: {e}");
            std::process::exit(1);
        });
        let queries: Vec<BenchQuery> = serde_json::from_str(&content).unwrap_or_else(|e| {
            eprintln!("error parsing NDCG query file: {e}");
            std::process::exit(1);
        });
        run_ndcg_benchmark(&db, &fts, &ci, &queries);
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
        run_mrr_benchmark(&db, &fts, &ci, &queries);
    }

    if ndcg_file.is_none() && mrr_file.is_none() {
        eprintln!("no query files provided. usage: graphiq-bench <db> <ndcg.json> <mrr.json>");
    }
}
