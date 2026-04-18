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
}

fn sym_name(db: &GraphDb, id: i64) -> String {
    db.conn()
        .query_row("SELECT name FROM symbols WHERE id = ?", [id], |row| {
            row.get::<_, String>(0)
        })
        .unwrap_or_default()
}

fn sym_file(db: &GraphDb, id: i64) -> String {
    db.conn()
        .query_row(
            "SELECT f.path FROM symbols s JOIN files f ON s.file_id = f.id WHERE s.id = ?",
            [id],
            |row| row.get::<_, String>(0),
        )
        .unwrap_or_default()
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
    if args.len() < 3 {
        eprintln!("usage: graphiq-diag <db-path> <queries.json>");
        std::process::exit(1);
    }

    let db = match GraphDb::open(Path::new(&args[1])) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    let queries: Vec<BenchQuery> = {
        let content = std::fs::read_to_string(&args[2]).unwrap();
        serde_json::from_str(&content).unwrap()
    };

    let fts = FtsSearch::new(&db);
    let ci = cruncher::build_cruncher_index(&db).unwrap();

    println!("=== Cruncher Diagnostic ===\n");

    for q in &queries {
        println!("{}", "─".repeat(80));
        println!("QUERY: \"{}\" [{}]", q.query, q.category);
        println!(
            "TARGET: {}",
            q.expected_symbol.as_deref().unwrap_or("?")
        );
        println!();

        let bm25: Vec<(i64, f64)> = fts
            .search(&q.query, Some(10))
            .into_iter()
            .map(|r| (r.symbol.id, r.bm25_score))
            .collect();
        let cr_solo = cruncher::cruncher_search_standalone(&q.query, &ci, 10);
        let cr_fused = cruncher::cruncher_search(&q.query, &ci, &bm25, 10);

        let expected = q.expected_symbol.as_deref().unwrap_or("");

        for (label, results) in [("BM25", &bm25), ("CR Solo", &cr_solo), ("CR Fused", &cr_fused)]
        {
            print!("{:<10} [", label);
            let hit = results.iter().position(|(id, _)| {
                let name = sym_name(&db, *id);
                name == expected || expected.contains(&name) || name.contains(expected)
            });
            match hit {
                Some(r) => print!("HIT rank={}", r + 1),
                None => print!("MISS"),
            }
            println!("]");

            for (rank, (id, score)) in results.iter().take(5).enumerate() {
                let name = sym_name(&db, *id);
                let file = sym_file(&db, *id);
                let marker = if name == expected
                    || expected.contains(&name)
                    || name.contains(expected)
                {
                    " <<"
                } else {
                    ""
                };
                println!(
                    "  {:>2}. {:10.3} {:<35} {}{}",
                    rank + 1,
                    score,
                    truncate(&name, 35),
                    truncate(&file, 40),
                    marker
                );
            }
            println!();
        }
    }
}
