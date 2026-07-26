//! Squirrel parser plugin — full-parse mode.
//!
//! Handles `.nut` files.
//! Uses tree-sitter-squirrel directly (no Python grammar package needed).

use intentdiff_plugin_sdk::ts_convert::{convert_ts_direct, TsDirectHooks};
use intentdiff_plugin_sdk::tree::{SemanticNode, SemanticNodeBuilder};

wit_bindgen::generate!({
    path: "wit/plugin.wit",
    world: "parser-plugin",
});

use crate::exports::intentdiff::plugin::parser::ExamplePair;
use crate::exports::intentdiff::plugin::parser::Guest;
use crate::exports::intentdiff::plugin::parser::LanguageInfoRecord;
use crate::exports::intentdiff::plugin::parser::ParserMode;

const PLUGIN_METADATA: &str = include_str!("../plugin_metadata.info");

fn language_info_for(ids: Vec<String>) -> Vec<LanguageInfoRecord> {
    let metadata = intentdiff_plugin_sdk::metadata::parse_plugin_metadata(PLUGIN_METADATA);
    ids.into_iter()
        .map(|language_id| {
            let info = metadata.language_or_default(&language_id);
            LanguageInfoRecord {
                language_id: info.language_id,
                language_name: info.language_name,
                language_short_name: info.language_short_name,
                monaco_language: info.monaco_language,
                default_filename: info.default_filename,
                language_file_extensions: info.language_file_extensions,
                author: metadata.author().to_string(),
                plugin_version: metadata.plugin_version().to_string(),
                last_updated: metadata.last_updated().to_string(),
            }
        })
        .collect()
}
struct SquirrelParser;

const TRIVIA: &[&str] = &["comment", "line_comment", "block_comment", "whitespace"];

const SEMANTIC_TYPES: &[&str] = &[
    // Root
    "source_file",
    // Declarations
    "function_declaration",
    "class_declaration",
    "enum_declaration",
    "constructor_declaration",
    // Variables
    "var_statement",
    "local_statement",
    "const_statement",
    "static_declaration",
    // Statements
    "expression_statement",
    "assignment_expression",
    "if_statement",
    "else_clause",
    "for_statement",
    "foreach_statement",
    "while_statement",
    "do_while_statement",
    "switch_statement",
    "case_clause",
    "default_clause",
    "try_statement",
    "catch_clause",
    "return_statement",
    "throw_statement",
    "break_statement",
    "continue_statement",
    "yield_statement",
    // Expressions
    "call_expression",
    "member_expression",
    "index_expression",
    "function_expression",
    "lambda_expression",
    "table_constructor",
    "array_constructor",
    "string",
    "integer",
    "float",
    "boolean",
    "null",
    "identifier",
];

fn is_semantic(node_type: &str) -> bool {
    SEMANTIC_TYPES.contains(&node_type)
}

fn is_class_like(node_type: &str) -> bool {
    matches!(node_type, "class_declaration")
}

fn is_method_like(node_type: &str) -> bool {
    matches!(
        node_type,
        "function_declaration" | "constructor_declaration"
    )
}

fn label_for_ts(node: tree_sitter::Node<'_>, source: &[u8]) -> String {
    let kind = node.kind();
    let txt = |n: tree_sitter::Node<'_>| n.utf8_text(source).unwrap_or("").to_string();
    if node.child_count() == 0 {
        return node.utf8_text(source).unwrap_or("").to_string();
    }
    match kind {
        "class_declaration" | "enum_declaration" => {
            for i in 0..node.child_count() {
                let c = node.child(i).unwrap();
                if c.kind() == "identifier" || c.kind() == "type_identifier" {
                    return txt(c);
                }
            }
        }
        "function_declaration" | "constructor_declaration" => {
            for i in 0..node.child_count() {
                let c = node.child(i).unwrap();
                if c.kind() == "identifier" || c.kind() == "function_name" {
                    return txt(c);
                }
            }
        }
        "var_statement" | "local_statement" | "const_statement" | "static_declaration" => {
            for i in 0..node.child_count() {
                let c = node.child(i).unwrap();
                if c.kind() == "identifier" {
                    return txt(c);
                }
                if c.kind() == "variable_declarator" {
                    if let Some(first) = c.child(0) {
                        return txt(first);
                    }
                }
            }
        }
        "call_expression" => {
            for i in 0..node.child_count() {
                let c = node.child(i).unwrap();
                if matches!(
                    c.kind(),
                    "identifier" | "member_expression" | "function_name"
                ) {
                    return txt(c);
                }
            }
        }
        _ => {}
    }
    for i in 0..node.child_count() {
        let c = node.child(i).unwrap();
        if c.kind() == "identifier" {
            return txt(c);
        }
    }
    kind.to_string()
}

fn convert_ts(
    node: tree_sitter::Node<'_>,
    source: &[u8],
    id_prefix: &str,
    parent_class: Option<&str>,
) -> Option<SemanticNode> {
    convert_ts_direct(
        node,
        source,
        id_prefix,
        parent_class,
        &TsDirectHooks {
            is_trivia: &|kind| TRIVIA.contains(&kind),
            class_label: &|n, s| is_class_like(n.kind()).then(|| label_for_ts(n, s)),
            keep_childless: &|n| is_semantic(n.kind()),
            unwrap_single: &|_, _| false,
            label: &|n, s| label_for_ts(n, s),
            is_method_like: &|n| is_method_like(n.kind()),
        },
    )
}

fn process_impl(source: &str) -> String {
    let mut parser = tree_sitter::Parser::new();
    let lang = tree_sitter_squirrel::LANGUAGE.into();
    if parser.set_language(&lang).is_err() {
        return r#"{"error":"Failed to load grammar"}"#.to_string();
    }
    let tree = match parser.parse(source, None) {
        Some(t) => t,
        None => return r#"{"error":"Parse failed"}"#.to_string(),
    };
    let root = tree.root_node();
    match convert_ts(root, source.as_bytes(), "0", None) {
        Some(n) => serde_json::to_string(&n).unwrap_or_else(|e| format!(r#"{{"error":"{}"}}"#, e)),
        None => r#"{"error":"Empty semantic tree"}"#.to_string(),
    }
}
impl Guest for SquirrelParser {
    fn get_parser_mode() -> ParserMode {
        ParserMode::FullParse
    }
    fn grammar_id() -> String {
        "squirrel".to_string()
    }
    fn detect_language(filename: String, _content: String) -> String {
        if filename.to_lowercase().ends_with(".nut") {
            return "squirrel".to_string();
        }
        String::new()
    }
    fn preprocess_source(source: String) -> String {
        source
    }
    fn example(_language: String) -> ExamplePair {
        ExamplePair {
            old: "function greet(name) {\n    print(\"Hello, \" + name + \"\\n\");\n}\n\nfunction add(a, b) {\n    return a + b;\n}\n".to_string(),
            new: "function greet(name) {\n    print(format(\"Hello, %s!\\n\", name));\n}\n\nfunction add(x, y) {\n    return x + y;\n}\n\nfunction multiply(x, y) {\n    return x * y;\n}\n".to_string(),
        }
    }
    fn process(input: String, _language: String, _filename: String) -> String {
        process_impl(&input)
    }
    fn trivia_node_types() -> Vec<String> {
        TRIVIA.iter().map(|s| s.to_string()).collect()
    }
    fn language_ids() -> Vec<String> {
        vec!["squirrel".to_string()]
    }
    fn language_info() -> Vec<LanguageInfoRecord> {
        language_info_for(Self::language_ids())
    }
    fn priority() -> i32 {
        0
    }
}

export!(SquirrelParser);

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exports::intentdiff::plugin::parser::Guest;
    use intentdiff_plugin_sdk::testing as t;

    #[test]
    fn grammar_id_nonempty() {
        assert!(!SquirrelParser::grammar_id().is_empty());
    }

    #[test]
    fn language_ids_contain_grammar_id() {
        let gid = SquirrelParser::grammar_id();
        let ids = SquirrelParser::language_ids();
        assert!(
            ids.contains(&gid),
            "language_ids {:?} must contain {:?}",
            ids,
            gid
        );
    }

    #[test]
    fn detect_language_known_ext() {
        let r = SquirrelParser::detect_language("test.nut".to_string(), "".to_string());
        assert_eq!(r.as_str(), "squirrel");
    }

    #[test]
    fn detect_language_unknown_ext() {
        let r = SquirrelParser::detect_language(
            "test.xyz_notareal_ext_9z8y".to_string(),
            "".to_string(),
        );
        assert_eq!(r.as_str(), "");
    }

    #[test]
    fn process_impl_empty_returns_valid_json() {
        let out = process_impl("");
        t::assert_valid_json(&out, "process(empty)");
    }

    #[test]
    fn process_impl_whitespace_returns_valid_json() {
        let out = process_impl("   \n  ");
        t::assert_valid_json(&out, "process(whitespace)");
    }
}
