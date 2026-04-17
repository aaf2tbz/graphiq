use std::collections::HashSet;

use crate::db::GraphDb;
use crate::fts::FtsResult;
use crate::symbol::{Symbol, SymbolKind, Visibility};

pub struct DirectoryExpander<'a> {
    db: &'a GraphDb,
}

impl<'a> DirectoryExpander<'a> {
    pub fn new(db: &'a GraphDb) -> Self {
        Self { db }
    }

    pub fn expand(
        &self,
        fts_results: &[FtsResult],
        existing_ids: &HashSet<i64>,
        max_siblings: usize,
        query: &str,
    ) -> Vec<DirectorySibling> {
        let query_lower = query.to_lowercase();
        let query_tokens: Vec<&str> = query_lower
            .split_whitespace()
            .filter(|t| t.len() >= 2)
            .collect();

        let seed_paths = self.collect_seed_paths(fts_results, 3);
        if seed_paths.is_empty() {
            return Vec::new();
        }

        let mut siblings = Vec::new();
        let mut seen = existing_ids.clone();
        let mut per_dir_count: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();

        for (file_id, path) in &seed_paths {
            for level in 1..=3 {
                let dir = parent_dir(path, level);
                if dir.is_empty() {
                    continue;
                }

                let dir_basename = dir.rsplit_once('/').map(|(_, n)| n).unwrap_or(&dir);
                let dir_matches_query = query_tokens.iter().any(|t| dir_basename.contains(t));
                let per_dir_limit = if dir_matches_query { 6 } else { 2 };

                let dir_key = format!("{}:{}", dir, level);
                if *per_dir_count.entry(dir_key.clone()).or_insert(0) >= per_dir_limit {
                    continue;
                }

                let dir_symbols = match self.db.symbols_by_path_prefix(&format!("{}/", dir), 50) {
                    Ok(s) => s,
                    Err(_) => continue,
                };

                let mut added = 0;
                for sym in dir_symbols {
                    if added >= per_dir_limit {
                        break;
                    }
                    if seen.contains(&sym.id) {
                        continue;
                    }
                    if !is_primary_export(&sym) {
                        continue;
                    }

                    seen.insert(sym.id);
                    let base_proximity = 1.0 / (level as f64).sqrt();
                    let proximity = if dir_matches_query {
                        base_proximity * 1.5
                    } else {
                        base_proximity
                    };
                    siblings.push(DirectorySibling {
                        symbol: sym,
                        seed_file_id: *file_id,
                        proximity,
                    });
                    added += 1;
                }
                per_dir_count.insert(dir_key, added);
            }
        }

        siblings.truncate(max_siblings);
        siblings
    }

    pub fn expand_cross_package(
        &self,
        fts_results: &[FtsResult],
        existing_ids: &HashSet<i64>,
        max_siblings: usize,
        query: &str,
    ) -> Vec<DirectorySibling> {
        let query_lower = query.to_lowercase();
        let query_tokens: Vec<&str> = query_lower
            .split_whitespace()
            .filter(|t| t.len() >= 3)
            .filter(|t| *t != "all" && *t != "the")
            .collect();

        if query_tokens.is_empty() {
            return Vec::new();
        }

        let seed_paths = self.collect_seed_paths(fts_results, 5);
        let seed_packages = extract_package_prefixes(&seed_paths);
        if seed_packages.is_empty() {
            return Vec::new();
        }

        let all_prefixes = match self.db.distinct_path_prefixes(2) {
            Ok(p) => p,
            Err(_) => return Vec::new(),
        };

        let mut matching_prefixes = Vec::new();
        for token in &query_tokens {
            for prefix in &all_prefixes {
                let basename = prefix.rsplit_once('/').map(|(_, n)| n).unwrap_or(prefix);
                if basename.contains(token) {
                    matching_prefixes.push(prefix.clone());
                }
            }
        }

        let mut sibling_packages: Vec<String> = Vec::new();
        for seed_pkg in &seed_packages {
            let seed_base = package_base_name(seed_pkg);
            for prefix in &matching_prefixes {
                let prefix_base = package_base_name(prefix);
                if seed_base == prefix_base && prefix != seed_pkg {
                    sibling_packages.push(prefix.clone());
                }
            }
        }

        if sibling_packages.is_empty() {
            for _token in &query_tokens {
                for prefix in &matching_prefixes {
                    if !seed_packages.contains(prefix) {
                        sibling_packages.push(prefix.clone());
                    }
                }
            }
        }

        sibling_packages.sort();
        sibling_packages.dedup();

        let mut siblings = Vec::new();
        let mut seen = existing_ids.clone();
        let mut per_pkg_count: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();

        for pkg_prefix in &sibling_packages {
            let pkg_limit = 1;
            if *per_pkg_count.entry(pkg_prefix.clone()).or_insert(0) >= pkg_limit {
                continue;
            }

            let dir_symbols = match self
                .db
                .symbols_by_path_prefix(&format!("{}/", pkg_prefix), 20)
            {
                Ok(s) => s,
                Err(_) => continue,
            };

            let mut added = 0;
            for sym in dir_symbols {
                if added >= pkg_limit {
                    break;
                }
                if seen.contains(&sym.id) {
                    continue;
                }
                if !is_primary_export(&sym) {
                    continue;
                }
                if !is_container_kind(&sym.kind) {
                    continue;
                }

                seen.insert(sym.id);
                siblings.push(DirectorySibling {
                    symbol: sym,
                    seed_file_id: 0,
                    proximity: 0.9,
                });
                added += 1;
            }
            per_pkg_count.insert(pkg_prefix.clone(), added);
        }

        siblings.truncate(max_siblings);
        siblings
    }

    fn collect_seed_paths(
        &self,
        fts_results: &[FtsResult],
        max_seeds: usize,
    ) -> Vec<(i64, String)> {
        let mut paths = Vec::new();
        let mut seen_dirs = HashSet::new();
        for fts in fts_results.iter().take(max_seeds * 3) {
            if paths.len() >= max_seeds {
                break;
            }
            if let Ok(Some(p)) = self.db.file_path_for_id(fts.symbol.file_id) {
                let dir = parent_dir(&p, 1);
                if seen_dirs.insert(dir) {
                    paths.push((fts.symbol.file_id, p));
                }
            }
        }
        paths
    }
}

fn parent_dir(path: &str, levels: usize) -> String {
    let mut dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    for _ in 0..levels.saturating_sub(1) {
        dir = dir.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    }
    dir.to_string()
}

fn is_primary_export(sym: &Symbol) -> bool {
    if sym.visibility != Visibility::Public {
        return false;
    }
    matches!(
        sym.kind,
        SymbolKind::Struct
            | SymbolKind::Class
            | SymbolKind::Enum
            | SymbolKind::Trait
            | SymbolKind::Interface
            | SymbolKind::Function
    )
}

fn is_container_kind(kind: &SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Struct
            | SymbolKind::Class
            | SymbolKind::Enum
            | SymbolKind::Trait
            | SymbolKind::Interface
    )
}

fn extract_package_prefixes(seed_paths: &[(i64, String)]) -> Vec<String> {
    let mut prefixes = Vec::new();
    for (_, path) in seed_paths {
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() >= 2 {
            prefixes.push(format!("{}/{}", parts[0], parts[1]));
        }
    }
    prefixes.sort();
    prefixes.dedup();
    prefixes
}

fn package_base_name(pkg_path: &str) -> &str {
    let name = pkg_path
        .rsplit_once('/')
        .map(|(_, n)| n)
        .unwrap_or(pkg_path);
    name.rsplit_once('-').map(|(_, rest)| rest).unwrap_or(name)
}

#[derive(Debug, Clone)]
pub struct DirectorySibling {
    pub symbol: Symbol,
    pub seed_file_id: i64,
    pub proximity: f64,
}
