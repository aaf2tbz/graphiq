use tree_sitter::Tree;

use crate::chunker::{
    extract_name, extract_signature, fill_unclaimed_gaps, make_parsed_symbol, ImportInfo,
    LanguageChunker, ParsedSymbol,
};
use crate::symbol::{SymbolKind, Visibility};

pub struct GoChunker;

impl Default for GoChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl GoChunker {
    pub fn new() -> Self {
        Self
    }

    fn extract_import_path(spec: &tree_sitter::Node, source: &str) -> Option<String> {
        spec.child_by_field_name("path").and_then(|path_node| {
            let text = path_node.utf8_text(source.as_bytes()).ok()?;
            let cleaned = text.trim_matches('"').to_string();
            if cleaned.is_empty() {
                None
            } else {
                Some(cleaned)
            }
        })
    }
}

impl LanguageChunker for GoChunker {
    fn language(&self) -> tree_sitter::Language {
        tree_sitter_go::LANGUAGE.into()
    }

    fn language_name(&self) -> &str {
        "go"
    }

    fn walk_declarations(
        &self,
        tree: &Tree,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<ParsedSymbol>,
    ) {
        for child in tree.root_node().children(&mut tree.root_node().walk()) {
            let kind = child.kind();

            match kind {
                "function_declaration" => {
                    let name = extract_name(child, source);
                    let sig = extract_signature(child, source);
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "go",
                        SymbolKind::Function,
                        name.as_deref(),
                        sig.as_deref(),
                        Visibility::Public,
                        serde_json::Value::Null,
                    ) {
                        symbols.push(sym);
                    }
                }
                "method_declaration" => {
                    let name = extract_name(child, source);
                    let sig = extract_signature(child, source);
                    let receiver = child
                        .child_by_field_name("receiver")
                        .and_then(|r| r.utf8_text(source.as_bytes()).ok())
                        .map(|s| s.trim().to_string());
                    let mut meta = serde_json::Map::new();
                    if let Some(recv) = receiver {
                        meta.insert("receiver".into(), serde_json::Value::String(recv));
                    }
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "go",
                        SymbolKind::Method,
                        name.as_deref(),
                        sig.as_deref(),
                        Visibility::Public,
                        serde_json::Value::Object(meta),
                    ) {
                        symbols.push(sym);
                    }
                }
                "type_declaration" => {
                    let type_spec = child
                        .children(&mut child.walk())
                        .find(|c| c.kind() == "type_spec");
                    if let Some(type_spec) = type_spec {
                        let name = extract_name(type_spec, source);
                        let inner = type_spec
                            .children(&mut type_spec.walk())
                            .find(|c| c.kind() == "struct_type" || c.kind() == "interface_type");
                        let sk = if inner.as_ref().map(|c| c.kind()) == Some("struct_type") {
                            SymbolKind::Struct
                        } else if inner.as_ref().map(|c| c.kind()) == Some("interface_type") {
                            SymbolKind::Interface
                        } else {
                            SymbolKind::TypeAlias
                        };
                        if let Some(sym) = make_parsed_symbol(
                            source,
                            child,
                            file_path,
                            "go",
                            sk,
                            name.as_deref(),
                            None,
                            Visibility::Public,
                            serde_json::Value::Null,
                        ) {
                            symbols.push(sym);
                        }
                    }
                }
                "import_declaration" => {
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "go",
                        SymbolKind::Import,
                        None,
                        None,
                        Visibility::Package,
                        serde_json::Value::Null,
                    ) {
                        symbols.push(sym);
                    }
                }
                _ => {}
            }
        }
    }

    fn extract_imports(&self, tree: &Tree, source: &str) -> Vec<ImportInfo> {
        let mut imports = Vec::new();

        for child in tree.root_node().children(&mut tree.root_node().walk()) {
            if child.kind() == "import_declaration" {
                let spec_list = child
                    .children(&mut child.walk())
                    .find(|c| c.kind() == "import_spec_list");

                if let Some(list) = spec_list {
                    for spec in list.children(&mut list.walk()) {
                        if spec.kind() == "import_spec" {
                            let path = Self::extract_import_path(&spec, source);
                            if let Some(p) = path {
                                imports.push(ImportInfo {
                                    module_path: p,
                                    names: Vec::new(),
                                });
                            }
                        }
                    }
                } else {
                    for gc in child.children(&mut child.walk()) {
                        if gc.kind() == "import_spec" {
                            let path = Self::extract_import_path(&gc, source);
                            if let Some(p) = path {
                                imports.push(ImportInfo {
                                    module_path: p,
                                    names: Vec::new(),
                                });
                            }
                        }
                    }
                }
            }
        }

        imports
    }

    fn fill_gaps(
        &self,
        _tree: &Tree,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<ParsedSymbol>,
    ) {
        fill_unclaimed_gaps(source, file_path, "go", symbols);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_go_function() {
        let source = r#"package main

func authenticate(credentials Credentials) (Auth, error) {
    user := db.FindUser(credentials.Email)
    return Auth{User: user}, nil
}

func (s *Server) Start() error {
    return s.listen()
}
"#;
        let chunker = GoChunker::new();
        let result = chunker.parse(source, "main.go");

        let funcs: Vec<_> = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Function)
            .collect();
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].name.as_deref(), Some("authenticate"));

        let methods: Vec<_> = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Method)
            .collect();
        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0].name.as_deref(), Some("Start"));
    }

    #[test]
    fn test_go_types() {
        let source = r#"package main

type Server struct {
    Addr string
    Handler http.Handler
}

type Handler interface {
    ServeHTTP(w ResponseWriter, r *Request)
}

type Port int
"#;
        let chunker = GoChunker::new();
        let result = chunker.parse(source, "types.go");

        let structs: Vec<_> = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Struct)
            .collect();
        assert_eq!(structs.len(), 1);
        assert_eq!(structs[0].name.as_deref(), Some("Server"));

        let ifaces: Vec<_> = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Interface)
            .collect();
        assert_eq!(ifaces.len(), 1);
        assert_eq!(ifaces[0].name.as_deref(), Some("Handler"));

        let aliases: Vec<_> = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::TypeAlias)
            .collect();
        assert_eq!(aliases.len(), 1);
        assert_eq!(aliases[0].name.as_deref(), Some("Port"));
    }

    #[test]
    fn test_go_imports() {
        let source = r#"package main

import "fmt"
import (
    "net/http"
    "os"
)
"#;
        let chunker = GoChunker::new();
        let result = chunker.parse(source, "main.go");

        assert!(result.imports.iter().any(|i| i.module_path == "fmt"));
        assert!(result.imports.iter().any(|i| i.module_path == "net/http"));
        assert!(result.imports.iter().any(|i| i.module_path == "os"));
    }
}
