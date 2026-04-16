use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use crate::calls;
use crate::chunker::LanguageChunker;
use crate::db::GraphDb;
use crate::edge::EdgeKind;
use crate::files::{content_hash, detect_language, walk_project, Language};
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

        Ok(stats)
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
        let mut stmt =
            conn.prepare("SELECT id, name, signature, doc_comment, source FROM symbols")?;
        let rows: Vec<(i64, String, Option<String>, Option<String>, Option<String>)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })?
            .flatten()
            .collect();

        let total = rows.len();
        eprintln!("  embedding {total} symbols ...");
        let start = Instant::now();
        let mut embedded = 0;

        let batch_size = 32;
        for chunk in rows.chunks(batch_size) {
            let texts: Vec<String> = chunk
                .iter()
                .map(|(_, name, sig, doc, src)| build_symbol_text(name, sig, doc, src))
                .collect();
            let results = embedder.embed_batch(&texts);
            for ((id, _, _, _, _), result) in chunk.iter().zip(results.into_iter()) {
                if let Ok(vec) = result {
                    let _ = self
                        .db
                        .put_embedding(*id, &vec, "jinaai/jina-embeddings-v2-base-code");
                    embedded += 1;
                }
            }
            eprintln!(
                "  {}/{} ({:.0}ms/ea, ~{:.0}s remaining)",
                embedded,
                total,
                start.elapsed().as_secs_f64() / embedded as f64 * 1000.0,
                (start.elapsed().as_secs_f64() / embedded as f64) * (total - embedded) as f64
            );
        }
        Ok(embedded)
    }
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
