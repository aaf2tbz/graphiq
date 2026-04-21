use std::collections::HashMap;
use std::path::Path;

use graphiq_core::cache::HotCache;
use graphiq_core::db::GraphDb;
use graphiq_core::index::Indexer;
use graphiq_core::search::{SearchEngine, SearchQuery};

#[derive(Debug, Clone, serde::Deserialize)]
struct LocomoQuery {
    query: String,
    category: String,
    #[serde(default)]
    expected_symbol: Option<String>,
    #[serde(default)]
    relevance: HashMap<String, u32>,
}

impl LocomoQuery {
    fn relevance_of(&self, name: &str) -> u32 {
        if let Some(r) = self.relevance.get(name) {
            return *r;
        }
        if let Some(exp) = &self.expected_symbol {
            if name == exp {
                return 3;
            }
        }
        0
    }

    fn total_relevant(&self, db: &GraphDb) -> usize {
        let conn = db.conn();
        let mut count = 0usize;
        if !self.relevance.is_empty() {
            for name in self.relevance.keys() {
                let n: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM symbols WHERE name = ?",
                        [name],
                        |row| row.get(0),
                    )
                    .unwrap_or(0);
                count += n as usize;
            }
        } else if let Some(exp) = &self.expected_symbol {
            let n: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM symbols WHERE name = ?",
                    [exp],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            count += n.max(1) as usize;
        }
        count
    }
}

struct LocomoResult {
    query: String,
    category: String,
    first_relevant_rank: Option<usize>,
    result_rels: Vec<u32>,
    total_relevant: usize,
    ndcg10: f64,
}

fn dcg_at_k(rels: &[f64], k: usize) -> f64 {
    rels.iter()
        .take(k)
        .enumerate()
        .map(|(i, r)| {
            if i == 0 {
                *r
            } else {
                *r / ((i + 1) as f64).log2()
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

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max - 3])
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: graphiq-locomo <project-path> [db-path] [query-file.json]");
        std::process::exit(1);
    }

    let project_path = Path::new(&args[1]);
    let db_path = args
        .get(2)
        .map(|s| s.clone())
        .unwrap_or_else(|| ".graphiq/locomo.db".into());

    if !project_path.exists() {
        eprintln!("project path not found: {}", project_path.display());
        std::process::exit(1);
    }

    let db = match GraphDb::open(Path::new(&db_path)) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error opening database: {e}");
            std::process::exit(1);
        }
    };

    println!("=== GraphIQ LoCoMo Benchmark ===\n");

    let project_name = project_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "?".into());
    print!("Indexing {} ... ", project_name);
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
        "Database: {} files, {} symbols, {} edges",
        db_stats.files, db_stats.symbols, db_stats.edges
    );

    let queries: Vec<LocomoQuery> = if let Some(qf) = args.get(3) {
        let content = std::fs::read_to_string(qf).unwrap_or_else(|e| {
            eprintln!("error reading query file: {e}");
            std::process::exit(1);
        });
        serde_json::from_str(&content).unwrap_or_else(|e| {
            eprintln!("error parsing query file: {e}");
            std::process::exit(1);
        })
    } else {
        eprintln!("no query file specified");
        std::process::exit(1);
    };

    let cache = HotCache::with_defaults();

    let cruncher = graphiq_core::cruncher::build_cruncher_index(&db).ok();

    let mut engine = SearchEngine::new(&db, &cache);
    if let Some(ref ci) = cruncher {
        engine = engine.with_cruncher(ci);
    }

    println!("\nRunning {} queries ...\n", queries.len());

    let mut results: Vec<LocomoResult> = Vec::new();

    for q in &queries {
        let search_result = engine.search(&SearchQuery::new(&q.query).top_k(10).debug(true));
        let result_rels: Vec<u32> = search_result
            .results
            .iter()
            .map(|r| q.relevance_of(&r.symbol.name))
            .collect();

        let first_relevant_rank = result_rels.iter().position(|&r| r > 0).map(|p| p + 1);

        let result_rels_f: Vec<f64> = result_rels.iter().map(|&r| r as f64).collect();

        let mut ideal: Vec<f64> = Vec::new();
        let conn = db.conn();
        if !q.relevance.is_empty() {
            for (name, grade) in &q.relevance {
                let count: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM symbols WHERE name = ?",
                        [name],
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
                    [exp],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            for _ in 0..count.max(1) {
                ideal.push(3.0);
            }
        }
        ideal.sort_by(|a, b| b.partial_cmp(a).unwrap());

        let ndcg10 = ndcg_at_k(&result_rels_f, &ideal, 10);
        let total_relevant = q.total_relevant(&db);

        results.push(LocomoResult {
            query: q.query.clone(),
            category: q.category.clone(),
            first_relevant_rank,
            result_rels,
            total_relevant,
            ndcg10,
        });
    }

    let n = results.len() as f64;

    let accuracy = results
        .iter()
        .filter(|r| {
            r.first_relevant_rank == Some(1) && r.result_rels.first().copied().unwrap_or(0) >= 2
        })
        .count() as f64
        / n
        * 100.0;

    let hit1 = results
        .iter()
        .filter(|r| r.first_relevant_rank.map_or(false, |rk| rk <= 1))
        .count() as f64
        / n
        * 100.0;
    let hit3 = results
        .iter()
        .filter(|r| r.first_relevant_rank.map_or(false, |rk| rk <= 3))
        .count() as f64
        / n
        * 100.0;
    let hit5 = results
        .iter()
        .filter(|r| r.first_relevant_rank.map_or(false, |rk| rk <= 5))
        .count() as f64
        / n
        * 100.0;
    let hit10 = results
        .iter()
        .filter(|r| r.first_relevant_rank.is_some())
        .count() as f64
        / n
        * 100.0;

    let mrr: f64 = results
        .iter()
        .map(|r| {
            r.first_relevant_rank
                .map(|rk| 1.0 / rk as f64)
                .unwrap_or(0.0)
        })
        .sum::<f64>()
        / n;

    let precision10: f64 = results
        .iter()
        .map(|r| {
            let relevant_in_10 = r.result_rels.iter().filter(|&&rel| rel > 0).count() as f64;
            relevant_in_10 / 10.0
        })
        .sum::<f64>()
        / n
        * 100.0;

    let recall10: f64 = results
        .iter()
        .map(|r| {
            if r.total_relevant == 0 {
                0.0
            } else {
                let relevant_in_10 = r.result_rels.iter().filter(|&&rel| rel > 0).count();
                relevant_in_10 as f64 / r.total_relevant as f64
            }
        })
        .sum::<f64>()
        / n
        * 100.0;

    let ndcg10_avg: f64 = results.iter().map(|r| r.ndcg10).sum::<f64>() / n;

    println!("| {:<16} {:>8} |", "Metric", "Score");
    println!("|{}---------|--------:|", "-".repeat(17));
    println!("| {:<16} {:>7.1}% |", "Accuracy", accuracy);
    println!("| {:<16} {:>7.1}% |", "Hit@1", hit1);
    println!("| {:<16} {:>7.1}% |", "Hit@3", hit3);
    println!("| {:<16} {:>7.1}% |", "Hit@5", hit5);
    println!("| {:<16} {:>7.1}% |", "Hit@10", hit10);
    println!("| {:<16} {:>8.3} |", "MRR", mrr);
    println!("| {:<16} {:>7.1}% |", "Precision@10", precision10);
    println!("| {:<16} {:>7.1}% |", "Recall@10", recall10);
    println!("| {:<16} {:>8.3} |", "NDCG@10", ndcg10_avg);

    let categories: Vec<&str> = {
        let mut cats: Vec<&str> = results.iter().map(|r| r.category.as_str()).collect();
        cats.sort();
        cats.dedup();
        cats
    };

    println!("\nBy query type:\n");
    println!(
        "| {:<18} {:>4} {:>8} {:>6} {:>6} {:>6} {:>6} {:>8} {:>8} {:>7} {:>8} |",
        "Type", "n", "Acc%", "H@1", "H@3", "H@5", "H@10", "MRR", "Prec%", "Rec%", "NDCG"
    );
    println!(
        "|{}------|-----|---------|------|------|------|------|--------|--------|-------|--------|",
        "-".repeat(19)
    );

    for cat in &categories {
        let cat_results: Vec<&LocomoResult> =
            results.iter().filter(|r| r.category == *cat).collect();
        let cn = cat_results.len() as f64;

        let c_acc = cat_results
            .iter()
            .filter(|r| {
                r.first_relevant_rank == Some(1) && r.result_rels.first().copied().unwrap_or(0) >= 2
            })
            .count() as f64
            / cn
            * 100.0;

        let c_h1 = cat_results
            .iter()
            .filter(|r| r.first_relevant_rank.map_or(false, |rk| rk <= 1))
            .count() as f64
            / cn
            * 100.0;
        let c_h3 = cat_results
            .iter()
            .filter(|r| r.first_relevant_rank.map_or(false, |rk| rk <= 3))
            .count() as f64
            / cn
            * 100.0;
        let c_h5 = cat_results
            .iter()
            .filter(|r| r.first_relevant_rank.map_or(false, |rk| rk <= 5))
            .count() as f64
            / cn
            * 100.0;
        let c_h10 = cat_results
            .iter()
            .filter(|r| r.first_relevant_rank.is_some())
            .count() as f64
            / cn
            * 100.0;

        let c_mrr: f64 = cat_results
            .iter()
            .map(|r| {
                r.first_relevant_rank
                    .map(|rk| 1.0 / rk as f64)
                    .unwrap_or(0.0)
            })
            .sum::<f64>()
            / cn;

        let c_prec: f64 = cat_results
            .iter()
            .map(|r| r.result_rels.iter().filter(|&&rel| rel > 0).count() as f64 / 10.0)
            .sum::<f64>()
            / cn
            * 100.0;

        let c_rec: f64 = cat_results
            .iter()
            .map(|r| {
                if r.total_relevant == 0 {
                    0.0
                } else {
                    r.result_rels.iter().filter(|&&rel| rel > 0).count() as f64
                        / r.total_relevant as f64
                }
            })
            .sum::<f64>()
            / cn
            * 100.0;

        let c_ndcg: f64 = cat_results.iter().map(|r| r.ndcg10).sum::<f64>() / cn;

        println!(
            "| {:<18} {:>4} {:>7.1}% {:>5.0}% {:>5.0}% {:>5.0}% {:>5.0}% {:>8.3} {:>7.1}% {:>6.1}% {:>8.3} |",
            cat,
            cat_results.len(),
            c_acc,
            c_h1,
            c_h3,
            c_h5,
            c_h10,
            c_mrr,
            c_prec,
            c_rec,
            c_ndcg,
        );
    }

    println!("\nPer-query detail:\n");
    println!(
        "{:<35} {:<18} {:>5} {:>5} {:>6} {:>6} {:>8}",
        "Query", "Category", "Rank", "Rel@", "Top", "Tot", "NDCG"
    );
    println!("{}", "-".repeat(95));

    for r in &results {
        let rank_str = r
            .first_relevant_rank
            .map(|rk| format!("{}", rk))
            .unwrap_or_else(|| "MISS".into());
        let top_rel = r.result_rels.first().copied().unwrap_or(0);
        let found = r.result_rels.iter().filter(|&&rel| rel > 0).count();
        println!(
            "{:<35} {:<18} {:>5} {:>5} {:>6} {:>6} {:>8.3}",
            truncate(&r.query, 35),
            r.category,
            rank_str,
            top_rel,
            found,
            r.total_relevant,
            r.ndcg10,
        );
    }
}
