use std::collections::{HashMap, HashSet};
use std::path::Path;

use graphiq_core::cruncher;
use graphiq_core::db::GraphDb;
use graphiq_core::fts::FtsSearch;
use graphiq_core::spectral::{ChannelFingerprint, PredictiveModel, SpectralIndex};
use graphiq_core::search::{SearchEngine, SearchQuery, SearchMode};
use graphiq_core::cache::HotCache;
use graphiq_core::query_family::QueryFamily;
use graphiq_core::self_model::RepoSelfModel;

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

fn bootstrap_ci(data: &[f64], n_bootstrap: usize, seed: u64) -> (f64, f64) {
    if data.len() < 2 {
        let mean = if data.is_empty() { 0.0 } else { data[0] };
        return (mean, mean);
    }
    let n = data.len();
    let mut rng = SimpleRng::new(seed);
    let mut bootstrap_means: Vec<f64> = Vec::with_capacity(n_bootstrap);
    for _ in 0..n_bootstrap {
        let mut sum = 0.0f64;
        for _ in 0..n {
            let idx = rng.next() as usize % n;
            sum += data[idx];
        }
        bootstrap_means.push(sum / n as f64);
    }
    bootstrap_means.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let lo = bootstrap_means[n_bootstrap * 5 / 100];
    let hi = bootstrap_means[n_bootstrap * 95 / 100];
    (lo, hi)
}

struct SimpleRng {
    state: u64,
}

impl SimpleRng {
    fn new(seed: u64) -> Self {
        Self { state: if seed == 0 { 1 } else { seed } }
    }
    fn next(&mut self) -> u64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        self.state
    }
}

#[derive(Debug, Clone)]
enum FailureKind {
    NoCandidate,
    UnderRanked { best_rank: usize },
    WrongSymbol,
    FamilyMisclassified { actual: QueryFamily },
}

impl std::fmt::Display for FailureKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FailureKind::NoCandidate => write!(f, "no_candidate"),
            FailureKind::UnderRanked { best_rank } => write!(f, "under_ranked(best={})", best_rank),
            FailureKind::WrongSymbol => write!(f, "wrong_symbol"),
            FailureKind::FamilyMisclassified { actual } => write!(f, "family_misclassified({:?})", actual),
        }
    }
}

const N_METHODS: usize = 12;
const METHOD_NAMES: [&str; N_METHODS] = ["BM25", "CRv1", "CRv2", "Goober", "GooV3", "GooV4", "GooV5", "Geometric", "Curved", "Deformed", "Routed", "CARE"];

fn care_fuse(
    bm25: &[(i64, f64)],
    goov5: &[(i64, f64)],
    routed: &[(i64, f64)],
    top_k: usize,
) -> Vec<(i64, f64)> {
    let goov5_map: HashMap<i64, f64> = goov5.iter().map(|(id, s)| (*id, *s)).collect();
    let routed_map: HashMap<i64, f64> = routed.iter().map(|(id, s)| (*id, *s)).collect();

    let g_max = goov5.iter().map(|(_, s)| *s).fold(0.0f64, f64::max).max(1e-10);
    let r_max = routed.iter().map(|(_, s)| *s).fold(0.0f64, f64::max).max(1e-10);

    let mut scored: HashMap<i64, f64> = HashMap::new();

    for &(id, g_score) in goov5 {
        let g_norm = g_score / g_max;
        if let Some(&r_score) = routed_map.get(&id) {
            let r_norm = r_score / r_max;
            scored.insert(id, 0.6 * g_norm + 0.4 * r_norm + 0.10);
        } else {
            scored.insert(id, 0.7 * g_norm);
        }
    }

    for &(id, r_score) in routed {
        if !scored.contains_key(&id) {
            let r_norm = r_score / r_max;
            let rank = routed.iter().position(|(sid, _)| *sid == id).unwrap();
            let rank_bonus = if rank == 0 { 0.15 } else if rank < 3 { 0.08 } else { 0.0 };
            scored.insert(id, 0.45 * r_norm + rank_bonus);
        }
    }

    let mut fused: Vec<(i64, f64)> = scored.into_iter().collect();
    fused.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    if bm25.len() >= 2 && bm25[0].1 / bm25[1].1.max(1e-10) > 1.2 {
        let anchor_id = bm25[0].0;
        if !goov5.is_empty() && goov5[0].0 == anchor_id {
            if let Some(pos) = fused.iter().position(|(id, _)| *id == anchor_id) {
                if pos != 0 {
                    let entry = fused.remove(pos);
                    fused.insert(0, entry);
                }
            }
        }
    }

    fused.truncate(top_k);
    fused
}

fn run_searches(
    db: &GraphDb,
    fts: &FtsSearch,
    ci: &cruncher::CruncherIndex,
    hi: &cruncher::HoloIndex,
    spectral: &Option<SpectralIndex>,
    predictive: &Option<PredictiveModel>,
    fingerprints: &Option<Vec<ChannelFingerprint>>,
    fp_id_to_idx: &Option<std::collections::HashMap<i64, usize>>,
    cache: &HotCache,
    self_model: &Option<RepoSelfModel>,
    query: &str,
    top_k: usize,
) -> [Vec<(i64, f64)>; N_METHODS] {
    let bm25: Vec<(i64, f64)> = fts
        .search(query, Some(top_k))
        .into_iter()
        .map(|r| (r.symbol.id, r.bm25_score))
        .collect();

    let cr_v1 = cruncher::cruncher_search(query, ci, &bm25, top_k);
    let cr_v2 = cruncher::cruncher_v2_search(query, ci, &bm25, top_k);
    let goober = cruncher::goober_search(query, ci, &bm25, top_k);
    let goober_v3 = cruncher::goober_v3_search(query, ci, &bm25, top_k);
    let goober_v4 = cruncher::goober_v4_search(query, ci, &bm25, top_k);
    let goober_v5 = cruncher::goober_v5_search(query, ci, hi, &bm25, top_k);

    let geometric = if let Some(spec) = spectral {
        cruncher::geometric_search(query, ci, hi, &bm25, spec, top_k, 1.0, 15, 5.0, 50, false, None, None, None, 1.0)
    } else {
        Vec::new()
    };

    let curved = if let Some(spec) = spectral {
        cruncher::geometric_search(query, ci, hi, &bm25, spec, top_k, 1.0, 15, 5.0, 50, true, None, None, None, 1.0)
    } else {
        Vec::new()
    };

    let deformed = if let Some(spec) = spectral {
        cruncher::geometric_search(query, ci, hi, &bm25, spec, top_k, 1.0, 15, 5.0, 50, false, predictive.as_ref(), fingerprints.as_deref(), fp_id_to_idx.as_ref(), 1.0)
    } else {
        Vec::new()
    };

    let routed = {
        let mut engine = SearchEngine::new(db, cache);
        engine = engine.with_goober(ci, hi);
        if let Some(ref spec) = spectral {
            engine = engine.with_spectral(spec);
        }
        if let Some(ref pm) = predictive {
            engine = engine.with_predictive(pm);
        }
        if let (Some(fps), Some(fp_map)) = (fingerprints.as_deref(), fp_id_to_idx.as_ref()) {
            engine = engine.with_fingerprints(fps, fp_map);
        }
        if let Some(ref sm) = self_model {
            engine = engine.with_self_model(sm);
        }
        let q = SearchQuery::new(query).top_k(top_k);
        let result = engine.search(&q);
        let routed_results: Vec<(i64, f64)> = result.results.iter().map(|r| (r.symbol.id, r.score)).collect();
        routed_results
    };

    let care = care_fuse(&bm25, &goober_v5, &routed, top_k);

    let out: [Vec<(i64, f64)>; N_METHODS] = [
        bm25, cr_v1, cr_v2, goober, goober_v3, goober_v4, goober_v5,
        geometric, curved, deformed, routed, care,
    ];
    out
}

fn run_ndcg_benchmark(
    db: &GraphDb,
    fts: &FtsSearch,
    ci: &cruncher::CruncherIndex,
    hi: &cruncher::HoloIndex,
    spectral: &Option<SpectralIndex>,
    predictive: &Option<PredictiveModel>,
    fingerprints: &Option<Vec<ChannelFingerprint>>,
    fp_id_to_idx: &Option<std::collections::HashMap<i64, usize>>,
    cache: &HotCache,
    self_model: &Option<RepoSelfModel>,
    queries: &[BenchQuery],
) {
    println!("\n{}", "=".repeat(76));
    println!("  NDCG@10 BENCHMARK  ({} queries)", queries.len());
    println!("{}", "=".repeat(76));

    let n = queries.len();
    let mut all_ndcg: [Vec<f64>; N_METHODS] = Default::default();
    let mut all_hits: [Vec<[bool; 5]>; N_METHODS] = Default::default();
    let mut cat_data: std::collections::HashMap<String, [Vec<f64>; N_METHODS]> =
        std::collections::HashMap::new();

    for q in queries {
        let ideal = compute_ideal_rels(db, q);
        let results = run_searches(db, fts, ci, hi, spectral, predictive, fingerprints, fp_id_to_idx, cache, self_model, &q.query, 10);

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
        "{:<12} {:>8} {:>18} {:>6} {:>6} {:>6} {:>6}",
        "Method", "NDCG@10", "95% CI", "H@1", "H@3", "H@5", "H@10"
    );
    println!("{}", "-".repeat(74));
    for (mi, name) in METHOD_NAMES.iter().enumerate() {
        let avg: f64 = all_ndcg[mi].iter().sum::<f64>() / n as f64;
        let (ci_lo, ci_hi) = bootstrap_ci(&all_ndcg[mi], 1000, 42 + mi as u64);
        let h1 = all_hits[mi].iter().filter(|h| h[0]).count();
        let h3 = all_hits[mi].iter().filter(|h| h[1]).count();
        let h5 = all_hits[mi].iter().filter(|h| h[2]).count();
        let h10 = all_hits[mi].iter().filter(|h| h[3]).count();
        println!(
            "{:<12} {:>8.3} [{:.3}, {:.3}] {:>6} {:>6} {:>6} {:>6}",
            name, avg, ci_lo, ci_hi, h1, h3, h5, h10
        );
    }

    println!("\n--- Per-Family Regression Gate ---\n");
    let mut cats: Vec<&String> = cat_data.keys().collect();
    cats.sort();
    let mut regressions: Vec<(String, usize, String, f64, f64)> = Vec::new();
    for cat in &cats {
        let d = &cat_data[*cat];
        let ndcg_vals: Vec<f64> = d.iter().map(|v| v.iter().sum::<f64>() / v.len() as f64).collect();
        let best_idx = ndcg_vals.iter().enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
            .map(|(i, _)| i)
            .unwrap_or(0);
        let routed_idx = 10;
        let routed_ndcg = ndcg_vals[routed_idx];
        let deformed_ndcg = ndcg_vals[9];
        let best_ndcg = ndcg_vals[best_idx];
        println!(
            "{:<20} best={:.3}({}) deformed={:.3} routed={:.3} gap={:+.3}",
            cat,
            best_ndcg,
            METHOD_NAMES[best_idx],
            deformed_ndcg,
            routed_ndcg,
            routed_ndcg - deformed_ndcg,
        );
        if routed_ndcg < deformed_ndcg - 0.05 {
            regressions.push(((*cat).clone(), d[routed_idx].len(), "Routed vs Deformed".into(), deformed_ndcg, routed_ndcg));
        }
    }
    if !regressions.is_empty() {
        println!("\n  REGRESSION GATES TRIGGERED:");
        for (cat, n, desc, baseline, actual) in &regressions {
            println!("    {} (n={}): {} dropped {:.3} -> {:.3} ({:.3})", cat, n, desc, baseline, actual, actual - baseline);
        }
    } else {
        println!("\n  No regression gates triggered.");
    }

    println!("\n--- By Category ---\n");
    let header = format!(
        "{:<20} {}",
        "Category",
        METHOD_NAMES.iter().map(|n| format!("{:>8}", n)).collect::<Vec<_>>().join("")
    );
    println!("{}", header);
    println!("{}", "-".repeat(header.len()));
    for cat in &cats {
        let d = &cat_data[*cat];
        let avg: Vec<f64> = d.iter().map(|v| v.iter().sum::<f64>() / v.len() as f64).collect();
        let row: Vec<String> = avg.iter().map(|v| format!("{:>8.3}", v)).collect();
        println!("{:<20} {}", cat, row.join(""));
    }

    println!("\n--- Per-Query ---\n");
    let q_header = format!(
        "{:<30} {}",
        "Query",
        METHOD_NAMES.iter().map(|n| format!("{:>7}", n)).collect::<Vec<_>>().join("")
    );
    println!("{}", q_header);
    println!("{}", "-".repeat(q_header.len()));
    for (i, q) in queries.iter().enumerate() {
        let vals: Vec<String> = (0..N_METHODS)
            .map(|mi| format!("{:>7.3}", all_ndcg[mi][i]))
            .collect();
        println!("{:<30} {}", truncate(&q.query, 30), vals.join(""));
    }
}

fn run_mrr_benchmark(
    db: &GraphDb,
    fts: &FtsSearch,
    ci: &cruncher::CruncherIndex,
    hi: &cruncher::HoloIndex,
    spectral: &Option<SpectralIndex>,
    predictive: &Option<PredictiveModel>,
    fingerprints: &Option<Vec<ChannelFingerprint>>,
    fp_id_to_idx: &Option<std::collections::HashMap<i64, usize>>,
    cache: &HotCache,
    self_model: &Option<RepoSelfModel>,
    queries: &[BenchQuery],
) {
    println!("\n{}", "=".repeat(76));
    println!("  MRR BENCHMARK  ({} queries)", queries.len());
    println!("{}", "=".repeat(76));

    let n = queries.len();

    struct MrrResult {
        rr: f64,
        hit_at: [bool; 10],
        precision_at_10: f64,
        recall_at_10: f64,
        found_rank: Option<usize>,
    }

    let mut all: [Vec<MrrResult>; N_METHODS] = Default::default();
    let mut failure_clusters: std::collections::HashMap<String, Vec<(String, String)>> = std::collections::HashMap::new();
    let cluster_names = ["no_candidate", "under_ranked", "wrong_symbol", "family_misclassified"];

    for q in queries {
        let results = run_searches(db, fts, ci, hi, spectral, predictive, fingerprints, fp_id_to_idx, cache, self_model, &q.query, 10);

        let _classified_family = graphiq_core::query_family::classify_query_family(&q.query);
        let expected = q.expected_symbol.as_deref().unwrap_or("");

        let total_relevant = {
            let mut count = 0usize;
            if !q.relevance.is_empty() {
                for (name, grade) in &q.relevance {
                    let c: i64 = db.conn()
                        .query_row("SELECT COUNT(*) FROM symbols WHERE name = ?", [name], |row| row.get(0))
                        .unwrap_or(0);
                    if *grade >= 2 { count += c as usize; }
                }
            } else {
                let c: i64 = db.conn()
                    .query_row("SELECT COUNT(*) FROM symbols WHERE name = ?", [expected], |row| row.get(0))
                    .unwrap_or(0);
                count = c.max(1) as usize;
            }
            count.max(1)
        };

        for (mi, hits) in results.iter().enumerate() {
            let found_rank = if !q.relevance.is_empty() {
                hits.iter().position(|(id, _)| q.relevance_of(&sym_name(db, *id)) >= 2)
            } else {
                hits.iter().position(|(id, _)| {
                    let name = sym_name(db, *id);
                    name == expected || expected.contains(&name) || name.contains(expected)
                })
            };

            let rr = found_rank.map(|r| 1.0 / (r + 1) as f64).unwrap_or(0.0);

            let mut hit_at = [false; 10];
            for k in 0..10 {
                hit_at[k] = found_rank.map_or(false, |r| r < (k + 1));
            }

            let relevant_in_top10: usize = hits.iter().take(10)
                .filter(|(id, _)| {
                    if !q.relevance.is_empty() {
                        q.relevance_of(&sym_name(db, *id)) >= 2
                    } else {
                        let name = sym_name(db, *id);
                        name == expected || expected.contains(&name) || name.contains(expected)
                    }
                })
                .count();

            let precision_at_10 = relevant_in_top10 as f64 / 10.0;
            let recall_at_10 = (relevant_in_top10 as f64 / total_relevant as f64).min(1.0);

            all[mi].push(MrrResult {
                rr,
                hit_at,
                precision_at_10,
                recall_at_10,
                found_rank,
            });

            if mi == 9 && found_rank.is_none() {
                let has_any = !hits.is_empty();
                if !has_any {
                    failure_clusters.entry("no_candidate".into()).or_default()
                        .push((q.query.clone(), q.category.clone()));
                } else {
                    let best_file_match = hits.iter().any(|(id, _)| {
                        if let Ok(Some(sym)) = db.get_symbol(*id) {
                            if let Some(ref exp) = q.expected_symbol {
                                let exp_syms = db.symbols_by_name(exp).unwrap_or_default();
                                exp_syms.iter().any(|s| s.file_id == sym.file_id)
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    });
                    if best_file_match {
                        failure_clusters.entry("wrong_symbol".into()).or_default()
                            .push((q.query.clone(), q.category.clone()));
                    } else {
                        failure_clusters.entry("under_ranked".into()).or_default()
                            .push((q.query.clone(), q.category.clone()));
                    }
                }
            }
        }
    }

    println!("\n--- Summary ---\n");
    println!(
        "{:<12} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6}",
        "Method", "MRR", "P@10", "R@10", "H@1", "H@2", "H@3", "H@5", "H@7", "H@10", "Miss"
    );
    println!("{}", "-".repeat(90));
    for (mi, name) in METHOD_NAMES.iter().enumerate() {
        let mrr: f64 = all[mi].iter().map(|r| r.rr).sum::<f64>() / n as f64;
        let p10: f64 = all[mi].iter().map(|r| r.precision_at_10).sum::<f64>() / n as f64;
        let r10: f64 = all[mi].iter().map(|r| r.recall_at_10).sum::<f64>() / n as f64;
        let h1 = all[mi].iter().filter(|r| r.hit_at[0]).count();
        let h2 = all[mi].iter().filter(|r| r.hit_at[1]).count();
        let h3 = all[mi].iter().filter(|r| r.hit_at[2]).count();
        let h5 = all[mi].iter().filter(|r| r.hit_at[4]).count();
        let h7 = all[mi].iter().filter(|r| r.hit_at[6]).count();
        let h10 = all[mi].iter().filter(|r| r.hit_at[9]).count();
        let miss = all[mi].iter().filter(|r| !r.hit_at[9]).count();
        println!(
            "{:<12} {:>6.3} {:>6.3} {:>6.3} {:>5}/{:<4} {:>5}/{:<4} {:>5}/{:<4} {:>5}/{:<4} {:>5}/{:<4} {:>5}/{:<4} {:>5}/{:<4}",
            name, mrr, p10, r10,
            h1, n, h2, n, h3, n, h5, n, h7, n, h10, n, miss, n
        );
    }

    println!("\n--- Per-Query ---\n");
    let q_header = format!(
        "{:<28} {}",
        "Query",
        METHOD_NAMES.iter().map(|n| format!("{:>6}", n)).collect::<Vec<_>>().join("")
    );
    println!("{}", q_header);
    println!("{}", "-".repeat(q_header.len()));
    for (i, q) in queries.iter().enumerate() {
        let vals: Vec<String> = (0..N_METHODS)
            .map(|mi| format!("{:>6.3}", all[mi][i].rr))
            .collect();
        println!("{:<28} {}", truncate(&q.query, 28), vals.join(""));
    }

    println!("\n--- Per-Query Ranks ---\n");
    let r_header = format!(
        "{:<28} {}",
        "Query",
        METHOD_NAMES.iter().map(|n| format!("{:>6}", n)).collect::<Vec<_>>().join("")
    );
    println!("{}", r_header);
    println!("{}", "-".repeat(r_header.len()));
    for (i, q) in queries.iter().enumerate() {
        let fmt = |r: Option<usize>| -> String {
            r.map(|v| format!("{}", v + 1)).unwrap_or_else(|| "MISS".into())
        };
        let vals: Vec<String> = (0..N_METHODS)
            .map(|mi| format!("{:>6}", fmt(all[mi][i].found_rank)))
            .collect();
        println!("{:<28} {}", truncate(&q.query, 28), vals.join(""));
    }

    println!("\n--- Failure Autopsy (Deformed) ---\n");
    for cluster in &cluster_names {
        if let Some(failures) = failure_clusters.get(*cluster) {
            println!("  {} ({} failures):", cluster, failures.len());
            for (query, category) in failures.iter().take(10) {
                println!("    [{}] {}", category, truncate(query, 50));
            }
            if failures.len() > 10 {
                println!("    ... and {} more", failures.len() - 10);
            }
        }
    }
    if failure_clusters.is_empty() {
        println!("  No failures detected for Deformed method.");
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: graphiq-bench <db-path> <ndcg-queries.json> <mrr-queries.json>");
        eprintln!("       graphiq-bench tune <db-path> <ndcg-queries.json> <mrr-queries.json>");
        eprintln!("       graphiq-bench profile <db-path> <mrr-queries.json>");
        eprintln!("       graphiq-bench fuzz <db-path>");
        std::process::exit(1);
    }

    if args[1] == "tune" {
        cmd_tune(&args);
        return;
    }

    if args[1] == "profile" {
        cmd_profile(&args);
        return;
    }

    if args[1] == "fuzz" {
        cmd_fuzz(&args);
        return;
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

    let hi = cruncher::build_holo_index(&db, &ci);

    eprintln!("Computing spectral index...");
    let spectral = match graphiq_core::spectral::compute_spectral(&db) {
        Ok(mut idx) => {
            eprintln!("Computing Ricci curvature...");
            let kappa = graphiq_core::spectral::compute_ricci_curvature(&idx.graph);
            idx.graph.edge_curvature = Some(kappa);
            Some(idx)
        }
        Err(e) => {
            eprintln!("  spectral computation failed: {e}, skipping Geometric");
            None
        }
    };

    eprintln!("Computing predictive model...");
    let predictive = match graphiq_core::spectral::compute_predictive_model(&db) {
        Ok(pm) => Some(pm),
        Err(e) => {
            eprintln!("  predictive model failed: {e}");
            None
        }
    };

    eprintln!("Computing channel fingerprints...");
    let (fp_vec, fp_id_map) = graphiq_core::spectral::compute_channel_fingerprints(&db);
    let fingerprints = Some(fp_vec);
    let fp_id_to_idx = Some(fp_id_map);

    eprintln!("Building self-model...");
    let self_model = graphiq_core::self_model::build_self_model(&db).ok();

    let ndcg_file = args.get(2).map(|s| s.as_str());
    let mrr_file = args.get(3).map(|s| s.as_str());

    let cache = HotCache::with_defaults();
    cache.prewarm(&db, 200);

    if let Some(file) = ndcg_file {
        let content = std::fs::read_to_string(file).unwrap_or_else(|e| {
            eprintln!("error reading NDCG query file: {e}");
            std::process::exit(1);
        });
        let queries: Vec<BenchQuery> = serde_json::from_str(&content).unwrap_or_else(|e| {
            eprintln!("error parsing NDCG query file: {e}");
            std::process::exit(1);
        });
        run_ndcg_benchmark(&db, &fts, &ci, &hi, &spectral, &predictive, &fingerprints, &fp_id_to_idx, &cache, &self_model, &queries);
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
        run_mrr_benchmark(&db, &fts, &ci, &hi, &spectral, &predictive, &fingerprints, &fp_id_to_idx, &cache, &self_model, &queries);
    }

    if ndcg_file.is_none() && mrr_file.is_none() {
        eprintln!("no query files provided. usage: graphiq-bench <db> <ndcg.json> <mrr.json>");
    }
}

fn cmd_tune(args: &[String]) {
    if args.len() < 3 {
        eprintln!("usage: graphiq-bench tune <db-path> <ndcg-queries.json> [mrr-queries.json]");
        std::process::exit(1);
    }

    let db_path = &args[2];
    let ndcg_file = args.get(3).map(|s| s.as_str());
    let mrr_file = args.get(4).map(|s| s.as_str());

    let db = match GraphDb::open(Path::new(db_path)) {
        Ok(d) => d,
        Err(e) => { eprintln!("error: {e}"); std::process::exit(1); }
    };

    let fts = FtsSearch::new(&db);
    let ci = match cruncher::build_cruncher_index(&db) {
        Ok(idx) => idx,
        Err(e) => { eprintln!("cruncher build failed: {e}"); std::process::exit(1); }
    };
    let hi = cruncher::build_holo_index(&db, &ci);

    eprintln!("Computing spectral index...");
    let spectral = match graphiq_core::spectral::compute_spectral(&db) {
        Ok(idx) => idx,
        Err(e) => { eprintln!("spectral failed: {e}"); std::process::exit(1); }
    };

    let ndcg_queries: Vec<BenchQuery> = if let Some(file) = ndcg_file {
        let content = std::fs::read_to_string(file).unwrap_or_else(|e| { eprintln!("error: {e}"); std::process::exit(1); });
        serde_json::from_str(&content).unwrap_or_else(|e| { eprintln!("parse error: {e}"); std::process::exit(1); })
    } else {
        Vec::new()
    };

    let mrr_queries: Vec<BenchQuery> = if let Some(file) = mrr_file {
        let content = std::fs::read_to_string(file).unwrap_or_else(|e| { eprintln!("error: {e}"); std::process::exit(1); });
        serde_json::from_str(&content).unwrap_or_else(|e| { eprintln!("parse error: {e}"); std::process::exit(1); })
    } else {
        Vec::new()
    };

    let heat_ts: Vec<f64> = vec![0.3, 0.5, 0.7, 1.0, 1.5, 2.0, 3.0, 5.0];
    let cheb_orders: Vec<usize> = vec![10, 15, 20, 30];
    let walk_weights: Vec<f64> = vec![1.0, 2.0, 3.0, 4.0, 5.0, 7.0, 10.0];
    let heat_top_ks: Vec<usize> = vec![50, 100, 200];

    println!("heat_t,cheb_order,walk_weight,heat_top_k,ndcg,mrr,h1,h3,h5,h10,mrr_acc,mrr_miss");

    let total = heat_ts.len() * cheb_orders.len() * walk_weights.len() * heat_top_ks.len();
    let mut count = 0usize;

    for &heat_t in &heat_ts {
        for &cheb_order in &cheb_orders {
            for &walk_weight in &walk_weights {
                for &heat_top_k in &heat_top_ks {
                    count += 1;
                    eprint!("\r{}/{}", count, total);

                    let mut ndcg_sum = 0.0f64;
                    let mut ndcg_n = 0usize;
                    let mut hits: [usize; 5] = [0; 5];

                    for q in &ndcg_queries {
                        let ideal = compute_ideal_rels(&db, q);
                        let results = cruncher::geometric_search(
                            &q.query, &ci, &hi,
                            &fts.search(&q.query, Some(10)).into_iter()
                                .map(|r| (r.symbol.id, r.bm25_score)).collect::<Vec<_>>(),
                            &spectral, 10, heat_t, cheb_order, walk_weight, heat_top_k, false,
                            None, None, None, 1.0,
                        );
                        let rels: Vec<f64> = results.iter()
                            .map(|(id, _)| q.relevance_of(&sym_name(&db, *id)) as f64)
                            .collect();
                        ndcg_sum += ndcg_at_k(&rels, &ideal, 10);
                        ndcg_n += 1;
                        let first_rel = results.iter().position(|(id, _)| q.relevance_of(&sym_name(&db, *id)) >= 2);
                        if let Some(r) = first_rel { if r < 1 { hits[0] += 1; } if r < 3 { hits[1] += 1; } if r < 5 { hits[2] += 1; } if r < 10 { hits[3] += 1; } }
                        if first_rel.is_some() { hits[4] += 1; }
                    }

                    let ndcg = if ndcg_n > 0 { ndcg_sum / ndcg_n as f64 } else { 0.0 };

                    let mut mrr_sum = 0.0f64;
                    let mut mrr_n = 0usize;
                    let mut mrr_acc = 0usize;
                    let mut mrr_miss = 0usize;

                    for q in &mrr_queries {
                        let expected = q.expected_symbol.as_deref().unwrap_or("");
                        let results = cruncher::geometric_search(
                            &q.query, &ci, &hi,
                            &fts.search(&q.query, Some(10)).into_iter()
                                .map(|r| (r.symbol.id, r.bm25_score)).collect::<Vec<_>>(),
                            &spectral, 10, heat_t, cheb_order, walk_weight, heat_top_k, false,
                            None, None, None, 1.0,
                        );
                        let found = results.iter().position(|(id, _)| {
                            let name = sym_name(&db, *id);
                            name == expected || expected.contains(&name) || name.contains(expected)
                        });
                        mrr_sum += found.map(|r| 1.0 / (r + 1) as f64).unwrap_or(0.0);
                        mrr_n += 1;
                        if found == Some(0) { mrr_acc += 1; }
                        if found.is_none() { mrr_miss += 1; }
                    }

                    let mrr = if mrr_n > 0 { mrr_sum / mrr_n as f64 } else { 0.0 };

                    println!("{},{},{},{},{:.4},{:.4},{},{},{},{},{:.3},{}",
                        heat_t, cheb_order, walk_weight, heat_top_k,
                        ndcg, mrr,
                        hits[0], hits[1], hits[2], hits[3],
                        if mrr_n > 0 { mrr_acc as f64 / mrr_n as f64 } else { 0.0 },
                        mrr_miss,
                    );
                }
            }
        }
    }
    eprintln!("\nDone.");
}

fn cmd_profile(args: &[String]) {
    if args.len() < 4 {
        eprintln!("usage: graphiq-bench profile <db-path> <mrr-queries.json>");
        std::process::exit(1);
    }
    let db_path = &args[2];
    let query_file = &args[3];

    let db = match GraphDb::open(Path::new(db_path)) {
        Ok(d) => d,
        Err(e) => { eprintln!("error: {e}"); std::process::exit(1); }
    };
    let stats = db.stats().unwrap();
    println!("=== Latency Profile ===");
    println!("Database: {} files, {} symbols, {} edges\n", stats.files, stats.symbols, stats.edges);

    let fts = FtsSearch::new(&db);
    let ci = match cruncher::build_cruncher_index(&db) {
        Ok(idx) => idx,
        Err(e) => { eprintln!("cruncher build failed: {e}"); std::process::exit(1); }
    };
    let hi = cruncher::build_holo_index(&db, &ci);

    let content = std::fs::read_to_string(query_file).unwrap_or_else(|e| {
        eprintln!("error reading query file: {e}"); std::process::exit(1);
    });
    let queries: Vec<BenchQuery> = serde_json::from_str(&content).unwrap_or_else(|e| {
        eprintln!("error parsing query file: {e}");
        std::process::exit(1);
    });

    let n_runs = 10;
    println!("Running {} queries x {} iterations...\n", queries.len(), n_runs);

    let mut all_durations: Vec<u128> = Vec::new();
    let methods = ["BM25", "GooberV4", "GooberV5"];

    for method_name in &methods {
        let mut method_durations: Vec<u128> = Vec::new();

        for _ in 0..n_runs {
            for q in &queries {
                let start = std::time::Instant::now();

                match *method_name {
                    "BM25" => { let _ = fts.search(&q.query, Some(10)); }
                    "GooberV4" => {
                        let bm25: Vec<(i64, f64)> = fts
                            .search(&q.query, Some(30))
                            .into_iter()
                            .map(|r| (r.symbol.id, r.bm25_score))
                            .collect();
                        let _ = cruncher::goober_v4_search(&q.query, &ci, &bm25, 10);
                    }
                    "GooberV5" => {
                        let bm25: Vec<(i64, f64)> = fts
                            .search(&q.query, Some(30))
                            .into_iter()
                            .map(|r| (r.symbol.id, r.bm25_score))
                            .collect();
                        let _ = cruncher::goober_v5_search(&q.query, &ci, &hi, &bm25, 10);
                    }
                    _ => {}
                }

                method_durations.push(start.elapsed().as_micros());
            }
        }

        method_durations.sort();
        let p50 = method_durations[method_durations.len() / 2];
        let p99 = method_durations[method_durations.len() * 99 / 100];
        let p_min = method_durations[0];
        let p_max = method_durations[method_durations.len() - 1];
        let avg: u128 = method_durations.iter().sum::<u128>() / method_durations.len() as u128;

        all_durations.extend(method_durations.iter());

        println!(
            "{:<12} min={:>5}us  p50={:>5}us  avg={:>5}us  p99={:>5}us  max={:>5}us",
            method_name, p_min, p50, avg, p99, p_max
        );
    }

    all_durations.sort();
    let overall_p50 = all_durations[all_durations.len() / 2];
    let overall_p99 = all_durations[all_durations.len() * 99 / 100];
    println!("\nOverall: p50={}us  p99={}us", overall_p50, overall_p99);

    let ci_mem_approx = ci.symbol_ids.len() * (
        std::mem::size_of::<i64>() +
        std::mem::size_of::<usize>() +
        std::mem::size_of::<usize>() +
        std::mem::size_of::<i64>() +
        std::mem::size_of::<usize>()
    );
    println!("\nIndex sizes (rough estimate):");
    println!("  CruncherIndex: ~{}KB", ci_mem_approx / 1024);
    println!("  HoloIndex: {} name holograms", hi.name_holos.len());
}

fn cmd_fuzz(args: &[String]) {
    if args.len() < 3 {
        eprintln!("usage: graphiq-bench fuzz <db-path>");
        std::process::exit(1);
    }
    let db_path = &args[2];

    let db = match GraphDb::open(Path::new(db_path)) {
        Ok(d) => d,
        Err(e) => { eprintln!("error: {e}"); std::process::exit(1); }
    };
    let stats = db.stats().unwrap();
    println!("=== Fuzz Test ===");
    println!("Database: {} files, {} symbols, {} edges\n", stats.files, stats.symbols, stats.edges);

    let fts = FtsSearch::new(&db);
    let ci = cruncher::build_cruncher_index(&db).expect("cruncher build failed");
    let hi = cruncher::build_holo_index(&db, &ci);

    let fuzz_queries: Vec<&str> = vec![
        "", " ", "  ", "\t", "\n",
        "a", "z", "0", ".", "-", "_",
        "parse(config)", "a && b || c", "foo.bar.baz",
        "rate-limit", "parse+config", "parse*config",
        "parse[0]", "{json: true}", "<html>",
        "a->b", "a=>b", "a::b", "a;b", "a,b",
        "\"quoted\"", "'single'", "\\escaped\\",
        "null", "undefined", "NaN",
        "the", "a an the", "is are was were",
        "parse parse parse parse",
        "a a a a a a a a a a",
        "parseConfig", "parse_config", "parse-config",
        "PascalCase", "UPPER_CASE", "miXeD_CaSe_Name",
        "123", "3.14", "0x1F", "1e10", "v2.0", "h264",
    ];

    let mut long = String::new();
    for i in 0..1000 { long.push_str(&format!("term{} ", i)); }
    let long_trimmed = long.trim_end();

    let mut passed = 0usize;
    let mut failed = 0usize;

    let all_queries: Vec<&str> = fuzz_queries.iter().chain(std::iter::once(&long_trimmed)).copied().collect();

    for q in &all_queries {
        let bm25: Vec<(i64, f64)> = fts
            .search(q, Some(30))
            .into_iter()
            .map(|r| (r.symbol.id, r.bm25_score))
            .collect();

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = cruncher::goober_v5_search(q, &ci, &hi, &bm25, 10);
        }));

        match result {
            Ok(_) => passed += 1,
            Err(_) => {
                failed += 1;
                eprintln!("PANIC on query: {:?}", q);
            }
        }
    }

    println!("{} passed, {} failed ({} total)", passed, failed, all_queries.len());
    if failed > 0 {
        std::process::exit(1);
    }
    println!("All fuzz queries handled without panic.");
}
