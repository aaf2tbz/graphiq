use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use crate::calls;
use crate::chunker::LanguageChunker;
use crate::db::GraphDb;
use crate::edge::EdgeKind;
use crate::files::{content_hash, detect_language, walk_project, Language};
use crate::motifs::{detect_motifs, motifs_to_hints, MotifEvidence};
use crate::roles::{infer_roles, roles_to_hints, RoleEvidence};
use crate::symbol::{SymbolBuilder, SymbolKind};

pub struct Indexer<'a> {
    db: &'a GraphDb,
}

impl<'a> Indexer<'a> {
    pub fn new(db: &'a GraphDb) -> Self {
        Self { db }
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

    pub fn index_project(&self, root: &Path) -> Result<IndexStats, Box<dyn std::error::Error>> {
        let files: Vec<PathBuf> = walk_project(root).collect();
        let stats = self.index_files(root, &files)?;
        Ok(stats)
    }

    pub fn index_files(
        &self,
        root: &Path,
        files: &[PathBuf],
    ) -> Result<IndexStats, Box<dyn std::error::Error>> {
        let mut stats = IndexStats::default();

        let file_data: Vec<_> = files
            .iter()
            .par_bridge()
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

        let mut global_name_to_ids: HashMap<String, Vec<i64>> = HashMap::new();
        let mut pending_rels: Vec<PendingEdge> = Vec::new();
        let mut pending_contains: Vec<(i64, i64)> = Vec::new();
        let mut pending_calls: Vec<PendingCallEdge> = Vec::new();
        let mut pending_imports: Vec<PendingImportEdge> = Vec::new();

        for (rel_path, source, lang, hash, mtime, line_count) in &file_data {
            let path_str = rel_path.to_string_lossy();

            let existing = self.db.get_file_by_path(&path_str)?;
            if let Some(ref f) = existing {
                if f.content_hash == *hash {
                    for sym in self.db.symbols_by_file(f.id)? {
                        global_name_to_ids
                            .entry(sym.name.clone())
                            .or_default()
                            .push(sym.id);
                    }
                    continue;
                }
                self.db.delete_symbols_for_file(f.id)?;
                self.db.delete_edges_for_symbols(f.id)?;
            }

            let file_id =
                self.db
                    .upsert_file(&path_str, lang.as_str(), hash, *mtime, *line_count)?;
            stats.files_indexed += 1;

            let chunker = get_chunker(*lang);
            let result = chunker.parse(source, &path_str);

            let mut file_name_to_id: HashMap<String, i64> = HashMap::new();
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
                    global_name_to_ids
                        .entry(sym_name.clone())
                        .or_default()
                        .push(id);

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

            for rel in &result.structural_rels {
                let edge_kind = match rel.rel_type.as_str() {
                    "implements" => Some(EdgeKind::Implements),
                    "extends" => Some(EdgeKind::Extends),
                    "overrides" => Some(EdgeKind::Overrides),
                    _ => None,
                };
                if let Some(kind) = edge_kind {
                    pending_rels.push(PendingEdge {
                        source_name: rel.source_name.clone(),
                        target_name: rel.target_name.clone(),
                        edge_kind: kind,
                        file_scope: Some(file_name_to_id.clone()),
                    });
                }
            }

            if let Some(ref tree) = result.tree {
                let call_sites = calls::extract_calls(source, tree, lang.as_str());
                for cs in &call_sites {
                    let caller_id =
                        find_enclosing_symbol(&file_name_to_id, &result.symbols, cs.line);
                    pending_calls.push(PendingCallEdge {
                        caller_id,
                        callee_name: cs.callee.clone(),
                    });
                }
            }

            for imp in &result.imports {
                for name in &imp.names {
                    pending_imports.push(PendingImportEdge {
                        importer_file_id: file_id,
                        importer_names: file_name_to_id.clone(),
                        imported_name: name.clone(),
                        module_path: imp.module_path.clone(),
                    });
                }
            }

            stats.imports_extracted += result.imports.len();
            stats.rels_extracted += result.structural_rels.len();
        }

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
            let target_id = resolve_symbol(&global_name_to_ids, &pc.callee_name);
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

        let importance_scores = self.db.compute_importance_scores()?;
        for (symbol_id, importance) in &importance_scores {
            let _ = self.db.update_importance(*symbol_id, *importance);
        }

        self.generate_search_hints()?;

        Ok(stats)
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

            let mut hop2_names: std::collections::HashSet<String> =
                std::collections::HashSet::new();
            if let Some(incoming) = in_by_id.get(id) {
                for (_, source_name) in incoming.iter().take(6) {
                    if let Some(&caller_id) = name_to_id.get(source_name) {
                        if let Some(caller_out) = out_by_id.get(&caller_id) {
                            for (_, callee_name) in caller_out.iter().take(4) {
                                if callee_name != _name {
                                    hop2_names.insert(callee_name.to_lowercase());
                                }
                            }
                        }
                        if let Some(caller_in) = in_by_id.get(&caller_id) {
                            for (_, co_caller_name) in caller_in.iter().take(4) {
                                if co_caller_name != _name && co_caller_name != source_name {
                                    hop2_names.insert(co_caller_name.to_lowercase());
                                }
                            }
                        }
                    }
                }
            }
            if !hop2_names.is_empty() {
                let hop2_list: Vec<String> = hop2_names.into_iter().take(8).collect();
                hints.push(format!("near {}", hop2_list.join(", ")));
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

            let hint_text = hints.join(". ");
            let _ = self.db.update_search_hints(*id, &hint_text);
        }

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
    file_scope: Option<HashMap<String, i64>>,
}

struct PendingCallEdge {
    caller_id: Option<i64>,
    callee_name: String,
}

struct PendingImportEdge {
    importer_file_id: i64,
    importer_names: HashMap<String, i64>,
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
    pub symbols_indexed: usize,
    pub imports_extracted: usize,
    pub rels_extracted: usize,
    pub calls_extracted: usize,
    pub edges_inserted: usize,
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
}
