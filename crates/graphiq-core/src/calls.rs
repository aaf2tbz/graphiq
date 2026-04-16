use tree_sitter::Tree;

#[derive(Debug, Clone)]
pub struct CallSite {
    pub callee: String,
    pub receiver: Option<String>,
    pub node_text: String,
    pub line: usize,
}

impl CallSite {
    pub fn display_name(&self) -> String {
        match &self.receiver {
            Some(r) => format!("{}.{}", r, self.callee),
            None => self.callee.clone(),
        }
    }
}

pub fn extract_calls(source: &str, tree: &Tree, language: &str) -> Vec<CallSite> {
    let source_bytes = source.as_bytes();
    match language {
        "typescript" | "javascript" | "tsx" | "jsx" => walk_and_collect(source_bytes, tree),
        "rust" => {
            let mut calls = walk_and_collect(source_bytes, tree);
            extract_rust_use_paths(source_bytes, tree, &mut calls);
            calls
        }
        "python" => {
            let mut calls = walk_and_collect(source_bytes, tree);
            dedup_calls(&mut calls);
            calls
        }
        "go" | "java" | "c" | "cpp" | "ruby" => walk_and_collect(source_bytes, tree),
        _ => regex_extract_calls(source),
    }
}

fn is_inside_string(node: tree_sitter::Node) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            "string" | "string_fragment" | "template_string" | "template_literal" | "comment"
            | "line_comment" | "block_comment" => return true,
            _ => current = parent.parent(),
        }
    }
    false
}

fn node_text(node: tree_sitter::Node, source: &[u8]) -> String {
    node.utf8_text(source)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn field_text(node: tree_sitter::Node, field: &str, source: &[u8]) -> Option<String> {
    node.child_by_field_name(field)
        .and_then(|n| n.utf8_text(source).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn extract_call_expression(source: &[u8], node: tree_sitter::Node, calls: &mut Vec<CallSite>) {
    if is_inside_string(node) {
        return;
    }

    let function = node.child_by_field_name("function");
    let arguments = node.child_by_field_name("arguments");

    let (callee, receiver) = match function {
        Some(fn_node) => {
            let kind = fn_node.kind();
            if kind == "member_expression"
                || kind == "selector_expression"
                || kind == "scoped_identifier"
                || kind == "field_expression"
                || kind == "attribute"
            {
                let prop = field_text(fn_node, "property", source)
                    .or_else(|| field_text(fn_node, "name", source))
                    .or_else(|| field_text(fn_node, "attribute", source))
                    .or_else(|| field_text(fn_node, "field", source))
                    .unwrap_or_default();
                let obj = field_text(fn_node, "object", source)
                    .or_else(|| field_text(fn_node, "scope", source))
                    .or_else(|| field_text(fn_node, "operand", source))
                    .unwrap_or_default();
                (prop, if obj.is_empty() { None } else { Some(obj) })
            } else {
                let text = fn_node
                    .utf8_text(source)
                    .ok()
                    .map(|s| s.trim().to_string())
                    .unwrap_or_default();
                if text.is_empty() || text.starts_with('(') {
                    (String::new(), None)
                } else {
                    (text, None)
                }
            }
        }
        None => (String::new(), None),
    };

    if !callee.is_empty() && !is_keyword_or_builtin(&callee) {
        let nt = node_text(node, source);
        let line = node.start_position().row;
        calls.push(CallSite {
            callee,
            receiver,
            node_text: nt,
            line,
        });
    }

    if let Some(fn_node) = function {
        let mut cursor = fn_node.walk();
        for child in fn_node.children(&mut cursor) {
            if child.is_named() && child.kind() == "call_expression" {
                extract_call_expression(source, child, calls);
            }
        }
    }

    if let Some(args) = arguments {
        let mut cursor = args.walk();
        for child in args.children(&mut cursor) {
            if child.is_named() && child.kind() == "call_expression" {
                extract_call_expression(source, child, calls);
            }
        }
    }
}

fn walk_and_collect(source: &[u8], tree: &Tree) -> Vec<CallSite> {
    let mut calls = Vec::new();
    walk_call_nodes(source, tree.root_node(), &mut calls);
    calls
}

fn walk_call_nodes(source: &[u8], node: tree_sitter::Node, calls: &mut Vec<CallSite>) {
    match node.kind() {
        "call_expression" | "call" => {
            extract_call_expression(source, node, calls);
            return;
        }
        "method_invocation" => {
            if is_inside_string(node) {
                return;
            }
            let member = field_text(node, "method", source).unwrap_or_default();
            let obj_text = field_text(node, "object", source).unwrap_or_default();

            if !member.is_empty() && !obj_text.is_empty() && !is_keyword_or_builtin(&member) {
                let nt = node_text(node, source);
                let line = node.start_position().row;
                calls.push(CallSite {
                    callee: member,
                    receiver: Some(obj_text),
                    node_text: nt,
                    line,
                });
            }
        }
        "await_expression" | "yield_expression" => {
            let value = node
                .child_by_field_name("argument")
                .or_else(|| node.child_by_field_name("value"));
            if let Some(val) = value {
                if val.kind() == "call_expression" {
                    extract_call_expression(source, val, calls);
                }
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_call_nodes(source, child, calls);
    }
}

fn extract_rust_use_paths(source: &[u8], tree: &Tree, calls: &mut Vec<CallSite>) {
    fn visit_use(source: &[u8], node: tree_sitter::Node, calls: &mut Vec<CallSite>) {
        if node.kind() == "use_item" || node.kind() == "use_declaration" {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.is_named() {
                    extract_use_tree(source, child, calls);
                }
            }
            return;
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            visit_use(source, child, calls);
        }
    }

    fn extract_use_tree(source: &[u8], node: tree_sitter::Node, calls: &mut Vec<CallSite>) {
        match node.kind() {
            "use_list" | "scoped_use_list" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() != ":" && child.kind() != "::" {
                        extract_use_tree(source, child, calls);
                    }
                }
            }
            "use_as_clause" => {
                let alias =
                    field_text(node, "alias", source).or_else(|| field_text(node, "name", source));
                if let Some(alias_text) = alias {
                    if !is_keyword_or_builtin(&alias_text) {
                        let line = node.start_position().row;
                        calls.push(CallSite {
                            callee: alias_text,
                            receiver: None,
                            node_text: node_text(node, source),
                            line,
                        });
                    }
                }
            }
            _ => {
                if let Ok(text) = node.utf8_text(source) {
                    let trimmed = text.trim();
                    let name = trimmed
                        .trim_start_matches("self::")
                        .trim_start_matches("Self::")
                        .trim_start_matches("super::")
                        .trim_start_matches("crate::");
                    if name.contains("::") {
                        if let Some((module, method)) = name.rsplit_once("::") {
                            let method = method.trim_end_matches(';').trim();
                            if !method.is_empty()
                                && !method.starts_with('{')
                                && !is_keyword_or_builtin(method)
                                && method != "*"
                            {
                                let line = node.start_position().row;
                                calls.push(CallSite {
                                    callee: method.to_string(),
                                    receiver: Some(module.to_string()),
                                    node_text: text.to_string(),
                                    line,
                                });
                            }
                        }
                    } else if !name.is_empty() && !is_keyword_or_builtin(name) {
                        let last = name.trim_end_matches(';').trim();
                        if !last.is_empty() && !last.starts_with('{') && last != "*" {
                            let line = node.start_position().row;
                            calls.push(CallSite {
                                callee: last.to_string(),
                                receiver: None,
                                node_text: text.to_string(),
                                line,
                            });
                        }
                    }
                }
            }
        }
    }

    visit_use(source, tree.root_node(), calls);
}

fn dedup_calls(calls: &mut Vec<CallSite>) {
    let mut seen = std::collections::HashSet::new();
    calls.retain(|c| seen.insert(c.display_name()));
}

fn regex_extract_calls(source: &str) -> Vec<CallSite> {
    let re = regex::Regex::new(r"(?:^|[^.\w])(\w+)\s*\(").unwrap();
    let mut calls = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for cap in re.captures_iter(source) {
        if let Some(name) = cap.get(1) {
            let s = name.as_str().to_string();
            let line = source[..name.start()].lines().count();
            if seen.insert(s.clone()) && !is_keyword_or_builtin(&s) {
                calls.push(CallSite {
                    callee: s,
                    receiver: None,
                    node_text: cap.get(0).unwrap().as_str().to_string(),
                    line,
                });
            }
        }
    }
    calls
}

fn is_keyword_or_builtin(s: &str) -> bool {
    matches!(
        s,
        "if" | "else"
            | "for"
            | "while"
            | "match"
            | "return"
            | "await"
            | "async"
            | "let"
            | "const"
            | "var"
            | "fn"
            | "function"
            | "new"
            | "delete"
            | "throw"
            | "try"
            | "catch"
            | "finally"
            | "import"
            | "export"
            | "from"
            | "class"
            | "extends"
            | "super"
            | "this"
            | "self"
            | "Self"
            | "print"
            | "println"
            | "println!"
            | "format!"
            | "vec!"
            | "dbg!"
            | "eprintln!"
            | "assert"
            | "assert_eq"
            | "assert_ne"
            | "assert!"
            | "panic!"
            | "unimplemented!"
            | "todo!"
            | "unreachable!"
            | "vec"
            | "Vec"
            | "String"
            | "HashMap"
            | "Option"
            | "Result"
            | "Some"
            | "None"
            | "Ok"
            | "Err"
            | "Box"
            | "Rc"
            | "Arc"
            | "true"
            | "false"
            | "mut"
            | "pub"
            | "use"
            | "mod"
            | "struct"
            | "enum"
            | "impl"
            | "trait"
            | "type"
            | "where"
            | "in"
            | "as"
            | "ref"
            | "static"
            | "dyn"
            | "box"
            | "move"
            | "loop"
            | "break"
            | "continue"
            | "yield"
            | "def"
            | "pass"
            | "with"
            | "isinstance"
            | "len"
            | "range"
            | "str"
            | "int"
            | "float"
            | "bool"
            | "list"
            | "dict"
            | "set"
            | "tuple"
            | "make"
            | "append"
            | "Error"
            | "Promise"
            | "console"
            | "log"
            | "typeof"
            | "instanceof"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_regex_fallback() {
        let calls = regex_extract_calls("unknown_lang_func();");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].callee, "unknown_lang_func");
    }

    #[test]
    fn test_regex_filters_keywords() {
        let calls = regex_extract_calls("for (var i = 0; i < 10; i++) {}");
        assert!(!calls.iter().any(|c| c.callee == "for" || c.callee == "var"));
    }

    #[test]
    fn test_callsite_display_name() {
        let cs = CallSite {
            callee: "method".into(),
            receiver: Some("obj".into()),
            node_text: "obj.method()".into(),
            line: 0,
        };
        assert_eq!(cs.display_name(), "obj.method");

        let cs2 = CallSite {
            callee: "func".into(),
            receiver: None,
            node_text: "func()".into(),
            line: 0,
        };
        assert_eq!(cs2.display_name(), "func");
    }
}
