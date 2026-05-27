//! Git-diff-aware impact analysis.
//!
//! Converts git changes into indexed symbols, then fans out through the
//! existing blast-radius graph so agents can triage changed code before edits,
//! review, or test selection.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::blast::compute_blast_radius;
use crate::db::GraphDb;
use crate::edge::{BlastDirection, EdgeKind};
use crate::graph::{bounded_bfs, TraverseDirection};
use crate::symbol::{Symbol, SymbolKind, Visibility};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ChangeSource {
    WorkingTree,
    BaseRef { base: String, head: String },
}

impl ChangeSource {
    pub fn label(&self) -> String {
        match self {
            ChangeSource::WorkingTree => "working tree".to_string(),
            ChangeSource::BaseRef { base, head } => format!("{base}...{head}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ImpactOptions {
    pub project_root: PathBuf,
    pub db_path: Option<PathBuf>,
    pub source: ChangeSource,
    pub depth: usize,
    pub top: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct LineRange {
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangedFile {
    pub path: String,
    pub old_path: Option<String>,
    pub status: String,
    pub changed_ranges: Vec<LineRange>,
    pub indexed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangedSymbol {
    pub symbol_id: i64,
    pub name: String,
    pub kind: String,
    pub file_path: String,
    pub line_start: u32,
    pub line_end: u32,
    pub touched_ranges: Vec<LineRange>,
    pub confidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AffectedSymbol {
    pub symbol_id: i64,
    pub name: String,
    pub kind: String,
    pub file_path: String,
    pub distance: usize,
    pub direction: String,
    pub via_changed_symbols: Vec<String>,
    pub edge_path: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LikelyTest {
    pub symbol_id: Option<i64>,
    pub name: String,
    pub file_path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

impl RiskLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            RiskLevel::Low => "low",
            RiskLevel::Medium => "medium",
            RiskLevel::High => "high",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactReport {
    pub source: String,
    pub changed_files: Vec<ChangedFile>,
    pub changed_symbols: Vec<ChangedSymbol>,
    pub dependents: Vec<AffectedSymbol>,
    pub dependencies: Vec<AffectedSymbol>,
    pub likely_tests: Vec<LikelyTest>,
    pub risk: RiskLevel,
    pub warnings: Vec<String>,
}

pub fn analyze_git_impact(db: &GraphDb, options: ImpactOptions) -> Result<ImpactReport, String> {
    let changed_files = collect_git_changes(&options.project_root, &options.source)?;
    analyze_changed_files(db, changed_files, options)
}

pub fn analyze_changed_files(
    db: &GraphDb,
    mut changed_files: Vec<ChangedFile>,
    options: ImpactOptions,
) -> Result<ImpactReport, String> {
    let mut warnings = index_warnings(db, &options);
    let changed_symbols = map_changed_symbols(db, &mut changed_files, &mut warnings)?;
    let (dependents, dependencies) = expand_impact(db, &changed_symbols, options.depth)?;
    let likely_tests = collect_likely_tests(db, &changed_symbols, &dependents, &dependencies)?;
    let risk = classify_risk(
        &changed_files,
        &changed_symbols,
        dependents.len() + dependencies.len(),
        &warnings,
    );

    Ok(ImpactReport {
        source: options.source.label(),
        changed_files,
        changed_symbols: cap_changed_symbols(changed_symbols, options.top),
        dependents: cap_affected(dependents, options.top),
        dependencies: cap_affected(dependencies, options.top),
        likely_tests: cap_tests(likely_tests, options.top),
        risk,
        warnings,
    })
}

pub fn collect_git_changes(
    project_root: &Path,
    source: &ChangeSource,
) -> Result<Vec<ChangedFile>, String> {
    let mut files = BTreeMap::<String, ChangedFile>::new();
    match source {
        ChangeSource::WorkingTree => {
            let unstaged = run_git(
                project_root,
                &[
                    "diff",
                    "--unified=0",
                    "--no-color",
                    "--no-ext-diff",
                    "--find-renames",
                ],
            )?;
            merge_files(&mut files, filter_impact_files(parse_diff(&unstaged)));

            let staged = run_git(
                project_root,
                &[
                    "diff",
                    "--cached",
                    "--unified=0",
                    "--no-color",
                    "--no-ext-diff",
                    "--find-renames",
                ],
            )?;
            merge_files(&mut files, filter_impact_files(parse_diff(&staged)));

            let untracked = run_git_bytes(
                project_root,
                &["ls-files", "--others", "--exclude-standard", "-z"],
            )?;
            for raw in untracked.split(|b| *b == 0) {
                if raw.is_empty() {
                    continue;
                }
                let path = String::from_utf8_lossy(raw).to_string();
                if is_ignored_impact_path(&path) {
                    continue;
                }
                files.entry(path.clone()).or_insert_with(|| ChangedFile {
                    path,
                    old_path: None,
                    status: "untracked".to_string(),
                    changed_ranges: Vec::new(),
                    indexed: false,
                });
            }
        }
        ChangeSource::BaseRef { base, head } => {
            let range = format!("{base}...{head}");
            let diff = run_git(
                project_root,
                &[
                    "diff",
                    "--unified=0",
                    "--no-color",
                    "--no-ext-diff",
                    "--find-renames",
                    &range,
                ],
            )?;
            merge_files(&mut files, filter_impact_files(parse_diff(&diff)));
        }
    }

    Ok(files.into_values().collect())
}

fn filter_impact_files(files: Vec<ChangedFile>) -> Vec<ChangedFile> {
    files
        .into_iter()
        .filter(|file| !is_ignored_impact_path(&file.path))
        .collect()
}

fn is_ignored_impact_path(path: &str) -> bool {
    path == ".graphiq" || path.starts_with(".graphiq/")
}

pub fn format_impact_report(report: &ImpactReport) -> String {
    let mut out = Vec::new();
    out.push(format!(
        "Impact: {} file(s), {} changed symbol(s), {} affected symbol(s), risk: {}",
        report.changed_files.len(),
        report.changed_symbols.len(),
        report.dependents.len() + report.dependencies.len(),
        report.risk.as_str(),
    ));
    out.push(format!("Source: {}", report.source));
    out.push(String::new());

    out.push("Changed Symbols".to_string());
    if report.changed_symbols.is_empty() {
        out.push("  (none mapped)".to_string());
    } else {
        for sym in &report.changed_symbols {
            out.push(format!(
                "  - {}::{} @ {}:{}-{} [{}]",
                sym.kind, sym.name, sym.file_path, sym.line_start, sym.line_end, sym.confidence
            ));
        }
    }
    out.push(String::new());

    out.push("Affected Dependents".to_string());
    format_affected_section(&mut out, &report.dependents);
    out.push(String::new());

    out.push("Touched Dependencies".to_string());
    format_affected_section(&mut out, &report.dependencies);
    out.push(String::new());

    out.push("Likely Tests".to_string());
    if report.likely_tests.is_empty() {
        out.push("  (none found)".to_string());
    } else {
        for test in &report.likely_tests {
            out.push(format!(
                "  - {} @ {} ({})",
                test.name, test.file_path, test.reason
            ));
        }
    }

    if !report.warnings.is_empty() {
        out.push(String::new());
        out.push("Warnings".to_string());
        for warning in &report.warnings {
            out.push(format!("  - {warning}"));
        }
    }

    out.join("\n")
}

fn format_affected_section(out: &mut Vec<String>, items: &[AffectedSymbol]) {
    if items.is_empty() {
        out.push("  (none)".to_string());
        return;
    }
    for sym in items {
        out.push(format!(
            "  - [{}] {}::{} @ {} via {}",
            sym.distance,
            sym.kind,
            sym.name,
            sym.file_path,
            sym.via_changed_symbols.join(", ")
        ));
    }
}

fn run_git(project_root: &Path, args: &[&str]) -> Result<String, String> {
    let bytes = run_git_bytes(project_root, args)?;
    String::from_utf8(bytes).map_err(|e| format!("git output was not valid UTF-8: {e}"))
}

fn run_git_bytes(project_root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(project_root)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!("git {:?} failed", args)
        } else {
            stderr
        });
    }
    Ok(output.stdout)
}

fn merge_files(target: &mut BTreeMap<String, ChangedFile>, files: Vec<ChangedFile>) {
    for file in files {
        target
            .entry(file.path.clone())
            .and_modify(|existing| {
                existing.status = merge_status(&existing.status, &file.status);
                if existing.old_path.is_none() {
                    existing.old_path = file.old_path.clone();
                }
                existing.changed_ranges.extend(file.changed_ranges.clone());
                dedup_ranges(&mut existing.changed_ranges);
            })
            .or_insert(file);
    }
}

fn merge_status(left: &str, right: &str) -> String {
    if left == right {
        left.to_string()
    } else if left == "untracked" || right == "untracked" {
        "untracked".to_string()
    } else if left == "deleted" || right == "deleted" {
        "modified".to_string()
    } else {
        "modified".to_string()
    }
}

fn parse_diff(diff: &str) -> Vec<ChangedFile> {
    let mut files = Vec::new();
    let mut current: Option<ChangedFile> = None;

    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("diff --git a/") {
            if let Some(file) = current.take() {
                files.push(finalize_file(file));
            }
            let (old_path, new_path) = split_diff_paths(rest);
            current = Some(ChangedFile {
                path: new_path,
                old_path: Some(old_path),
                status: "modified".to_string(),
                changed_ranges: Vec::new(),
                indexed: false,
            });
            continue;
        }

        let Some(file) = current.as_mut() else {
            continue;
        };

        if line.starts_with("new file mode") {
            file.status = "added".to_string();
        } else if line.starts_with("deleted file mode") {
            file.status = "deleted".to_string();
        } else if let Some(path) = line.strip_prefix("rename from ") {
            file.status = "renamed".to_string();
            file.old_path = Some(path.to_string());
        } else if let Some(path) = line.strip_prefix("rename to ") {
            file.status = "renamed".to_string();
            file.path = path.to_string();
        } else if let Some(path) = line.strip_prefix("+++ ") {
            if path != "/dev/null" {
                file.path = strip_diff_prefix(path).to_string();
            }
        } else if let Some(path) = line.strip_prefix("--- ") {
            if path != "/dev/null" && file.old_path.is_none() {
                file.old_path = Some(strip_diff_prefix(path).to_string());
            }
        } else if line.starts_with("@@") {
            if let Some(range) = parse_hunk_range(line) {
                file.changed_ranges.push(range);
            }
        }
    }

    if let Some(file) = current.take() {
        files.push(finalize_file(file));
    }

    files
}

fn finalize_file(mut file: ChangedFile) -> ChangedFile {
    if file.status == "deleted" {
        if let Some(old) = file.old_path.clone() {
            file.path = old;
        }
    }
    dedup_ranges(&mut file.changed_ranges);
    file
}

fn split_diff_paths(rest: &str) -> (String, String) {
    match rest.split_once(" b/") {
        Some((old_path, new_path)) => (old_path.to_string(), new_path.to_string()),
        None => (rest.to_string(), rest.to_string()),
    }
}

fn strip_diff_prefix(path: &str) -> &str {
    path.strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))
        .unwrap_or(path)
}

fn parse_hunk_range(line: &str) -> Option<LineRange> {
    let old_range = parse_hunk_side(line, '-')?;
    let new_range = parse_hunk_side(line, '+')?;
    let (start, count) = if new_range.1 == 0 && old_range.1 > 0 {
        old_range
    } else {
        new_range
    };
    let end = if count == 0 {
        start
    } else {
        start.saturating_add(count).saturating_sub(1)
    };
    Some(LineRange { start, end })
}

fn parse_hunk_side(line: &str, marker: char) -> Option<(u32, u32)> {
    let idx = line.find(marker)?;
    let after_marker = &line[idx + 1..];
    let end = after_marker
        .find(|c: char| c == ' ' || c == '@')
        .unwrap_or(after_marker.len());
    let range = &after_marker[..end];
    match range.split_once(',') {
        Some((start, count)) => Some((start.parse().ok()?, count.parse().ok()?)),
        None => Some((range.parse().ok()?, 1)),
    }
}

fn dedup_ranges(ranges: &mut Vec<LineRange>) {
    ranges.sort_by_key(|r| (r.start, r.end));
    ranges.dedup();
}

fn index_warnings(db: &GraphDb, options: &ImpactOptions) -> Vec<String> {
    let mut warnings = Vec::new();
    if let Some(db_path) = options.db_path.as_ref() {
        if let Some(db_dir) = db_path.parent() {
            match crate::manifest::read_manifest(db_dir) {
                Ok(Some(manifest)) => {
                    let current = crate::manifest::FreshnessHash::from_db(db);
                    if manifest.freshness.is_stale_vs(&current) {
                        warnings
                            .push("index manifest appears stale; impact may be incomplete".into());
                    }
                }
                Ok(None) => warnings.push("index manifest missing; freshness is unknown".into()),
                Err(e) => warnings.push(format!("could not read index manifest: {e}")),
            }
        }
    }
    warnings
}

fn map_changed_symbols(
    db: &GraphDb,
    changed_files: &mut [ChangedFile],
    warnings: &mut Vec<String>,
) -> Result<Vec<ChangedSymbol>, String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    for file in changed_files {
        let lookup_path = lookup_path(db, file);
        let source_file = match lookup_path
            .as_ref()
            .and_then(|p| db.get_file_by_path(p).ok().flatten())
        {
            Some(f) => f,
            None => {
                warnings.push(format!("changed file '{}' is not indexed", file.path));
                continue;
            }
        };
        file.indexed = true;

        let symbols = db
            .symbols_by_file(source_file.id)
            .map_err(|e| format!("failed to load symbols for {}: {e}", file.path))?;
        if symbols.is_empty() {
            warnings.push(format!(
                "changed file '{}' has no indexed symbols",
                file.path
            ));
            continue;
        }

        let mut matched = Vec::new();
        if !file.changed_ranges.is_empty() {
            for sym in &symbols {
                let touched: Vec<LineRange> = file
                    .changed_ranges
                    .iter()
                    .filter(|range| intersects_symbol(range, sym))
                    .cloned()
                    .collect();
                if !touched.is_empty() {
                    matched.push((sym.clone(), touched, "direct".to_string()));
                }
            }
        }

        if matched.is_empty() && !file.changed_ranges.is_empty() {
            if let Some(sym) = nearest_symbol(&symbols, &file.changed_ranges) {
                matched.push((
                    sym.clone(),
                    file.changed_ranges.clone(),
                    "nearest".to_string(),
                ));
                warnings.push(format!(
                    "mapped '{}' to nearest symbol '{}' because no line range intersected an indexed symbol",
                    file.path, sym.name
                ));
            }
        }

        if matched.is_empty() {
            for sym in symbols
                .iter()
                .filter(|s| is_reportable_file_level_symbol(s))
                .take(5)
            {
                matched.push((sym.clone(), Vec::new(), "file".to_string()));
            }
        }

        for (sym, ranges, confidence) in matched {
            if !seen.insert(sym.id) {
                continue;
            }
            out.push(ChangedSymbol {
                symbol_id: sym.id,
                name: sym.name,
                kind: sym.kind.as_str().to_string(),
                file_path: file.path.clone(),
                line_start: sym.line_start,
                line_end: sym.line_end,
                touched_ranges: ranges,
                confidence,
            });
        }
    }

    Ok(out)
}

fn lookup_path(db: &GraphDb, file: &ChangedFile) -> Option<String> {
    if db.get_file_by_path(&file.path).ok().flatten().is_some() {
        return Some(file.path.clone());
    }
    file.old_path
        .as_ref()
        .filter(|p| db.get_file_by_path(p).ok().flatten().is_some())
        .cloned()
}

fn intersects_symbol(range: &LineRange, sym: &Symbol) -> bool {
    range.end >= sym.line_start && range.start <= sym.line_end
}

fn nearest_symbol<'a>(symbols: &'a [Symbol], ranges: &[LineRange]) -> Option<&'a Symbol> {
    symbols.iter().min_by_key(|sym| {
        ranges
            .iter()
            .map(|range| {
                if intersects_symbol(range, sym) {
                    0
                } else if range.end < sym.line_start {
                    sym.line_start - range.end
                } else {
                    range.start.saturating_sub(sym.line_end)
                }
            })
            .min()
            .unwrap_or(u32::MAX)
    })
}

fn is_reportable_file_level_symbol(sym: &Symbol) -> bool {
    matches!(
        sym.kind,
        SymbolKind::Function
            | SymbolKind::Method
            | SymbolKind::Class
            | SymbolKind::Struct
            | SymbolKind::Enum
            | SymbolKind::Trait
            | SymbolKind::Interface
            | SymbolKind::Module
    ) && sym.visibility == Visibility::Public
}

fn expand_impact(
    db: &GraphDb,
    changed_symbols: &[ChangedSymbol],
    depth: usize,
) -> Result<(Vec<AffectedSymbol>, Vec<AffectedSymbol>), String> {
    let mut dependents = HashMap::<i64, AffectedSymbol>::new();
    let mut dependencies = HashMap::<i64, AffectedSymbol>::new();
    let changed_ids: HashSet<i64> = changed_symbols.iter().map(|s| s.symbol_id).collect();

    for changed in changed_symbols {
        let radius =
            compute_blast_radius(db, changed.symbol_id, depth, BlastDirection::Both, None)?;

        for entry in radius.backward {
            if changed_ids.contains(&entry.symbol_id) {
                continue;
            }
            upsert_affected(&mut dependents, entry, "dependent", &changed.name);
        }

        for entry in radius.forward {
            if changed_ids.contains(&entry.symbol_id) {
                continue;
            }
            upsert_affected(&mut dependencies, entry, "dependency", &changed.name);
        }
    }

    let mut dependents: Vec<_> = dependents.into_values().collect();
    let mut dependencies: Vec<_> = dependencies.into_values().collect();
    sort_affected(&mut dependents);
    sort_affected(&mut dependencies);
    Ok((dependents, dependencies))
}

fn upsert_affected(
    map: &mut HashMap<i64, AffectedSymbol>,
    entry: crate::edge::BlastEntry,
    direction: &str,
    changed_name: &str,
) {
    let edge_path: Vec<String> = entry.path.iter().map(|k| k.as_str().to_string()).collect();
    map.entry(entry.symbol_id)
        .and_modify(|existing| {
            if entry.distance < existing.distance {
                existing.distance = entry.distance;
                existing.edge_path = edge_path.clone();
            }
            if !existing
                .via_changed_symbols
                .iter()
                .any(|name| name == changed_name)
            {
                existing.via_changed_symbols.push(changed_name.to_string());
            }
        })
        .or_insert(AffectedSymbol {
            symbol_id: entry.symbol_id,
            name: entry.symbol_name,
            kind: entry.symbol_kind,
            file_path: entry.file_path,
            distance: entry.distance,
            direction: direction.to_string(),
            via_changed_symbols: vec![changed_name.to_string()],
            edge_path,
        });
}

fn sort_affected(items: &mut [AffectedSymbol]) {
    items.sort_by(|a, b| {
        a.distance
            .cmp(&b.distance)
            .then_with(|| a.file_path.cmp(&b.file_path))
            .then_with(|| a.name.cmp(&b.name))
    });
}

fn collect_likely_tests(
    db: &GraphDb,
    changed_symbols: &[ChangedSymbol],
    dependents: &[AffectedSymbol],
    dependencies: &[AffectedSymbol],
) -> Result<Vec<LikelyTest>, String> {
    let mut tests = HashMap::<i64, LikelyTest>::new();
    let mut seed_ids: Vec<i64> = changed_symbols.iter().map(|s| s.symbol_id).collect();
    seed_ids.extend(dependents.iter().map(|s| s.symbol_id));
    seed_ids.extend(dependencies.iter().map(|s| s.symbol_id));
    seed_ids.sort_unstable();
    seed_ids.dedup();

    let raw = bounded_bfs(
        db,
        &seed_ids,
        TraverseDirection::Incoming,
        &[EdgeKind::Tests],
        1,
    );
    for (symbol_id, _, _) in raw {
        if let Some(sym) = db.get_symbol(symbol_id).map_err(|e| e.to_string())? {
            let file_path = db
                .file_path_for_id(sym.file_id)
                .map_err(|e| e.to_string())?
                .unwrap_or_default();
            tests.insert(
                symbol_id,
                LikelyTest {
                    symbol_id: Some(symbol_id),
                    name: sym.name,
                    file_path,
                    reason: "tests edge".to_string(),
                },
            );
        }
    }

    for changed in changed_symbols {
        for test in fallback_tests_for_symbol(db, changed)? {
            if let Some(id) = test.symbol_id {
                tests.entry(id).or_insert(test);
            }
        }
    }

    let mut out: Vec<_> = tests.into_values().collect();
    out.sort_by(|a, b| {
        a.file_path
            .cmp(&b.file_path)
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(out)
}

fn fallback_tests_for_symbol(
    db: &GraphDb,
    changed: &ChangedSymbol,
) -> Result<Vec<LikelyTest>, String> {
    let pattern = format!("%{}%", changed.name);
    let mut stmt = db
        .conn()
        .prepare(
            "SELECT s.id, s.name, f.path
             FROM symbols s
             JOIN files f ON f.id = s.file_id
             WHERE (f.path LIKE '%test%' OR f.path LIKE '%spec%' OR f.path LIKE '%__tests__%')
               AND (s.name LIKE ?1 OR s.source LIKE ?1)
             ORDER BY f.path, s.name
             LIMIT 20",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([pattern], |row| {
            Ok(LikelyTest {
                symbol_id: Some(row.get(0)?),
                name: row.get(1)?,
                file_path: row.get(2)?,
                reason: "test file reference".to_string(),
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

fn classify_risk(
    changed_files: &[ChangedFile],
    changed_symbols: &[ChangedSymbol],
    affected_count: usize,
    warnings: &[String],
) -> RiskLevel {
    let public_api_changed = changed_symbols.iter().any(|s| {
        matches!(
            s.kind.as_str(),
            "class" | "interface" | "struct" | "enum" | "trait" | "function"
        ) && s.confidence != "nearest"
    });
    let incomplete = warnings.iter().any(|w| {
        w.contains("not indexed")
            || w.contains("stale")
            || w.contains("freshness")
            || w.contains("unknown")
    });

    if public_api_changed || affected_count > 50 || changed_files.len() > 10 || incomplete {
        RiskLevel::High
    } else if affected_count >= 10 || changed_symbols.iter().any(|s| s.confidence != "direct") {
        RiskLevel::Medium
    } else {
        RiskLevel::Low
    }
}

fn cap_changed_symbols(mut symbols: Vec<ChangedSymbol>, top: usize) -> Vec<ChangedSymbol> {
    symbols.sort_by(|a, b| {
        a.file_path
            .cmp(&b.file_path)
            .then_with(|| a.line_start.cmp(&b.line_start))
            .then_with(|| a.name.cmp(&b.name))
    });
    symbols.truncate(top);
    symbols
}

fn cap_affected(mut symbols: Vec<AffectedSymbol>, top: usize) -> Vec<AffectedSymbol> {
    sort_affected(&mut symbols);
    symbols.truncate(top);
    symbols
}

fn cap_tests(mut tests: Vec<LikelyTest>, top: usize) -> Vec<LikelyTest> {
    tests.truncate(top);
    tests
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol::SymbolBuilder;

    #[test]
    fn parses_modified_file_hunks() {
        let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
index 111..222 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -10,2 +10,3 @@
@@ -30 +31 @@
";
        let files = parse_diff(diff);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "src/lib.rs");
        assert_eq!(files[0].status, "modified");
        assert_eq!(
            files[0].changed_ranges,
            vec![
                LineRange { start: 10, end: 12 },
                LineRange { start: 31, end: 31 }
            ]
        );
    }

    #[test]
    fn parses_rename_and_deletion_paths() {
        let renamed = "\
diff --git a/src/old.rs b/src/new.rs
similarity index 90%
rename from src/old.rs
rename to src/new.rs
@@ -1 +1 @@
";
        let files = parse_diff(renamed);
        assert_eq!(files[0].status, "renamed");
        assert_eq!(files[0].old_path.as_deref(), Some("src/old.rs"));
        assert_eq!(files[0].path, "src/new.rs");

        let deleted = "\
diff --git a/src/dead.rs b/src/dead.rs
deleted file mode 100644
--- a/src/dead.rs
+++ /dev/null
@@ -1,2 +0,0 @@
";
        let files = parse_diff(deleted);
        assert_eq!(files[0].status, "deleted");
        assert_eq!(files[0].path, "src/dead.rs");
        assert_eq!(
            files[0].changed_ranges,
            vec![LineRange { start: 1, end: 2 }]
        );
    }

    #[test]
    fn filters_graphiq_artifacts() {
        let files = filter_impact_files(vec![
            ChangedFile {
                path: ".graphiq/graphiq.db".to_string(),
                old_path: None,
                status: "modified".to_string(),
                changed_ranges: Vec::new(),
                indexed: false,
            },
            ChangedFile {
                path: "src/lib.rs".to_string(),
                old_path: None,
                status: "modified".to_string(),
                changed_ranges: Vec::new(),
                indexed: false,
            },
        ]);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "src/lib.rs");
    }

    #[test]
    fn maps_line_ranges_to_symbols() {
        let db = GraphDb::open_in_memory().unwrap();
        let fid = db.upsert_file("src/lib.rs", "rust", "abc", 0, 100).unwrap();
        let a = SymbolBuilder::new(
            fid,
            "alpha".to_string(),
            SymbolKind::Function,
            "fn alpha() {}".to_string(),
            "rust".to_string(),
        )
        .lines(5, 12)
        .build();
        let b = SymbolBuilder::new(
            fid,
            "beta".to_string(),
            SymbolKind::Function,
            "fn beta() {}".to_string(),
            "rust".to_string(),
        )
        .lines(20, 30)
        .build();
        db.insert_symbol(&a).unwrap();
        db.insert_symbol(&b).unwrap();

        let options = ImpactOptions {
            project_root: PathBuf::from("."),
            db_path: None,
            source: ChangeSource::WorkingTree,
            depth: 1,
            top: 10,
        };
        let file = ChangedFile {
            path: "src/lib.rs".to_string(),
            old_path: None,
            status: "modified".to_string(),
            changed_ranges: vec![LineRange { start: 22, end: 22 }],
            indexed: false,
        };
        let report = analyze_changed_files(&db, vec![file], options).unwrap();
        assert_eq!(report.changed_symbols.len(), 1);
        assert_eq!(report.changed_symbols[0].name, "beta");
        assert_eq!(report.changed_symbols[0].confidence, "direct");
    }

    #[test]
    fn maps_deleted_file_old_side_ranges_to_all_removed_symbols() {
        let db = GraphDb::open_in_memory().unwrap();
        let fid = db
            .upsert_file("src/deleted.rs", "rust", "abc", 0, 100)
            .unwrap();
        let alpha = SymbolBuilder::new(
            fid,
            "alpha".to_string(),
            SymbolKind::Function,
            "fn alpha() {}".to_string(),
            "rust".to_string(),
        )
        .lines(1, 2)
        .build();
        let beta = SymbolBuilder::new(
            fid,
            "beta".to_string(),
            SymbolKind::Function,
            "fn beta() {}".to_string(),
            "rust".to_string(),
        )
        .lines(5, 6)
        .build();
        db.insert_symbol(&alpha).unwrap();
        db.insert_symbol(&beta).unwrap();

        let diff = "\
diff --git a/src/deleted.rs b/src/deleted.rs
deleted file mode 100644
--- a/src/deleted.rs
+++ /dev/null
@@ -1,6 +0,0 @@
";
        let changed_files = parse_diff(diff);
        let options = ImpactOptions {
            project_root: PathBuf::from("."),
            db_path: None,
            source: ChangeSource::WorkingTree,
            depth: 1,
            top: 10,
        };
        let report = analyze_changed_files(&db, changed_files, options).unwrap();
        let names: Vec<_> = report
            .changed_symbols
            .iter()
            .map(|sym| sym.name.as_str())
            .collect();
        assert_eq!(names, vec!["alpha", "beta"]);
        assert!(report
            .changed_symbols
            .iter()
            .all(|sym| sym.confidence == "direct"));
    }

    #[test]
    fn uses_nearest_symbol_when_hunk_misses_ranges() {
        let db = GraphDb::open_in_memory().unwrap();
        let fid = db.upsert_file("src/lib.rs", "rust", "abc", 0, 100).unwrap();
        let sym = SymbolBuilder::new(
            fid,
            "nearby".to_string(),
            SymbolKind::Function,
            "fn nearby() {}".to_string(),
            "rust".to_string(),
        )
        .lines(20, 30)
        .build();
        db.insert_symbol(&sym).unwrap();

        let options = ImpactOptions {
            project_root: PathBuf::from("."),
            db_path: None,
            source: ChangeSource::WorkingTree,
            depth: 1,
            top: 10,
        };
        let file = ChangedFile {
            path: "src/lib.rs".to_string(),
            old_path: None,
            status: "modified".to_string(),
            changed_ranges: vec![LineRange { start: 15, end: 15 }],
            indexed: false,
        };
        let report = analyze_changed_files(&db, vec![file], options).unwrap();
        assert_eq!(report.changed_symbols[0].name, "nearby");
        assert_eq!(report.changed_symbols[0].confidence, "nearest");
        assert!(report.risk == RiskLevel::High || report.risk == RiskLevel::Medium);
    }

    #[test]
    fn finds_tests_via_edges() {
        let db = GraphDb::open_in_memory().unwrap();
        let fid = db.upsert_file("src/lib.rs", "rust", "abc", 0, 100).unwrap();
        let tfid = db
            .upsert_file("tests/lib_test.rs", "rust", "def", 0, 100)
            .unwrap();
        let target = SymbolBuilder::new(
            fid,
            "target".to_string(),
            SymbolKind::Function,
            "fn target() {}".to_string(),
            "rust".to_string(),
        )
        .lines(1, 5)
        .build();
        let test = SymbolBuilder::new(
            tfid,
            "test_target".to_string(),
            SymbolKind::Function,
            "fn test_target() { target(); }".to_string(),
            "rust".to_string(),
        )
        .lines(1, 5)
        .build();
        let target_id = db.insert_symbol(&target).unwrap();
        let test_id = db.insert_symbol(&test).unwrap();
        db.insert_edge(
            test_id,
            target_id,
            EdgeKind::Tests,
            0.5,
            serde_json::Value::Null,
        )
        .unwrap();

        let changed = vec![ChangedSymbol {
            symbol_id: target_id,
            name: "target".to_string(),
            kind: "function".to_string(),
            file_path: "src/lib.rs".to_string(),
            line_start: 1,
            line_end: 5,
            touched_ranges: Vec::new(),
            confidence: "direct".to_string(),
        }];
        let tests = collect_likely_tests(&db, &changed, &[], &[]).unwrap();
        assert_eq!(tests.len(), 1);
        assert_eq!(tests[0].name, "test_target");
    }
}
