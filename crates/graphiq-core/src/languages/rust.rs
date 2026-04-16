use tree_sitter::Tree;

use crate::chunker::{
    extract_name, extract_signature, fill_unclaimed_gaps, make_parsed_symbol, ImportInfo,
    LanguageChunker, ParsedSymbol, StructuralRelation,
};
use crate::symbol::{SymbolKind, Visibility};

pub struct RustChunker;

impl Default for RustChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl RustChunker {
    pub fn new() -> Self {
        Self
    }

    fn extract_visibility(&self, node: tree_sitter::Node, source: &str) -> Visibility {
        for child in node.children(&mut node.walk()) {
            if child.kind() == "visibility_modifier" {
                let text = child.utf8_text(source.as_bytes()).unwrap_or("");
                if text.contains("pub(crate)") || text.contains("pub(super)") {
                    return Visibility::Package;
                }
                return Visibility::Public;
            }
        }
        Visibility::Private
    }

    fn extract_impl_items(
        &self,
        impl_node: tree_sitter::Node,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<ParsedSymbol>,
    ) {
        let mut cursor = impl_node.walk();
        for child in impl_node.children(&mut cursor) {
            let kind = child.kind();
            if kind == "declaration_list" || kind == "trait_item" {
                let mut inner = child.walk();
                for item in child.children(&mut inner) {
                    let ik = item.kind();
                    if matches!(
                        ik,
                        "function_item" | "const_item" | "type_item" | "assoc_type_item"
                    ) {
                        let name = extract_name(item, source);
                        let sig = extract_signature(item, source);
                        let sk = match ik {
                            "function_item" => SymbolKind::Method,
                            "const_item" => SymbolKind::Constant,
                            _ => SymbolKind::TypeAlias,
                        };

                        let mut meta = serde_json::Map::new();
                        meta.insert("impl_member".into(), serde_json::Value::Bool(true));

                        if let Some(sym) = make_parsed_symbol(
                            source,
                            item,
                            file_path,
                            "rust",
                            sk,
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

impl LanguageChunker for RustChunker {
    fn language(&self) -> tree_sitter::Language {
        tree_sitter_rust::LANGUAGE.into()
    }

    fn language_name(&self) -> &str {
        "rust"
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
                "use_declaration" => {
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "rust",
                        SymbolKind::Import,
                        None,
                        None,
                        Visibility::Package,
                        serde_json::Value::Null,
                    ) {
                        symbols.push(sym);
                    }
                }
                "function_item" => {
                    let name = extract_name(child, source);
                    let sig = extract_signature(child, source);
                    let vis = self.extract_visibility(child, source);
                    let mut meta = serde_json::Map::new();
                    meta.insert(
                        "public".into(),
                        serde_json::Value::Bool(vis == Visibility::Public),
                    );
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "rust",
                        SymbolKind::Function,
                        name.as_deref(),
                        sig.as_deref(),
                        vis,
                        serde_json::Value::Object(meta),
                    ) {
                        symbols.push(sym);
                    }
                }
                "struct_item" => {
                    let name = extract_name(child, source);
                    let vis = self.extract_visibility(child, source);
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "rust",
                        SymbolKind::Struct,
                        name.as_deref(),
                        None,
                        vis,
                        serde_json::Value::Null,
                    ) {
                        symbols.push(sym);
                    }
                }
                "enum_item" => {
                    let name = extract_name(child, source);
                    let vis = self.extract_visibility(child, source);
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "rust",
                        SymbolKind::Enum,
                        name.as_deref(),
                        None,
                        vis,
                        serde_json::Value::Null,
                    ) {
                        symbols.push(sym);
                    }
                }
                "trait_item" => {
                    let name = extract_name(child, source);
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "rust",
                        SymbolKind::Trait,
                        name.as_deref(),
                        None,
                        Visibility::Public,
                        serde_json::Value::Null,
                    ) {
                        symbols.push(sym);
                    }
                    self.extract_impl_items(child, source, file_path, symbols);
                }
                "impl_item" => {
                    let name = extract_name(child, source);
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "rust",
                        SymbolKind::Module,
                        name.as_deref(),
                        None,
                        Visibility::Public,
                        serde_json::Value::Null,
                    ) {
                        symbols.push(sym);
                    }
                    self.extract_impl_items(child, source, file_path, symbols);
                }
                "mod_item" => {
                    let name = extract_name(child, source);
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "rust",
                        SymbolKind::Module,
                        name.as_deref(),
                        None,
                        Visibility::Public,
                        serde_json::Value::Null,
                    ) {
                        symbols.push(sym);
                    }
                }
                "const_item" | "static_item" => {
                    let name = extract_name(child, source);
                    let vis = self.extract_visibility(child, source);
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "rust",
                        SymbolKind::Constant,
                        name.as_deref(),
                        None,
                        vis,
                        serde_json::Value::Null,
                    ) {
                        symbols.push(sym);
                    }
                }
                "type_item" => {
                    let name = extract_name(child, source);
                    let vis = self.extract_visibility(child, source);
                    if let Some(sym) = make_parsed_symbol(
                        source,
                        child,
                        file_path,
                        "rust",
                        SymbolKind::TypeAlias,
                        name.as_deref(),
                        None,
                        vis,
                        serde_json::Value::Null,
                    ) {
                        symbols.push(sym);
                    }
                }
                "macro_invocation" => {
                    let text = child.utf8_text(source.as_bytes()).unwrap_or("");
                    let name = text.lines().next().unwrap_or("").trim();
                    if name.len() <= 80 && !name.starts_with('#') {
                        if let Some(sym) = make_parsed_symbol(
                            source,
                            child,
                            file_path,
                            "rust",
                            SymbolKind::Macro,
                            Some(name),
                            None,
                            Visibility::Public,
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

    fn extract_imports(&self, tree: &Tree, source: &str) -> Vec<ImportInfo> {
        let mut imports = Vec::new();

        for child in tree.root_node().children(&mut tree.root_node().walk()) {
            if child.kind() == "use_declaration" {
                let mut names = Vec::new();
                let mut module_path = String::new();

                if let Some(arg_node) = child.child_by_field_name("argument") {
                    let arg_text = arg_node.utf8_text(source.as_bytes()).unwrap_or("");
                    parse_use_path(arg_text, &mut module_path, &mut names);
                }

                if !module_path.is_empty() || !names.is_empty() {
                    imports.push(ImportInfo { module_path, names });
                }
            }
        }

        imports
    }

    fn extract_structural_rels(&self, tree: &Tree, source: &str) -> Vec<StructuralRelation> {
        let mut rels = Vec::new();
        let root = tree.root_node();

        for node in root.children(&mut root.walk()) {
            match node.kind() {
                "impl_item" => {
                    let trait_node = node.child_by_field_name("trait");
                    let type_node = node.child_by_field_name("type");
                    if let (Some(trait_n), Some(type_n)) = (trait_node, type_node) {
                        let trait_name = trait_n
                            .utf8_text(source.as_bytes())
                            .unwrap_or("")
                            .trim()
                            .to_string();
                        let type_name = type_n
                            .utf8_text(source.as_bytes())
                            .unwrap_or("")
                            .trim()
                            .to_string();
                        if !trait_name.is_empty() && !type_name.is_empty() {
                            rels.push(StructuralRelation {
                                source_name: type_name,
                                target_name: trait_name,
                                rel_type: "implements".into(),
                            });
                        }
                    }
                }
                "struct_item" => {
                    if let Some(field_list) = node.child_by_field_name("body") {
                        for child in field_list.children(&mut field_list.walk()) {
                            if child.kind() == "field_declaration" {
                                if let Some(type_n) = child.child_by_field_name("type") {
                                    let type_name = type_n
                                        .utf8_text(source.as_bytes())
                                        .unwrap_or("")
                                        .trim()
                                        .to_string();
                                    if !type_name.is_empty()
                                        && type_name.starts_with(|c: char| c.is_uppercase())
                                    {
                                        let struct_name = node
                                            .child_by_field_name("name")
                                            .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                                            .unwrap_or("")
                                            .trim()
                                            .to_string();
                                        if !struct_name.is_empty() {
                                            rels.push(StructuralRelation {
                                                source_name: struct_name,
                                                target_name: type_name,
                                                rel_type: "contains".into(),
                                            });
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

        rels
    }

    fn fill_gaps(
        &self,
        _tree: &Tree,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<ParsedSymbol>,
    ) {
        fill_unclaimed_gaps(source, file_path, "rust", symbols);
    }
}

fn parse_use_path(text: &str, module_path: &mut String, names: &mut Vec<String>) {
    let trimmed = text.trim();

    if let Some(brace_pos) = trimmed.find("::{") {
        *module_path = trimmed[..brace_pos].to_string();
        let inner = &trimmed[brace_pos + 3..];
        let inner = inner.trim_start_matches('{').trim_end_matches('}');
        for item in inner.split(',') {
            let item = item.trim();
            if item.is_empty() {
                continue;
            }
            if let Some(as_pos) = item.find(" as ") {
                names.push(item[..as_pos].trim().to_string());
            } else {
                names.push(item.to_string());
            }
        }
    } else if trimmed.ends_with("::*") {
        *module_path = trimmed.trim_end_matches("::*").to_string();
    } else if let Some(as_pos) = trimmed.find(" as ") {
        let path = trimmed[..as_pos].trim();
        if let Some(last) = path.rfind("::") {
            *module_path = path[..last].to_string();
            names.push(path[last + 2..].to_string());
        } else {
            *module_path = path.to_string();
        }
    } else if let Some(last) = trimmed.rfind("::") {
        *module_path = trimmed[..last].to_string();
        names.push(trimmed[last + 2..].to_string());
    } else {
        *module_path = trimmed.to_string();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rust_function() {
        let source = r#"use std::collections::HashMap;

pub fn authenticate(credentials: &Credentials) -> Result<Auth, Error> {
    let user = db::find_user(&credentials.email)?;
    Ok(Auth::new(user))
}

fn private_helper() -> bool {
    true
}
"#;
        let chunker = RustChunker::new();
        let result = chunker.parse(source, "src/auth.rs");

        let funcs: Vec<_> = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Function)
            .collect();
        assert_eq!(funcs.len(), 2);
        assert_eq!(funcs[0].name.as_deref(), Some("authenticate"));
        assert_eq!(funcs[1].name.as_deref(), Some("private_helper"));
        assert_eq!(funcs[0].visibility, Visibility::Public);
        assert_eq!(funcs[1].visibility, Visibility::Private);
    }

    #[test]
    fn test_rust_struct_and_impl() {
        let source = r#"
pub struct User {
    id: String,
    email: String,
}

impl User {
    pub fn new(id: &str, email: &str) -> Self {
        Self { id: id.to_string(), email: email.to_string() }
    }

    fn is_valid(&self) -> bool {
        !self.email.is_empty()
    }
}
"#;
        let chunker = RustChunker::new();
        let result = chunker.parse(source, "src/user.rs");

        let structs: Vec<_> = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Struct)
            .collect();
        assert_eq!(structs.len(), 1);
        assert_eq!(structs[0].name.as_deref(), Some("User"));

        let methods: Vec<_> = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Method)
            .collect();
        assert_eq!(methods.len(), 2);
        assert_eq!(methods[0].name.as_deref(), Some("new"));
        assert_eq!(methods[1].name.as_deref(), Some("is_valid"));
    }

    #[test]
    fn test_rust_extract_imports() {
        let source = "use crate::chunker::LanguageChunker;";
        let chunker = RustChunker::new();
        let result = chunker.parse(source, "test.rs");
        assert_eq!(result.imports.len(), 1);
        assert_eq!(result.imports[0].module_path, "crate::chunker");
        assert_eq!(result.imports[0].names, vec!["LanguageChunker"]);
    }

    #[test]
    fn test_rust_extract_grouped_imports() {
        let source = "use crate::files::{SourceFile, walk_project, content_hash};";
        let chunker = RustChunker::new();
        let result = chunker.parse(source, "test.rs");
        assert_eq!(result.imports.len(), 1);
        assert_eq!(result.imports[0].module_path, "crate::files");
        assert!(result.imports[0].names.contains(&"SourceFile".to_string()));
    }

    #[test]
    fn test_rust_structural_rels() {
        let source = r#"
struct User {
    name: String,
    address: Address,
}

impl Display for User {
    fn fmt(&self, f: &mut Formatter) -> Result {
        write!(f, "{}", self.name)
    }
}
"#;
        let chunker = RustChunker::new();
        let result = chunker.parse(source, "src/user.rs");
        assert!(result
            .structural_rels
            .iter()
            .any(|r| r.rel_type == "implements"
                && r.source_name == "User"
                && r.target_name == "Display"));
        assert!(result
            .structural_rels
            .iter()
            .any(|r| r.rel_type == "contains" && r.target_name == "Address"));
    }
}
