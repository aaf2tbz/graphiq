//! Symbol types — the core data model for code entities.
//!
//! Defines [`SymbolKind`] (function, method, class, struct, etc.),
//! [`Visibility`] (public, private, protected), [`Symbol`] (the main
//! symbol record with name, location, source, signature), [`SourceFile`]
//! (file metadata), and [`SymbolBuilder`] (fluent constructor with
//! sensible defaults).
//!
//! Symbols are the nodes in the code intelligence graph. Each symbol
//! represents a single code entity extracted by Tree-sitter parsing.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Classification of a code entity's type.
///
/// Maps to Tree-sitter node types: Function, Method, Class, Struct, Enum,
/// Interface, Trait, Module, Constant, Field, Import, etc.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SymbolKind {
    Function,
    Method,
    Constructor,
    Destructor,
    Class,
    Interface,
    Struct,
    Enum,
    EnumVariant,
    Trait,
    TypeAlias,
    Module,
    Namespace,
    Constant,
    Field,
    Property,
    Macro,
    Import,
    Export,
    Section,
}

impl SymbolKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SymbolKind::Function => "function",
            SymbolKind::Method => "method",
            SymbolKind::Constructor => "constructor",
            SymbolKind::Destructor => "destructor",
            SymbolKind::Class => "class",
            SymbolKind::Interface => "interface",
            SymbolKind::Struct => "struct",
            SymbolKind::Enum => "enum",
            SymbolKind::EnumVariant => "enum_variant",
            SymbolKind::Trait => "trait",
            SymbolKind::TypeAlias => "type_alias",
            SymbolKind::Module => "module",
            SymbolKind::Namespace => "namespace",
            SymbolKind::Constant => "constant",
            SymbolKind::Field => "field",
            SymbolKind::Property => "property",
            SymbolKind::Macro => "macro",
            SymbolKind::Import => "import",
            SymbolKind::Export => "export",
            SymbolKind::Section => "section",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "function" => Some(SymbolKind::Function),
            "method" => Some(SymbolKind::Method),
            "constructor" => Some(SymbolKind::Constructor),
            "destructor" => Some(SymbolKind::Destructor),
            "class" => Some(SymbolKind::Class),
            "interface" => Some(SymbolKind::Interface),
            "struct" => Some(SymbolKind::Struct),
            "enum" => Some(SymbolKind::Enum),
            "enum_variant" => Some(SymbolKind::EnumVariant),
            "trait" => Some(SymbolKind::Trait),
            "type_alias" => Some(SymbolKind::TypeAlias),
            "module" => Some(SymbolKind::Module),
            "namespace" => Some(SymbolKind::Namespace),
            "constant" => Some(SymbolKind::Constant),
            "field" => Some(SymbolKind::Field),
            "property" => Some(SymbolKind::Property),
            "macro" => Some(SymbolKind::Macro),
            "import" => Some(SymbolKind::Import),
            "export" => Some(SymbolKind::Export),
            "section" => Some(SymbolKind::Section),
            _ => None,
        }
    }

    pub fn default_importance(&self) -> f64 {
        match self {
            SymbolKind::Function => 0.9,
            SymbolKind::Method => 0.85,
            SymbolKind::Constructor => 0.85,
            SymbolKind::Destructor => 0.7,
            SymbolKind::Class => 0.85,
            SymbolKind::Interface => 0.85,
            SymbolKind::Struct => 0.8,
            SymbolKind::Enum => 0.75,
            SymbolKind::EnumVariant => 0.6,
            SymbolKind::Trait => 0.85,
            SymbolKind::TypeAlias => 0.65,
            SymbolKind::Module => 0.5,
            SymbolKind::Namespace => 0.5,
            SymbolKind::Constant => 0.7,
            SymbolKind::Field => 0.6,
            SymbolKind::Property => 0.65,
            SymbolKind::Macro => 0.7,
            SymbolKind::Import => 0.3,
            SymbolKind::Export => 0.4,
            SymbolKind::Section => 0.2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Visibility {
    Public,
    Private,
    Protected,
    Package,
    Anonymous,
}

impl Visibility {
    pub fn as_str(&self) -> &'static str {
        match self {
            Visibility::Public => "public",
            Visibility::Private => "private",
            Visibility::Protected => "protected",
            Visibility::Package => "package",
            Visibility::Anonymous => "anonymous",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "public" => Some(Visibility::Public),
            "private" => Some(Visibility::Private),
            "protected" => Some(Visibility::Protected),
            "package" => Some(Visibility::Package),
            "anonymous" => Some(Visibility::Anonymous),
            _ => None,
        }
    }
}

/// A single code entity extracted from the codebase.
///
/// Represents a function, type, variable, import, or other symbol with
/// its name, location (file + line range), source code, signature,
/// visibility, doc comment, and search hints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    pub id: i64,
    pub file_id: i64,
    pub name: String,
    pub qualified_name: Option<String>,
    pub kind: SymbolKind,
    pub line_start: u32,
    pub line_end: u32,
    pub signature: Option<String>,
    pub visibility: Visibility,
    pub doc_comment: Option<String>,
    pub source: String,
    pub name_decomposed: String,
    pub content_hash: String,
    pub language: String,
    pub metadata: serde_json::Value,
    pub importance: f64,
    pub search_hints: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceFile {
    pub id: i64,
    pub path: PathBuf,
    pub language: String,
    pub content_hash: String,
    pub mtime_ms: i64,
    pub line_count: u32,
}

#[derive(Debug, Clone)]
pub struct SymbolBuilder {
    pub file_id: i64,
    pub name: String,
    pub kind: SymbolKind,
    pub line_start: u32,
    pub line_end: u32,
    pub source: String,
    pub language: String,
    pub qualified_name: Option<String>,
    pub signature: Option<String>,
    pub visibility: Visibility,
    pub doc_comment: Option<String>,
    pub metadata: serde_json::Value,
}

impl SymbolBuilder {
    pub fn new(
        file_id: i64,
        name: String,
        kind: SymbolKind,
        source: String,
        language: String,
    ) -> Self {
        Self {
            file_id,
            name,
            kind,
            line_start: 0,
            line_end: 0,
            source,
            language,
            qualified_name: None,
            signature: None,
            visibility: Visibility::Public,
            doc_comment: None,
            metadata: serde_json::Value::Null,
        }
    }

    pub fn lines(mut self, start: u32, end: u32) -> Self {
        self.line_start = start;
        self.line_end = end;
        self
    }

    pub fn qualified_name(mut self, qn: impl Into<String>) -> Self {
        self.qualified_name = Some(qn.into());
        self
    }

    pub fn signature(mut self, sig: impl Into<String>) -> Self {
        self.signature = Some(sig.into());
        self
    }

    pub fn visibility(mut self, v: Visibility) -> Self {
        self.visibility = v;
        self
    }

    pub fn doc_comment(mut self, doc: impl Into<String>) -> Self {
        self.doc_comment = Some(doc.into());
        self
    }

    pub fn metadata(mut self, meta: serde_json::Value) -> Self {
        self.metadata = meta;
        self
    }

    pub fn build(self) -> Symbol {
        use crate::files::content_hash;
        use crate::tokenize::decompose_identifier;

        Symbol {
            id: 0,
            file_id: self.file_id,
            name: self.name.clone(),
            qualified_name: self.qualified_name,
            kind: self.kind,
            line_start: self.line_start,
            line_end: self.line_end,
            signature: self.signature,
            visibility: self.visibility,
            doc_comment: self.doc_comment,
            source: self.source.clone(),
            name_decomposed: decompose_identifier(&self.name),
            content_hash: content_hash(self.source.as_bytes()),
            language: self.language,
            metadata: self.metadata,
            importance: self.kind.default_importance(),
            search_hints: String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_kind_roundtrip() {
        for kind in [
            SymbolKind::Function,
            SymbolKind::Class,
            SymbolKind::Trait,
            SymbolKind::Enum,
            SymbolKind::Method,
            SymbolKind::Section,
        ] {
            assert_eq!(SymbolKind::from_str(kind.as_str()), Some(kind));
        }
    }

    #[test]
    fn test_visibility_roundtrip() {
        for vis in [
            Visibility::Public,
            Visibility::Private,
            Visibility::Protected,
        ] {
            assert_eq!(Visibility::from_str(vis.as_str()), Some(vis));
        }
    }

    #[test]
    fn test_symbol_builder() {
        let sym = SymbolBuilder::new(
            1,
            "authenticateUser".into(),
            SymbolKind::Function,
            "fn authenticateUser() {}".into(),
            "typescript".into(),
        )
        .lines(10, 15)
        .signature("fn authenticateUser(): Promise<User>")
        .build();

        assert_eq!(sym.name, "authenticateUser");
        assert_eq!(sym.name_decomposed, "authenticate user");
        assert_eq!(sym.kind, SymbolKind::Function);
        assert_eq!(sym.line_start, 10);
        assert_eq!(sym.line_end, 15);
        assert_eq!(sym.importance, 0.9);
    }
}
