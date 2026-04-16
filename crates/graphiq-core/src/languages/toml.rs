use tree_sitter::Tree;

use crate::chunker::{make_parsed_symbol, LanguageChunker, ParsedSymbol};
use crate::symbol::{SymbolKind, Visibility};

pub struct TomlChunker;

impl Default for TomlChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl TomlChunker {
    pub fn new() -> Self {
        Self
    }

    fn extract_pair(
        pair_node: tree_sitter::Node,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<ParsedSymbol>,
    ) {
        let name = pair_node
            .children(&mut pair_node.walk())
            .find(|c| c.kind() == "bare_key" || c.kind() == "quoted_key")
            .and_then(|k| {
                k.utf8_text(source.as_bytes())
                    .ok()
                    .map(|s| s.trim().trim_matches('"').to_string())
            });
        if let Some(sym) = make_parsed_symbol(
            source,
            pair_node,
            file_path,
            "toml",
            SymbolKind::Constant,
            name.as_deref(),
            None,
            Visibility::Public,
            serde_json::Value::Null,
        ) {
            symbols.push(sym);
        }
    }
}

impl LanguageChunker for TomlChunker {
    fn language(&self) -> tree_sitter::Language {
        tree_sitter_toml_ng::language().into()
    }

    fn language_name(&self) -> &str {
        "toml"
    }

    fn walk_declarations(
        &self,
        tree: &Tree,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<ParsedSymbol>,
    ) {
        for child in tree.root_node().children(&mut tree.root_node().walk()) {
            match child.kind() {
                "table" | "table_array_element" => {
                    let name = child
                        .children(&mut child.walk())
                        .find(|c| {
                            c.kind() == "bare_key"
                                || c.kind() == "dotted_key"
                                || c.kind() == "quoted_key"
                        })
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                        .map(|s| s.trim().trim_matches('"').to_string());
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "toml",
                        SymbolKind::Module,
                        name.as_deref(),
                        None,
                        Visibility::Public,
                        serde_json::Value::Null,
                    ) {
                        symbols.push(sym);
                    }
                    for tc in child.children(&mut child.walk()) {
                        if tc.kind() == "pair" {
                            Self::extract_pair(tc, source, file_path, symbols);
                        }
                    }
                }
                "pair" => {
                    Self::extract_pair(child, source, file_path, symbols);
                }
                _ => {}
            }
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
    fn test_toml_pairs_and_tables() {
        let source = r#"[package]
name = "graphiq"
version = "1.0.0"

[dependencies]
serde = "1.0"
tokio = "1.0"
"#;
        let chunker = TomlChunker::new();
        let result = chunker.parse(source, "Cargo.toml");

        let modules: Vec<_> = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Module)
            .collect();
        assert!(modules.len() >= 2);

        let constants: Vec<_> = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Constant)
            .collect();
        assert!(constants.len() >= 2);

        let names: Vec<&str> = constants.iter().filter_map(|s| s.name.as_deref()).collect();
        assert!(names.contains(&"name"));
        assert!(names.contains(&"version"));
    }
}
