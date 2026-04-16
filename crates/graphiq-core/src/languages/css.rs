use tree_sitter::Tree;

use crate::chunker::{fill_unclaimed_gaps, make_parsed_symbol, LanguageChunker, ParsedSymbol};
use crate::symbol::{SymbolKind, Visibility};

pub struct CssChunker;

impl Default for CssChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl CssChunker {
    pub fn new() -> Self {
        Self
    }
}

impl LanguageChunker for CssChunker {
    fn language(&self) -> tree_sitter::Language {
        tree_sitter_css::LANGUAGE.into()
    }

    fn language_name(&self) -> &str {
        "css"
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
                "rule_set" => {
                    let name = child
                        .child_by_field_name("selectors")
                        .and_then(|s| {
                            s.children(&mut s.walk())
                                .find(|c| c.is_named())
                                .and_then(|c| c.utf8_text(source.as_bytes()).ok())
                                .map(|t| t.trim().to_string())
                        })
                        .or_else(|| {
                            child
                                .children(&mut child.walk())
                                .find(|c| c.kind() == "selectors")
                                .and_then(|s| {
                                    s.children(&mut s.walk())
                                        .find(|c| c.is_named())
                                        .and_then(|c| c.utf8_text(source.as_bytes()).ok())
                                        .map(|t| t.trim().to_string())
                                })
                        });
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "css",
                        SymbolKind::Struct,
                        name.as_deref(),
                        None,
                        Visibility::Public,
                        serde_json::Value::Null,
                    ) {
                        symbols.push(sym);
                    }
                }
                "keyframes_statement" | "media_statement" => {
                    let name = child
                        .child_by_field_name("name")
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                        .map(|s| s.to_string());
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "css",
                        SymbolKind::Module,
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
        fill_unclaimed_gaps(source, file_path, "css", symbols);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_css_rules() {
        let source = r#".container {
  max-width: 1200px;
  margin: 0 auto;
}

.header {
  padding: 1rem;
}

@media (max-width: 768px) {
  .container {
    padding: 0 1rem;
  }
}
"#;
        let chunker = CssChunker::new();
        let result = chunker.parse(source, "styles.css");

        let rules: Vec<_> = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Struct)
            .collect();
        assert!(rules.len() >= 2);

        let media = result.symbols.iter().find(|s| s.kind == SymbolKind::Module);
        assert!(media.is_some());
    }
}
