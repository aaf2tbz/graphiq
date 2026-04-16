use tree_sitter::Tree;

use crate::chunker::{
    extract_name, extract_signature, fill_unclaimed_gaps, make_parsed_symbol, ImportInfo,
    LanguageChunker, ParsedSymbol, StructuralRelation,
};
use crate::symbol::{SymbolKind, Visibility};

pub struct PythonChunker;

impl Default for PythonChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl PythonChunker {
    pub fn new() -> Self {
        Self
    }
}

impl LanguageChunker for PythonChunker {
    fn language(&self) -> tree_sitter::Language {
        tree_sitter_python::LANGUAGE.into()
    }

    fn language_name(&self) -> &str {
        "python"
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
                "import_statement" | "import_from_statement" | "future_import_statement" => {
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "python",
                        SymbolKind::Import,
                        None,
                        None,
                        Visibility::Package,
                        serde_json::Value::Null,
                    ) {
                        symbols.push(sym);
                    }
                }
                "function_definition" => {
                    let name = extract_name(child, source);
                    let sig = extract_signature(child, source);
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "python",
                        SymbolKind::Function,
                        name.as_deref(),
                        sig.as_deref(),
                        Visibility::Public,
                        serde_json::Value::Null,
                    ) {
                        symbols.push(sym);
                    }
                }
                "class_definition" => {
                    let name = extract_name(child, source);
                    let sig = extract_signature(child, source);
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "python",
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
                "decorated_definition" => {
                    let inner = child.child_by_field_name("definition");
                    if let Some(inner) = inner {
                        let inner_kind = inner.kind();
                        if inner_kind == "function_definition" {
                            let name = extract_name(inner, source);
                            let sig = extract_signature(inner, source);
                            let mut meta = serde_json::Map::new();
                            let dec_text = child.utf8_text(source.as_bytes()).unwrap_or("");
                            for line in dec_text.lines() {
                                let trimmed = line.trim();
                                if trimmed.starts_with('@') {
                                    meta.insert(
                                        "decorator".into(),
                                        serde_json::Value::String(trimmed.to_string()),
                                    );
                                    break;
                                }
                            }
                            if let Some(sym) = make_parsed_symbol(
                                source,
                                child,
                                file_path,
                                "python",
                                SymbolKind::Function,
                                name.as_deref(),
                                sig.as_deref(),
                                Visibility::Public,
                                serde_json::Value::Object(meta),
                            ) {
                                symbols.push(sym);
                            }
                        } else if inner_kind == "class_definition" {
                            let name = extract_name(inner, source);
                            let sig = extract_signature(inner, source);
                            if let Some(sym) = make_parsed_symbol(
                                source,
                                child,
                                file_path,
                                "python",
                                SymbolKind::Class,
                                name.as_deref(),
                                sig.as_deref(),
                                Visibility::Public,
                                serde_json::Value::Null,
                            ) {
                                symbols.push(sym);
                            }
                            Self::extract_class_members(inner, source, file_path, symbols);
                        }
                    }
                }
                "expression_statement" => {
                    for expr_child in child.children(&mut child.walk()) {
                        if expr_child.kind() == "assignment" {
                            if let Some(lhs) = expr_child.child_by_field_name("left") {
                                if let Ok(text) = lhs.utf8_text(source.as_bytes()) {
                                    let name = text.trim();
                                    if name.chars().all(|c| c.is_ascii_uppercase() || c == '_')
                                        && name.contains('_')
                                    {
                                        if let Some(sym) = make_parsed_symbol(
                                            source,
                                            child,
                                            file_path,
                                            "python",
                                            SymbolKind::Constant,
                                            Some(name),
                                            None,
                                            Visibility::Public,
                                            serde_json::Value::Null,
                                        ) {
                                            symbols.push(sym);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn extract_imports(&self, tree: &Tree, source: &str) -> Vec<ImportInfo> {
        let mut imports = Vec::new();
        for child in tree.root_node().children(&mut tree.root_node().walk()) {
            if child.kind() == "import_from_statement" {
                let mut module_path = String::new();
                let mut names = Vec::new();
                for gc in child.children(&mut child.walk()) {
                    match gc.kind() {
                        "dotted_name" | "relative_import" => {
                            module_path = gc
                                .utf8_text(source.as_bytes())
                                .unwrap_or("")
                                .trim()
                                .to_string();
                        }
                        "import_specifier" | "wildcard_import" => {
                            if gc.kind() == "wildcard_import" {
                                names.push("*".to_string());
                            } else if let Some(name_node) = gc.child_by_field_name("name") {
                                if let Ok(name) = name_node.utf8_text(source.as_bytes()) {
                                    names.push(name.trim().to_string());
                                }
                            }
                        }
                        "aliased_import" => {
                            if let Some(name_node) = gc.child_by_field_name("name") {
                                if let Ok(name) = name_node.utf8_text(source.as_bytes()) {
                                    names.push(name.trim().to_string());
                                }
                            }
                        }
                        _ => {}
                    }
                }
                if !module_path.is_empty() || !names.is_empty() {
                    imports.push(ImportInfo { module_path, names });
                }
            } else if child.kind() == "import_statement" {
                let text = child.utf8_text(source.as_bytes()).unwrap_or("").trim();
                let module_path = text.trim_start_matches("import ").trim().to_string();
                if !module_path.is_empty() {
                    imports.push(ImportInfo {
                        module_path,
                        names: Vec::new(),
                    });
                }
            }
        }
        imports
    }

    fn extract_structural_rels(&self, tree: &Tree, source: &str) -> Vec<StructuralRelation> {
        let mut rels = Vec::new();
        for node in tree.root_node().children(&mut tree.root_node().walk()) {
            if node.kind() == "class_definition" {
                let class_name = node
                    .child_by_field_name("name")
                    .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if class_name.is_empty() {
                    continue;
                }
                for arg in node.children_by_field_name("superclasses", &mut node.walk()) {
                    if let Ok(text) = arg.utf8_text(source.as_bytes()) {
                        let parent = text.trim().to_string();
                        if !parent.is_empty() {
                            rels.push(StructuralRelation {
                                source_name: class_name.clone(),
                                target_name: parent,
                                rel_type: "extends".into(),
                            });
                        }
                    }
                }
            }
        }
        rels
    }

    fn fill_gaps(
        &self,
        _tree: &Tree,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<ParsedSymbol>,
    ) {
        fill_unclaimed_gaps(source, file_path, "python", symbols);
    }
}

impl PythonChunker {
    fn extract_class_members(
        class_node: tree_sitter::Node,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<ParsedSymbol>,
    ) {
        let body = match class_node.child_by_field_name("body") {
            Some(b) => b,
            None => return,
        };

        for child in body.children(&mut body.walk()) {
            let kind = child.kind();
            if kind == "function_definition" {
                let name = extract_name(child, source);
                let sig = extract_signature(child, source);
                let mut meta = serde_json::Map::new();
                meta.insert("class_member".into(), serde_json::Value::Bool(true));
                if let Some(sym) = make_parsed_symbol(
                    source,
                    child,
                    file_path,
                    "python",
                    SymbolKind::Method,
                    name.as_deref(),
                    sig.as_deref(),
                    Visibility::Public,
                    serde_json::Value::Object(meta),
                ) {
                    symbols.push(sym);
                }
            } else if kind == "decorated_definition" {
                if let Some(inner) = child.child_by_field_name("definition") {
                    if inner.kind() == "function_definition" {
                        let name = extract_name(inner, source);
                        let sig = extract_signature(inner, source);
                        let mut meta = serde_json::Map::new();
                        meta.insert("class_member".into(), serde_json::Value::Bool(true));
                        if let Some(sym) = make_parsed_symbol(
                            source,
                            child,
                            file_path,
                            "python",
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_python_function() {
        let source = r#"
import os
from typing import List

def authenticate(username: str, password: str) -> bool:
    return verify(username, password)

class AuthService:
    def login(self, email: str) -> User:
        return self.db.find(email)
"#;
        let chunker = PythonChunker::new();
        let result = chunker.parse(source, "src/auth.py");

        let func = result
            .symbols
            .iter()
            .find(|s| s.kind == SymbolKind::Function);
        assert!(func.is_some());
        assert_eq!(func.unwrap().name.as_deref(), Some("authenticate"));

        let class = result.symbols.iter().find(|s| s.kind == SymbolKind::Class);
        assert!(class.is_some());
        assert_eq!(class.unwrap().name.as_deref(), Some("AuthService"));

        let methods: Vec<_> = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Method)
            .collect();
        assert_eq!(methods.len(), 1);
    }
}
