use tree_sitter::Tree;

use crate::chunker::{make_parsed_symbol, LanguageChunker, ParsedSymbol};
use crate::symbol::{SymbolKind, Visibility};

pub struct JsonChunker;

impl Default for JsonChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl JsonChunker {
    pub fn new() -> Self {
        Self
    }

    fn walk_pairs(
        node: tree_sitter::Node,
        source: &str,
        file_path: &str,
        depth: usize,
        symbols: &mut Vec<ParsedSymbol>,
    ) {
        if depth > 2 {
            return;
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "pair" {
                let key = child
                    .child_by_field_name("key")
                    .and_then(|k| k.utf8_text(source.as_bytes()).ok())
                    .map(|s| s.trim_matches('"').to_string());

                if let Some(sym) = make_parsed_symbol(
                    source,
                    child,
                    file_path,
                    "json",
                    SymbolKind::Constant,
                    key.as_deref(),
                    None,
                    Visibility::Public,
                    serde_json::json!({"depth": depth}),
                ) {
                    symbols.push(sym);
                }

                if let Some(value_node) = child.child_by_field_name("value") {
                    Self::walk_pairs(value_node, source, file_path, depth + 1, symbols);
                }
            } else if child.kind() == "array" {
                let mut arr_cursor = child.walk();
                for elem in child.children(&mut arr_cursor) {
                    if elem.kind() == "object" {
                        Self::walk_pairs(elem, source, file_path, depth + 1, symbols);
                    }
                }
            }
        }
    }
}

impl LanguageChunker for JsonChunker {
    fn language(&self) -> tree_sitter::Language {
        tree_sitter_json::LANGUAGE.into()
    }

    fn language_name(&self) -> &str {
        "json"
    }

    fn walk_declarations(
        &self,
        tree: &Tree,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<ParsedSymbol>,
    ) {
        let root = tree.root_node();
        if let Some(top) = root.child(0) {
            Self::walk_pairs(top, source, file_path, 0, symbols);
        }
    }

    fn fill_gaps(
        &self,
        _tree: &Tree,
        _source: &str,
        _file_path: &str,
        _symbols: &mut Vec<ParsedSymbol>,
    ) {
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_pairs() {
        let source = r#"{
  "name": "graphiq",
  "version": "1.0.0",
  "dependencies": {
    "serde": "1.0",
    "tokio": "1.0"
  }
}"#;
        let chunker = JsonChunker::new();
        let result = chunker.parse(source, "package.json");

        let constants: Vec<_> = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Constant)
            .collect();
        assert!(constants.len() >= 3);

        let names: Vec<&str> = constants.iter().filter_map(|s| s.name.as_deref()).collect();
        assert!(names.contains(&"name"));
        assert!(names.contains(&"version"));
        assert!(names.contains(&"dependencies"));
    }
}
