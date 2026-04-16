use tree_sitter::{Node, Tree};

use crate::chunker::{
    extract_name, extract_signature, fill_unclaimed_gaps, make_parsed_symbol, ImportInfo,
    LanguageChunker, ParsedSymbol, StructuralRelation,
};
use crate::symbol::{SymbolKind, Visibility};

pub struct TypeScriptChunker {
    language: tree_sitter::Language,
}

impl Default for TypeScriptChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeScriptChunker {
    pub fn new() -> Self {
        Self {
            language: tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        }
    }

    pub fn tsx() -> Self {
        Self {
            language: tree_sitter_typescript::LANGUAGE_TSX.into(),
        }
    }
}

impl LanguageChunker for TypeScriptChunker {
    fn language(&self) -> tree_sitter::Language {
        self.language.clone()
    }

    fn language_name(&self) -> &str {
        "typescript"
    }

    fn walk_declarations(
        &self,
        tree: &Tree,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<ParsedSymbol>,
    ) {
        let mut cursor = tree.root_node().walk();

        let decl_kinds = [
            "function_declaration",
            "generator_function_declaration",
            "arrow_function",
            "class_declaration",
            "interface_declaration",
            "type_alias_declaration",
            "enum_declaration",
            "variable_declaration",
            "lexical_declaration",
        ];

        for child in tree.root_node().children(&mut cursor) {
            let kind = child.kind();

            if kind == "import_statement" || kind == "import_declaration" {
                if let Some(sym) = make_parsed_symbol(
                    source,
                    child,
                    file_path,
                    "typescript",
                    SymbolKind::Import,
                    None,
                    None,
                    Visibility::Package,
                    serde_json::Value::Null,
                ) {
                    symbols.push(sym);
                }
                continue;
            }

            if kind == "export_statement" {
                if let Some(named_child) = child.child(1) {
                    let inner_kind = named_child.kind();
                    if matches!(
                        inner_kind,
                        "function_declaration"
                            | "class_declaration"
                            | "interface_declaration"
                            | "arrow_function"
                            | "variable_declaration"
                    ) {
                        let name = extract_name(named_child, source);
                        let sig = extract_signature(named_child, source);
                        let sk = match inner_kind {
                            "function_declaration" | "generator_function_declaration" => {
                                SymbolKind::Function
                            }
                            "class_declaration" => SymbolKind::Class,
                            "interface_declaration" => SymbolKind::Interface,
                            "arrow_function" => SymbolKind::Function,
                            "variable_declaration" => SymbolKind::Constant,
                            _ => SymbolKind::Section,
                        };

                        let mut meta = serde_json::Map::new();
                        meta.insert("exported".into(), serde_json::Value::Bool(true));

                        if let Some(sym) = make_parsed_symbol(
                            source,
                            named_child,
                            file_path,
                            "typescript",
                            sk,
                            name.as_deref(),
                            sig.as_deref(),
                            Visibility::Public,
                            serde_json::Value::Object(meta),
                        ) {
                            symbols.push(sym);
                        }

                        if inner_kind == "class_declaration" {
                            Self::extract_class_members(named_child, source, file_path, symbols);
                        }
                        continue;
                    }
                }
                let name = extract_name(child, source);
                if let Some(sym) = make_parsed_symbol(
                    source,
                    child,
                    file_path,
                    "typescript",
                    SymbolKind::Export,
                    name.as_deref(),
                    None,
                    Visibility::Public,
                    serde_json::Value::Null,
                ) {
                    symbols.push(sym);
                }
                continue;
            }

            if decl_kinds.contains(&kind) {
                let name = extract_name(child, source);
                let sig = extract_signature(child, source);
                let sk = match kind {
                    "function_declaration" | "generator_function_declaration" => {
                        SymbolKind::Function
                    }
                    "class_declaration" => SymbolKind::Class,
                    "interface_declaration" => SymbolKind::Interface,
                    "type_alias_declaration" => SymbolKind::TypeAlias,
                    "enum_declaration" => SymbolKind::Enum,
                    "variable_declaration" | "lexical_declaration" => SymbolKind::Constant,
                    _ => SymbolKind::Section,
                };

                let mut meta = serde_json::Map::new();
                if child.child_by_field_name("export").is_some() {
                    meta.insert("exported".into(), serde_json::Value::Bool(true));
                }

                if let Some(sym) = make_parsed_symbol(
                    source,
                    child,
                    file_path,
                    "typescript",
                    sk,
                    name.as_deref(),
                    sig.as_deref(),
                    Visibility::Public,
                    serde_json::Value::Object(meta),
                ) {
                    symbols.push(sym);
                }

                if kind == "class_declaration" {
                    Self::extract_class_members(child, source, file_path, symbols);
                }
            }
        }
    }

    fn extract_imports(&self, tree: &Tree, source: &str) -> Vec<ImportInfo> {
        let mut imports = Vec::new();
        let mut cursor = tree.root_node().walk();

        for child in tree.root_node().children(&mut cursor) {
            if child.kind() == "import_statement" || child.kind() == "import_declaration" {
                let mut module_path = String::new();
                let mut names = Vec::new();
                Self::walk_import_nodes(child, source, &mut module_path, &mut names);
                if !module_path.is_empty() {
                    imports.push(ImportInfo { module_path, names });
                }
            }
        }
        imports
    }

    fn extract_structural_rels(&self, tree: &Tree, source: &str) -> Vec<StructuralRelation> {
        let mut rels = Vec::new();
        let root = tree.root_node();

        let mut class_nodes: Vec<tree_sitter::Node> = Vec::new();
        for node in root.children(&mut root.walk()) {
            if node.kind() == "class_declaration" {
                class_nodes.push(node);
            } else if node.kind() == "export_statement" {
                if let Some(inner) = node.child_by_field_name("declaration") {
                    if inner.kind() == "class_declaration" {
                        class_nodes.push(inner);
                    }
                }
            }
        }

        for node in class_nodes {
            let class_name = node
                .child_by_field_name("name")
                .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                .unwrap_or("")
                .trim()
                .to_string();
            if class_name.is_empty() {
                continue;
            }

            for cn in node.children(&mut node.walk()) {
                match cn.kind() {
                    "class_heritage" => {
                        for hc in cn.children(&mut cn.walk()) {
                            match hc.kind() {
                                "extends_clause" => {
                                    for ec in hc.children(&mut hc.walk()) {
                                        if ec.is_named() {
                                            let parent = ec
                                                .utf8_text(source.as_bytes())
                                                .unwrap_or("")
                                                .trim()
                                                .to_string();
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
                                "implements_clause" => {
                                    for ic in hc.children(&mut hc.walk()) {
                                        if ic.kind() == "type_identifier"
                                            || ic.kind() == "identifier"
                                            || ic.kind() == "generic_type"
                                        {
                                            let iface = ic
                                                .utf8_text(source.as_bytes())
                                                .unwrap_or("")
                                                .trim()
                                                .to_string();
                                            if !iface.is_empty() {
                                                rels.push(StructuralRelation {
                                                    source_name: class_name.clone(),
                                                    target_name: iface,
                                                    rel_type: "implements".into(),
                                                });
                                            }
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
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
        fill_unclaimed_gaps(source, file_path, "typescript", symbols);
    }
}

impl TypeScriptChunker {
    fn extract_class_members(
        class_node: Node,
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
            if matches!(
                kind,
                "method_definition"
                    | "public_field_definition"
                    | "property_definition"
                    | "method_signature"
                    | "abstract_method_declaration"
                    | "constructor_definition"
            ) {
                let name = child
                    .child_by_field_name("name")
                    .or_else(|| child.children(&mut child.walk()).find(|c| c.is_named()))
                    .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                    .map(|s| s.to_string());

                let sig = extract_signature(child, source);

                let sk = match kind {
                    "method_definition"
                    | "method_signature"
                    | "abstract_method_declaration"
                    | "constructor_definition" => SymbolKind::Method,
                    "public_field_definition" | "property_definition" => SymbolKind::Field,
                    _ => SymbolKind::Section,
                };

                let mut meta = serde_json::Map::new();
                meta.insert("class_member".into(), serde_json::Value::Bool(true));

                if let Some(sym) = make_parsed_symbol(
                    source,
                    child,
                    file_path,
                    "typescript",
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

    fn walk_import_nodes(
        node: tree_sitter::Node,
        source: &str,
        module_path: &mut String,
        names: &mut Vec<String>,
    ) {
        for gc in node.children(&mut node.walk()) {
            match gc.kind() {
                "string" => {
                    *module_path = gc
                        .utf8_text(source.as_bytes())
                        .unwrap_or("")
                        .trim_matches('"')
                        .trim_matches('\'')
                        .trim_start_matches('`')
                        .trim_end_matches('`')
                        .to_string();
                }
                "import_specifier" => {
                    if let Some(name_node) = gc.child_by_field_name("name") {
                        if let Ok(name) = name_node.utf8_text(source.as_bytes()) {
                            names.push(name.trim().to_string());
                        }
                    }
                }
                "import_clause" | "named_imports" | "import_specifiers" | "es_import_clause" => {
                    Self::walk_import_nodes(gc, source, module_path, names);
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_typescript_function() {
        let source = r#"
import { something } from './module';

export async function authenticateUser(
  credentials: Credentials
): Promise<AuthResult> {
  const user = await db.findUser(credentials.email);
  return createSession(user);
}

const MAX_RETRIES = 3;
"#;
        let chunker = TypeScriptChunker::new();
        let result = chunker.parse(source, "src/auth.ts");

        let func = result
            .symbols
            .iter()
            .find(|s| s.kind == SymbolKind::Function);
        assert!(func.is_some());
        let func = func.unwrap();
        assert_eq!(func.name.as_deref(), Some("authenticateUser"));
        assert!(func.source.contains("async function authenticateUser"));

        let imp = result.symbols.iter().find(|s| s.kind == SymbolKind::Import);
        assert!(imp.is_some());

        let constant = result
            .symbols
            .iter()
            .find(|s| s.kind == SymbolKind::Constant);
        assert!(constant.is_some());
    }

    #[test]
    fn test_typescript_class() {
        let source = r#"
class AuthService {
  private user: User | null = null;

  async login(email: string, password: string): Promise<boolean> {
    this.user = await db.findUser(email);
    return this.user !== null;
  }

  logout(): void {
    this.user = null;
  }
}
"#;
        let chunker = TypeScriptChunker::new();
        let result = chunker.parse(source, "src/auth/service.ts");

        let class = result.symbols.iter().find(|s| s.kind == SymbolKind::Class);
        assert!(class.is_some());
        assert_eq!(class.unwrap().name.as_deref(), Some("AuthService"));

        let methods: Vec<_> = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Method)
            .collect();
        assert_eq!(methods.len(), 2);
    }

    #[test]
    fn test_typescript_extract_imports() {
        let source = r#"
import { authenticate } from './auth';
import { User, Session } from './models/user';
"#;
        let chunker = TypeScriptChunker::new();
        let result = chunker.parse(source, "src/imports.ts");

        assert_eq!(result.imports.len(), 2);
        assert_eq!(result.imports[0].module_path, "./auth");
        assert!(result.imports[0]
            .names
            .contains(&"authenticate".to_string()));
        assert_eq!(result.imports[1].module_path, "./models/user");
        assert!(result.imports[1].names.contains(&"User".to_string()));
    }

    #[test]
    fn test_typescript_structural_rels() {
        let source = r#"
class AuthService extends BaseService implements IAuth {
  login(): void {}
}
"#;
        let chunker = TypeScriptChunker::new();
        let result = chunker.parse(source, "src/auth.ts");

        assert!(result
            .structural_rels
            .iter()
            .any(|r| r.rel_type == "extends" && r.target_name == "BaseService"));
        assert!(result
            .structural_rels
            .iter()
            .any(|r| r.rel_type == "implements" && r.target_name == "IAuth"));
    }
}
