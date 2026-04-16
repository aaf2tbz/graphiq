use tree_sitter::Tree;

use crate::chunker::{
    extract_signature, fill_unclaimed_gaps, make_parsed_symbol, LanguageChunker, ParsedSymbol,
};
use crate::symbol::{SymbolKind, Visibility};

pub struct CppChunker;

impl Default for CppChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl CppChunker {
    pub fn new() -> Self {
        Self
    }

    fn extract_function_name(node: tree_sitter::Node, source: &str) -> Option<String> {
        for child in node.children(&mut node.walk()) {
            match child.kind() {
                "function_declarator" => {
                    return Self::extract_function_name(child, source);
                }
                "identifier" => {
                    return child
                        .utf8_text(source.as_bytes())
                        .ok()
                        .map(|s| s.to_string());
                }
                "qualified_identifier" => {
                    return child
                        .utf8_text(source.as_bytes())
                        .ok()
                        .map(|s| s.to_string());
                }
                "pointer_declarator"
                | "reference_declarator"
                | "parenthesized_declarator"
                | "declarator" => {
                    if let Some(name) = Self::extract_function_name(child, source) {
                        return Some(name);
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn walk_template(
        node: tree_sitter::Node,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<ParsedSymbol>,
    ) {
        for child in node.children(&mut node.walk()) {
            match child.kind() {
                "function_definition" => {
                    let name = Self::extract_function_name(child, source);
                    let sig = extract_signature(child, source);
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "cpp",
                        SymbolKind::Function,
                        name.as_deref(),
                        sig.as_deref(),
                        Visibility::Public,
                        serde_json::json!({"templated": true}),
                    ) {
                        symbols.push(sym);
                    }
                }
                "class_specifier" => {
                    if let Some(name_node) = child.child_by_field_name("name") {
                        let name = name_node
                            .utf8_text(source.as_bytes())
                            .ok()
                            .map(|s| s.to_string());
                        if let Some(sym) = make_parsed_symbol(
                            source,
                            node,
                            file_path,
                            "cpp",
                            SymbolKind::Class,
                            name.as_deref(),
                            None,
                            Visibility::Public,
                            serde_json::json!({"templated": true}),
                        ) {
                            symbols.push(sym);
                        }
                    }
                }
                "struct_specifier" => {
                    if let Some(name_node) = child.child_by_field_name("name") {
                        let name = name_node
                            .utf8_text(source.as_bytes())
                            .ok()
                            .map(|s| s.to_string());
                        if let Some(sym) = make_parsed_symbol(
                            source,
                            node,
                            file_path,
                            "cpp",
                            SymbolKind::Struct,
                            name.as_deref(),
                            None,
                            Visibility::Public,
                            serde_json::json!({"templated": true}),
                        ) {
                            symbols.push(sym);
                        }
                    }
                }
                "template_declaration" => {
                    Self::walk_template(child, source, file_path, symbols);
                }
                _ => {}
            }
        }
    }

    fn walk_declarations_recursive(
        node: tree_sitter::Node,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<ParsedSymbol>,
    ) {
        for child in node.children(&mut node.walk()) {
            let kind = child.kind();

            match kind {
                "function_definition" => {
                    let name = Self::extract_function_name(child, source);
                    let sig = extract_signature(child, source);
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "cpp",
                        SymbolKind::Function,
                        name.as_deref(),
                        sig.as_deref(),
                        Visibility::Public,
                        serde_json::Value::Null,
                    ) {
                        symbols.push(sym);
                    }
                }
                "class_specifier" => {
                    if let Some(name_node) = child.child_by_field_name("name") {
                        let name = name_node
                            .utf8_text(source.as_bytes())
                            .ok()
                            .map(|s| s.to_string());
                        if let Some(sym) = make_parsed_symbol(
                            source,
                            child,
                            file_path,
                            "cpp",
                            SymbolKind::Class,
                            name.as_deref(),
                            None,
                            Visibility::Public,
                            serde_json::Value::Null,
                        ) {
                            symbols.push(sym);
                        }
                    }
                }
                "struct_specifier" => {
                    if let Some(name_node) = child.child_by_field_name("name") {
                        let name = name_node
                            .utf8_text(source.as_bytes())
                            .ok()
                            .map(|s| s.to_string());
                        if let Some(sym) = make_parsed_symbol(
                            source,
                            child,
                            file_path,
                            "cpp",
                            SymbolKind::Struct,
                            name.as_deref(),
                            None,
                            Visibility::Public,
                            serde_json::Value::Null,
                        ) {
                            symbols.push(sym);
                        }
                    }
                }
                "enum_specifier" => {
                    if let Some(name_node) = child.child_by_field_name("name") {
                        let name = name_node
                            .utf8_text(source.as_bytes())
                            .ok()
                            .map(|s| s.to_string());
                        if let Some(sym) = make_parsed_symbol(
                            source,
                            child,
                            file_path,
                            "cpp",
                            SymbolKind::Enum,
                            name.as_deref(),
                            None,
                            Visibility::Public,
                            serde_json::Value::Null,
                        ) {
                            symbols.push(sym);
                        }
                    }
                }
                "namespace_definition" => {
                    let name = child
                        .child_by_field_name("name")
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok().map(|s| s.to_string()));
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "cpp",
                        SymbolKind::Namespace,
                        name.as_deref(),
                        None,
                        Visibility::Public,
                        serde_json::Value::Null,
                    ) {
                        symbols.push(sym);
                    }
                    if let Some(body) = child.child_by_field_name("body") {
                        Self::walk_declarations_recursive(body, source, file_path, symbols);
                    }
                }
                "template_declaration" => {
                    Self::walk_template(child, source, file_path, symbols);
                }
                "alias_declaration" => {
                    let name = child
                        .child_by_field_name("name")
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok().map(|s| s.to_string()));
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "cpp",
                        SymbolKind::TypeAlias,
                        name.as_deref(),
                        None,
                        Visibility::Public,
                        serde_json::Value::Null,
                    ) {
                        symbols.push(sym);
                    }
                }
                "type_definition" => {
                    let name = child.child_by_field_name("declarator").and_then(|d| {
                        for gc in d.children(&mut d.walk()) {
                            if gc.kind() == "identifier" || gc.kind() == "type_identifier" {
                                return gc.utf8_text(source.as_bytes()).ok().map(|s| s.to_string());
                            }
                        }
                        None
                    });
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "cpp",
                        SymbolKind::TypeAlias,
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
                        "cpp",
                        SymbolKind::Import,
                        None,
                        None,
                        Visibility::Package,
                        serde_json::Value::Null,
                    ) {
                        symbols.push(sym);
                    }
                }
                "declaration_list" | "field_declaration_list" => {
                    Self::walk_declarations_recursive(child, source, file_path, symbols);
                }
                _ => {}
            }
        }
    }
}

impl LanguageChunker for CppChunker {
    fn language(&self) -> tree_sitter::Language {
        tree_sitter_cpp::LANGUAGE.into()
    }

    fn language_name(&self) -> &str {
        "cpp"
    }

    fn walk_declarations(
        &self,
        tree: &Tree,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<ParsedSymbol>,
    ) {
        Self::walk_declarations_recursive(tree.root_node(), source, file_path, symbols);
    }

    fn fill_gaps(
        &self,
        _tree: &Tree,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<ParsedSymbol>,
    ) {
        fill_unclaimed_gaps(source, file_path, "cpp", symbols);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpp_class_and_namespace() {
        let source = r#"#include <string>

namespace auth {

class AuthService {
public:
    AuthService(std::string secret);
    bool authenticate(const std::string& token);
private:
    std::string secret_;
};

}
"#;
        let chunker = CppChunker::new();
        let result = chunker.parse(source, "auth.cpp");

        let ns = result
            .symbols
            .iter()
            .find(|s| s.kind == SymbolKind::Namespace);
        assert!(ns.is_some());
        assert_eq!(ns.unwrap().name.as_deref(), Some("auth"));

        let class = result.symbols.iter().find(|s| s.kind == SymbolKind::Class);
        assert!(class.is_some());
        assert_eq!(class.unwrap().name.as_deref(), Some("AuthService"));
    }

    #[test]
    fn test_cpp_template() {
        let source = r#"template<typename T>
T max_value(T a, T b) {
    return a > b ? a : b;
}

template<typename K, typename V>
class HashMap {
public:
    void insert(K key, V value);
};
"#;
        let chunker = CppChunker::new();
        let result = chunker.parse(source, "containers.cpp");

        let funcs: Vec<_> = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Function)
            .collect();
        assert!(!funcs.is_empty());

        let classes: Vec<_> = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Class)
            .collect();
        assert!(!classes.is_empty());
    }
}
