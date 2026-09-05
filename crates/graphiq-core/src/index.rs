//! Codebase indexing pipeline.
//!
//! Walks project files, parses them with Tree-sitter, extracts symbols and
//! edges, computes deep graph edges (type flow, error surfaces, data shapes),
//! infers structural roles and motifs, generates search hints, and builds
//! the CruncherIndex for fast search.
//!
//! Entry point: [`Indexer::index_project`] — walks files, calls `index_files`
//! which parallelizes per-file parsing with rayon.
//!
//! After indexing, call `compute_edge_evidence`, `compute_structural_aliases`,
//! `compute_numeric_bridges`, `compute_deep_graph`, and `generate_search_hints`
//! to populate all derived data.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rayon::prelude::*;

use crate::calls;
use crate::chunker::{LanguageChunker, ParseResult};
use crate::db::GraphDb;
use crate::edge::EdgeKind;
use crate::files::{content_hash, detect_language, is_data_file, walk_project, Language};
use crate::motifs::{detect_motifs, motifs_to_hints, MotifEvidence};
use crate::roles::{infer_roles, roles_to_hints, RoleEvidence};
use crate::symbol::{SymbolBuilder, SymbolKind};

/// Indexes a codebase into the graph database.
///
/// Created per-indexing operation with a reference to the GraphDb.
/// Call [`index_project`] to walk files, extract symbols/edges, and
/// compute all derived artifacts.
pub struct Indexer<'a> {
    db: &'a GraphDb,
    executable_evidence: bool,
}

struct FilePlan {
    existing: Option<crate::symbol::SourceFile>,
    changed: bool,
    evidence_candidate: bool,
}

struct ParsedFile {
    result: ParseResult,
    call_sites: Vec<calls::CallSite>,
    evidence_call_sites: Vec<calls::CallSite>,
}

#[derive(Clone)]
struct SymbolRefInfo {
    name: String,
    file_path: String,
    kind: SymbolKind,
    metadata: serde_json::Value,
}

struct PendingTestEvidence {
    test_id: i64,
    callee_name: String,
    receiver: Option<String>,
    node_text: String,
    line: usize,
    local_name_to_ids: Arc<HashMap<String, Vec<i64>>>,
}

#[derive(Default)]
struct TestEvidenceAggregate {
    calls: Vec<PendingTestEvidenceCall>,
    resolutions: BTreeSet<String>,
}

struct PendingTestEvidenceCall {
    callee_name: String,
    receiver: Option<String>,
    node_text: String,
    line: usize,
}

struct PreservedInboundEdge {
    source_id: i64,
    target_file_id: i64,
    target_name: String,
    target_kind: String,
    kind: EdgeKind,
    weight: f64,
    metadata: serde_json::Value,
}

impl ParsedFile {
    fn empty() -> Self {
        Self {
            result: ParseResult::empty(),
            call_sites: Vec::new(),
            evidence_call_sites: Vec::new(),
        }
    }
}

/// Store project-relative paths with the same slash-separated representation
/// on every platform. This keeps persisted paths, import resolution, search
/// filters, and user-facing results portable between Windows and Unix.
fn stored_relative_path(path: &Path) -> String {
    let path = path.to_string_lossy();
    if cfg!(windows) {
        path.replace('\\', "/")
    } else {
        path.into_owned()
    }
}

impl<'a> Indexer<'a> {
    pub fn new(db: &'a GraphDb) -> Self {
        Self {
            db,
            executable_evidence: false,
        }
    }

    /// Enable the isolated Executable Evidence experiment.
    ///
    /// This records test-to-production `Tests` edges with provenance metadata.
    /// The mode is deliberately opt-in and should use a separate database; the
    /// default indexer remains the speed-only baseline.
    pub fn with_executable_evidence(mut self, enabled: bool) -> Self {
        self.executable_evidence = enabled;
        self
    }

    fn validate_executable_evidence_mode(&self) -> Result<(), Box<dyn std::error::Error>> {
        const META_KEY: &str = "feature.executable_evidence";
        let requested = if self.executable_evidence {
            "enabled"
        } else {
            "disabled"
        };

        if let Some(existing) = self.db.get_meta(META_KEY)? {
            if existing != requested {
                return Err(format!(
                    "index feature mismatch: executable evidence is {existing}, requested {requested}; use a separate --db for the experiment"
                )
                .into());
            }
        } else {
            let stats = self.db.stats()?;
            if self.executable_evidence && (stats.files > 0 || stats.symbols > 0 || stats.edges > 0)
            {
                return Err(
                    "cannot enable executable evidence on an existing index; use a separate --db or --force-reindex"
                        .into(),
                );
            }
            self.db.set_meta(META_KEY, requested)?;
        }
        Ok(())
    }

    fn resolve_import_to_file(
        &self,
        module_path: &str,
        _imported_name: &str,
        _root: &Path,
    ) -> Result<i64, ()> {
        let path_variants = generate_path_variants(module_path);
        for variant in &path_variants {
            if let Ok(Some(f)) = self.db.get_file_by_path(variant) {
                return Ok(f.id);
            }
        }
        Err(())
    }

    /// Index all files in a project directory.
    ///
    /// Walks the project with `walk_project`, parallelizes per-file
    /// symbol/edge extraction with rayon, then computes deep graph
    /// edges, edge evidence, structural aliases, numeric bridges,
    /// and search hints.
    pub fn index_project(&self, root: &Path) -> Result<IndexStats, Box<dyn std::error::Error>> {
        let canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        self.db
            .set_meta("project_root", &canonical.to_string_lossy())?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.db.set_meta("indexed_at", &now.to_string())?;
        let files: Vec<PathBuf> = walk_project(root).collect();
        let stats = self.index_files_with_pruning(root, &files, true)?;
        Ok(stats)
    }

    pub fn index_files(
        &self,
        root: &Path,
        files: &[PathBuf],
    ) -> Result<IndexStats, Box<dyn std::error::Error>> {
        self.index_files_with_pruning(root, files, false)
    }

    fn index_files_with_pruning(
        &self,
        root: &Path,
        files: &[PathBuf],
        prune_missing: bool,
    ) -> Result<IndexStats, Box<dyn std::error::Error>> {
        self.db.begin_bulk_index()?;
        let result = self.index_files_in_transaction(root, files, prune_missing);
        match result {
            Ok(stats) => {
                let commit = if stats.files_indexed == 0 && stats.files_deleted == 0 {
                    self.db.commit_bulk_index_without_fts()
                } else {
                    self.db.commit_bulk_index()
                };
                match commit {
                    Ok(()) => Ok(stats),
                    Err(err) => Err(Box::new(err)),
                }
            }
            Err(err) => {
                let _ = self.db.rollback_bulk_index();
                Err(err)
            }
        }
    }

    fn index_files_in_transaction(
        &self,
        root: &Path,
        files: &[PathBuf],
        prune_missing: bool,
    ) -> Result<IndexStats, Box<dyn std::error::Error>> {
        use std::time::Instant;

        let total_start = Instant::now();
        let mut phase_start = Instant::now();
        let mut stats = IndexStats::default();
        let mut files_deleted = 0usize;

        self.validate_executable_evidence_mode()?;

        if prune_missing {
            let current_paths: HashSet<String> = files
                .iter()
                .filter_map(|path| path.strip_prefix(root).ok())
                .map(stored_relative_path)
                .collect();
            for path in self.db.file_paths()? {
                if !current_paths.contains(&stored_relative_path(Path::new(&path)))
                    && self.db.delete_file(&path)?
                {
                    files_deleted += 1;
                }
            }
            if files_deleted > 0 {
                eprintln!(
                    "  pruned {} file(s) no longer present in project",
                    files_deleted
                );
            }
        }
        stats.files_deleted = files_deleted;

        let file_data: Vec<_> = files
            .par_iter()
            .filter_map(|path| {
                let rel = path.strip_prefix(root).ok()?;
                let content = std::fs::read(path).ok()?;
                let lang = detect_language(path);
                let hash = content_hash(&content);
                let metadata = std::fs::metadata(path).ok()?;
                let mtime = metadata
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);
                let text = String::from_utf8_lossy(&content).to_string();
                let line_count = text.lines().count() as u32;
                Some((rel.to_path_buf(), text, lang, hash, mtime, line_count))
            })
            .collect();
        eprintln!(
            "  phase file scan/read/hash: {} files in {:.2}s",
            file_data.len(),
            phase_start.elapsed().as_secs_f64()
        );
        phase_start = Instant::now();

        let mut global_name_to_ids: HashMap<String, Vec<i64>> = HashMap::new();
        let mut pending_rels: Vec<PendingEdge> = Vec::new();
        let mut pending_contains: Vec<(i64, i64)> = Vec::new();
        let mut preserved_inbound: Vec<PreservedInboundEdge> = Vec::new();
        let mut pending_calls: Vec<PendingCallEdge> = Vec::new();
        let mut pending_imports: Vec<PendingImportEdge> = Vec::new();
        let mut pending_test_evidence: Vec<PendingTestEvidence> = Vec::new();
        let mut test_assertions: HashMap<i64, Vec<calls::CallSite>> = HashMap::new();
        let mut test_symbol_ids: HashSet<i64> = HashSet::new();
        let mut symbol_refs: HashMap<i64, SymbolRefInfo> = HashMap::new();

        // Determine which files need parsing before entering the parallel
        // parse phase. This keeps all SQLite access on the single writer
        // connection while allowing Tree-sitter and call extraction to use
        // Rayon workers.
        let file_plans: Vec<FilePlan> = file_data
            .iter()
            .map(|(rel_path, _, _, hash, _, _)| {
                let path_str = stored_relative_path(rel_path);
                let existing = self.db.get_file_by_path(&path_str)?;
                let changed = existing
                    .as_ref()
                    .map(|f| f.content_hash != *hash)
                    .unwrap_or(true);
                let evidence_candidate = self.executable_evidence
                    && (crate::test_evidence::is_test_path(&path_str)
                        || existing
                            .as_ref()
                            .map(|file| {
                                self.db.symbols_by_file(file.id).map(|symbols| {
                                    symbols.iter().any(|symbol| {
                                        crate::test_evidence::is_test_symbol_name(&symbol.name)
                                    })
                                })
                            })
                            .transpose()?
                            .unwrap_or(false));
                Ok::<FilePlan, crate::db::DbError>(FilePlan {
                    existing,
                    changed,
                    evidence_candidate,
                })
            })
            .collect::<Result<_, _>>()?;

        let executable_evidence = self.executable_evidence;
        let parsed_files: Vec<ParsedFile> = file_data
            .par_iter()
            .zip(file_plans.par_iter())
            .map(|((rel_path, source, lang, _, _, _), plan)| {
                let parse_for_evidence = executable_evidence && plan.evidence_candidate;
                if (!plan.changed && !parse_for_evidence)
                    || is_data_file(rel_path, source.len() as u64)
                {
                    return ParsedFile::empty();
                }

                let path_str = stored_relative_path(rel_path);
                let chunker = get_chunker(*lang);
                let mut result = chunker.parse(source, &path_str);
                // Call extraction only needs the tree during this worker call.
                // Dropping it before returning keeps the collected results
                // compact while preserving the existing call-site behavior.
                let call_sites = result
                    .tree
                    .as_ref()
                    .map(|tree| calls::extract_calls(source, tree, lang.as_str()))
                    .unwrap_or_default();
                let evidence_candidate = executable_evidence
                    && (plan.evidence_candidate
                        || result.symbols.iter().any(|symbol| {
                            symbol
                                .name
                                .as_deref()
                                .is_some_and(crate::test_evidence::is_test_symbol_name)
                        }));
                let evidence_call_sites = if evidence_candidate {
                    result
                        .tree
                        .as_ref()
                        .map(|tree| {
                            calls::extract_calls_with_assertions(source, tree, lang.as_str())
                        })
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                result.tree = None;
                ParsedFile {
                    result,
                    call_sites,
                    evidence_call_sites,
                }
            })
            .collect();

        for (((rel_path, _source, lang, hash, mtime, line_count), plan), parsed_file) in file_data
            .iter()
            .zip(file_plans.iter())
            .zip(parsed_files.iter())
        {
            let path_str = stored_relative_path(rel_path);

            if let Some(ref f) = plan.existing {
                if !plan.changed {
                    let existing_symbols = self.db.symbols_by_file(f.id)?;
                    let mut local_name_to_id = HashMap::new();
                    let mut local_name_to_ids: HashMap<String, Vec<i64>> = HashMap::new();
                    for sym in existing_symbols {
                        local_name_to_id.insert(sym.name.clone(), sym.id);
                        local_name_to_ids
                            .entry(sym.name.clone())
                            .or_default()
                            .push(sym.id);
                        global_name_to_ids
                            .entry(sym.name.clone())
                            .or_default()
                            .push(sym.id);
                        symbol_refs.insert(
                            sym.id,
                            SymbolRefInfo {
                                name: sym.name.clone(),
                                file_path: path_str.to_string(),
                                kind: sym.kind,
                                metadata: sym.metadata.clone(),
                            },
                        );
                        if self.executable_evidence
                            && crate::test_evidence::is_test_symbol(&path_str, &sym.name)
                        {
                            test_symbol_ids.insert(sym.id);
                        }
                    }
                    if self.executable_evidence && plan.evidence_candidate {
                        collect_test_evidence_calls(
                            &parsed_file.evidence_call_sites,
                            &parsed_file.result.symbols,
                            Arc::new(local_name_to_id),
                            Arc::new(local_name_to_ids),
                            &test_symbol_ids,
                            &mut pending_test_evidence,
                            &mut test_assertions,
                        );
                    }
                    continue;
                }
                for (source_id, target_name, target_kind, kind, weight, metadata) in
                    self.db.incoming_edges_for_file(f.id)?
                {
                    if self.executable_evidence
                        && kind == EdgeKind::Tests
                        && crate::test_evidence::is_generated_edge(&metadata)
                    {
                        continue;
                    }
                    preserved_inbound.push(PreservedInboundEdge {
                        source_id,
                        target_file_id: f.id,
                        target_name,
                        target_kind,
                        kind,
                        weight,
                        metadata,
                    });
                }
                self.db.delete_symbols_for_file(f.id)?;
            }

            let file_id =
                self.db
                    .upsert_file(&path_str, lang.as_str(), hash, *mtime, *line_count)?;
            stats.files_indexed += 1;

            // Data files (dependency lockfiles, oversized generated data) are
            // file-tracked for freshness but never symbol-extracted. Extracting
            // every JSON key from a package-lock.json otherwise dominates the
            // graph with thousands of low-value Constant symbols and silently
            // pollutes search results and the codebase briefing.
            let result = &parsed_file.result;

            let mut file_name_to_id: HashMap<String, i64> = HashMap::new();
            let mut file_name_to_ids: HashMap<String, Vec<i64>> = HashMap::new();
            let mut containers: Vec<(String, SymbolKind, i64)> = Vec::new();

            for sym in &result.symbols {
                let sym_name = sym.name.clone().unwrap_or_default();
                let sb = SymbolBuilder::new(
                    file_id,
                    sym_name.clone(),
                    sym.kind,
                    sym.source.clone(),
                    lang.as_str().to_string(),
                )
                .lines(sym.line_start as u32, sym.line_end as u32);

                let sb = if let Some(ref sig) = sym.signature {
                    sb.signature(sig)
                } else {
                    sb
                };

                let built = sb
                    .visibility(sym.visibility)
                    .metadata(sym.metadata.clone())
                    .build();

                if let Ok(id) = self.db.insert_symbol(&built) {
                    stats.symbols_indexed += 1;
                    file_name_to_id.insert(sym_name.clone(), id);
                    file_name_to_ids
                        .entry(sym_name.clone())
                        .or_default()
                        .push(id);
                    global_name_to_ids
                        .entry(sym_name.clone())
                        .or_default()
                        .push(id);
                    symbol_refs.insert(
                        id,
                        SymbolRefInfo {
                            name: sym_name.clone(),
                            file_path: path_str.to_string(),
                            kind: sym.kind,
                            metadata: sym.metadata.clone(),
                        },
                    );
                    if self.executable_evidence
                        && crate::test_evidence::is_test_symbol(&path_str, &sym_name)
                    {
                        test_symbol_ids.insert(id);
                    }

                    if is_container_kind(sym.kind) {
                        containers.push((sym_name.clone(), sym.kind, id));
                    }

                    let is_member = sym
                        .metadata
                        .get("class_member")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    if is_member {
                        if let Some((_, _, container_id)) = containers.last() {
                            pending_contains.push((*container_id, id));
                        }
                    }
                }
            }

            // Calls, imports, and structural relations all use the same
            // per-file lookup. Sharing it avoids cloning the complete map for
            // every extracted relation/call/import.
            let file_name_to_id = Arc::new(file_name_to_id);
            let file_name_to_ids = Arc::new(file_name_to_ids);

            for rel in &result.structural_rels {
                let edge_kind = match rel.rel_type.as_str() {
                    "implements" => Some(EdgeKind::Implements),
                    "extends" => Some(EdgeKind::Extends),
                    "overrides" => Some(EdgeKind::Overrides),
                    "contains" => Some(EdgeKind::Contains),
                    _ => None,
                };
                if let Some(kind) = edge_kind {
                    pending_rels.push(PendingEdge {
                        source_name: rel.source_name.clone(),
                        target_name: rel.target_name.clone(),
                        edge_kind: kind,
                        file_scope: Some(Arc::clone(&file_name_to_id)),
                    });
                }
            }

            for cs in &parsed_file.call_sites {
                let caller_id = find_enclosing_symbol(&file_name_to_id, &result.symbols, cs.line);
                pending_calls.push(PendingCallEdge {
                    caller_id,
                    callee_name: cs.callee.clone(),
                    file_name_to_id: Arc::clone(&file_name_to_id),
                });
            }

            if self.executable_evidence {
                collect_test_evidence_calls(
                    &parsed_file.evidence_call_sites,
                    &parsed_file.result.symbols,
                    Arc::clone(&file_name_to_id),
                    Arc::clone(&file_name_to_ids),
                    &test_symbol_ids,
                    &mut pending_test_evidence,
                    &mut test_assertions,
                );
            }

            for imp in &result.imports {
                for name in &imp.names {
                    pending_imports.push(PendingImportEdge {
                        importer_file_id: file_id,
                        importer_names: Arc::clone(&file_name_to_id),
                        imported_name: name.clone(),
                        module_path: imp.module_path.clone(),
                    });
                }
            }

            stats.imports_extracted += result.imports.len();
            stats.rels_extracted += result.structural_rels.len();
        }

        // Reconnect inbound structural edges whose target symbols survived a
        // file replacement. Deleting and reinserting a changed file otherwise
        // removes valid edges from unchanged callers/importers.
        for edge in preserved_inbound {
            if !is_relinkable_edge_kind(edge.kind) || self.db.get_symbol(edge.source_id)?.is_none()
            {
                continue;
            }
            let Some(target_id) = self.db.symbol_id_by_file_name_kind(
                edge.target_file_id,
                &edge.target_name,
                &edge.target_kind,
            )?
            else {
                continue;
            };
            if edge.source_id != target_id {
                if self
                    .db
                    .insert_edge(
                        edge.source_id,
                        target_id,
                        edge.kind,
                        edge.weight,
                        edge.metadata,
                    )
                    .is_ok()
                {
                    stats.edges_inserted += 1;
                }
            }
        }
        eprintln!(
            "  phase parse/symbol extraction: {} changed files, {} symbols in {:.2}s",
            stats.files_indexed,
            stats.symbols_indexed,
            phase_start.elapsed().as_secs_f64()
        );
        // With no changed files, the existing graph and derived artifacts are
        // already valid. Avoid rerunning every derived pass on every no-op
        // filesystem scan.
        if stats.files_indexed == 0 && stats.files_deleted == 0 {
            eprintln!("  phase derived indexing: skipped (no changed files)");
            eprintln!(
                "  total index pipeline: {:.2}s",
                total_start.elapsed().as_secs_f64()
            );
            return Ok(stats);
        }

        // Executable evidence is a derived relation. When the experiment is
        // enabled, rebuild all of its observations whenever the project graph
        // changes so newly resolvable calls and removed calls cannot leave
        // stale evidence behind. This work is completely outside the default
        // speed-only indexing path.
        if self.executable_evidence {
            let removed = self.db.delete_edges_with_metadata_source(
                EdgeKind::Tests,
                crate::test_evidence::EDGE_SOURCE,
            )?;
            if removed > 0 {
                eprintln!("  executable evidence: removed {} stale edges", removed);
            }
        }

        phase_start = Instant::now();

        for (container_id, member_id) in &pending_contains {
            let _ = self.db.insert_edge(
                *container_id,
                *member_id,
                EdgeKind::Contains,
                EdgeKind::Contains.path_weight(),
                serde_json::Value::Null,
            );
            stats.edges_inserted += 1;
        }

        for rel in &pending_rels {
            let source_id = rel
                .file_scope
                .as_ref()
                .and_then(|m| m.get(&rel.source_name))
                .copied()
                .or_else(|| resolve_symbol(&global_name_to_ids, &rel.source_name));
            let target_id = resolve_symbol(&global_name_to_ids, &rel.target_name);

            if let (Some(sid), Some(tid)) = (source_id, target_id) {
                let _ = self.db.insert_edge(
                    sid,
                    tid,
                    rel.edge_kind,
                    rel.edge_kind.path_weight(),
                    serde_json::Value::Null,
                );
                stats.edges_inserted += 1;
            }
        }

        let mut call_edges_inserted = 0;
        for pc in &pending_calls {
            let target_id = pc
                .file_name_to_id
                .get(&pc.callee_name)
                .copied()
                .or_else(|| resolve_symbol(&global_name_to_ids, &pc.callee_name));
            if let (Some(caller_id), Some(tid)) = (pc.caller_id, target_id) {
                if caller_id != tid {
                    let _ = self.db.insert_edge(
                        caller_id,
                        tid,
                        EdgeKind::Calls,
                        EdgeKind::Calls.path_weight(),
                        serde_json::Value::Null,
                    );
                    call_edges_inserted += 1;
                }
            }
        }
        stats.edges_inserted += call_edges_inserted;
        stats.calls_extracted = pending_calls.len();

        let mut evidence_by_pair: BTreeMap<(i64, i64), TestEvidenceAggregate> = BTreeMap::new();
        for pending in pending_test_evidence {
            let Some((target_id, resolution)) = resolve_test_target(
                &pending.callee_name,
                pending.receiver.as_deref(),
                &pending.local_name_to_ids,
                &global_name_to_ids,
                &symbol_refs,
            ) else {
                continue;
            };

            if pending.test_id == target_id {
                continue;
            }

            let evidence = evidence_by_pair
                .entry((pending.test_id, target_id))
                .or_default();
            evidence.calls.push(PendingTestEvidenceCall {
                callee_name: pending.callee_name,
                receiver: pending.receiver,
                node_text: pending.node_text,
                line: pending.line,
            });
            evidence.resolutions.insert(resolution.to_string());
        }

        let mut executable_evidence_edges = 0usize;
        for ((test_id, target_id), evidence) in evidence_by_pair {
            let Some(test_info) = symbol_refs.get(&test_id) else {
                continue;
            };
            let metadata =
                test_evidence_metadata(test_info, &evidence, test_assertions.get(&test_id));
            self.db.insert_edge(
                test_id,
                target_id,
                EdgeKind::Tests,
                EdgeKind::Tests.path_weight(),
                metadata,
            )?;
            executable_evidence_edges += 1;
        }
        stats.executable_evidence_edges = executable_evidence_edges;
        if self.executable_evidence {
            eprintln!(
                "  phase executable evidence: {} test edges",
                executable_evidence_edges
            );
        }

        let mut import_edges_inserted = 0;
        for pi in &pending_imports {
            let target_id = resolve_symbol(&global_name_to_ids, &pi.imported_name);
            if let Some(tid) = target_id {
                let importer_ids: Vec<i64> = pi
                    .importer_names
                    .iter()
                    .filter_map(|(name, &id)| {
                        if name == &pi.imported_name {
                            Some(id)
                        } else {
                            None
                        }
                    })
                    .collect();

                if let Some(&imp_id) = importer_ids.first() {
                    if imp_id != tid {
                        let _ = self.db.insert_edge(
                            imp_id,
                            tid,
                            EdgeKind::Imports,
                            EdgeKind::Imports.path_weight(),
                            serde_json::json!({ "module": pi.module_path }),
                        );
                        import_edges_inserted += 1;
                    }
                } else {
                    for (_, &imp_id) in pi.importer_names.iter().take(1) {
                        if imp_id != tid {
                            let _ = self.db.insert_edge(
                                imp_id,
                                tid,
                                EdgeKind::References,
                                EdgeKind::References.path_weight(),
                                serde_json::json!({ "module": pi.module_path, "via": "import" }),
                            );
                            import_edges_inserted += 1;
                        }
                        break;
                    }
                }
            }

            if let Ok(target_file) =
                self.resolve_import_to_file(&pi.module_path, &pi.imported_name, root)
            {
                let _ = self
                    .db
                    .insert_file_edge(pi.importer_file_id, target_file, "imports");
            }
        }
        stats.edges_inserted += import_edges_inserted;
        eprintln!(
            "  phase primary edge writes: {} edges in {:.2}s",
            stats.edges_inserted,
            phase_start.elapsed().as_secs_f64()
        );
        phase_start = Instant::now();

        let importance_scores = self.db.compute_importance_scores()?;
        for (symbol_id, importance) in &importance_scores {
            let _ = self.db.update_importance(*symbol_id, *importance);
        }
        eprintln!(
            "  phase importance scores: {} symbols in {:.2}s",
            importance_scores.len(),
            phase_start.elapsed().as_secs_f64()
        );
        phase_start = Instant::now();

        self.compute_edge_evidence()?;
        eprintln!(
            "  phase edge evidence: {:.2}s",
            phase_start.elapsed().as_secs_f64()
        );
        phase_start = Instant::now();

        self.generate_search_hints()?;
        eprintln!(
            "  phase search hints: {:.2}s",
            phase_start.elapsed().as_secs_f64()
        );
        phase_start = Instant::now();

        self.compute_structural_aliases()?;
        eprintln!(
            "  phase structural aliases: {:.2}s",
            phase_start.elapsed().as_secs_f64()
        );
        phase_start = Instant::now();

        self.compute_numeric_bridges()?;
        eprintln!(
            "  phase numeric bridges: {:.2}s",
            phase_start.elapsed().as_secs_f64()
        );
        phase_start = Instant::now();

        self.compute_deep_graph()?;
        eprintln!(
            "  phase deep/source graph: {:.2}s",
            phase_start.elapsed().as_secs_f64()
        );
        phase_start = Instant::now();

        self.build_neighbor_hints()?;
        eprintln!(
            "  phase neighbor hints: {:.2}s",
            phase_start.elapsed().as_secs_f64()
        );
        eprintln!(
            "  total index pipeline: {:.2}s",
            total_start.elapsed().as_secs_f64()
        );

        Ok(stats)
    }

    fn compute_numeric_bridges(&self) -> Result<(), Box<dyn std::error::Error>> {
        use crate::numeric_bridges::compute_numeric_bridges;

        let stats = compute_numeric_bridges(self.db).map_err(|e| e)?;
        eprintln!(
            "  numeric bridges: {} literals, {} constants, {} edges",
            stats.literals_found, stats.constants_found, stats.bridge_edges_created
        );
        Ok(())
    }

    fn compute_deep_graph(&self) -> Result<(), Box<dyn std::error::Error>> {
        use crate::deep_graph::{compute_deep_graph_edges, compute_source_graph_edges};

        let stats = compute_deep_graph_edges(self.db)?;
        eprintln!(
            "  deep graph: {} type-flow, {} error-type, {} data-shape edges",
            stats.type_flow_edges, stats.error_type_edges, stats.data_shape_edges
        );
        let src_stats = compute_source_graph_edges(self.db)?;
        eprintln!(
            "  source graph: {} string-literal, {} comment-ref edges",
            src_stats.string_literal_edges, src_stats.comment_ref_edges
        );
        Ok(())
    }

    fn compute_edge_evidence(&self) -> Result<(), Box<dyn std::error::Error>> {
        use crate::edge_evidence::{infer_edge_evidence, write_edge_evidence};

        let evidence = infer_edge_evidence(self.db).map_err(|e| e)?;
        let updated = write_edge_evidence(self.db, &evidence).map_err(|e| e)?;
        eprintln!("  inferred evidence for {} edges", updated);
        Ok(())
    }

    fn compute_structural_aliases(&self) -> Result<(), Box<dyn std::error::Error>> {
        use crate::structural_alias::compute_structural_aliases;

        let stats = compute_structural_aliases(self.db)?;
        eprintln!(
            "  structural aliases: {} collision sets, {} symbols aliased",
            stats.collision_sets, stats.symbols_aliased
        );
        Ok(())
    }

    fn generate_search_hints(&self) -> Result<(), Box<dyn std::error::Error>> {
        use std::collections::HashMap;

        let mut out_by_id: HashMap<i64, Vec<(String, String)>> = HashMap::new();
        for (source_id, kind, target_name) in self.db.outgoing_edges_grouped()? {
            out_by_id
                .entry(source_id)
                .or_default()
                .push((kind, target_name));
        }

        let mut in_by_id: HashMap<i64, Vec<(String, String)>> = HashMap::new();
        for (target_id, kind, source_name) in self.db.incoming_edges_grouped()? {
            in_by_id
                .entry(target_id)
                .or_default()
                .push((kind, source_name));
        }

        let conn = self.db.conn();
        let mut stmt = conn
            .prepare("SELECT id, name, name_decomposed, kind, doc_comment, file_id, signature, source FROM symbols")?;
        let symbols: Vec<(
            i64,
            String,
            String,
            String,
            Option<String>,
            i64,
            Option<String>,
            Option<String>,
        )> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                ))
            })?
            .flatten()
            .collect();

        let name_to_decomposed: HashMap<String, String> = symbols
            .iter()
            .map(|(_, name, decomposed, _, _, _, _, _)| (name.clone(), decomposed.clone()))
            .collect();

        let name_to_id: HashMap<String, i64> = symbols
            .iter()
            .map(|(id, name, _, _, _, _, _, _)| (name.clone(), *id))
            .collect();

        for (id, _name, name_decomposed, kind_str, doc_comment, file_id, signature, source) in
            &symbols
        {
            let mut hints = Vec::new();

            hints.push(name_decomposed.clone());

            let stemmed_decomposed = crate::tokenize::stem_text(name_decomposed);
            if stemmed_decomposed != *name_decomposed {
                hints.push(stemmed_decomposed);
            }

            let morph_hints: Vec<String> = name_decomposed
                .split_whitespace()
                .filter_map(|w| morphological_variants(w))
                .collect();
            if !morph_hints.is_empty() {
                hints.push(morph_hints.join(" "));
            }

            if let Some(ref doc) = doc_comment {
                if !doc.is_empty() {
                    let cleaned = doc.lines().take(3).collect::<Vec<_>>().join(" ");
                    hints.push(cleaned);
                }
            }

            let mut caller_concepts: Vec<String> = Vec::new();
            let mut callee_concepts: Vec<String> = Vec::new();
            if let Some(outgoing) = out_by_id.get(id) {
                for (kind, target_name) in outgoing.iter().take(8) {
                    hints.push(format_edge_role(kind, target_name, true));
                    if kind == "calls" {
                        if let Some(decomp) = name_to_decomposed.get(target_name) {
                            callee_concepts.push(decomp.clone());
                        }
                    }
                }
            }
            if let Some(incoming) = in_by_id.get(id) {
                for (kind, source_name) in incoming.iter().take(8) {
                    hints.push(format_edge_role(kind, source_name, false));
                    if kind == "calls" {
                        if let Some(decomp) = name_to_decomposed.get(source_name) {
                            caller_concepts.push(decomp.clone());
                        }
                    }
                }
            }

            if !caller_concepts.is_empty() && !callee_concepts.is_empty() {
                let callers = caller_concepts
                    .iter()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ");
                let callees = callee_concepts
                    .iter()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ");
                hints.push(format!("connects {} to {}", callers, callees));
            } else if !callee_concepts.is_empty() {
                let callees = callee_concepts
                    .iter()
                    .take(4)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ");
                hints.push(format!("uses {}", callees));
            } else if !caller_concepts.is_empty() {
                let callers = caller_concepts
                    .iter()
                    .take(4)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ");
                hints.push(format!("used by {}", callers));
            }

            let mut hop2_word_count: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();
            if let Some(incoming) = in_by_id.get(id) {
                for (edge_kind, source_name) in incoming.iter().take(8) {
                    if let Some(decomp) = name_to_decomposed.get(source_name) {
                        if edge_kind == "references" || edge_kind == "calls" {
                            for word in decomp.split_whitespace() {
                                let wl = word.to_lowercase();
                                if wl.len() >= 4 {
                                    *hop2_word_count.entry(wl).or_insert(0) += 1;
                                }
                            }
                        }
                    }
                    if let Some(&caller_id) = name_to_id.get(source_name) {
                        if let Some(caller_out) = out_by_id.get(&caller_id) {
                            for (_, callee_name) in caller_out.iter().take(4) {
                                if callee_name != _name {
                                    if let Some(decomp) = name_to_decomposed.get(callee_name) {
                                        for word in decomp.split_whitespace() {
                                            let wl = word.to_lowercase();
                                            if wl.len() >= 4 {
                                                *hop2_word_count.entry(wl).or_insert(0) += 1;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            let hop2_consensus: Vec<String> = hop2_word_count
                .into_iter()
                .filter(|(_, count)| *count >= 2)
                .map(|(word, _)| word)
                .take(8)
                .collect();
            if !hop2_consensus.is_empty() {
                hints.push(format!("related {}", hop2_consensus.join(", ")));
            }

            if let Ok(Some((container_id, container_name))) = self.db.container_for(*id) {
                hints.push(format!("member of {}", container_name));
                if let Some(decomp) = name_to_decomposed.get(&container_name) {
                    hints.push(format!("part of {}", decomp));
                }
                if let Some(container_out) = out_by_id.get(&container_id) {
                    for (_, target_name) in container_out.iter().take(4) {
                        if let Some(decomp) = name_to_decomposed.get(target_name) {
                            hints.push(format!("via {} {}", container_name, decomp));
                        }
                    }
                }
            }

            let file_path = get_file_path(self.db, *file_id);
            if let Some(ref fp) = file_path {
                let file_role = infer_file_role(fp);
                if let Some(role) = file_role {
                    hints.push(role);
                }
                if let Some(module_name) = extract_module_name(fp) {
                    hints.push(format!("module {}", module_name));
                }
            }

            let kind_hints = kind_to_hint(kind_str);
            if let Some(kh) = kind_hints {
                hints.push(kh);
            }

            let role_evidence = RoleEvidence {
                name: _name.clone(),
                name_decomposed: name_decomposed.clone(),
                file_path: file_path.clone(),
                callee_names: out_by_id
                    .get(id)
                    .map(|v| {
                        v.iter()
                            .filter(|(k, _)| k == "calls")
                            .map(|(_, n)| n.clone())
                            .collect()
                    })
                    .unwrap_or_default(),
                caller_names: in_by_id
                    .get(id)
                    .map(|v| {
                        v.iter()
                            .filter(|(k, _)| k == "calls")
                            .map(|(_, n)| n.clone())
                            .collect()
                    })
                    .unwrap_or_default(),
                outgoing_edge_kinds: out_by_id
                    .get(id)
                    .map(|v| v.iter().map(|(k, _)| k.clone()).collect())
                    .unwrap_or_default(),
                container_name: self.db.container_for(*id).ok().flatten().map(|(_, n)| n),
                signature: signature.clone(),
                source_text: source.clone(),
            };
            let symbol_roles = infer_roles(&role_evidence);
            if !symbol_roles.is_empty() {
                hints.push(roles_to_hints(&symbol_roles));
            }

            let motif_evidence = MotifEvidence {
                has_call_in: in_by_id
                    .get(id)
                    .map_or(false, |v| v.iter().any(|(k, _)| k == "calls")),
                has_call_out: out_by_id
                    .get(id)
                    .map_or(false, |v| v.iter().any(|(k, _)| k == "calls")),
                call_in_count: in_by_id
                    .get(id)
                    .map_or(0, |v| v.iter().filter(|(k, _)| k == "calls").count()),
                call_out_count: out_by_id
                    .get(id)
                    .map_or(0, |v| v.iter().filter(|(k, _)| k == "calls").count()),
                has_contains_out: out_by_id
                    .get(id)
                    .map_or(false, |v| v.iter().any(|(k, _)| k == "contains")),
                contains_count: out_by_id
                    .get(id)
                    .map_or(0, |v| v.iter().filter(|(k, _)| k == "contains").count()),
                has_implements_out: out_by_id
                    .get(id)
                    .map_or(false, |v| v.iter().any(|(k, _)| k == "implements")),
                has_extends_out: out_by_id
                    .get(id)
                    .map_or(false, |v| v.iter().any(|(k, _)| k == "extends")),
                has_imports_in: in_by_id
                    .get(id)
                    .map_or(false, |v| v.iter().any(|(k, _)| k == "imports")),
                imports_in_count: in_by_id
                    .get(id)
                    .map_or(0, |v| v.iter().filter(|(k, _)| k == "imports").count()),
                has_tests_in: in_by_id
                    .get(id)
                    .map_or(false, |v| v.iter().any(|(k, _)| k == "tests")),
                is_container: is_container_kind_str(kind_str),
            };
            let symbol_motifs = detect_motifs(&motif_evidence);
            if !symbol_motifs.is_empty() {
                hints.push(motifs_to_hints(&symbol_motifs));
            }

            let source_terms = extract_source_terms(self.db, *id);
            if !source_terms.is_empty() {
                hints.push(source_terms);
            }

            let sig_str = signature.as_deref().unwrap_or("");
            let src_str = source.as_deref().unwrap_or("");
            let sig_type_hints = extract_signature_type_hints(sig_str, src_str);
            if !sig_type_hints.is_empty() {
                hints.push(sig_type_hints);
            }

            let callee_name_list: Vec<String> = out_by_id
                .get(id)
                .map(|v| {
                    v.iter()
                        .filter(|(k, _)| k == "calls")
                        .map(|(_, n)| n.clone())
                        .take(10)
                        .collect()
                })
                .unwrap_or_default();
            let caller_name_list: Vec<String> = in_by_id
                .get(id)
                .map(|v| v.iter().map(|(_, n)| n.clone()).take(10).collect())
                .unwrap_or_default();

            let desc = crate::behavioral::generate_behavioral_descriptors(
                _name,
                kind_str,
                signature.as_deref(),
                source.as_deref(),
                &callee_name_list,
                &caller_name_list,
                file_path.as_deref(),
            );
            if !desc.phrases.is_empty() {
                hints.push(desc.phrases.join(". "));
            }

            let hint_text = hints.join(". ");
            let _ = self.db.update_search_hints(*id, &hint_text);
        }

        Ok(())
    }

    fn build_neighbor_hints(&self) -> Result<(), Box<dyn std::error::Error>> {
        let conn = self.db.conn();

        let total_symbols: i64 =
            conn.query_row("SELECT COUNT(*) FROM symbols", [], |row| row.get(0))?;

        let mut symbols: Vec<(i64, String, String, Option<String>)> = Vec::new();
        {
            let mut stmt = conn
                .prepare("SELECT s.id, s.name, s.name_decomposed, s.search_hints FROM symbols s")?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?;
            for row in rows {
                symbols.push(row?);
            }
        }

        let mut name_to_decomposed: HashMap<String, String> = HashMap::new();
        for (_, name, decomposed, _) in &symbols {
            name_to_decomposed.insert(name.clone(), decomposed.clone());
        }

        let mut name_to_id: HashMap<String, i64> = HashMap::new();
        for (id, name, _, _) in &symbols {
            name_to_id.insert(name.clone(), *id);
        }

        let mut out_by_id: HashMap<i64, Vec<(String, String)>> = HashMap::new();
        for (source_id, kind, target_name) in self.db.outgoing_edges_grouped()? {
            out_by_id
                .entry(source_id)
                .or_default()
                .push((kind, target_name));
        }

        let mut in_by_id: HashMap<i64, Vec<(String, String)>> = HashMap::new();
        for (target_id, kind, source_name) in self.db.incoming_edges_grouped()? {
            in_by_id
                .entry(target_id)
                .or_default()
                .push((kind, source_name));
        }

        let mut file_groups: HashMap<i64, Vec<(i64, String, String)>> = HashMap::new();
        {
            let mut stmt =
                conn.prepare("SELECT s.id, s.name, s.name_decomposed, s.file_id FROM symbols s")?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })?;
            for row in rows {
                let (id, name, decomp, file_id) = row?;
                file_groups
                    .entry(file_id)
                    .or_default()
                    .push((id, name, decomp));
            }
        }

        let mut symbol_file_ids: HashMap<i64, i64> = HashMap::new();
        {
            let mut stmt = conn.prepare("SELECT id, file_id FROM symbols")?;
            let rows =
                stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?;
            for row in rows {
                let (id, fid) = row?;
                symbol_file_ids.insert(id, fid);
            }
        }

        const GENERIC: &[&str] = &[
            "get",
            "set",
            "push",
            "pop",
            "remove",
            "add",
            "delete",
            "update",
            "create",
            "new",
            "init",
            "start",
            "stop",
            "run",
            "execute",
            "process",
            "handle",
            "parse",
            "format",
            "to_string",
            "from",
            "into",
            "default",
            "clone",
            "eq",
            "drop",
            "send",
            "sync",
            "copy",
            "main",
            "test",
            "iter",
            "next",
            "len",
            "is_empty",
            "as_ref",
            "as_mut",
            "deref",
            "index",
            "call",
            "read",
            "write",
            "open",
            "close",
            "check",
            "make",
            "build",
            "apply",
            "return",
            "result",
            "value",
            "data",
            "self",
            "some",
            "none",
            "true",
            "false",
            "ok",
            "err",
            "poll",
            "task",
            "spawn",
            "block",
            "async",
            "await",
            "future",
        ];

        let mut global_term_doc_freq: HashMap<String, usize> = HashMap::new();
        for (_, _, decomposed, _) in &symbols {
            let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
            for word in decomposed.split_whitespace() {
                let wl = word.to_lowercase();
                if wl.len() >= 3 && !GENERIC.contains(&wl.as_str()) {
                    if seen.insert(wl.clone()) {
                        *global_term_doc_freq.entry(wl).or_insert(0) += 1;
                    }
                }
            }
        }

        let df_threshold = (total_symbols as f64 * 0.25) as usize;

        let mut total_updated = 0usize;

        for (id, _name, name_decomposed, existing_hints) in &symbols {
            let mut neighbor_terms: HashMap<String, usize> = HashMap::new();
            let own_terms: std::collections::HashSet<String> = name_decomposed
                .split_whitespace()
                .map(|w| w.to_lowercase())
                .collect();

            if let Some(outgoing) = out_by_id.get(id) {
                for (kind, target_name) in outgoing {
                    if kind != "calls" && kind != "references" && kind != "tests" {
                        continue;
                    }
                    if let Some(decomp) = name_to_decomposed.get(target_name) {
                        for word in decomp.split_whitespace() {
                            let wl = word.to_lowercase();
                            if wl.len() >= 3
                                && !own_terms.contains(&wl)
                                && !GENERIC.contains(&wl.as_str())
                            {
                                *neighbor_terms.entry(wl).or_insert(0) += 1;
                            }
                        }
                    }
                }
            }

            if let Some(incoming) = in_by_id.get(id) {
                for (kind, source_name) in incoming {
                    if kind != "calls" && kind != "references" && kind != "tests" {
                        continue;
                    }
                    if let Some(decomp) = name_to_decomposed.get(source_name) {
                        for word in decomp.split_whitespace() {
                            let wl = word.to_lowercase();
                            if wl.len() >= 3
                                && !own_terms.contains(&wl)
                                && !GENERIC.contains(&wl.as_str())
                            {
                                *neighbor_terms.entry(wl).or_insert(0) += 1;
                            }
                        }
                    }
                }
            }

            if let Some(&fid) = symbol_file_ids.get(id) {
                if let Some(siblings) = file_groups.get(&fid) {
                    for (sib_id, sib_name, sib_decomp) in siblings {
                        if *sib_id == *id {
                            continue;
                        }
                        if let Some(decomp) = name_to_decomposed.get(sib_name) {
                            for word in decomp.split_whitespace() {
                                let wl = word.to_lowercase();
                                if wl.len() >= 3
                                    && !own_terms.contains(&wl)
                                    && !GENERIC.contains(&wl.as_str())
                                {
                                    *neighbor_terms.entry(wl).or_insert(0) += 1;
                                }
                            }
                        }
                        for word in sib_decomp.split_whitespace() {
                            let wl = word.to_lowercase();
                            if wl.len() >= 3
                                && !own_terms.contains(&wl)
                                && !GENERIC.contains(&wl.as_str())
                            {
                                *neighbor_terms.entry(wl).or_insert(0) += 1;
                            }
                        }
                    }
                }
            }

            let mut distinctive: Vec<(String, usize)> = neighbor_terms
                .into_iter()
                .filter(|(term, count)| {
                    *count >= 2
                        && !own_terms.contains(term.as_str())
                        && global_term_doc_freq.get(term).copied().unwrap_or(0) < df_threshold
                })
                .collect();

            distinctive.sort_by(|a, b| b.1.cmp(&a.1));
            distinctive.truncate(20);

            if distinctive.is_empty() {
                continue;
            }

            let neighbor_hint = format!(
                "neighbor {}",
                distinctive
                    .iter()
                    .map(|(t, _)| t.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            );

            let updated_hints = match existing_hints {
                Some(h) if !h.is_empty() => format!("{}. {}", h, neighbor_hint),
                _ => neighbor_hint,
            };

            self.db.update_search_hints(*id, &updated_hints)?;
            total_updated += 1;
        }

        eprintln!("  neighbor hints: {} symbols enriched", total_updated);
        Ok(())
    }

    #[cfg(feature = "embed")]
    pub fn embed_symbols(
        &self,
        cache_dir: Option<std::path::PathBuf>,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        use crate::embed::{build_symbol_text, Embedder};
        use std::time::Instant;

        let embedder = Embedder::new(cache_dir)?;
        let conn = self.db.conn();

        let total_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM symbols", [], |row| row.get(0))
            .unwrap_or(0);

        let mut stmt = conn.prepare(
            "SELECT s.id, s.name, s.signature, s.doc_comment, s.source, f.path
             FROM symbols s
             JOIN files f ON s.file_id = f.id
             WHERE s.visibility = 'public'
               AND s.importance > 0.15
               AND s.id NOT IN (SELECT symbol_id FROM symbol_embeddings)",
        )?;

        let rows: Vec<(
            i64,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            String,
        )> = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })?
            .flatten()
            .filter(|r| !is_embed_test_path(&r.5))
            .collect();

        let to_embed = rows.len();
        if to_embed == 0 {
            eprintln!("  all {total_count} symbols already embedded, nothing to do");
            return Ok(0);
        }
        eprintln!(
            "  embedding {to_embed}/{total_count} symbols (filtered: public, importance>0.15, non-test, not yet embedded) ..."
        );

        let start = Instant::now();
        let mut embedded = 0;

        let batch_size = 32;
        for chunk in rows.chunks(batch_size) {
            let texts: Vec<String> = chunk
                .iter()
                .map(|(_, name, sig, doc, src, _)| build_symbol_text(name, sig, doc, src))
                .collect();
            let results = embedder.embed_batch(&texts);
            for ((id, _, _, _, _, _), result) in chunk.iter().zip(results.into_iter()) {
                if let Ok(vec) = result {
                    let _ = self
                        .db
                        .put_embedding(*id, &vec, "nomic-ai/nomic-embed-text-v1.5");
                    embedded += 1;
                }
            }
            if embedded > 0 {
                let elapsed = start.elapsed().as_secs_f64();
                let rate_ms = elapsed / embedded as f64 * 1000.0;
                let remaining = (elapsed / embedded as f64) * (to_embed - embedded) as f64;
                eprintln!(
                    "  {embedded}/{to_embed} ({rate_ms:.0}ms/ea, ~{remaining:.0}s remaining)",
                );
            }
        }
        Ok(embedded)
    }
}

#[cfg(feature = "embed")]
fn is_embed_test_path(path: &str) -> bool {
    let lower = path.to_lowercase();
    let patterns = [
        "/test",
        "/tests/",
        "/__tests__/",
        "/spec/",
        "_test.",
        "_spec.",
        ".test.",
        ".spec.",
        "test_",
        "/benches/",
        "/benchmark/",
        "/fixtures/",
        "/mocks/",
    ];
    patterns.iter().any(|p| lower.contains(p))
}

fn collect_test_evidence_calls(
    call_sites: &[calls::CallSite],
    symbols: &[crate::chunker::ParsedSymbol],
    local_name_to_id: Arc<HashMap<String, i64>>,
    local_name_to_ids: Arc<HashMap<String, Vec<i64>>>,
    test_symbol_ids: &HashSet<i64>,
    pending: &mut Vec<PendingTestEvidence>,
    assertions: &mut HashMap<i64, Vec<calls::CallSite>>,
) {
    for call in call_sites {
        let Some(caller_id) = find_enclosing_symbol(&local_name_to_id, symbols, call.line) else {
            continue;
        };
        if !test_symbol_ids.contains(&caller_id) {
            continue;
        }

        if crate::test_evidence::is_assertion_call(call) {
            assertions.entry(caller_id).or_default().push(call.clone());
        } else {
            pending.push(PendingTestEvidence {
                test_id: caller_id,
                callee_name: call.callee.clone(),
                receiver: call.receiver.clone(),
                node_text: call.node_text.clone(),
                line: call.line,
                local_name_to_ids: Arc::clone(&local_name_to_ids),
            });
        }
    }
}

fn is_callable_test_target(info: &SymbolRefInfo) -> bool {
    !crate::test_evidence::is_test_symbol(&info.file_path, &info.name)
        && !matches!(
            info.kind,
            SymbolKind::Import
                | SymbolKind::Export
                | SymbolKind::Section
                | SymbolKind::Field
                | SymbolKind::Constant
                | SymbolKind::TypeAlias
                | SymbolKind::Module
                | SymbolKind::Namespace
        )
}

fn receiver_matches(info: &SymbolRefInfo, receiver: &str) -> bool {
    let wanted = receiver
        .trim()
        .trim_start_matches('&')
        .trim_start_matches('*')
        .to_lowercase();
    if wanted.is_empty() {
        return false;
    }

    let Some(declared) = info
        .metadata
        .get("receiver")
        .and_then(serde_json::Value::as_str)
    else {
        return false;
    };

    declared
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|token| !token.is_empty())
        .any(|token| token.eq_ignore_ascii_case(&wanted))
}

fn resolve_test_target(
    callee_name: &str,
    receiver: Option<&str>,
    local_name_to_ids: &HashMap<String, Vec<i64>>,
    global_name_to_ids: &HashMap<String, Vec<i64>>,
    symbol_refs: &HashMap<i64, SymbolRefInfo>,
) -> Option<(i64, &'static str)> {
    if callee_name.is_empty() {
        return None;
    }

    let callable = |id: &i64| {
        symbol_refs
            .get(id)
            .map(is_callable_test_target)
            .unwrap_or(false)
    };

    let local: Vec<i64> = local_name_to_ids
        .get(callee_name)
        .into_iter()
        .flatten()
        .filter(|id| callable(id))
        .copied()
        .collect();

    let global: Vec<i64> = global_name_to_ids
        .get(callee_name)
        .into_iter()
        .flatten()
        .filter(|id| callable(id))
        .copied()
        .collect();

    if let Some(receiver) = receiver {
        // A receiver-qualified call is only evidence when the receiver can be
        // matched to exactly one declared method. Falling back to a unique
        // name would silently attach calls such as `client.Close()` to an
        // unrelated `Close` implementation from another package.
        let local_matches: Vec<i64> = local
            .iter()
            .filter(|id| {
                symbol_refs
                    .get(id)
                    .map(|info| receiver_matches(info, receiver))
                    .unwrap_or(false)
            })
            .copied()
            .collect();
        if local_matches.len() == 1 {
            return Some((local_matches[0], "same_file_receiver"));
        }
        if local_matches.len() > 1 {
            return None;
        }

        let global_matches: Vec<i64> = global
            .iter()
            .filter(|id| {
                symbol_refs
                    .get(id)
                    .map(|info| receiver_matches(info, receiver))
                    .unwrap_or(false)
            })
            .copied()
            .collect();
        if global_matches.len() == 1 {
            return Some((global_matches[0], "global_receiver"));
        }
        return None;
    }

    if local.len() == 1 {
        return Some((local[0], "same_file"));
    }
    if !local.is_empty() {
        return None;
    }
    if global.len() == 1 {
        return Some((global[0], "unique_global"));
    }

    None
}

fn short_evidence_text(text: &str) -> String {
    const MAX_CHARS: usize = 240;
    let mut shortened: String = text.chars().take(MAX_CHARS).collect();
    if text.chars().count() > MAX_CHARS {
        shortened.push('…');
    }
    shortened
}

fn test_evidence_metadata(
    test_info: &SymbolRefInfo,
    evidence: &TestEvidenceAggregate,
    assertions: Option<&Vec<calls::CallSite>>,
) -> serde_json::Value {
    let mut calls: Vec<_> = evidence.calls.iter().collect();
    calls.sort_by(|a, b| {
        a.line
            .cmp(&b.line)
            .then_with(|| a.callee_name.cmp(&b.callee_name))
            .then_with(|| a.node_text.cmp(&b.node_text))
    });
    let call_records: Vec<serde_json::Value> = calls
        .iter()
        .take(32)
        .map(|call| {
            serde_json::json!({
                "callee": call.callee_name,
                "receiver": call.receiver,
                "line": call.line + 1,
                "source": short_evidence_text(&call.node_text),
            })
        })
        .collect();

    let assertion_count = assertions.map_or(0, |calls| calls.len());
    let mut assertion_calls: Vec<&calls::CallSite> = assertions.into_iter().flatten().collect();
    assertion_calls.sort_by(|a, b| {
        a.line
            .cmp(&b.line)
            .then_with(|| a.callee.cmp(&b.callee))
            .then_with(|| a.node_text.cmp(&b.node_text))
    });
    let assertion_records: Vec<serde_json::Value> = assertion_calls
        .into_iter()
        .take(32)
        .map(|assertion| {
            serde_json::json!({
                "callee": assertion.callee,
                "receiver": assertion.receiver,
                "line": assertion.line + 1,
                "source": short_evidence_text(&assertion.node_text),
            })
        })
        .collect();

    serde_json::json!({
        "source": crate::test_evidence::EDGE_SOURCE,
        "version": 1,
        "test_file": test_info.file_path,
        "test_symbol": test_info.name,
        "resolution": evidence.resolutions.iter().collect::<Vec<_>>(),
        "call_count": evidence.calls.len(),
        "calls": call_records,
        "assertion_count": assertion_count,
        "assertions": assertion_records,
    })
}

fn resolve_symbol(name_map: &HashMap<String, Vec<i64>>, name: &str) -> Option<i64> {
    name_map.get(name).and_then(|ids| ids.first().copied())
}

fn find_enclosing_symbol(
    file_name_to_id: &HashMap<String, i64>,
    symbols: &[crate::chunker::ParsedSymbol],
    line: usize,
) -> Option<i64> {
    let mut best: Option<(&str, i64, usize)> = None;
    for sym in symbols {
        if let Some(ref name) = sym.name {
            if line >= sym.line_start && line <= sym.line_end {
                let span = sym.line_end - sym.line_start;
                let is_better = best
                    .as_ref()
                    .map_or(true, |(_, _, best_span)| span < *best_span);
                if is_better {
                    if let Some(&id) = file_name_to_id.get(name) {
                        best = Some((name.as_str(), id, span));
                    }
                }
            }
        }
    }
    best.map(|(_, id, _)| id)
}

fn is_container_kind(kind: crate::symbol::SymbolKind) -> bool {
    matches!(
        kind,
        crate::symbol::SymbolKind::Class
            | crate::symbol::SymbolKind::Struct
            | crate::symbol::SymbolKind::Interface
            | crate::symbol::SymbolKind::Trait
            | crate::symbol::SymbolKind::Enum
            | crate::symbol::SymbolKind::Module
            | crate::symbol::SymbolKind::Namespace
    )
}

fn is_relinkable_edge_kind(kind: EdgeKind) -> bool {
    !matches!(
        kind,
        EdgeKind::SharesConstant
            | EdgeKind::ReferencesConstant
            | EdgeKind::SharesType
            | EdgeKind::SharesErrorType
            | EdgeKind::SharesDataShape
    )
}

fn is_container_kind_str(kind: &str) -> bool {
    matches!(
        kind,
        "class" | "struct" | "interface" | "trait" | "enum" | "module" | "namespace"
    )
}

struct PendingEdge {
    source_name: String,
    target_name: String,
    edge_kind: EdgeKind,
    file_scope: Option<Arc<HashMap<String, i64>>>,
}

struct PendingCallEdge {
    caller_id: Option<i64>,
    callee_name: String,
    file_name_to_id: Arc<HashMap<String, i64>>,
}

struct PendingImportEdge {
    importer_file_id: i64,
    importer_names: Arc<HashMap<String, i64>>,
    imported_name: String,
    module_path: String,
}

fn get_chunker(lang: Language) -> Box<dyn LanguageChunker> {
    match lang {
        Language::TypeScript | Language::JavaScript | Language::JSX => {
            Box::new(crate::languages::typescript::TypeScriptChunker::new())
        }
        Language::TSX => Box::new(crate::languages::typescript::TypeScriptChunker::tsx()),
        Language::Rust => Box::new(crate::languages::rust::RustChunker::new()),
        Language::Python => Box::new(crate::languages::python::PythonChunker::new()),
        Language::Go => Box::new(crate::languages::go::GoChunker::new()),
        Language::Java => Box::new(crate::languages::java::JavaChunker::new()),
        Language::C => Box::new(crate::languages::c::CChunker::new()),
        Language::Cpp => Box::new(crate::languages::cpp::CppChunker::new()),
        Language::Ruby => Box::new(crate::languages::ruby::RubyChunker::new()),
        Language::Json => Box::new(crate::languages::json::JsonChunker::new()),
        Language::Yaml => Box::new(crate::languages::yaml::YamlChunker::new()),
        Language::Toml => Box::new(crate::languages::toml::TomlChunker::new()),
        Language::Html => Box::new(crate::languages::html::HtmlChunker::new()),
        Language::Css | Language::Scss => Box::new(crate::languages::css::CssChunker::new()),
        _ => Box::new(crate::languages::rust::RustChunker::new()),
    }
}

#[derive(Debug, Default)]
pub struct IndexStats {
    pub files_indexed: usize,
    pub files_deleted: usize,
    pub symbols_indexed: usize,
    pub imports_extracted: usize,
    pub rels_extracted: usize,
    pub calls_extracted: usize,
    pub edges_inserted: usize,
    pub executable_evidence_edges: usize,
}

fn generate_path_variants(module_path: &str) -> Vec<String> {
    let mut variants = Vec::new();
    let normalized = module_path
        .replace('.', "/")
        .replace("::", "/")
        .replace('\\', "/");

    for ext in &[
        "", ".ts", ".tsx", ".js", ".jsx", ".rs", ".py", ".go", ".java",
    ] {
        variants.push(format!("{}{}", normalized, ext));
        variants.push(format!("src/{}{}", normalized, ext));
        variants.push(format!("lib/{}{}", normalized, ext));
    }

    let parts: Vec<&str> = normalized.split('/').collect();
    if parts.len() > 1 {
        let file_name = parts.last().unwrap();
        for ext in &[
            "", ".ts", ".tsx", ".js", ".jsx", ".rs", ".py", ".go", ".java",
        ] {
            variants.push(format!("{}{}", file_name, ext));
        }
    }

    variants
}

fn format_edge_role(edge_kind: &str, other_name: &str, is_outgoing: bool) -> String {
    match edge_kind {
        "calls" => {
            if is_outgoing {
                format!("calls {}", other_name)
            } else {
                format!("called by {}", other_name)
            }
        }
        "imports" => {
            if is_outgoing {
                format!("imports {}", other_name)
            } else {
                format!("imported by {}", other_name)
            }
        }
        "contains" => {
            if is_outgoing {
                format!("contains {}", other_name)
            } else {
                format!("contained in {}", other_name)
            }
        }
        "implements" => {
            if is_outgoing {
                format!("implements {}", other_name)
            } else {
                format!("implemented by {}", other_name)
            }
        }
        "extends" => {
            if is_outgoing {
                format!("extends {}", other_name)
            } else {
                format!("extended by {}", other_name)
            }
        }
        "references" => {
            if is_outgoing {
                format!("references {}", other_name)
            } else {
                format!("referenced by {}", other_name)
            }
        }
        "tests" => {
            if is_outgoing {
                format!("tests {}", other_name)
            } else {
                format!("tested in {}", other_name)
            }
        }
        "overrides" => {
            if is_outgoing {
                format!("overrides {}", other_name)
            } else {
                format!("overridden by {}", other_name)
            }
        }
        "shares_constant" => {
            if is_outgoing {
                format!("shares constant with {}", other_name)
            } else {
                format!("shares constant with {}", other_name)
            }
        }
        "references_constant" => {
            if is_outgoing {
                format!("uses constant {}", other_name)
            } else {
                format!("constant used by {}", other_name)
            }
        }
        "shares_type" => {
            if is_outgoing {
                format!("shares type with {}", other_name)
            } else {
                format!("shares type with {}", other_name)
            }
        }
        "shares_error_type" => {
            if is_outgoing {
                format!("handles same error as {}", other_name)
            } else {
                format!("handles same error as {}", other_name)
            }
        }
        "shares_data_shape" => {
            if is_outgoing {
                format!("accesses same fields as {}", other_name)
            } else {
                format!("accesses same fields as {}", other_name)
            }
        }
        _ => String::new(),
    }
}

fn kind_to_hint(kind_str: &str) -> Option<String> {
    match kind_str {
        "function" => Some("function".into()),
        "method" => Some("method".into()),
        "class" => Some("class".into()),
        "struct" => Some("struct".into()),
        "interface" => Some("interface".into()),
        "trait" => Some("trait definition".into()),
        "enum" => Some("enum".into()),
        "module" => Some("module".into()),
        _ => None,
    }
}

fn get_file_path(db: &GraphDb, file_id: i64) -> Option<String> {
    let conn = db.conn();
    conn.query_row(
        "SELECT path FROM files WHERE id = ?1",
        rusqlite::params![file_id],
        |row| row.get(0),
    )
    .ok()
}

fn infer_file_role(path: &str) -> Option<String> {
    let lower = path.to_lowercase();
    if lower.contains("/test") || lower.contains("/tests/") || lower.contains("_test.") {
        Some("test file".into())
    } else if lower.ends_with("/main.rs")
        || lower.ends_with("/main.ts")
        || lower.ends_with("/index.ts")
        || lower.ends_with("/index.js")
    {
        Some("entry point".into())
    } else if lower.contains("/src/lib") || lower.contains("/mod.rs") {
        Some("library module".into())
    } else if lower.contains("/cli/") || lower.contains("cli.rs") || lower.contains("cli.ts") {
        Some("cli command".into())
    } else if lower.contains("/bench/") || lower.contains("bench.rs") {
        Some("benchmark".into())
    } else {
        None
    }
}

fn extract_module_name(path: &str) -> Option<String> {
    let path = path.trim_start_matches("./");
    let stem = path.rsplit('/').next()?;
    let name = stem.rsplit_once('.').map(|(n, _)| n).unwrap_or(stem);
    if name.is_empty() {
        return None;
    }
    Some(crate::tokenize::decompose_identifier(name))
}

fn morphological_variants(word: &str) -> Option<String> {
    let variants: Vec<&str> = match word {
        "expand" => vec!["expansion", "expanding"],
        "expander" => vec!["expansion", "expand"],
        "parse" => vec!["parser", "parsers", "parsing"],
        "parser" => vec!["parse", "parsers", "parsing"],
        "chunk" => vec!["chunker", "chunking"],
        "chunker" => vec!["chunk", "parser", "parsing"],
        "search" => vec!["searching", "searcher"],
        "index" => vec!["indexer", "indexing", "indices"],
        "indexer" => vec!["index", "indexing"],
        "rank" => vec!["ranking", "rerank", "reranking"],
        "rerank" => vec!["ranking", "rank", "reranking"],
        "token" => vec!["tokenizer", "tokenize", "tokenizing"],
        "tokenize" => vec!["tokenizer", "token", "tokenizing"],
        "cache" => vec!["caching", "cached"],
        "blast" => vec!["blasting", "explosion"],
        "embed" => vec!["embedding", "embeddings"],
        "graph" => vec!["graphs", "graphing"],
        "traverse" => vec!["traversal", "traversing"],
        "decompose" => vec!["decomposition", "decomposing"],
        _ => return None,
    };
    Some(variants.join(" "))
}

fn extract_signature_type_hints(signature: &str, source: &str) -> String {
    let mut terms = Vec::new();
    let mut seen = std::collections::HashSet::new();

    let add = |terms: &mut Vec<String>, seen: &mut std::collections::HashSet<String>, s: &str| {
        let lower = s.to_lowercase();
        if lower.len() >= 3 && seen.insert(lower.clone()) {
            let decomp = crate::tokenize::decompose_identifier(&lower);
            terms.push(decomp);
        }
    };

    if source.contains("async fn")
        || source.contains("async function")
        || source.contains("Promise<")
    {
        for &t in &["async", "await"] {
            add(&mut terms, &mut seen, t);
        }
    }

    let return_type = extract_return_type(signature);
    if !return_type.is_empty() {
        let decomp = crate::tokenize::decompose_identifier(&return_type);
        for word in decomp.split_whitespace() {
            if word.len() >= 3 {
                add(&mut terms, &mut seen, word);
            }
        }
    }

    for param_type in extract_param_types(signature) {
        let decomp = crate::tokenize::decompose_identifier(&param_type);
        for word in decomp.split_whitespace() {
            if word.len() >= 3 {
                add(&mut terms, &mut seen, word);
            }
        }
    }

    terms.join(" ")
}

fn extract_return_type(signature: &str) -> String {
    if let Some(pos) = signature.rfind("->") {
        let after = &signature[pos + 2..].trim();
        let end = after
            .find(|c: char| c == '{' || c == '(' || c == ';' || c == '\n')
            .unwrap_or(after.len());
        let ty = after[..end].trim().trim_end_matches('+');
        let clean = ty.trim_start_matches("impl ").trim_start_matches("dyn ");
        let name_part = clean.split('<').next().unwrap_or("").trim();
        let name_part = name_part.split('+').next().unwrap_or("").trim();
        if !name_part.is_empty() && name_part.len() < 60 {
            return name_part.to_string();
        }
    }
    if let Some(pos) = signature.rfind(':') {
        let after = &signature[pos + 1..].trim();
        let end = after
            .find(|c: char| c == ',' || c == ')' || c == '{' || c == ';')
            .unwrap_or(after.len());
        let ty = after[..end].trim();
        if !ty.is_empty() && ty.len() < 40 {
            return ty.to_string();
        }
    }
    String::new()
}

fn extract_param_types(signature: &str) -> Vec<String> {
    let mut types = Vec::new();
    let mut depth = 0;
    let mut start = None;

    for (i, c) in signature.char_indices() {
        match c {
            '(' if depth == 0 => {
                start = Some(i + 1);
                depth = 1;
            }
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    if let Some(s) = start {
                        let params = &signature[s..i];
                        for param in params.split(',') {
                            let param = param.trim();
                            if let Some(colon_pos) = param.find(':') {
                                let ty = param[colon_pos + 1..].trim();
                                let ty = ty.trim_start_matches("impl ").trim_start_matches("dyn ");
                                let name_part = ty.split('<').next().unwrap_or("").trim();
                                if !name_part.is_empty() && name_part.len() < 40 {
                                    types.push(name_part.to_string());
                                }
                            }
                        }
                    }
                    break;
                }
            }
            _ => {}
        }
    }
    types
}

fn extract_source_terms(db: &GraphDb, symbol_id: i64) -> String {
    let sym = match db.get_symbol(symbol_id) {
        Ok(Some(s)) => s,
        _ => return String::new(),
    };

    let mut seen = std::collections::HashSet::new();
    let mut terms = Vec::new();

    let names = [
        sym.name.clone(),
        sym.signature.unwrap_or_default(),
        sym.doc_comment.unwrap_or_default(),
        sym.source.clone(),
    ];
    let combined = names.join(" ");

    for word in combined.split(|c: char| !c.is_alphanumeric() && c != '_') {
        if word.len() < 3 {
            continue;
        }
        let lower = word.to_lowercase();
        let stop = [
            "the", "for", "and", "not", "has", "all", "new", "let", "mut", "pub", "use", "self",
            "true", "false", "none", "some", "from", "into", "with", "return", "match", "where",
            "impl", "fn", "mod", "ref", "box", "move",
        ];
        if stop.contains(&lower.as_str()) {
            continue;
        }
        if seen.insert(lower.clone()) {
            let decomp = crate::tokenize::decompose_identifier(&lower);
            terms.push(decomp);
        }
        if terms.len() >= 40 {
            break;
        }
    }

    terms.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_index_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let db = GraphDb::open_in_memory().unwrap();
        let indexer = Indexer::new(&db);
        let stats = indexer.index_project(tmp.path()).unwrap();
        assert_eq!(stats.files_indexed, 0);
    }

    #[test]
    fn test_noop_reindex_preserves_existing_fts() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("main.rs"),
            "pub fn searchable_symbol() -> i32 { 1 }\n",
        )
        .unwrap();

        let db = GraphDb::open_in_memory().unwrap();
        let indexer = Indexer::new(&db);
        indexer.index_project(tmp.path()).unwrap();
        let before: i64 = db
            .conn()
            .query_row("SELECT count(*) FROM symbols_fts", [], |row| row.get(0))
            .unwrap();

        let stats = indexer.index_project(tmp.path()).unwrap();
        let after: i64 = db
            .conn()
            .query_row("SELECT count(*) FROM symbols_fts", [], |row| row.get(0))
            .unwrap();
        assert_eq!(stats.files_indexed, 0);
        assert_eq!(stats.symbols_indexed, 0);
        assert_eq!(before, after);
    }

    #[test]
    fn test_reindex_prunes_deleted_files() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("keep.rs"), "pub fn keep() {}\n").unwrap();
        let deleted = tmp.path().join("deleted.rs");
        std::fs::write(&deleted, "pub fn deleted() {}\n").unwrap();

        let db = GraphDb::open_in_memory().unwrap();
        let indexer = Indexer::new(&db);
        indexer.index_project(tmp.path()).unwrap();
        assert!(db.get_file_by_path("deleted.rs").unwrap().is_some());

        std::fs::remove_file(deleted).unwrap();
        let stats = indexer.index_project(tmp.path()).unwrap();

        assert_eq!(stats.files_deleted, 1);
        assert!(db.get_file_by_path("deleted.rs").unwrap().is_none());
        assert!(db.symbols_by_name("deleted").unwrap().is_empty());
    }

    #[test]
    fn test_reindex_changed_target_preserves_inbound_edges() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("target.ts");
        std::fs::write(&target, "export function target() { return 'old'; }\n").unwrap();
        std::fs::write(
            tmp.path().join("caller.ts"),
            "import { target } from './target';\nexport function caller() { return target(); }\n",
        )
        .unwrap();

        let db = GraphDb::open_in_memory().unwrap();
        let indexer = Indexer::new(&db);
        indexer.index_project(tmp.path()).unwrap();
        assert_eq!(db.edges_by_kind(EdgeKind::Calls, 100).unwrap().len(), 1);

        std::fs::write(&target, "export function target() { return 'new'; }\n").unwrap();
        indexer.index_project(tmp.path()).unwrap();

        assert_eq!(db.edges_by_kind(EdgeKind::Calls, 100).unwrap().len(), 1);
        assert_eq!(
            db.edges_by_kind(EdgeKind::References, 100).unwrap().len(),
            1
        );
    }

    #[test]
    fn test_index_rust_file() {
        let tmp = TempDir::new().unwrap();
        let rust_file = tmp.path().join("main.rs");
        std::fs::write(
            &rust_file,
            r#"
use std::io;

fn main() {
    println!("hello");
}

pub struct Config {
    name: String,
}
"#,
        )
        .unwrap();

        let db = GraphDb::open_in_memory().unwrap();
        let indexer = Indexer::new(&db);
        let stats = indexer.index_project(tmp.path()).unwrap();

        assert_eq!(stats.files_indexed, 1);
        assert!(stats.symbols_indexed >= 3);
        assert!(stats.imports_extracted >= 1);

        let sym_count = db.symbol_count().unwrap();
        assert!(sym_count >= 3);
    }

    #[test]
    fn test_index_creates_contains_edges() {
        let tmp = TempDir::new().unwrap();
        let ts_file = tmp.path().join("service.ts");
        std::fs::write(
            &ts_file,
            r#"
class AuthService {
    constructor(secret: string) {}
    authenticate(token: string): boolean {
        return true;
    }
}
"#,
        )
        .unwrap();

        let db = GraphDb::open_in_memory().unwrap();
        let indexer = Indexer::new(&db);
        let stats = indexer.index_project(tmp.path()).unwrap();

        assert!(stats.edges_inserted > 0);
        let edge_count = db.edge_count().unwrap();
        assert!(edge_count > 0);

        let auth_class = db.symbols_by_name("AuthService").unwrap();
        assert!(!auth_class.is_empty());
        let class_id = auth_class[0].id;
        let edges = db.edges_from(class_id).unwrap();
        let contains_edges: Vec<_> = edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Contains)
            .collect();
        assert!(contains_edges.len() >= 2);
    }

    #[test]
    fn test_index_creates_calls_edges() {
        let tmp = TempDir::new().unwrap();
        let ts_file = tmp.path().join("app.ts");
        std::fs::write(
            &ts_file,
            r#"
function greet(name: string): string {
    return formatName(name);
}

function formatName(name: string): string {
    return name.toUpperCase();
}
"#,
        )
        .unwrap();

        let db = GraphDb::open_in_memory().unwrap();
        let indexer = Indexer::new(&db);
        let stats = indexer.index_project(tmp.path()).unwrap();

        assert!(stats.calls_extracted > 0, "should extract call sites");

        let all_edges: Vec<_> = db
            .edges_by_kind(EdgeKind::Calls, 100)
            .unwrap()
            .into_iter()
            .collect();
        assert!(!all_edges.is_empty(), "should have Calls edges");

        let has_greet_to_format = all_edges.iter().any(|e| {
            let source = db.get_symbol(e.source_id).unwrap().unwrap();
            let target = db.get_symbol(e.target_id).unwrap().unwrap();
            source.name == "greet" && target.name == "formatName"
        });
        assert!(has_greet_to_format, "greet should call formatName");
    }

    #[test]
    fn test_executable_evidence_is_opt_in_and_records_observations() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(
            tmp.path().join("src/service.ts"),
            "export function produce(): boolean { return true; }\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("src/service.test.ts"),
            "function testService(): void { const value = produce(); expect(value).toBe(true); }\n",
        )
        .unwrap();

        let baseline_db = GraphDb::open_in_memory().unwrap();
        Indexer::new(&baseline_db)
            .index_project(tmp.path())
            .unwrap();
        assert!(baseline_db
            .edges_by_kind(EdgeKind::Tests, 100)
            .unwrap()
            .is_empty());

        let evidence_db = GraphDb::open_in_memory().unwrap();
        let stats = Indexer::new(&evidence_db)
            .with_executable_evidence(true)
            .index_project(tmp.path())
            .unwrap();

        assert_eq!(stats.executable_evidence_edges, 1);
        let edges = evidence_db.edges_by_kind(EdgeKind::Tests, 100).unwrap();
        assert_eq!(edges.len(), 1);
        let source = evidence_db.get_symbol(edges[0].source_id).unwrap().unwrap();
        let target = evidence_db.get_symbol(edges[0].target_id).unwrap().unwrap();
        assert_eq!(source.name, "testService");
        assert_eq!(target.name, "produce");
        assert_eq!(
            edges[0].metadata.get("source").and_then(|v| v.as_str()),
            Some(crate::test_evidence::EDGE_SOURCE)
        );
        assert_eq!(
            edges[0].metadata.get("test_file").and_then(|v| v.as_str()),
            Some("src/service.test.ts")
        );
        assert_eq!(
            edges[0]
                .metadata
                .get("resolution")
                .and_then(|v| v.as_array())
                .and_then(|v| v.first())
                .and_then(|v| v.as_str()),
            Some("unique_global")
        );
        assert_eq!(
            edges[0].metadata.get("call_count").and_then(|v| v.as_u64()),
            Some(1)
        );
        assert_eq!(
            edges[0]
                .metadata
                .get("assertion_count")
                .and_then(|v| v.as_u64()),
            Some(1)
        );
    }

    #[test]
    fn test_executable_evidence_does_not_guess_ambiguous_targets() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(
            tmp.path().join("src/one.ts"),
            "export function produce(): boolean { return true; }\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("src/two.ts"),
            "export function produce(): boolean { return false; }\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("src/service.test.ts"),
            "function testService(): void { produce(); }\n",
        )
        .unwrap();

        let db = GraphDb::open_in_memory().unwrap();
        Indexer::new(&db)
            .with_executable_evidence(true)
            .index_project(tmp.path())
            .unwrap();

        assert!(db.edges_by_kind(EdgeKind::Tests, 100).unwrap().is_empty());
    }

    #[test]
    fn test_executable_evidence_uses_receiver_to_disambiguate_methods() {
        let mut refs = HashMap::new();
        refs.insert(
            1,
            SymbolRefInfo {
                name: "Close".into(),
                file_path: "src/client.go".into(),
                kind: SymbolKind::Method,
                metadata: serde_json::json!({"receiver": "(c *Client)"}),
            },
        );
        refs.insert(
            2,
            SymbolRefInfo {
                name: "Close".into(),
                file_path: "src/server.go".into(),
                kind: SymbolKind::Method,
                metadata: serde_json::json!({"receiver": "(s *Server)"}),
            },
        );
        let mut global = HashMap::new();
        global.insert("Close".into(), vec![1, 2]);

        assert_eq!(
            resolve_test_target("Close", Some("client"), &HashMap::new(), &global, &refs),
            Some((1, "global_receiver"))
        );
        assert_eq!(
            resolve_test_target("Close", Some("unknown"), &HashMap::new(), &global, &refs),
            None
        );
    }

    #[test]
    fn test_executable_evidence_aggregates_repeated_calls() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("service.ts"),
            "function produce(): boolean { return true; }\nfunction TestProduce(): void { produce(); produce(); assert(produce()); }\n",
        )
        .unwrap();

        let db = GraphDb::open_in_memory().unwrap();
        Indexer::new(&db)
            .with_executable_evidence(true)
            .index_project(tmp.path())
            .unwrap();

        let edges = db.edges_by_kind(EdgeKind::Tests, 100).unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(
            edges[0].metadata.get("call_count").and_then(|v| v.as_u64()),
            Some(3)
        );
        assert_eq!(
            edges[0]
                .metadata
                .get("assertion_count")
                .and_then(|v| v.as_u64()),
            Some(1)
        );
    }

    #[test]
    fn test_executable_evidence_does_not_add_assertion_call_edges() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("service.ts"),
            "function assert(): void {}\nfunction produce(): boolean { return true; }\nfunction TestProduce(): void { assert(); produce(); }\n",
        )
        .unwrap();

        let baseline_db = GraphDb::open_in_memory().unwrap();
        Indexer::new(&baseline_db)
            .index_project(tmp.path())
            .unwrap();
        let evidence_db = GraphDb::open_in_memory().unwrap();
        Indexer::new(&evidence_db)
            .with_executable_evidence(true)
            .index_project(tmp.path())
            .unwrap();

        let call_names = |db: &GraphDb| {
            let mut names = db
                .edges_by_kind(EdgeKind::Calls, 100)
                .unwrap()
                .into_iter()
                .map(|edge| db.get_symbol(edge.target_id).unwrap().unwrap().name)
                .collect::<Vec<_>>();
            names.sort_unstable();
            names
        };
        assert_eq!(call_names(&baseline_db), call_names(&evidence_db));
    }

    #[test]
    fn test_executable_evidence_rebuilds_when_resolution_becomes_ambiguous() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("service.ts"),
            "export function produce(): boolean { return true; }\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("service.test.ts"),
            "function testService(): void { produce(); }\n",
        )
        .unwrap();

        let db = GraphDb::open_in_memory().unwrap();
        let indexer = Indexer::new(&db).with_executable_evidence(true);
        indexer.index_project(tmp.path()).unwrap();
        assert_eq!(db.edges_by_kind(EdgeKind::Tests, 100).unwrap().len(), 1);

        std::fs::write(
            tmp.path().join("other.ts"),
            "export function produce(): boolean { return false; }\n",
        )
        .unwrap();
        indexer.index_project(tmp.path()).unwrap();
        assert!(db.edges_by_kind(EdgeKind::Tests, 100).unwrap().is_empty());
    }

    #[test]
    fn test_executable_evidence_rebuilds_after_target_change() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("service.ts");
        std::fs::write(
            &target,
            "export function produce(): boolean { return true; }\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("service.test.ts"),
            "function testService(): void { produce(); }\n",
        )
        .unwrap();

        let db = GraphDb::open_in_memory().unwrap();
        let indexer = Indexer::new(&db).with_executable_evidence(true);
        indexer.index_project(tmp.path()).unwrap();
        let first = db.edges_by_kind(EdgeKind::Tests, 100).unwrap();
        assert_eq!(first.len(), 1);

        std::fs::write(
            &target,
            "export function produce(): boolean { return false; }\n",
        )
        .unwrap();
        indexer.index_project(tmp.path()).unwrap();

        let second = db.edges_by_kind(EdgeKind::Tests, 100).unwrap();
        assert_eq!(second.len(), 1);
        let second_target = db.get_symbol(second[0].target_id).unwrap().unwrap();
        assert_eq!(second_target.name, "produce");
        assert!(second_target.source.contains("return false"));

        std::fs::write(
            tmp.path().join("service.test.ts"),
            "function testService(): void { expect(true).toBe(true); }\n",
        )
        .unwrap();
        indexer.index_project(tmp.path()).unwrap();
        assert!(db.edges_by_kind(EdgeKind::Tests, 100).unwrap().is_empty());
    }

    #[test]
    fn test_executable_evidence_requires_a_separate_index_mode() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("service.ts"), "function service() {}\n").unwrap();

        let db = GraphDb::open_in_memory().unwrap();
        Indexer::new(&db).index_project(tmp.path()).unwrap();
        let error = Indexer::new(&db)
            .with_executable_evidence(true)
            .index_project(tmp.path())
            .unwrap_err()
            .to_string();
        assert!(error.contains("separate --db"));

        let evidence_db = GraphDb::open_in_memory().unwrap();
        Indexer::new(&evidence_db)
            .with_executable_evidence(true)
            .index_project(tmp.path())
            .unwrap();
        let error = Indexer::new(&evidence_db)
            .index_project(tmp.path())
            .unwrap_err()
            .to_string();
        assert!(error.contains("separate --db"));
    }

    #[test]
    fn test_index_creates_import_edges() {
        let tmp = TempDir::new().unwrap();
        let util_file = tmp.path().join("utils.ts");
        std::fs::write(
            &util_file,
            r#"
export function helper(): void {}
export class Util {}
"#,
        )
        .unwrap();

        let app_file = tmp.path().join("app.ts");
        std::fs::write(
            &app_file,
            r#"
import { helper, Util } from './utils';

function run(): void {
    helper();
}
"#,
        )
        .unwrap();

        let db = GraphDb::open_in_memory().unwrap();
        let indexer = Indexer::new(&db);
        let stats = indexer.index_project(tmp.path()).unwrap();

        assert!(stats.imports_extracted > 0, "should extract imports");

        let all_import_edges = db.edges_by_kind(EdgeKind::Imports, 100).unwrap();
        let all_ref_edges = db.edges_by_kind(EdgeKind::References, 100).unwrap();
        assert!(
            !all_import_edges.is_empty() || !all_ref_edges.is_empty(),
            "should have import/reference edges"
        );
    }

    #[test]
    fn test_index_creates_file_edges() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("utils.ts"), "export function helper() {}\n").unwrap();
        std::fs::write(
            tmp.path().join("app.ts"),
            "import { helper } from './utils';\nfunction run() { helper(); }\n",
        )
        .unwrap();

        let db = GraphDb::open_in_memory().unwrap();
        let indexer = Indexer::new(&db);
        indexer.index_project(tmp.path()).unwrap();

        let file_edge_count = db.file_edge_count().unwrap();
        assert!(file_edge_count > 0, "should have file-level import edges");
    }

    #[test]
    fn test_index_computes_importance() {
        let tmp = TempDir::new().unwrap();
        let ts_file = tmp.path().join("app.ts");
        std::fs::write(
            &ts_file,
            r#"
function core(): string { return "hello"; }

function consumer(): string { return core(); }

function orchestrator(): string {
    return consumer();
}
"#,
        )
        .unwrap();

        let db = GraphDb::open_in_memory().unwrap();
        let indexer = Indexer::new(&db);
        indexer.index_project(tmp.path()).unwrap();

        let core_sym = db.symbols_by_name("core").unwrap();
        assert!(!core_sym.is_empty());
        let core_importance = core_sym[0].importance;

        let orchestrator_sym = db.symbols_by_name("orchestrator").unwrap();
        let orch_importance = orchestrator_sym[0].importance;

        assert!(
            core_importance > orch_importance,
            "core (called by others) should have higher importance than orchestrator: {} vs {}",
            core_importance,
            orch_importance,
        );
        assert!(
            core_importance > 0.3,
            "core should have importance > baseline 0.3"
        );
    }

    #[test]
    fn test_intra_file_call_with_duplicate_names() {
        let tmp = TempDir::new().unwrap();
        let a_file = tmp.path().join("a.ts");
        std::fs::write(
            &a_file,
            r#"
function helper(): string { return "a"; }
function consume(): string { return helper(); }
"#,
        )
        .unwrap();

        let b_file = tmp.path().join("b.ts");
        std::fs::write(
            &b_file,
            r#"
function helper(): string { return "b"; }
function run(): string { return helper(); }
"#,
        )
        .unwrap();

        let db = GraphDb::open_in_memory().unwrap();
        let indexer = Indexer::new(&db);
        indexer.index_project(tmp.path()).unwrap();

        let all_calls: Vec<_> = db
            .edges_by_kind(EdgeKind::Calls, 100)
            .unwrap()
            .into_iter()
            .collect();

        let resolve_name = |id: i64| -> String { db.get_symbol(id).unwrap().unwrap().name.clone() };
        let resolve_file = |id: i64| -> String {
            let sym = db.get_symbol(id).unwrap().unwrap();
            db.file_path_for_id(sym.file_id).unwrap().unwrap()
        };

        let a_helpers: Vec<_> = all_calls
            .iter()
            .filter(|e| {
                let src = resolve_name(e.source_id);
                let tgt = resolve_name(e.target_id);
                src == "consume" && tgt == "helper"
            })
            .collect();
        assert_eq!(
            a_helpers.len(),
            1,
            "consume should call helper in same file"
        );
        assert!(
            resolve_file(a_helpers[0].source_id).contains("a.ts"),
            "consume→helper edge should be within a.ts"
        );
        assert!(
            resolve_file(a_helpers[0].target_id).contains("a.ts"),
            "consume→helper target should be a.ts helper, not b.ts helper"
        );

        let b_helpers: Vec<_> = all_calls
            .iter()
            .filter(|e| {
                let src = resolve_name(e.source_id);
                let tgt = resolve_name(e.target_id);
                src == "run" && tgt == "helper"
            })
            .collect();
        assert_eq!(b_helpers.len(), 1, "run should call helper in same file");
        assert!(
            resolve_file(b_helpers[0].target_id).contains("b.ts"),
            "run→helper target should be b.ts helper, not a.ts helper"
        );
    }

    #[test]
    fn test_data_files_are_tracked_but_not_symbol_extracted() {
        // A package-lock.json with hundreds of JSON keys must NOT produce junk
        // Constant symbols, but the file should still be file-tracked so
        // freshness/staleness continues to work. A real source file alongside it
        // is parsed normally.
        let tmp = TempDir::new().unwrap();

        // Build a synthetic lockfile with many keys at multiple depths.
        let mut lock = String::from("{\n");
        for i in 0..50 {
            lock.push_str(&format!(
                "  \"node_modules/pkg-{i}\": {{ \"version\": \"1.0.0\", \"resolved\": \"https://example.com/pkg-{i}\" }},\n"
            ));
        }
        lock.push_str("  \"name\": \"demo\",\n  \"version\": \"1.0.0\"\n}\n");
        std::fs::write(tmp.path().join("package-lock.json"), lock).unwrap();

        let rust = tmp.path().join("main.rs");
        std::fs::write(
            &rust,
            r#"
pub fn handle_request() -> u32 { 200 }
"#,
        )
        .unwrap();

        let db = GraphDb::open_in_memory().unwrap();
        let indexer = Indexer::new(&db);
        let stats = indexer.index_project(tmp.path()).unwrap();

        // Both files are tracked.
        assert_eq!(stats.files_indexed, 2);

        // The rust file produced its function symbol.
        assert!(
            !db.symbols_by_name("handle_request").unwrap().is_empty(),
            "real source should be symbol-extracted"
        );

        // The lockfile was file-tracked …
        let lock_file = db
            .get_file_by_path("package-lock.json")
            .unwrap()
            .expect("lockfile should be file-tracked");
        // … but produced zero symbols.
        let lock_syms = db.symbols_by_file(lock_file.id).unwrap();
        assert_eq!(
            lock_syms.len(),
            0,
            "package-lock.json must not contribute symbols (got {lock_syms:?})"
        );

        // Generic lockfile keys that would otherwise pollute search must be absent.
        assert!(db.symbols_by_name("version").unwrap().is_empty());
        assert!(db.symbols_by_name("dependencies").unwrap().is_empty());
        assert!(db.symbols_by_name("node_modules/pkg-0").unwrap().is_empty());
    }
}
