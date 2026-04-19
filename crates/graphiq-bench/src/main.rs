use std::path::Path;

use graphiq_core::cruncher;
use graphiq_core::db::GraphDb;
use graphiq_core::fts::FtsSearch;
use graphiq_core::spectral::{ChannelFingerprint, PredictiveModel, SpectralIndex};
use graphiq_core::search::{SearchEngine, SearchQuery};
use graphiq_core::cache::HotCache;
use graphiq_core::self_model::RepoSelfModel;
use graphiq_core::query_family::classify_query_family;

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

fn grep_search(db: &GraphDb, query: &str, top_k: usize) -> Vec<(i64, f64)> {
    let terms: Vec<&str> = query
        .split_whitespace()
        .filter(|t| t.len() >= 2)
        .collect();
    if terms.is_empty() {
        return Vec::new();
    }

    let conn = db.conn();
    let mut candidates: std::collections::HashMap<i64, f64> = std::collections::HashMap::new();

    for term in &terms {
        let lower = term.to_lowercase();
        let pattern = format!("%{}%", lower.replace('_', "%"));

        let mut stmt = conn
            .prepare("SELECT id, name FROM symbols WHERE lower(name) LIKE ?1")
            .unwrap();

        let rows: Vec<(i64, String)> = stmt
            .query_map([&pattern], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        for (id, name) in rows {
            let name_lower = name.to_lowercase();
            let score = if name_lower == lower {
                3.0
            } else if name_lower.starts_with(&lower) {
                2.5
            } else if name_lower.contains(&lower) {
                2.0
            } else {
                1.0
            };
            *candidates.entry(id).or_insert(0.0) += score;
        }
    }

    for term in &terms {
        let lower = term.to_lowercase();
        let words: Vec<String> = lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() >= 2)
            .map(|w| w.to_string())
            .collect();

        for word in &words {
            let pattern = format!("%{}%", word);
            let mut stmt = conn
                .prepare("SELECT id FROM symbols WHERE lower(source) LIKE ?1")
                .unwrap();
            let rows: Vec<i64> = stmt
                .query_map([&pattern], |row| row.get(0))
                .unwrap()
                .filter_map(|r| r.ok())
                .collect();

            for id in rows {
                *candidates.entry(id).or_insert(0.0) += 0.5;
            }
        }
    }

    let mut ranked: Vec<(i64, f64)> = candidates.into_iter().collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked.truncate(top_k);
    ranked
}

const METHODS: &[&str] = &["GraphIQ", "Grep"];

fn run_ndcg(fe: &FullEngine, queries: &[BenchQuery]) {
    let n = queries.len();
    let n_methods = METHODS.len();
    let cutoffs: &[usize] = &[3, 5, 10];
    let mut per_query: Vec<Vec<Vec<f64>>> = vec![vec![vec![]; cutoffs.len()]; n_methods];
    let mut cat_data: std::collections::HashMap<String, Vec<Vec<f64>>> =
        std::collections::HashMap::new();

    for q in queries {
        let ideal = compute_ideal_rels(fe.db, q);

        for mi in 0..n_methods {
            let hits = match mi {
                0 => fe.run_router(&q.query, 10),
                1 => grep_search(fe.db, &q.query, 10),
                _ => Vec::new(),
            };
            let rels: Vec<f64> = hits
                .iter()
                .map(|(id, _)| q.relevance_of(&sym_name(fe.db, *id)) as f64)
                .collect();
            for (ki, &k) in cutoffs.iter().enumerate() {
                let ndcg = ndcg_at_k(&rels, &ideal, k);
                per_query[mi][ki].push(ndcg);
            }
            let ndcg10 = per_query[mi][cutoffs.len() - 1].last().copied().unwrap_or(0.0);
            cat_data
                .entry(q.category.clone())
                .or_insert_with(|| vec![vec![]; n_methods])[mi]
                .push(ndcg10);
        }
    }

    println!("\n{}", "=".repeat(72));
    println!("  NDCG@K  ({} queries)", n);
    println!("{}", "=".repeat(72));

    println!("\n{:<10} {:>8} {:>8} {:>8}", "Method", "H@3", "H@5", "H@10");
    println!("{}", "-".repeat(40));
    for mi in 0..n_methods {
        print!("{:<10}", METHODS[mi]);
        for ki in 0..cutoffs.len() {
            let avg: f64 = per_query[mi][ki].iter().sum::<f64>() / n as f64;
            print!(" {:>8.3}", avg);
        }
        println!();
    }

    println!("\n--- By Category (NDCG@10) ---\n");
    let mut cats: Vec<&String> = cat_data.keys().collect();
    cats.sort();
    print!("{:<20}", "Category");
    for m in METHODS { print!("{:>10}", m); }
    println!();
    println!("{}", "-".repeat(20 + 10 * n_methods));
    for cat in &cats {
        let d = &cat_data[*cat];
        print!("{:<20}", cat);
        for mi in 0..n_methods {
            let avg = d[mi].iter().sum::<f64>() / d[mi].len() as f64;
            print!("{:>10.3}", avg);
        }
        println!();
    }

    println!("\n--- Per Query (NDCG@10) ---\n");
    print!("{:<30}", "Query");
    for m in METHODS { print!("{:>10}", m); }
    println!();
    println!("{}", "-".repeat(30 + 10 * n_methods));
    for (i, q) in queries.iter().enumerate() {
        print!("{:<30}", truncate(&q.query, 30));
        for mi in 0..n_methods {
            print!("{:>10.3}", per_query[mi][cutoffs.len() - 1][i]);
        }
        println!();
    }
}

fn run_mrr(fe: &FullEngine, queries: &[BenchQuery]) {
    let n = queries.len();
    let n_methods = METHODS.len();

    #[derive(Clone)]
    struct MrrRow {
        rr: f64,
        found_rank: Option<usize>,
        hits_in_10: usize,
        relevant_total: usize,
    }

    let mut all: Vec<Vec<MrrRow>> = vec![vec![]; n_methods];
    let mut cat_data: std::collections::HashMap<String, Vec<Vec<f64>>> =
        std::collections::HashMap::new();

    for q in queries {
        let expected = q.expected_symbol.as_deref().unwrap_or("");

        let relevant_total = if !q.relevance.is_empty() {
            q.relevance.values().filter(|&&v| v >= 2).count()
        } else {
            1
        };

        for mi in 0..n_methods {
            let hits = match mi {
                0 => fe.run_router(&q.query, 10),
                1 => grep_search(fe.db, &q.query, 10),
                _ => Vec::new(),
            };

            let found_rank = if !q.relevance.is_empty() {
                hits.iter().position(|(id, _)| {
                    q.relevance_of(&sym_name(fe.db, *id)) >= 2
                })
            } else {
                hits.iter().position(|(id, _)| {
                    let name = sym_name(fe.db, *id);
                    name == expected || expected.contains(&name) || name.contains(expected)
                })
            };

            let hits_in_10 = hits.iter()
                .filter(|(id, _)| {
                    if !q.relevance.is_empty() {
                        q.relevance_of(&sym_name(fe.db, *id)) >= 2
                    } else {
                        let name = sym_name(fe.db, *id);
                        name == expected || expected.contains(&name) || name.contains(expected)
                    }
                })
                .count();

            let rr = found_rank.map(|r| 1.0 / (r + 1) as f64).unwrap_or(0.0);
            all[mi].push(MrrRow { rr, found_rank, hits_in_10, relevant_total });
            cat_data
                .entry(q.category.clone())
                .or_insert_with(|| vec![vec![]; n_methods])[mi]
                .push(rr);
        }
    }

    println!("\n{}", "=".repeat(84));
    println!("  MRR@10  ({} queries)", n);
    println!("{}", "=".repeat(84));

    println!("\n{:<10} {:>8} {:>7} {:>7} {:>7} {:>9} {:>9}", "Method", "MRR", "P@10", "R@10", "H@10", "Acc@1", "Acc@10");
    println!("{}", "-".repeat(70));
    for mi in 0..n_methods {
        let mrr: f64 = all[mi].iter().map(|r| r.rr).sum::<f64>() / n as f64;
        let p10: f64 = all[mi].iter().map(|r| r.hits_in_10 as f64 / 10.0).sum::<f64>() / n as f64;
        let r10: f64 = all[mi].iter()
            .filter(|r| r.relevant_total > 0)
            .map(|r| (r.hits_in_10 as f64 / r.relevant_total as f64).min(1.0))
            .sum::<f64>() / n as f64;
        let h10 = all[mi].iter().filter(|r| r.found_rank.is_some()).count();
        let acc1 = all[mi].iter().filter(|r| r.found_rank == Some(0)).count();
        let acc10 = h10;
        println!("{:<10} {:>8.3} {:>7.3} {:>7.3} {:>5}/{}  {:>5}/{}  {:>5}/{}",
            METHODS[mi], mrr, p10, r10, h10, n, acc1, n, acc10, n);
    }

    println!("\n--- By Category (MRR) ---\n");
    let mut cats: Vec<&String> = cat_data.keys().collect();
    cats.sort();
    print!("{:<20}", "Category");
    for m in METHODS { print!("{:>10}", m); }
    println!();
    println!("{}", "-".repeat(20 + 10 * n_methods));
    for cat in &cats {
        let d = &cat_data[*cat];
        print!("{:<20}", cat);
        for mi in 0..n_methods {
            let avg = d[mi].iter().sum::<f64>() / d[mi].len() as f64;
            print!("{:>10.3}", avg);
        }
        println!();
    }

    println!("\n--- Per Query (rank) ---\n");
    print!("{:<30}", "Query");
    for m in METHODS { print!("{:>10}", m); }
    println!();
    println!("{}", "-".repeat(30 + 10 * n_methods));
    for (i, q) in queries.iter().enumerate() {
        print!("{:<30}", truncate(&q.query, 30));
        for mi in 0..n_methods {
            let fmt = match all[mi][i].found_rank {
                Some(r) => format!("{}", r + 1),
                None => "MISS".into(),
            };
            print!("{:>10}", fmt);
        }
        println!();
    }
}

const ALL_METHODS: &[&str] = &[
    "BM25", "CRv1", "CRv2", "Goober", "GooV3", "GooV4", "GooV5",
    "Geometric", "Curved", "Deformed",
];

struct FullEngine<'a> {
    db: &'a GraphDb,
    fts: &'a FtsSearch<'a>,
    ci: &'a cruncher::CruncherIndex,
    hi: &'a cruncher::HoloIndex,
    spectral: &'a Option<SpectralIndex>,
    predictive: &'a Option<PredictiveModel>,
    fingerprints: &'a [ChannelFingerprint],
    fp_id_to_idx: &'a std::collections::HashMap<i64, usize>,
    cache: &'a HotCache,
    self_model: &'a Option<RepoSelfModel>,
    engine: SearchEngine<'a>,
}

impl<'a> FullEngine<'a> {
    fn run_method(&self, method_idx: usize, query: &str, top_k: usize) -> Vec<(i64, f64)> {
        let bm25: Vec<(i64, f64)> = self.fts.search(query, Some(200))
            .into_iter()
            .map(|r| (r.symbol.id, r.bm25_score))
            .collect();

        match method_idx {
            0 => bm25.iter().take(top_k).cloned().collect(),
            1 => cruncher::cruncher_search(query, self.ci, &bm25, top_k),
            2 => cruncher::cruncher_v2_search(query, self.ci, &bm25, top_k),
            3 => cruncher::goober_search(query, self.ci, &bm25, top_k),
            4 => cruncher::goober_v3_search(query, self.ci, &bm25, top_k),
            5 => cruncher::goober_v4_search(query, self.ci, &bm25, top_k),
            6 => cruncher::goober_v5_search(query, self.ci, self.hi, &bm25, top_k),
            7 => self.spectral.as_ref().map_or(Vec::new(), |spec| {
                cruncher::geometric_search(query, self.ci, self.hi, &bm25, spec, top_k,
                    1.0, 15, 5.0, 50, false, None, None, None, 1.0)
            }),
            8 => self.spectral.as_ref().map_or(Vec::new(), |spec| {
                cruncher::geometric_search(query, self.ci, self.hi, &bm25, spec, top_k,
                    1.0, 15, 5.0, 50, true, None, None, None, 1.0)
            }),
            9 => self.spectral.as_ref().map_or(Vec::new(), |spec| {
                cruncher::geometric_search(query, self.ci, self.hi, &bm25, spec, top_k,
                    1.0, 15, 5.0, 50, false,
                    self.predictive.as_ref(), Some(self.fingerprints), Some(self.fp_id_to_idx), 1.0)
            }),
            _ => Vec::new(),
        }
    }

    fn run_router(&self, query: &str, top_k: usize) -> Vec<(i64, f64)> {
        let q = SearchQuery::new(query).top_k(top_k);
        self.engine.search(&q)
            .results.iter()
            .map(|r| (r.symbol.id, r.score))
            .collect()
    }
}

fn cmd_diagnose(fe: &FullEngine, queries: &[BenchQuery]) {
    let n_methods = ALL_METHODS.len();
    println!("\n{}", "=".repeat(100));
    println!("  DIAGNOSTIC: Router vs All Methods  ({} queries)", queries.len());
    println!("{}", "=".repeat(100));

    let mut router_wins = 0usize;
    let mut router_ties = 0usize;
    let mut router_loses = 0usize;

    for q in queries {
        let family = classify_query_family(&q.query);
        let expected = q.expected_symbol.as_deref().unwrap_or("");

        let router_result = fe.run_router(&q.query, 10);
        let router_rank = if !q.relevance.is_empty() {
            router_result.iter().position(|(id, _)| {
                q.relevance_of(&sym_name(fe.db, *id)) >= 2
            })
        } else {
            router_result.iter().position(|(id, _)| {
                let name = sym_name(fe.db, *id);
                name == expected || expected.contains(&name) || name.contains(expected)
            })
        };

        let mut method_ranks: Vec<Option<usize>> = Vec::with_capacity(n_methods);
        for mi in 0..n_methods {
            let result = fe.run_method(mi, &q.query, 10);
            let rank = if !q.relevance.is_empty() {
                result.iter().position(|(id, _)| {
                    q.relevance_of(&sym_name(fe.db, *id)) >= 2
                })
            } else {
                result.iter().position(|(id, _)| {
                    let name = sym_name(fe.db, *id);
                    name == expected || expected.contains(&name) || name.contains(expected)
                })
            };
            method_ranks.push(rank);
        }

        let best_rank = method_ranks.iter()
            .filter_map(|r| *r)
            .min()
            .unwrap_or(usize::MAX);

        let router_r = router_rank.unwrap_or(usize::MAX);
        let winner_idx = method_ranks.iter()
            .enumerate()
            .filter(|(_, r)| r.map_or(false, |v| v == best_rank))
            .map(|(i, _)| i)
            .next()
            .unwrap_or(0);

        let verdict = if router_r < best_rank {
            router_wins += 1;
            "ROUTER_WINS"
        } else if router_r == best_rank {
            router_ties += 1;
            "TIE"
        } else {
            router_loses += 1;
            "ROUTER_LOSES"
        };

        println!("\n  [{}] {}", verdict, truncate(&q.query, 60));
        println!("    family={:?}  router_rank={}  best_possible={} (via {})",
            family,
            router_rank.map(|r| r + 1).map(|v| format!("{}", v)).unwrap_or_else(|| "MISS".into()),
            if best_rank == usize::MAX { "MISS".into() } else { format!("{}", best_rank + 1) },
            ALL_METHODS[winner_idx],
        );

        print!("    ranks: ");
        for (mi, rank) in method_ranks.iter().enumerate() {
            let r_str = rank.map(|r| format!("{}", r + 1)).unwrap_or_else(|| "MISS".into());
            let marker = if *rank == Some(best_rank) && best_rank != usize::MAX { "*" } else { " " };
            print!("{}{}={} ", marker, ALL_METHODS[mi], r_str);
        }
        println!();

        if router_r > best_rank && best_rank != usize::MAX {
            let best_result = fe.run_method(winner_idx, &q.query, 10);
            let router_top3: Vec<String> = router_result.iter().take(3)
                .map(|(id, _)| sym_name(fe.db, *id))
                .collect();
            let best_top3: Vec<String> = best_result.iter().take(3)
                .map(|(id, _)| sym_name(fe.db, *id))
                .collect();
            println!("    router_top3: {:?}", router_top3);
            println!("    {}_top3:   {:?}", ALL_METHODS[winner_idx], best_top3);
        }
    }

    println!("\n{}", "=".repeat(60));
    println!("  Router: {} wins, {} ties, {} loses (of {})", router_wins, router_ties, router_loses, queries.len());
    println!("{}", "=".repeat(60));
}

fn run_speed_bench(fe: &FullEngine, queries: &[BenchQuery]) {
    use std::time::Instant;

    let n_queries = queries.len();
    let warmup_iters = 5;
    let bench_iters = 50;

    println!("\n{}", "=".repeat(70));
    println!("  Speed Benchmark  ({} queries, {} iterations after {} warmup)", n_queries, bench_iters, warmup_iters);
    println!("{}", "=".repeat(70));

    let mut g_times: Vec<Vec<f64>> = vec![vec![]; n_queries];
    let mut r_times: Vec<Vec<f64>> = vec![vec![]; n_queries];
    let mut g_rr: Vec<f64> = vec![0.0; n_queries];
    let mut r_rr: Vec<f64> = vec![0.0; n_queries];

    for (qi, q) in queries.iter().enumerate() {
        let expected = q.expected_symbol.as_deref().unwrap_or("");

        for _ in 0..warmup_iters {
            let _ = fe.run_router(&q.query, 10);
            let _ = grep_search(fe.db, &q.query, 10);
        }

        for _ in 0..bench_iters {
            let t = Instant::now();
            let g_hits = fe.run_router(&q.query, 10);
            g_times[qi].push(t.elapsed().as_micros() as f64);

            let t = Instant::now();
            let r_hits = grep_search(fe.db, &q.query, 10);
            r_times[qi].push(t.elapsed().as_micros() as f64);

            let g_rank = g_hits.iter().position(|(id, _)| {
                let name = sym_name(fe.db, *id);
                name == expected || expected.contains(&name) || name.contains(expected)
            });
            let r_rank = r_hits.iter().position(|(id, _)| {
                let name = sym_name(fe.db, *id);
                name == expected || expected.contains(&name) || name.contains(expected)
            });
            if g_rank.is_some() { g_rr[qi] = 1.0 / (g_rank.unwrap() as f64 + 1.0); }
            if r_rank.is_some() { r_rr[qi] = 1.0 / (r_rank.unwrap() as f64 + 1.0); }
        }
    }

    let g_mrr: f64 = g_rr.iter().sum::<f64>() / n_queries as f64;
    let r_mrr: f64 = r_rr.iter().sum::<f64>() / n_queries as f64;

    let mut all_g: Vec<f64> = g_times.iter().flatten().copied().collect();
    let mut all_r: Vec<f64> = r_times.iter().flatten().copied().collect();
    all_g.sort_by(|a, b| a.partial_cmp(b).unwrap());
    all_r.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let percentile = |v: &[f64], p: f64| -> f64 {
        if v.is_empty() { return 0.0; }
        let idx = ((p / 100.0) * (v.len() - 1) as f64).round() as usize;
        v[idx.min(v.len() - 1)]
    };

    let g_med = percentile(&all_g, 50.0);
    let g_p95 = percentile(&all_g, 95.0);
    let g_p99 = percentile(&all_g, 99.0);
    let r_med = percentile(&all_r, 50.0);
    let r_p95 = percentile(&all_r, 95.0);
    let _r_p99 = percentile(&all_r, 99.0);

    println!("\n{:<12} {:>8} {:>8} {:>8}  {:>8} {:>8} {:>8}", "", "MRR", "Med(us)", "P95(us)", "MRR", "Med(us)", "P95(us)");
    println!("{:<12} {:>8} {:>8} {:>8}  {:>8} {:>8} {:>8}", "", "----", "-------", "-------", "----", "-------", "-------");
    println!("{:<12} {:>8.3} {:>8.0} {:>8.0}  {:>8.3} {:>8.0} {:>8.0}", "OVERALL",
        g_mrr, g_med, g_p95, r_mrr, r_med, r_p95);

    println!("\n--- Per Query ---\n");
    println!("{:<35} {:>6} {:>8} {:>8}  {:>6} {:>8} {:>8}", "Query", "G RR", "G med", "G p95", "R RR", "R med", "R p95");
    println!("{}", "-".repeat(90));
    for (qi, q) in queries.iter().enumerate() {
        let g_t = &mut g_times[qi];
        let r_t = &mut r_times[qi];
        g_t.sort_by(|a, b| a.partial_cmp(b).unwrap());
        r_t.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!("{:<35} {:>6.3} {:>7.0}us {:>7.0}us  {:>6.3} {:>7.0}us {:>7.0}us",
            truncate(&q.query, 35),
            g_rr[qi], percentile(g_t, 50.0), percentile(g_t, 95.0),
            r_rr[qi], percentile(r_t, 50.0), percentile(r_t, 95.0));
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: graphiq-bench <db-path> [ndcg-queries.json] [mrr-queries.json]");
        eprintln!("       graphiq-bench speed <db-path> <mrr-queries.json>");
        std::process::exit(1);
    }

    if args.get(1).map(|s| s.as_str()) == Some("speed") {
        if args.len() < 4 {
            eprintln!("usage: graphiq-bench speed <db-path> <mrr-queries.json>");
            std::process::exit(1);
        }
        let db_path = &args[2];
        let db = match GraphDb::open(Path::new(db_path)) {
            Ok(d) => d,
            Err(e) => { eprintln!("error opening database: {e}"); std::process::exit(1); }
        };
        let stats = db.stats().unwrap();
        println!("GraphIQ Speed Benchmark");
        println!("{} files, {} symbols, {} edges\n", stats.files, stats.symbols, stats.edges);

        let fts = FtsSearch::new(&db);
        let ci = match cruncher::build_cruncher_index(&db) {
            Ok(idx) => idx,
            Err(e) => { eprintln!("cruncher build failed: {e}"); std::process::exit(1); }
        };
        let hi = cruncher::build_holo_index(&db, &ci);
        eprintln!("Computing spectral index...");
        let spectral = match graphiq_core::spectral::compute_spectral(&db) {
            Ok(mut idx) => {
                let kappa = graphiq_core::spectral::compute_ricci_curvature(&idx.graph);
                idx.graph.edge_curvature = Some(kappa);
                Some(idx)
            }
            Err(e) => { eprintln!("spectral failed: {e}"); None }
        };
        let predictive = graphiq_core::spectral::compute_predictive_model(&db).ok();
        let (fp_vec, fp_id_map) = graphiq_core::spectral::compute_channel_fingerprints(&db);
        let self_model = graphiq_core::self_model::build_self_model(&db).ok();
        let cache = HotCache::with_defaults();
        cache.prewarm(&db, 200);
        let mut engine = SearchEngine::new(&db, &cache).with_goober(&ci, &hi);
        if let Some(ref spec) = spectral { engine = engine.with_spectral(spec); }
        if let Some(ref pm) = predictive { engine = engine.with_predictive(pm); }
        engine = engine.with_fingerprints(&fp_vec, &fp_id_map);
        if let Some(ref sm) = self_model { engine = engine.with_self_model(sm); }

        let fe = FullEngine {
            db: &db, fts: &fts, ci: &ci, hi: &hi,
            spectral: &spectral, predictive: &predictive,
            fingerprints: &fp_vec, fp_id_to_idx: &fp_id_map,
            cache: &cache, self_model: &self_model, engine,
        };

        let content = std::fs::read_to_string(&args[3]).unwrap_or_else(|e| {
            eprintln!("error reading query file: {e}"); std::process::exit(1);
        });
        let mut queries: Vec<BenchQuery> = serde_json::from_str(&content).unwrap_or_else(|e| {
            eprintln!("error parsing query file: {e}"); std::process::exit(1);
        });
        queries.truncate(10);

        run_speed_bench(&fe, &queries);
        return;
    }

    let db_path = &args[1];
    let db = match GraphDb::open(Path::new(db_path)) {
        Ok(d) => d,
        Err(e) => { eprintln!("error opening database: {e}"); std::process::exit(1); }
    };

    let stats = db.stats().unwrap();
    println!("GraphIQ Benchmark");
    println!("{} files, {} symbols, {} edges\n", stats.files, stats.symbols, stats.edges);

    let fts = FtsSearch::new(&db);

    let ci = match cruncher::build_cruncher_index(&db) {
        Ok(idx) => idx,
        Err(e) => { eprintln!("cruncher build failed: {e}"); std::process::exit(1); }
    };

    let hi = cruncher::build_holo_index(&db, &ci);

    eprintln!("Computing spectral index...");
    let spectral = match graphiq_core::spectral::compute_spectral(&db) {
        Ok(mut idx) => {
            eprintln!("Computing Ricci curvature...");
            let kappa = graphiq_core::spectral::compute_ricci_curvature(&idx.graph);
            idx.graph.edge_curvature = Some(kappa);
            Some(idx)
        }
        Err(e) => { eprintln!("spectral failed: {e}"); None }
    };

    let predictive = graphiq_core::spectral::compute_predictive_model(&db).ok();

    eprintln!("Computing channel fingerprints...");
    let (fp_vec, fp_id_map) = graphiq_core::spectral::compute_channel_fingerprints(&db);

    let self_model = graphiq_core::self_model::build_self_model(&db).ok();

    let cache = HotCache::with_defaults();
    cache.prewarm(&db, 200);

    let mut engine = SearchEngine::new(&db, &cache)
        .with_goober(&ci, &hi);
    if let Some(ref spec) = spectral {
        engine = engine.with_spectral(spec);
    }
    if let Some(ref pm) = predictive {
        engine = engine.with_predictive(pm);
    }
    engine = engine.with_fingerprints(&fp_vec, &fp_id_map);
    if let Some(ref sm) = self_model {
        engine = engine.with_self_model(sm);
    }

    let ndcg_file = args.get(2).filter(|s| !s.is_empty()).map(|s| s.as_str());
    let mrr_file = args.get(3).filter(|s| !s.is_empty()).map(|s| s.as_str());

    let fe = FullEngine {
        db: &db, fts: &fts, ci: &ci, hi: &hi,
        spectral: &spectral, predictive: &predictive,
        fingerprints: &fp_vec, fp_id_to_idx: &fp_id_map,
        cache: &cache, self_model: &self_model, engine,
    };

    if let Some(file) = ndcg_file {
        let content = std::fs::read_to_string(file).unwrap_or_else(|e| {
            eprintln!("error reading NDCG query file: {e}"); std::process::exit(1);
        });
        let queries: Vec<BenchQuery> = serde_json::from_str(&content).unwrap_or_else(|e| {
            eprintln!("error parsing NDCG query file: {e}"); std::process::exit(1);
        });
        run_ndcg(&fe, &queries);
        cmd_diagnose(&fe, &queries);
    }

    if let Some(file) = mrr_file {
        let content = std::fs::read_to_string(file).unwrap_or_else(|e| {
            eprintln!("error reading MRR query file: {e}"); std::process::exit(1);
        });
        let queries: Vec<BenchQuery> = serde_json::from_str(&content).unwrap_or_else(|e| {
            eprintln!("error parsing MRR query file: {e}"); std::process::exit(1);
        });
        run_mrr(&fe, &queries);
        cmd_diagnose(&fe, &queries);
    }

    if ndcg_file.is_none() && mrr_file.is_none() {
        eprintln!("no query files provided.");
    }
}
