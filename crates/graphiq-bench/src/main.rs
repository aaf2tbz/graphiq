use std::path::Path;
use std::time::Instant;

use graphiq_core::af26;
use graphiq_core::afmo;
use graphiq_core::cache::HotCache;
use graphiq_core::db::GraphDb;
use graphiq_core::index::Indexer;
use graphiq_core::lsa;
use graphiq_core::search::{SearchEngine, SearchQuery};
use graphiq_core::spectral;

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
            match lsa::store_lsa_basis(&db, &idx.term_basis, &idx.term_index, &idx.term_idf) {
                Ok(()) => {}
                Err(e) => eprintln!("  basis store failed: {e}"),
            }

            match lsa::store_lsa_sigma(&db, &idx.singular_values) {
                Ok(()) => eprintln!("  stored {} singular values", idx.singular_values.len()),
                Err(e) => eprintln!("  sigma store failed: {e}"),
            }

            println!("done ({} terms × {} symbols, dim={})", n_terms, n_syms, dim);
            Some((idx, sym_id_to_idx))
        }
        Err(e) => {
            println!("failed: {e}");
            None
        }
    };

    print!("Computing Spectral ... ");
    let spectral_index = match spectral::compute_spectral(&db) {
        Ok(idx) => {
            let n_syms = idx.symbol_ids.len();
            let dim = idx.symbol_coords.first().map(|v| v.len()).unwrap_or(0);
            match spectral::store_spectral_coords(&db, &idx.symbol_ids, &idx.symbol_coords) {
                Ok(c) => eprintln!("  stored {} spectral coords", c),
                Err(e) => eprintln!("  store failed: {e}"),
            }
            println!("done ({} symbols, dim={})", n_syms, dim);
            Some(idx)
        }
        Err(e) => {
            println!("failed: {e}");
            None
        }
    };

    print!("Computing AF26 ... ");
    let af26_index = match af26::compute_af26(&db) {
        Ok(idx) => {
            println!(
                "done ({} symbols, dim={}, {} gravity edges)",
                idx.symbol_ids.len(),
                idx.sigma.len(),
                idx.gravity.iter().map(|e| e.len()).sum::<usize>() / 2
            );
            Some(idx)
        }
        Err(e) => {
            println!("failed: {e}");
            None
        }
    };

    print!("Computing AFMO (hyperbolic) ... ");
    let afmo_index = match afmo::compute_afmo(&db) {
        Ok(idx) => {
            println!(
                "done ({} symbols, hdim={}, {} gravity edges)",
                idx.symbol_ids.len(),
                idx.hyperbolic_dim,
                idx.gravity.iter().map(|e| e.len()).sum::<usize>() / 2
            );
            Some(idx)
        }
        Err(e) => {
            println!("failed: {e}");
            None
        }
    };

    print!("Computing HRR (holographic) ... ");
    let hrr_index = match graphiq_core::hrr::compute_hrr(&db) {
        Ok(idx) => {
            println!(
                "done ({} symbols, {}D)",
                idx.symbol_ids.len(),
                idx.holograms.first().map(|h| h.len()).unwrap_or(0)
            );
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
    let cache = HotCache::with_defaults();
    let engine = if let Some(ref hrr_idx) = hrr_index {
        SearchEngine::new(&db, &cache).with_hrr(hrr_idx)
    } else {
        SearchEngine::new(&db, &cache)
    };

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

    let mut lsa_results: Vec<BenchResult> = Vec::new();
    let mut blade_results: Vec<BenchResult> = Vec::new();

    if let Some((ref lsa_idx, ref sym_map)) = lsa_index {
        println!("\n=== Pure LSA Evaluation (angular distance on hypersphere) ===\n");
        let conn = db.conn();

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

        println!("\n=== Spherical Cap Search (IDF-weighted cap union) ===\n");
        let mut cap_results: Vec<BenchResult> = Vec::new();

        for q in &queries {
            let cap_hits = lsa::spherical_cap_search(
                &q.query,
                &lsa_idx.term_index,
                &lsa_idx.term_basis,
                &lsa_idx.term_idf,
                &lsa_idx.symbol_vecs,
                &lsa_idx.symbol_ids,
                10,
            );

            let result_rels: Vec<f64> = cap_hits
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

            let ideal_rels = compute_ideal_rels(&db, q);
            let ndcg = ndcg_at_k(&result_rels, &ideal_rels, 10);

            let best_relevance = cap_hits
                .iter()
                .map(|(sym_id, _)| {
                    let name: String = conn
                        .query_row("SELECT name FROM symbols WHERE id = ?", [*sym_id], |row| {
                            row.get(0)
                        })
                        .unwrap_or_default();
                    q.relevance_of(&name)
                })
                .max()
                .unwrap_or(0);

            let best_rank = cap_hits
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

            cap_results.push(BenchResult {
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

        let cap_total = cap_results.len();
        let cap_ndcg: f64 = cap_results.iter().map(|r| r.ndcg).sum::<f64>() / cap_total as f64;
        let cap_h1 = cap_results.iter().filter(|r| r.hit_at_1).count();
        let cap_h3 = cap_results.iter().filter(|r| r.hit_at_3).count();
        let cap_h10 = cap_results.iter().filter(|r| r.hit_at_10).count();

        println!("Cap NDCG@10: {:.3}", cap_ndcg);
        println!(
            "Cap Hit@1: {}/{} ({:.0}%)",
            cap_h1,
            cap_total,
            cap_h1 as f64 / cap_total as f64 * 100.0
        );
        println!(
            "Cap Hit@3: {}/{} ({:.0}%)",
            cap_h3,
            cap_total,
            cap_h3 as f64 / cap_total as f64 * 100.0
        );
        println!(
            "Cap Hit@10: {}/{} ({:.0}%)",
            cap_h10,
            cap_total,
            cap_h10 as f64 / cap_total as f64 * 100.0
        );

        println!("\n--- Per-Query: Cap vs Centroid vs BM25 ---\n");
        println!(
            "{:<30} {:<15} {:>6} {:>6} {:>6} {:>6} {:>6}",
            "Query", "Category", "Cap", "Centr", "BM25", "C@1", "B@1"
        );
        println!("{}", "-".repeat(110));
        for ((bm25, lsa), cap) in results
            .iter()
            .zip(lsa_results.iter())
            .zip(cap_results.iter())
        {
            println!(
                "{:<30} {:<15} {:>6.3} {:>6.3} {:>6.3} {:>6} {:>6}",
                truncate(&bm25.query, 30),
                bm25.category,
                cap.ndcg,
                lsa.ndcg,
                bm25.ndcg,
                if cap.hit_at_1 { "Y" } else { "N" },
                if bm25.hit_at_1 { "Y" } else { "N" },
            );
        }

        println!("\n=== Blade Search (geometric product / outer product query) ===\n");

        for q in &queries {
            let blade_hits = lsa::blade_search(
                &q.query,
                &lsa_idx.term_index,
                &lsa_idx.term_basis,
                &lsa_idx.term_idf,
                &lsa_idx.symbol_vecs,
                &lsa_idx.symbol_ids,
                10,
            );

            let result_rels: Vec<f64> = blade_hits
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

            let ideal_rels = compute_ideal_rels(&db, q);
            let ndcg = ndcg_at_k(&result_rels, &ideal_rels, 10);

            let best_relevance = blade_hits
                .iter()
                .map(|(sym_id, _)| {
                    let name: String = conn
                        .query_row("SELECT name FROM symbols WHERE id = ?", [*sym_id], |row| {
                            row.get(0)
                        })
                        .unwrap_or_default();
                    q.relevance_of(&name)
                })
                .max()
                .unwrap_or(0);

            let best_rank = blade_hits
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

            blade_results.push(BenchResult {
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

        let bl_total = blade_results.len();
        let bl_ndcg: f64 = blade_results.iter().map(|r| r.ndcg).sum::<f64>() / bl_total as f64;
        let bl_h1 = blade_results.iter().filter(|r| r.hit_at_1).count();
        let bl_h10 = blade_results.iter().filter(|r| r.hit_at_10).count();

        println!("Blade NDCG@10: {:.3}", bl_ndcg);
        println!(
            "Blade Hit@1: {}/{} ({:.0}%)",
            bl_h1,
            bl_total,
            bl_h1 as f64 / bl_total as f64 * 100.0
        );
        println!(
            "Blade Hit@10: {}/{} ({:.0}%)",
            bl_h10,
            bl_total,
            bl_h10 as f64 / bl_total as f64 * 100.0
        );

        println!("\n--- Per-Query: Blade vs Centroid vs BM25 ---\n");
        println!(
            "{:<30} {:<15} {:>8} {:>8} {:>8} {:>6} {:>6}",
            "Query", "Category", "Blade", "Centr", "BM25", "B@1", "M@1"
        );
        println!("{}", "-".repeat(110));
        for (bm25, lsa) in results.iter().zip(lsa_results.iter()) {
            let bl = blade_results.iter().find(|r| r.query == bm25.query);
            if let Some(bl) = bl {
                println!(
                    "{:<30} {:<15} {:>8.3} {:>8.3} {:>8.3} {:>6} {:>6}",
                    truncate(&bm25.query, 30),
                    bm25.category,
                    bl.ndcg,
                    lsa.ndcg,
                    bm25.ndcg,
                    if bl.hit_at_1 { "Y" } else { "N" },
                    if bm25.hit_at_1 { "Y" } else { "N" },
                );
            }
        }
    }

    if let Some(ref sp_idx) = spectral_index {
        let conn = db.conn();
        println!("\n=== Spectral Search (Laplacian harmonic modes) ===\n");
        let mut sp_results: Vec<BenchResult> = Vec::new();

        for q in &queries {
            let hits = spectral::spectral_search(&q.query, sp_idx, &db, 10);

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

            let ideal_rels = compute_ideal_rels(&db, q);
            let ndcg = ndcg_at_k(&result_rels, &ideal_rels, 10);

            let best_relevance = hits
                .iter()
                .map(|(sym_id, _)| {
                    let name: String = conn
                        .query_row("SELECT name FROM symbols WHERE id = ?", [*sym_id], |row| {
                            row.get(0)
                        })
                        .unwrap_or_default();
                    q.relevance_of(&name)
                })
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

            sp_results.push(BenchResult {
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

        let sp_total = sp_results.len();
        let sp_ndcg: f64 = sp_results.iter().map(|r| r.ndcg).sum::<f64>() / sp_total as f64;
        let sp_h1 = sp_results.iter().filter(|r| r.hit_at_1).count();
        let sp_h3 = sp_results.iter().filter(|r| r.hit_at_3).count();
        let sp_h10 = sp_results.iter().filter(|r| r.hit_at_10).count();

        println!("Spectral NDCG@10: {:.3}", sp_ndcg);
        println!(
            "Spectral Hit@1: {}/{} ({:.0}%)",
            sp_h1,
            sp_total,
            sp_h1 as f64 / sp_total as f64 * 100.0
        );
        println!(
            "Spectral Hit@3: {}/{} ({:.0}%)",
            sp_h3,
            sp_total,
            sp_h3 as f64 / sp_total as f64 * 100.0
        );
        println!(
            "Spectral Hit@10: {}/{} ({:.0}%)",
            sp_h10,
            sp_total,
            sp_h10 as f64 / sp_total as f64 * 100.0
        );

        println!("\n--- Per-Query: Spectral vs LSA vs BM25 ---\n");
        println!(
            "{:<30} {:<15} {:>8} {:>8} {:>8} {:>6} {:>6}",
            "Query", "Category", "Spectr", "LSA", "BM25", "S@1", "B@1"
        );
        println!("{}", "-".repeat(110));
        for (bm25, lsa) in results.iter().zip(lsa_results.iter()) {
            let sp = sp_results.iter().find(|r| r.query == bm25.query);
            if let Some(sp) = sp {
                println!(
                    "{:<30} {:<15} {:>8.3} {:>8.3} {:>8.3} {:>6} {:>6}",
                    truncate(&bm25.query, 30),
                    bm25.category,
                    sp.ndcg,
                    lsa.ndcg,
                    bm25.ndcg,
                    if sp.hit_at_1 { "Y" } else { "N" },
                    if bm25.hit_at_1 { "Y" } else { "N" },
                );
            }
        }
    }

    if let Some(ref af26_idx) = af26_index {
        let conn = db.conn();

        let eval_hits = |hits: &[(i64, f64)], q: &BenchQuery| -> BenchResult {
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
            let ideal_rels = compute_ideal_rels(&db, q);
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
                latency_us: 0,
                warm_latency_us: 0,
            }
        };

        let print_summary = |label: &str, res: &[BenchResult]| {
            let total = res.len();
            let ndcg: f64 = res.iter().map(|r| r.ndcg).sum::<f64>() / total as f64;
            let h1 = res.iter().filter(|r| r.hit_at_1).count();
            let h3 = res.iter().filter(|r| r.hit_at_3).count();
            let h10 = res.iter().filter(|r| r.hit_at_10).count();
            println!("{} NDCG@10: {:.3}", label, ndcg);
            println!(
                "{} Hit@1: {}/{} ({:.0}%)",
                label,
                h1,
                total,
                h1 as f64 / total as f64 * 100.0
            );
            println!(
                "{} Hit@3: {}/{} ({:.0}%)",
                label,
                h3,
                total,
                h3 as f64 / total as f64 * 100.0
            );
            println!(
                "{} Hit@10: {}/{} ({:.0}%)",
                label,
                h10,
                total,
                h10 as f64 / total as f64 * 100.0
            );
        };

        // --- AF26 Pure Semantic (centroid + geometric mean) ---
        println!("\n=== AF26 Pure (centroid + geometric mean) ===\n");
        let af26_pure: Vec<BenchResult> = queries
            .iter()
            .map(|q| eval_hits(&af26::af26_search(&q.query, af26_idx, 10), q))
            .collect();
        print_summary("AF26-Pure", &af26_pure);

        // --- AF26 Pipeline Boost (geometric reranker on BM25 candidates) ---
        println!("\n=== AF26 Pipeline (geometric boost on BM25+expanded) ===\n");
        let fts = graphiq_core::fts::FtsSearch::new(&db);
        let expander = graphiq_core::graph::StructuralExpander::new(&db);
        let af26_pipe: Vec<BenchResult> = queries
            .iter()
            .map(|q| {
                let fts_results = fts.search(&q.query, Some(200));
                let fts_ids: Vec<i64> = fts_results.iter().map(|r| r.symbol.id).collect();
                let expanded = expander.expand(&fts_results, 20, 2);
                let exp_ids: Vec<i64> = expanded.iter().map(|e| e.symbol.id).collect();
                let hits = af26::af26_pipeline_boost(&q.query, &fts_ids, &exp_ids, af26_idx, 10);
                eval_hits(&hits, q)
            })
            .collect();
        print_summary("AF26-Pipe", &af26_pipe);

        // --- AF26 Manifold (curved-space semantic search) ---
        println!("\n=== AF26 Manifold (graph-curved semantic space) ===\n");
        let af26_manifold: Vec<BenchResult> = queries
            .iter()
            .map(|q| eval_hits(&af26::af26_manifold_search(&q.query, af26_idx, 10), q))
            .collect();
        print_summary("AF26-Manifold", &af26_manifold);

        // --- AF26 Combined Boost (multiplicative on top of BM25 pipeline) ---
        println!("\n=== AF26 Combined Boost (multiplicative on BM25 pipeline) ===\n");
        let af26_combined: Vec<BenchResult> = queries
            .iter()
            .enumerate()
            .map(|(qi, q)| {
                let pipeline_result = engine.search(&SearchQuery::new(&q.query).top_k(50));
                let cids: Vec<i64> = pipeline_result
                    .results
                    .iter()
                    .map(|r| r.symbol.id)
                    .collect();
                let cscores: Vec<f64> = pipeline_result.results.iter().map(|r| r.score).collect();
                let boosted = af26::af26_combined_boost(&q.query, &cids, &cscores, af26_idx);
                let top10: Vec<(i64, f64)> = boosted.into_iter().take(10).collect();
                eval_hits(&top10, q)
            })
            .collect();
        print_summary("AF26-Combined", &af26_combined);

        // --- AF27 Hybrid (discrete cell + continuous local geometry on BM25 pipeline) ---
        println!("\n=== AF27 Hybrid (discrete + continuous on BM25 pipeline) ===\n");
        let af27_hybrid: Vec<BenchResult> = queries
            .iter()
            .enumerate()
            .map(|(qi, q)| {
                let pipeline_result = engine.search(&SearchQuery::new(&q.query).top_k(50));
                let cids: Vec<i64> = pipeline_result
                    .results
                    .iter()
                    .map(|r| r.symbol.id)
                    .collect();
                let cscores: Vec<f64> = pipeline_result.results.iter().map(|r| r.score).collect();
                let boosted = af26::af27_hybrid_search(&q.query, &cids, &cscores, af26_idx);
                let top10: Vec<(i64, f64)> = boosted.into_iter().take(10).collect();
                eval_hits(&top10, q)
            })
            .collect();
        print_summary("AF27-Hybrid", &af27_hybrid);

        // --- Comparison table ---
        println!("\n--- Per-Query: AF27-Hybrid vs AF26-Combined vs BM25 ---\n");
        println!(
            "{:<30} {:<15} {:>8} {:>8} {:>8}",
            "Query", "Category", "AF27", "Comb", "BM25"
        );
        println!("{}", "-".repeat(100));
        for (i, bm25) in results.iter().enumerate() {
            let af27 = af27_hybrid.get(i);
            let comb = af26_combined.get(i);
            println!(
                "{:<30} {:<15} {:>8.3} {:>8.3} {:>8.3}",
                truncate(&bm25.query, 30),
                bm25.category,
                af27.map(|r| r.ndcg).unwrap_or(0.0),
                comb.map(|r| r.ndcg).unwrap_or(0.0),
                bm25.ndcg,
            );
        }
    }

    if let Some(ref afmo_idx) = afmo_index {
        let conn = db.conn();

        let eval_hits_afmo = |hits: &[(i64, f64)], q: &BenchQuery| -> BenchResult {
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
            let ideal_rels = compute_ideal_rels(&db, q);
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
                latency_us: 0,
                warm_latency_us: 0,
            }
        };

        let print_summary_afmo = |label: &str, res: &[BenchResult]| {
            let total = res.len();
            let ndcg: f64 = res.iter().map(|r| r.ndcg).sum::<f64>() / total as f64;
            let h1 = res.iter().filter(|r| r.hit_at_1).count();
            let h3 = res.iter().filter(|r| r.hit_at_3).count();
            let h10 = res.iter().filter(|r| r.hit_at_10).count();
            println!("{} NDCG@10: {:.3}", label, ndcg);
            println!(
                "{} Hit@1: {}/{} ({:.0}%)",
                label,
                h1,
                total,
                h1 as f64 / total as f64 * 100.0
            );
            println!(
                "{} Hit@3: {}/{} ({:.0}%)",
                label,
                h3,
                total,
                h3 as f64 / total as f64 * 100.0
            );
            println!(
                "{} Hit@10: {}/{} ({:.0}%)",
                label,
                h10,
                total,
                h10 as f64 / total as f64 * 100.0
            );
        };

        // --- AFMO Pure (hyperbolic distance only) ---
        println!("\n=== AFMO Pure (Poincaré distance) ===\n");
        let afmo_pure: Vec<BenchResult> = queries
            .iter()
            .map(|q| eval_hits_afmo(&afmo::afmo_search(&q.query, afmo_idx, 10), q))
            .collect();
        print_summary_afmo("AFMO-Pure", &afmo_pure);

        // --- AFMO Rerank (hyperbolic gate on BM25 pipeline) ---
        println!("\n=== AFMO Rerank (hyperbolic gate on BM25 pipeline) ===\n");
        let afmo_rerank: Vec<BenchResult> = queries
            .iter()
            .map(|q| {
                let pipeline_result = engine.search(&SearchQuery::new(&q.query).top_k(50));
                let cids: Vec<i64> = pipeline_result
                    .results
                    .iter()
                    .map(|r| r.symbol.id)
                    .collect();
                let cscores: Vec<f64> = pipeline_result.results.iter().map(|r| r.score).collect();
                let reranked = afmo::afmo_rerank(&q.query, &cids, &cscores, afmo_idx);
                let top10: Vec<(i64, f64)> = reranked.into_iter().take(10).collect();
                eval_hits_afmo(&top10, q)
            })
            .collect();
        print_summary_afmo("AFMO-Rerank", &afmo_rerank);

        // --- Comparison: AFMO vs BM25 ---
        println!("\n--- Per-Query: AFMO-Rerank vs AFMO-Pure vs BM25 ---\n");
        println!(
            "{:<30} {:<15} {:>8} {:>8} {:>8}",
            "Query", "Category", "AFMO-R", "AFMO-P", "BM25"
        );
        println!("{}", "-".repeat(100));
        for (i, bm25) in results.iter().enumerate() {
            let ar = afmo_rerank.get(i);
            let ap = afmo_pure.get(i);
            println!(
                "{:<30} {:<15} {:>8.3} {:>8.3} {:>8.3}",
                truncate(&bm25.query, 30),
                bm25.category,
                ar.map(|r| r.ndcg).unwrap_or(0.0),
                ap.map(|r| r.ndcg).unwrap_or(0.0),
                bm25.ndcg,
            );
        }
    }

    if let Some(hrr_idx) = &hrr_index {
        use graphiq_core::hrr;

        let conn = db.conn();

        let eval_hits_hrr = |hits: &[(i64, f64)], q: &BenchQuery| -> BenchResult {
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
            let ideal_rels = compute_ideal_rels(&db, q);
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
                latency_us: 0,
                warm_latency_us: 0,
            }
        };

        let print_summary_hrr = |label: &str, res: &[BenchResult]| {
            let total = res.len();
            let ndcg: f64 = res.iter().map(|r| r.ndcg).sum::<f64>() / total as f64;
            let h1 = res.iter().filter(|r| r.hit_at_1).count();
            let h3 = res.iter().filter(|r| r.hit_at_3).count();
            let h10 = res.iter().filter(|r| r.hit_at_10).count();
            println!("{} NDCG@10: {:.3}", label, ndcg);
            println!(
                "{} Hit@1: {}/{} ({:.0}%)",
                label,
                h1,
                total,
                h1 as f64 / total as f64 * 100.0
            );
            println!(
                "{} Hit@3: {}/{} ({:.0}%)",
                label,
                h3,
                total,
                h3 as f64 / total as f64 * 100.0
            );
            println!(
                "{} Hit@10: {}/{} ({:.0}%)",
                label,
                h10,
                total,
                h10 as f64 / total as f64 * 100.0
            );
        };

        println!("\n=== HRR Pure (holographic) ===\n");
        let hrr_pure: Vec<BenchResult> = queries
            .iter()
            .map(|q| eval_hits_hrr(&hrr::hrr_search(&q.query, hrr_idx, 10), q))
            .collect();
        print_summary_hrr("HRR-Pure", &hrr_pure);

        println!("\n=== HRR Rerank (holographic on BM25 pipeline) ===\n");
        let hrr_rerank: Vec<BenchResult> = queries
            .iter()
            .map(|q| {
                let pipeline_result = engine.search(&SearchQuery::new(&q.query).top_k(50));
                let cids: Vec<i64> = pipeline_result
                    .results
                    .iter()
                    .map(|r| r.symbol.id)
                    .collect();
                let cscores: Vec<f64> = pipeline_result.results.iter().map(|r| r.score).collect();
                let reranked = hrr::hrr_rerank(&q.query, &cids, &cscores, hrr_idx);
                let top10: Vec<(i64, f64)> = reranked.into_iter().take(10).collect();
                eval_hits_hrr(&top10, q)
            })
            .collect();
        print_summary_hrr("HRR-Rerank", &hrr_rerank);

        if let Some(afmo_idx) = &afmo_index {
            println!("\n=== HRR+AFMO Combined Rerank ===\n");
            let combined: Vec<BenchResult> = queries
                .iter()
                .map(|q| {
                    let pipeline_result = engine.search(&SearchQuery::new(&q.query).top_k(50));
                    let cids: Vec<i64> = pipeline_result
                        .results
                        .iter()
                        .map(|r| r.symbol.id)
                        .collect();
                    let cscores: Vec<f64> =
                        pipeline_result.results.iter().map(|r| r.score).collect();
                    let hrr_done = hrr::hrr_rerank(&q.query, &cids, &cscores, hrr_idx);
                    let afmo_ids: Vec<i64> = hrr_done.iter().map(|(id, _)| *id).collect();
                    let afmo_scores: Vec<f64> = hrr_done.iter().map(|(_, s)| *s).collect();
                    let reranked = afmo::afmo_rerank(&q.query, &afmo_ids, &afmo_scores, afmo_idx);
                    let top10: Vec<(i64, f64)> = reranked.into_iter().take(10).collect();
                    eval_hits_hrr(&top10, q)
                })
                .collect();
            print_summary_hrr("HRR+AFMO", &combined);
        }

        println!("\n=== HRR Antivector Rerank (beta sweep) ===\n");
        for beta in [0.02, 0.05, 0.08, 0.12, 0.18, 0.25].iter() {
            let anti_res: Vec<BenchResult> = queries
                .iter()
                .map(|q| {
                    let pipeline_result = engine.search(&SearchQuery::new(&q.query).top_k(50));
                    let cids: Vec<i64> = pipeline_result
                        .results
                        .iter()
                        .map(|r| r.symbol.id)
                        .collect();
                    let cscores: Vec<f64> =
                        pipeline_result.results.iter().map(|r| r.score).collect();
                    let reranked =
                        hrr::hrr_antivector_rerank(&q.query, &cids, &cscores, hrr_idx, *beta);
                    let top10: Vec<(i64, f64)> = reranked.into_iter().take(10).collect();
                    eval_hits_hrr(&top10, q)
                })
                .collect();
            let ndcg: f64 = anti_res.iter().map(|r| r.ndcg).sum::<f64>() / anti_res.len() as f64;
            let h1 = anti_res.iter().filter(|r| r.hit_at_1).count();
            println!(
                "AntiR beta={:.2}  NDCG={:.3}  H@1={}/{}",
                beta,
                ndcg,
                h1,
                anti_res.len()
            );
        }

        println!("\n=== HRR Bivector Dual (seeds=5, weight sweep) ===\n");
        let mut best_biv_agg = 0.0f64;
        let mut best_biv_w = 0.0f64;
        let mut best_biv_results: Vec<BenchResult> = Vec::new();
        for biv_w in [0.3, 0.4, 0.5, 0.6, 0.8, 1.0, 1.2].iter() {
            let biv_res: Vec<BenchResult> = queries
                .iter()
                .map(|q| {
                    let pipeline_result = engine.search(&SearchQuery::new(&q.query).top_k(50));
                    let bm25_ids: Vec<i64> = pipeline_result
                        .results
                        .iter()
                        .take(5)
                        .map(|r| r.symbol.id)
                        .collect();
                    let bm25_scores: Vec<f64> =
                        pipeline_result.results.iter().map(|r| r.score).collect();

                    let (expanded, _) = hrr::hrr_bivector_expand_scored(&bm25_ids, hrr_idx, 50);

                    let mut rrf: std::collections::HashMap<i64, f64> =
                        std::collections::HashMap::new();
                    let k = 60.0;
                    for (rank, &id) in bm25_ids.iter().enumerate() {
                        *rrf.entry(id).or_insert(0.0) += 1.0 / (k + rank as f64 + 1.0);
                    }
                    for (rank, (id, _)) in expanded.iter().enumerate() {
                        *rrf.entry(*id).or_insert(0.0) += *biv_w / (k + rank as f64 + 1.0);
                    }

                    let mut merged: Vec<(i64, f64)> = rrf.into_iter().collect();
                    merged.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
                    let merged_50: Vec<i64> = merged.iter().take(50).map(|(id, _)| *id).collect();
                    let merged_scores: Vec<f64> = merged.iter().take(50).map(|(_, s)| *s).collect();

                    let reranked = hrr::hrr_rerank(&q.query, &merged_50, &merged_scores, hrr_idx);
                    let top10: Vec<(i64, f64)> = reranked.into_iter().take(10).collect();

                    eval_hits_hrr(&top10, q)
                })
                .collect();
            let ndcg: f64 = biv_res.iter().map(|r| r.ndcg).sum::<f64>() / biv_res.len() as f64;
            let h1 = biv_res.iter().filter(|r| r.hit_at_1).count();
            if ndcg > best_biv_agg {
                best_biv_agg = ndcg;
                best_biv_w = *biv_w;
                best_biv_results = biv_res;
            }
            println!(
                "Biv5 w={:.1}  NDCG={:.3}  H@1={}/{}",
                biv_w,
                ndcg,
                h1,
                queries.len()
            );
        }
        let hrr_bivec_dual = best_biv_results;
        println!("\nBest Biv5: w={:.1} NDCG={:.3}", best_biv_w, best_biv_agg);

        println!("\n--- Per-Query: HRR-Rerank vs HRR-Pure vs BivAdapt vs BM25 ---\n");
        println!(
            "{:<30} {:<15} {:>8} {:>8} {:>8} {:>8}",
            "Query", "Category", "HRR-R", "HRR-P", "BivA", "BM25"
        );
        println!("{}", "-".repeat(100));
        for (i, bm25) in results.iter().enumerate() {
            let hr = hrr_rerank.get(i);
            let hp = hrr_pure.get(i);
            let ba = hrr_bivec_dual.get(i);
            println!(
                "{:<30} {:<15} {:>8.3} {:>8.3} {:>8.3} {:>8.3}",
                truncate(&bm25.query, 30),
                bm25.category,
                hr.map(|r| r.ndcg).unwrap_or(0.0),
                hp.map(|r| r.ndcg).unwrap_or(0.0),
                ba.map(|r| r.ndcg).unwrap_or(0.0),
                bm25.ndcg,
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
