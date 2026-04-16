use tree_sitter::Tree;

use crate::chunker::{
    extract_name, extract_signature, fill_unclaimed_gaps, make_parsed_symbol, LanguageChunker,
    ParsedSymbol,
};
use crate::symbol::{SymbolKind, Visibility};

pub struct RubyChunker;

impl Default for RubyChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl RubyChunker {
    pub fn new() -> Self {
        Self
    }

    fn extract_class_members(
        node: tree_sitter::Node,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<ParsedSymbol>,
    ) {
        let body = match node.child_by_field_name("body") {
            Some(b) => b,
            None => {
                for child in node.children(&mut node.walk()) {
                    if child.kind() == "body_statement" {
                        Self::extract_methods_from_body(child, source, file_path, symbols);
                        return;
                    }
                }
                return;
            }
        };
        Self::extract_methods_from_body(body, source, file_path, symbols);
    }

    fn extract_methods_from_body(
        body: tree_sitter::Node,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<ParsedSymbol>,
    ) {
        for child in body.children(&mut body.walk()) {
            if child.kind() == "method" || child.kind() == "singleton_method" {
                let name = extract_name(child, source);
                let sig = extract_signature(child, source);
                let mut meta = serde_json::Map::new();
                meta.insert("class_member".into(), serde_json::Value::Bool(true));
                if let Some(sym) = make_parsed_symbol(
                    source,
                    child,
                    file_path,
                    "ruby",
                    SymbolKind::Method,
                    name.as_deref(),
                    sig.as_deref(),
                    Visibility::Public,
                    serde_json::Value::Object(meta),
                ) {
                    symbols.push(sym);
                }
            }
        }
    }
}

impl LanguageChunker for RubyChunker {
    fn language(&self) -> tree_sitter::Language {
        tree_sitter_ruby::LANGUAGE.into()
    }

    fn language_name(&self) -> &str {
        "ruby"
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
                "method" | "singleton_method" => {
                    let name = extract_name(child, source);
                    let sig = extract_signature(child, source);
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "ruby",
                        SymbolKind::Function,
                        name.as_deref(),
                        sig.as_deref(),
                        Visibility::Public,
                        serde_json::Value::Null,
                    ) {
                        symbols.push(sym);
                    }
                }
                "class" => {
                    let name = extract_name(child, source);
                    let sig = extract_signature(child, source);
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "ruby",
                        SymbolKind::Class,
                        name.as_deref(),
                        sig.as_deref(),
                        Visibility::Public,
                        serde_json::Value::Null,
                    ) {
                        symbols.push(sym);
                    }
                    Self::extract_class_members(child, source, file_path, symbols);
                }
                "module" => {
                    let name = extract_name(child, source);
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "ruby",
                        SymbolKind::Module,
                        name.as_deref(),
                        None,
                        Visibility::Public,
                        serde_json::Value::Null,
                    ) {
                        symbols.push(sym);
                    }
                    Self::extract_class_members(child, source, file_path, symbols);
                }
                "call" => {
                    let method_name = child
                        .child_by_field_name("method")
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                        .map(|s| s.to_string());
                    if let Some(ref mname) = method_name {
                        if mname == "require" || mname == "require_relative" {
                            if let Some(sym) = make_parsed_symbol(
                                source,
                                child,
                                file_path,
                                "ruby",
                                SymbolKind::Import,
                                None,
                                None,
                                Visibility::Package,
                                serde_json::Value::Null,
                            ) {
                                symbols.push(sym);
                            }
                        }
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
        fill_unclaimed_gaps(source, file_path, "ruby", symbols);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ruby_class() {
        let source = r#"require 'json'

class AuthService
  def initialize(secret)
    @secret = secret
  end

  def authenticate(token)
    verify(token)
  end

  def self.create(secret)
    new(secret)
  end
end
"#;
        let chunker = RubyChunker::new();
        let result = chunker.parse(source, "auth.rb");

        let class = result.symbols.iter().find(|s| s.kind == SymbolKind::Class);
        assert!(class.is_some());
        assert_eq!(class.unwrap().name.as_deref(), Some("AuthService"));

        let methods: Vec<_> = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Method)
            .collect();
        assert_eq!(methods.len(), 3);
    }

    #[test]
    fn test_ruby_module() {
        let source = r#"module Auth
  def self.verify(token)
    true
  end
end
"#;
        let chunker = RubyChunker::new();
        let result = chunker.parse(source, "auth_module.rb");

        let module = result.symbols.iter().find(|s| s.kind == SymbolKind::Module);
        assert!(module.is_some());
        assert_eq!(module.unwrap().name.as_deref(), Some("Auth"));
    }
}
