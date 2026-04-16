use tree_sitter::Tree;

use crate::chunker::{fill_unclaimed_gaps, make_parsed_symbol, LanguageChunker, ParsedSymbol};
use crate::symbol::{SymbolKind, Visibility};

pub struct YamlChunker;

impl Default for YamlChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl YamlChunker {
    pub fn new() -> Self {
        Self
    }

    fn walk_mapping(
        node: tree_sitter::Node,
        source: &str,
        file_path: &str,
        depth: usize,
        symbols: &mut Vec<ParsedSymbol>,
    ) {
        if depth > 4 {
            return;
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "block_mapping_pair" | "flow_mapping_pair" => {
                    let key_node = child.child_by_field_name("key");
                    let name = key_node.and_then(|k| {
                        k.utf8_text(source.as_bytes())
                            .ok()
                            .map(|s| s.trim().to_string())
                    });

                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "yaml",
                        SymbolKind::Constant,
                        name.as_deref(),
                        None,
                        Visibility::Public,
                        serde_json::json!({"depth": depth}),
                    ) {
                        symbols.push(sym);
                    }

                    if let Some(value_node) = child.child_by_field_name("value") {
                        Self::walk_mapping(value_node, source, file_path, depth + 1, symbols);
                    }
                }
                "block_mapping" | "flow_mapping" | "stream" | "document" | "block_node"
                | "flow_node" => {
                    Self::walk_mapping(child, source, file_path, depth, symbols);
                }
                _ => {}
            }
        }
    }
}

impl LanguageChunker for YamlChunker {
    fn language(&self) -> tree_sitter::Language {
        tree_sitter_yaml::LANGUAGE.into()
    }

    fn language_name(&self) -> &str {
        "yaml"
    }

    fn walk_declarations(
        &self,
        tree: &Tree,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<ParsedSymbol>,
    ) {
        let root = tree.root_node();
        Self::walk_mapping(root, source, file_path, 0, symbols);
    }

    fn fill_gaps(
        &self,
        _tree: &Tree,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<ParsedSymbol>,
    ) {
        fill_unclaimed_gaps(source, file_path, "yaml", symbols);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_yaml_pairs() {
        let source = r#"name: graphiq
version: 1.0.0
dependencies:
  serde: "1.0"
  tokio: "1.0"
"#;
        let chunker = YamlChunker::new();
        let result = chunker.parse(source, "config.yaml");

        let constants: Vec<_> = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Constant)
            .collect();
        assert!(constants.len() >= 3);

        let names: Vec<&str> = constants.iter().filter_map(|s| s.name.as_deref()).collect();
        assert!(names.contains(&"name"));
        assert!(names.contains(&"version"));
    }
}
