use tree_sitter::Tree;

use crate::chunker::{
    extract_signature, fill_unclaimed_gaps, make_parsed_symbol, LanguageChunker,
    ParsedSymbol,
};
use crate::symbol::{SymbolKind, Visibility};

pub struct CChunker;

impl Default for CChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl CChunker {
    pub fn new() -> Self {
        Self
    }

    fn extract_function_name(node: tree_sitter::Node, source: &str) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "function_declarator" {
                return Self::extract_function_name(child, source);
            }
            if child.kind() == "identifier" {
                return child
                    .utf8_text(source.as_bytes())
                    .ok()
                    .map(|s| s.to_string());
            }
            if child.kind() == "pointer_declarator" || child.kind() == "parenthesized_declarator" {
                if let Some(name) = Self::extract_function_name(child, source) {
                    return Some(name);
                }
            }
            if child.kind() == "declarator" {
                return Self::extract_function_name(child, source);
            }
        }
        None
    }

    fn extract_struct_name(node: tree_sitter::Node, source: &str) -> Option<String> {
        for child in node.children(&mut node.walk()) {
            if child.kind() == "type_identifier" {
                return child
                    .utf8_text(source.as_bytes())
                    .ok()
                    .map(|s| s.to_string());
            }
        }
        None
    }
}

impl LanguageChunker for CChunker {
    fn language(&self) -> tree_sitter::Language {
        tree_sitter_c::LANGUAGE.into()
    }

    fn language_name(&self) -> &str {
        "c"
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
                "function_definition" => {
                    let name = Self::extract_function_name(child, source);
                    let sig = extract_signature(child, source);
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "c",
                        SymbolKind::Function,
                        name.as_deref(),
                        sig.as_deref(),
                        Visibility::Public,
                        serde_json::Value::Null,
                    ) {
                        symbols.push(sym);
                    }
                }
                "struct_specifier" => {
                    let name = Self::extract_struct_name(child, source);
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "c",
                        SymbolKind::Struct,
                        name.as_deref(),
                        None,
                        Visibility::Public,
                        serde_json::Value::Null,
                    ) {
                        symbols.push(sym);
                    }
                }
                "enum_specifier" => {
                    let name = Self::extract_struct_name(child, source);
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "c",
                        SymbolKind::Enum,
                        name.as_deref(),
                        None,
                        Visibility::Public,
                        serde_json::Value::Null,
                    ) {
                        symbols.push(sym);
                    }
                }
                "type_definition" => {
                    let inner_type = child.child_by_field_name("type");
                    let declarator_name = child.child_by_field_name("declarator").and_then(|d| {
                        if d.kind() == "type_identifier" {
                            d.utf8_text(source.as_bytes()).ok().map(|s| s.to_string())
                        } else {
                            for gc in d.children(&mut d.walk()) {
                                if gc.kind() == "identifier" || gc.kind() == "type_identifier" {
                                    return gc
                                        .utf8_text(source.as_bytes())
                                        .ok()
                                        .map(|s| s.to_string());
                                }
                            }
                            None
                        }
                    });
                    let sk = if inner_type.as_ref().map(|t| t.kind()) == Some("struct_specifier") {
                        SymbolKind::Struct
                    } else if inner_type.as_ref().map(|t| t.kind()) == Some("enum_specifier") {
                        SymbolKind::Enum
                    } else {
                        SymbolKind::TypeAlias
                    };
                    let name = if sk == SymbolKind::TypeAlias {
                        child.child_by_field_name("declarator").and_then(|d| {
                            if d.kind() == "type_identifier" {
                                d.utf8_text(source.as_bytes()).ok().map(|s| s.to_string())
                            } else {
                                for gc in d.children(&mut d.walk()) {
                                    if gc.kind() == "identifier" || gc.kind() == "type_identifier" {
                                        return gc
                                            .utf8_text(source.as_bytes())
                                            .ok()
                                            .map(|s| s.to_string());
                                    }
                                }
                                None
                            }
                        })
                    } else {
                        declarator_name
                    };
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "c",
                        sk,
                        name.as_deref(),
                        None,
                        Visibility::Public,
                        serde_json::Value::Null,
                    ) {
                        symbols.push(sym);
                    }
                }
                "preproc_include" => {
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "c",
                        SymbolKind::Import,
                        None,
                        None,
                        Visibility::Package,
                        serde_json::Value::Null,
                    ) {
                        symbols.push(sym);
                    }
                }
                "preproc_def" => {
                    let name = child
                        .child_by_field_name("name")
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok().map(|s| s.to_string()));
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "c",
                        SymbolKind::Constant,
                        name.as_deref(),
                        None,
                        Visibility::Package,
                        serde_json::Value::Null,
                    ) {
                        symbols.push(sym);
                    }
                }
                "preproc_function_def" => {
                    let name = child
                        .child_by_field_name("name")
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok().map(|s| s.to_string()));
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "c",
                        SymbolKind::Macro,
                        name.as_deref(),
                        None,
                        Visibility::Public,
                        serde_json::Value::Null,
                    ) {
                        symbols.push(sym);
                    }
                }
                _ => {}
            }
        }
    }

    fn fill_gaps(
        &self,
        _tree: &Tree,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<ParsedSymbol>,
    ) {
        fill_unclaimed_gaps(source, file_path, "c", symbols);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_c_function() {
        let source = r#"#include <stdio.h>
#include <stdlib.h>

#define MAX_SIZE 1024

int authenticate(const char *token) {
    return verify_token(token);
}

static void cleanup(void) {
    free(buffer);
}
"#;
        let chunker = CChunker::new();
        let result = chunker.parse(source, "auth.c");

        let funcs: Vec<_> = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Function)
            .collect();
        assert_eq!(funcs.len(), 2);
        assert_eq!(funcs[0].name.as_deref(), Some("authenticate"));
        assert_eq!(funcs[1].name.as_deref(), Some("cleanup"));
    }

    #[test]
    fn test_c_struct_and_enum() {
        let source = r#"typedef struct {
    int x;
    int y;
} Point;

enum Status {
    OK = 0,
    ERR = -1
};
"#;
        let chunker = CChunker::new();
        let result = chunker.parse(source, "types.c");

        let structs: Vec<_> = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Struct)
            .collect();
        assert!(!structs.is_empty());

        let enums: Vec<_> = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Enum)
            .collect();
        assert!(!enums.is_empty());
    }

    #[test]
    fn test_c_preproc() {
        let source = r#"#define MAX_RETRIES 3
#define DBG(fmt, ...) fprintf(stderr, fmt, ##__VA_ARGS__)
"#;
        let chunker = CChunker::new();
        let result = chunker.parse(source, "config.h");

        let constants: Vec<_> = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Constant)
            .collect();
        assert_eq!(constants.len(), 1);

        let macros: Vec<_> = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Macro)
            .collect();
        assert_eq!(macros.len(), 1);
    }
}
