use tree_sitter::Tree;

use crate::chunker::{fill_unclaimed_gaps, make_parsed_symbol, LanguageChunker, ParsedSymbol};
use crate::symbol::{SymbolKind, Visibility};

pub struct HtmlChunker;

impl Default for HtmlChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl HtmlChunker {
    pub fn new() -> Self {
        Self
    }

    fn tag_name(node: tree_sitter::Node, source: &str) -> Option<String> {
        for child in node.children(&mut node.walk()) {
            if child.kind() == "tag_name" {
                return child
                    .utf8_text(source.as_bytes())
                    .ok()
                    .map(|s| s.to_string());
            }
        }
        None
    }
}

impl LanguageChunker for HtmlChunker {
    fn language(&self) -> tree_sitter::Language {
        tree_sitter_html::LANGUAGE.into()
    }

    fn language_name(&self) -> &str {
        "html"
    }

    fn walk_declarations(
        &self,
        tree: &Tree,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<ParsedSymbol>,
    ) {
        fn walk_elements(
            node: tree_sitter::Node,
            source: &str,
            file_path: &str,
            depth: usize,
            symbols: &mut Vec<ParsedSymbol>,
        ) {
            if depth > 5 {
                return;
            }

            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                let kind = child.kind();

                if kind == "element" {
                    let tag = HtmlChunker::tag_name(child, source);
                    let sk = match tag.as_deref() {
                        Some("header") | Some("footer") | Some("nav") | Some("main")
                        | Some("section") | Some("article") | Some("aside") => SymbolKind::Section,
                        _ => SymbolKind::Module,
                    };
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "html",
                        sk,
                        tag.as_deref(),
                        None,
                        Visibility::Public,
                        serde_json::json!({"depth": depth}),
                    ) {
                        symbols.push(sym);
                    }
                    walk_elements(child, source, file_path, depth + 1, symbols);
                } else if kind == "script_element" || kind == "style_element" {
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "html",
                        SymbolKind::Section,
                        Some(kind.strip_suffix("_element").unwrap_or(kind)),
                        None,
                        Visibility::Public,
                        serde_json::Value::Null,
                    ) {
                        symbols.push(sym);
                    }
                }
            }
        }

        walk_elements(tree.root_node(), source, file_path, 0, symbols);
    }

    fn fill_gaps(
        &self,
        _tree: &Tree,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<ParsedSymbol>,
    ) {
        fill_unclaimed_gaps(source, file_path, "html", symbols);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_html_elements() {
        let source = r#"<!DOCTYPE html>
<html>
<head><title>Test</title></head>
<body>
  <header>Header</header>
  <main>
    <section id="content">Content</section>
  </main>
  <script src="app.js"></script>
  <style>body { margin: 0; }</style>
</body>
</html>
"#;
        let chunker = HtmlChunker::new();
        let result = chunker.parse(source, "index.html");

        let sections: Vec<_> = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Section)
            .collect();
        assert!(sections.len() >= 3);
    }
}
