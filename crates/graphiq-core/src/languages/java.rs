use tree_sitter::Tree;

use crate::chunker::{
    extract_name, extract_signature, fill_unclaimed_gaps, make_parsed_symbol, ImportInfo,
    LanguageChunker, ParsedSymbol,
};
use crate::symbol::{SymbolKind, Visibility};

pub struct JavaChunker;

impl Default for JavaChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl JavaChunker {
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
            None => return,
        };

        for child in body.children(&mut body.walk()) {
            let kind = child.kind();
            match kind {
                "method_declaration" => {
                    let name = extract_name(child, source);
                    let sig = extract_signature(child, source);
                    let vis = Self::extract_visibility(child, source);
                    let mut meta = serde_json::Map::new();
                    meta.insert("class_member".into(), serde_json::Value::Bool(true));
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "java",
                        SymbolKind::Method,
                        name.as_deref(),
                        sig.as_deref(),
                        vis,
                        serde_json::Value::Object(meta),
                    ) {
                        symbols.push(sym);
                    }
                }
                "constructor_declaration" => {
                    let name = extract_name(child, source);
                    let sig = extract_signature(child, source);
                    let mut meta = serde_json::Map::new();
                    meta.insert("class_member".into(), serde_json::Value::Bool(true));
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "java",
                        SymbolKind::Constructor,
                        name.as_deref(),
                        sig.as_deref(),
                        Visibility::Public,
                        serde_json::Value::Object(meta),
                    ) {
                        symbols.push(sym);
                    }
                }
                "field_declaration" => {
                    if let Some(var_dec) = child.child_by_field_name("declarator") {
                        let name = var_dec
                            .child_by_field_name("name")
                            .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                            .map(|s| s.to_string());
                        let vis = Self::extract_visibility(child, source);
                        if let Some(sym) = make_parsed_symbol(
                            source,
                            child,
                            file_path,
                            "java",
                            SymbolKind::Field,
                            name.as_deref(),
                            None,
                            vis,
                            serde_json::Value::Null,
                        ) {
                            symbols.push(sym);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn extract_visibility(node: tree_sitter::Node, source: &str) -> Visibility {
        for child in node.children(&mut node.walk()) {
            if child.kind() == "modifiers" {
                for mod_child in child.children(&mut child.walk()) {
                    let text = mod_child.utf8_text(source.as_bytes()).unwrap_or("");
                    match text {
                        "public" => return Visibility::Public,
                        "private" => return Visibility::Private,
                        "protected" => return Visibility::Protected,
                        _ => {}
                    }
                }
            }
        }
        Visibility::Package
    }
}

impl LanguageChunker for JavaChunker {
    fn language(&self) -> tree_sitter::Language {
        tree_sitter_java::LANGUAGE.into()
    }

    fn language_name(&self) -> &str {
        "java"
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
                "class_declaration" => {
                    let name = extract_name(child, source);
                    let sig = extract_signature(child, source);
                    let vis = Self::extract_visibility(child, source);
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "java",
                        SymbolKind::Class,
                        name.as_deref(),
                        sig.as_deref(),
                        vis,
                        serde_json::Value::Null,
                    ) {
                        symbols.push(sym);
                    }
                    Self::extract_class_members(child, source, file_path, symbols);
                }
                "interface_declaration" => {
                    let name = extract_name(child, source);
                    let sig = extract_signature(child, source);
                    let vis = Self::extract_visibility(child, source);
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "java",
                        SymbolKind::Interface,
                        name.as_deref(),
                        sig.as_deref(),
                        vis,
                        serde_json::Value::Null,
                    ) {
                        symbols.push(sym);
                    }
                    Self::extract_class_members(child, source, file_path, symbols);
                }
                "enum_declaration" => {
                    let name = extract_name(child, source);
                    let sig = extract_signature(child, source);
                    let vis = Self::extract_visibility(child, source);
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "java",
                        SymbolKind::Enum,
                        name.as_deref(),
                        sig.as_deref(),
                        vis,
                        serde_json::Value::Null,
                    ) {
                        symbols.push(sym);
                    }
                    Self::extract_class_members(child, source, file_path, symbols);
                }
                "method_declaration" => {
                    let name = extract_name(child, source);
                    let sig = extract_signature(child, source);
                    let vis = Self::extract_visibility(child, source);
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "java",
                        SymbolKind::Method,
                        name.as_deref(),
                        sig.as_deref(),
                        vis,
                        serde_json::Value::Null,
                    ) {
                        symbols.push(sym);
                    }
                }
                "constructor_declaration" => {
                    let name = extract_name(child, source);
                    let sig = extract_signature(child, source);
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "java",
                        SymbolKind::Constructor,
                        name.as_deref(),
                        sig.as_deref(),
                        Visibility::Public,
                        serde_json::Value::Null,
                    ) {
                        symbols.push(sym);
                    }
                }
                "import_declaration" => {
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "java",
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
                let text = child
                    .utf8_text(source.as_bytes())
                    .unwrap_or("")
                    .trim()
                    .trim_end_matches(';')
                    .trim_start_matches("import ")
                    .trim_start_matches("static ")
                    .to_string();

                if text.ends_with(".*") {
                    imports.push(ImportInfo {
                        module_path: text.trim_end_matches(".*").to_string(),
                        names: vec!["*".to_string()],
                    });
                } else if let Some(last_dot) = text.rfind('.') {
                    imports.push(ImportInfo {
                        module_path: text[..last_dot].to_string(),
                        names: vec![text[last_dot + 1..].to_string()],
                    });
                } else if !text.is_empty() {
                    imports.push(ImportInfo {
                        module_path: text,
                        names: Vec::new(),
                    });
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
        fill_unclaimed_gaps(source, file_path, "java", symbols);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_java_class() {
        let source = r#"import java.util.List;

public class AuthService {
    private String secret;

    public AuthService(String secret) {
        this.secret = secret;
    }

    public boolean authenticate(String token) {
        return verify(token);
    }

    private boolean verify(String token) {
        return true;
    }
}
"#;
        let chunker = JavaChunker::new();
        let result = chunker.parse(source, "AuthService.java");

        let class = result.symbols.iter().find(|s| s.kind == SymbolKind::Class);
        assert!(class.is_some());
        assert_eq!(class.unwrap().name.as_deref(), Some("AuthService"));

        let methods: Vec<_> = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Method)
            .collect();
        assert_eq!(methods.len(), 2);

        let ctors: Vec<_> = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Constructor)
            .collect();
        assert_eq!(ctors.len(), 1);
    }

    #[test]
    fn test_java_imports() {
        let source = r#"import java.util.List;
import java.util.ArrayList;
import java.io.*;
"#;
        let chunker = JavaChunker::new();
        let result = chunker.parse(source, "Test.java");

        assert_eq!(result.imports.len(), 3);
        assert_eq!(result.imports[0].module_path, "java.util");
        assert!(result.imports[0].names.contains(&"List".to_string()));
        assert!(result
            .imports
            .iter()
            .any(|i| i.module_path == "java.io" && i.names.contains(&"*".to_string())));
    }
}
