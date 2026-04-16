use tree_sitter::{Language, Node, Parser, Tree};

use crate::symbol::{SymbolKind, Visibility};

#[derive(Debug, Clone)]
pub struct ImportInfo {
    pub module_path: String,
    pub names: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct StructuralRelation {
    pub source_name: String,
    pub target_name: String,
    pub rel_type: String,
}

#[derive(Debug, Clone)]
pub struct ParsedSymbol {
    pub name: Option<String>,
    pub kind: SymbolKind,
    pub line_start: usize,
    pub line_end: usize,
    pub source: String,
    pub signature: Option<String>,
    pub visibility: Visibility,
    pub doc_comment: Option<String>,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct ParseResult {
    pub symbols: Vec<ParsedSymbol>,
    pub imports: Vec<ImportInfo>,
    pub structural_rels: Vec<StructuralRelation>,
    pub tree: Option<Tree>,
}

pub trait LanguageChunker: Send + Sync {
    fn language(&self) -> Language;
    fn language_name(&self) -> &str;

    fn parse(&self, source: &str, file_path: &str) -> ParseResult {
        let mut parser = Parser::new();
        if parser.set_language(&self.language()).is_err() {
            return ParseResult {
                symbols: FileChunker::chunk_file(source, file_path, self.language_name()),
                imports: Vec::new(),
                structural_rels: Vec::new(),
                tree: None,
            };
        }
        let tree = match parser.parse(source, None) {
            Some(t) => t,
            None => {
                return ParseResult {
                    symbols: FileChunker::chunk_file(source, file_path, self.language_name()),
                    imports: Vec::new(),
                    structural_rels: Vec::new(),
                    tree: None,
                };
            }
        };

        let mut symbols = Vec::new();
        self.walk_declarations(&tree, source, file_path, &mut symbols);
        self.fill_gaps(&tree, source, file_path, &mut symbols);
        symbols.sort_by_key(|s| s.line_start);

        let imports = self.extract_imports(&tree, source);
        let structural_rels = self.extract_structural_rels(&tree, source);

        ParseResult {
            symbols,
            imports,
            structural_rels,
            tree: Some(tree),
        }
    }

    fn walk_declarations(
        &self,
        tree: &Tree,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<ParsedSymbol>,
    );

    fn fill_gaps(
        &self,
        _tree: &Tree,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<ParsedSymbol>,
    ) {
        fill_unclaimed_gaps(source, file_path, self.language_name(), symbols);
    }

    fn extract_imports(&self, _tree: &Tree, _source: &str) -> Vec<ImportInfo> {
        Vec::new()
    }

    fn extract_structural_rels(&self, _tree: &Tree, _source: &str) -> Vec<StructuralRelation> {
        Vec::new()
    }
}

fn lines_before(node: Node, source: &str, count: usize) -> String {
    let start_byte = node.start_byte();
    if start_byte == 0 {
        return String::new();
    }
    let prefix = &source[..start_byte];
    let mut lines = prefix.rsplit('\n');
    lines.next();
    let take = std::cmp::min(count, 3);
    lines
        .take(take)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n")
}

#[allow(clippy::too_many_arguments)]
pub fn make_parsed_symbol(
    source: &str,
    node: Node,
    _file_path: &str,
    _language: &str,
    kind: SymbolKind,
    name: Option<&str>,
    signature: Option<&str>,
    visibility: Visibility,
    extra_metadata: serde_json::Value,
) -> Option<ParsedSymbol> {
    let start_byte = node.start_byte();
    let end_byte = node.end_byte().min(source.len());

    if start_byte >= source.len() || end_byte <= start_byte {
        return None;
    }

    let text = &source[start_byte..end_byte];
    if text.trim().is_empty() {
        return None;
    }

    let context = lines_before(node, source, 3);

    let mut metadata = extra_metadata;
    if let serde_json::Value::Object(ref mut map) = metadata {
        if !context.is_empty() {
            map.insert(
                "context_before".to_string(),
                serde_json::Value::String(context),
            );
        }
    } else if metadata.is_null() {
        let mut map = serde_json::Map::new();
        if !context.is_empty() {
            map.insert(
                "context_before".to_string(),
                serde_json::Value::String(context),
            );
        }
        metadata = serde_json::Value::Object(map);
    }

    Some(ParsedSymbol {
        name: name.map(|s| s.to_string()),
        kind,
        line_start: node.start_position().row,
        line_end: node.end_position().row,
        source: text.to_string(),
        signature: signature.map(|s| s.to_string()),
        visibility,
        doc_comment: None,
        metadata,
    })
}

pub fn extract_name(node: Node, source: &str) -> Option<String> {
    if let Some(child) = node.child_by_field_name("name") {
        return child
            .utf8_text(source.as_bytes())
            .ok()
            .map(|s| s.to_string());
    }
    let first_named = node.children(&mut node.walk()).find(|c| c.is_named())?;
    first_named
        .utf8_text(source.as_bytes())
        .ok()
        .map(|s| s.to_string())
}

pub fn extract_signature(node: Node, source: &str) -> Option<String> {
    let text = node.utf8_text(source.as_bytes()).ok()?;
    let first_line = text.lines().next()?.trim();
    if first_line.len() <= 120 {
        Some(first_line.to_string())
    } else {
        None
    }
}

pub fn fill_unclaimed_gaps(
    source: &str,
    _file_path: &str,
    _language: &str,
    symbols: &mut Vec<ParsedSymbol>,
) {
    let mut claimed: Vec<(usize, usize)> =
        symbols.iter().map(|s| (s.line_start, s.line_end)).collect();
    claimed.sort();

    let source_lines: Vec<&str> = source.lines().collect();
    let total_lines = source_lines.len();
    let max_gap = 50;

    let mut gap_start = 0;
    for (start, end) in &claimed {
        if *start > gap_start && *end > gap_start {
            let effective_end = std::cmp::min(*start, gap_start + max_gap);
            if effective_end > gap_start {
                let text: String = source_lines[gap_start..effective_end].join("\n");
                if !text.trim().is_empty() {
                    symbols.push(ParsedSymbol {
                        name: None,
                        kind: SymbolKind::Section,
                        line_start: gap_start,
                        line_end: effective_end,
                        source: text,
                        signature: None,
                        visibility: Visibility::Package,
                        doc_comment: None,
                        metadata: serde_json::Value::Null,
                    });
                }
            }
        }
        gap_start = gap_start.max(end + 1);
    }

    if gap_start < total_lines {
        let effective_end = std::cmp::min(total_lines, gap_start + max_gap);
        let text: String = source_lines[gap_start..effective_end].join("\n");
        if !text.trim().is_empty() {
            symbols.push(ParsedSymbol {
                name: None,
                kind: SymbolKind::Section,
                line_start: gap_start,
                line_end: effective_end,
                source: text,
                signature: None,
                visibility: Visibility::Package,
                doc_comment: None,
                metadata: serde_json::Value::Null,
            });
        }
    }
}

struct FileChunker;

impl FileChunker {
    fn chunk_file(content: &str, _relative: &str, _language: &str) -> Vec<ParsedSymbol> {
        let lines: Vec<&str> = content.lines().collect();
        if lines.is_empty() {
            return Vec::new();
        }

        let mut symbols = Vec::new();
        let mut current_start = 0;
        let max_section_lines = 50;

        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            let is_boundary = Self::is_declaration(trimmed);

            if is_boundary && i > current_start {
                let text = lines[current_start..i].join("\n");
                if !text.trim().is_empty() {
                    symbols.push(ParsedSymbol {
                        name: None,
                        kind: SymbolKind::Section,
                        line_start: current_start,
                        line_end: i,
                        source: text,
                        signature: None,
                        visibility: Visibility::Package,
                        doc_comment: None,
                        metadata: serde_json::Value::Null,
                    });
                }
                current_start = i;
            }

            if (i - current_start >= max_section_lines)
                || (i == lines.len() - 1 && i >= current_start)
            {
                let end = if i == lines.len() - 1 { i + 1 } else { i };
                let text = lines[current_start..end].join("\n");
                if !text.trim().is_empty() {
                    symbols.push(ParsedSymbol {
                        name: None,
                        kind: SymbolKind::Section,
                        line_start: current_start,
                        line_end: end,
                        source: text,
                        signature: None,
                        visibility: Visibility::Package,
                        doc_comment: None,
                        metadata: serde_json::Value::Null,
                    });
                }
                current_start = end;
            }
        }

        symbols
    }

    fn is_declaration(trimmed: &str) -> bool {
        let keywords = [
            "fn ",
            "function ",
            "async function ",
            "const ",
            "let ",
            "var ",
            "class ",
            "interface ",
            "type ",
            "enum ",
            "struct ",
            "impl ",
            "trait ",
            "def ",
            "pub fn ",
            "pub struct ",
            "pub enum ",
            "pub trait ",
            "pub mod ",
            "mod ",
            "export function ",
            "export async function ",
            "export const ",
            "export default ",
            "export class ",
            "export interface ",
            "export type ",
            "@",
            "#[",
        ];
        keywords.iter().any(|kw| trimmed.starts_with(kw))
    }
}
