// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

//! Bundled syntax extraction. No compiler, project build, or filesystem access.

mod c_family;
pub use c_family::CFamilyParser;

use std::path::PathBuf;

use tree_sitter::{Node, Parser};

use crate::model::{ComplexityEvent, ControlKind, FileFacts, MethodFacts, SourceLanguage};
use crate::selection::language_for_path;

pub trait SourceParser {
    fn parse(&mut self, path: PathBuf, source: &str) -> Result<FileFacts, String>;
}

/// Worker-local parsers; the selection policy is the sole extension registry.
pub struct BundledParser {
    csharp: CSharpParser,
    c: CFamilyParser,
    cpp: CFamilyParser,
}

impl BundledParser {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            csharp: CSharpParser::new()?,
            c: CFamilyParser::c()?,
            cpp: CFamilyParser::cpp()?,
        })
    }
}

impl SourceParser for BundledParser {
    fn parse(&mut self, path: PathBuf, source: &str) -> Result<FileFacts, String> {
        match language_for_path(&path) {
            Some(SourceLanguage::CSharp) => self.csharp.parse(path, source),
            Some(SourceLanguage::C) => self.c.parse(path, source),
            Some(SourceLanguage::Cpp) => self.cpp.parse(path, source),
            Some(SourceLanguage::CHeader) => {
                self.c.parse(path.clone(), source).or_else(|c_error| {
                    self.cpp.parse(path, source).map_err(|cpp_error| {
                        format!("header rejected by both grammars: {c_error}; {cpp_error}")
                    })
                })
            }
            None => Err("unsupported source language".into()),
        }
    }
}

pub struct CSharpParser {
    parser: Parser,
}

impl CSharpParser {
    pub fn new() -> Result<Self, String> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_c_sharp::LANGUAGE.into())
            .map_err(|error| format!("cannot initialize bundled C# grammar: {error}"))?;
        Ok(Self { parser })
    }
}

#[derive(Clone, Copy)]
struct Frame<'tree> {
    node: Node<'tree>,
    method: Option<usize>,
    nesting: u32,
    direct_else_alternative: bool,
}

impl SourceParser for CSharpParser {
    fn parse(&mut self, path: PathBuf, source: &str) -> Result<FileFacts, String> {
        let tree = self
            .parser
            .parse(source, None)
            .ok_or_else(|| "C# parser did not produce a syntax tree".to_owned())?;
        let root = tree.root_node();
        if root.has_error() {
            return Err(syntax_error(root));
        }
        let mut facts = FileFacts {
            path,
            bytes: source.len() as u64,
            lines: source.lines().count().try_into().unwrap_or(u32::MAX),
            ..FileFacts::default()
        };
        let mut top_level = None;
        let mut stack = vec![Frame {
            node: root,
            method: None,
            nesting: 0,
            direct_else_alternative: false,
        }];
        while let Some(mut frame) = stack.pop() {
            let node = frame.node;
            let kind = node.kind();
            if kind == "comment" {
                continue;
            }
            if is_callable(node) {
                frame.method = Some(facts.methods.len());
                frame.nesting = 0;
                let name = node
                    .child_by_field_name("name")
                    .or_else(|| node.child_by_field_name("operator"))
                    .map(|name| text(name, source).to_owned())
                    .unwrap_or_else(|| format!("<{kind}@{}>", line(node)));
                facts.methods.push(MethodFacts {
                    name,
                    start_line: line(node),
                    end_line: node.end_position().row as u32 + 1,
                    events: vec![],
                });
            } else if kind == "global_statement" {
                let index = *top_level.get_or_insert_with(|| {
                    let index = facts.methods.len();
                    facts.methods.push(MethodFacts {
                        name: "<top-level>".into(),
                        start_line: line(node),
                        end_line: line(node),
                        events: vec![],
                    });
                    index
                });
                facts.methods[index].end_line = node.end_position().row as u32 + 1;
                frame.method = Some(index);
            }
            let control = control_kind(node, source, frame.direct_else_alternative);
            if let (Some(method), Some(kind)) = (frame.method, control) {
                facts.methods[method].events.push(ComplexityEvent {
                    kind,
                    nesting: frame.nesting,
                    line: line(node),
                });
            }
            let nesting_control = matches!(
                control,
                Some(
                    ControlKind::If
                        | ControlKind::Loop
                        | ControlKind::Switch
                        | ControlKind::Catch
                        | ControlKind::Ternary
                )
            ) || kind == "if_statement"
                || (kind == "conditional_else_clause"
                    && node
                        .child_by_field_name("alternative")
                        .is_some_and(|alternative| alternative.kind() != "if_statement"));
            for index in (0..node.child_count()).rev() {
                if let Some(child) = node.child(index) {
                    // Conditional-compilation expressions share binary-expression
                    // node kinds with runtime expressions. They are not runtime
                    // decisions; visit every branch body, but not its directive condition.
                    if kind.starts_with("preproc_")
                        && node.child_by_field_name("condition") == Some(child)
                    {
                        continue;
                    }
                    // Conditional else wrappers are transparent until the actual
                    // else body. Otherwise an else-if receives an extra nesting
                    // cost simply because a directive surrounds its source.
                    let direct_else_alternative =
                        (matches!(kind, "if_statement" | "conditional_else_clause")
                            && node.field_name_for_child(index as u32) == Some("alternative"))
                            || (is_conditional_directive(node) && frame.direct_else_alternative);
                    let else_if = direct_else_alternative
                        && (child.kind() == "if_statement" || is_conditional_directive(child));
                    let conditional_else_body = is_conditional_directive(node)
                        && frame.direct_else_alternative
                        && child.is_named()
                        && !is_conditional_directive(child)
                        && !matches!(
                            child.kind(),
                            "if_statement" | "conditional_else_clause" | "comment"
                        )
                        && !child.kind().starts_with("preproc_");
                    stack.push(Frame {
                        node: child,
                        direct_else_alternative,
                        nesting: if (nesting_control && !else_if) || conditional_else_body {
                            frame.nesting.saturating_add(1)
                        } else {
                            frame.nesting
                        },
                        ..frame
                    });
                }
            }
        }
        Ok(facts)
    }
}

fn text<'source>(node: Node<'_>, source: &'source str) -> &'source str {
    &source[node.byte_range()]
}

fn line(node: Node<'_>) -> u32 {
    node.start_position().row as u32 + 1
}

fn syntax_error(root: Node<'_>) -> String {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.is_error() || node.is_missing() {
            return format!(
                "unsupported or malformed C# syntax at line {}, column {}",
                line(node),
                node.start_position().column + 1
            );
        }
        for index in (0..node.child_count()).rev() {
            if let Some(child) = node.child(index) {
                if child.has_error() || child.is_missing() {
                    stack.push(child);
                }
            }
        }
    }
    "unsupported or malformed C# syntax".into()
}

fn is_callable(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "method_declaration"
            | "constructor_declaration"
            | "destructor_declaration"
            | "operator_declaration"
            | "conversion_operator_declaration"
            | "accessor_declaration"
            | "local_function_statement"
            | "lambda_expression"
            | "anonymous_method_expression"
    ) || matches!(node.kind(), "property_declaration" | "indexer_declaration")
        && node
            .child_by_field_name("value")
            .is_some_and(|value| value.kind() == "arrow_expression_clause")
}

fn boolean_operator<'source>(node: Node<'_>, source: &'source str) -> Option<&'source str> {
    if node.kind() != "binary_expression" {
        return None;
    }
    let operator = text(node.child_by_field_name("operator")?, source);
    matches!(operator, "&&" | "||").then_some(operator)
}

fn is_conditional_directive(node: Node<'_>) -> bool {
    matches!(node.kind(), "preproc_if" | "preproc_elif" | "preproc_else")
        || matches!(
            node.kind(),
            "preproc_if_in_else_if_tail" | "preproc_if_in_else_tail"
        )
}

/// A shared else token costs once if any source alternative is not a direct
/// else-if. Empty alternatives are not vacuously all-if alternatives.
fn has_non_if_alternative(node: Node<'_>) -> bool {
    if !is_conditional_directive(node) {
        return node.kind() != "if_statement";
    }
    let mut stack = vec![node];
    while let Some(node) = stack.pop() {
        if !is_conditional_directive(node) {
            if node.kind() != "if_statement" {
                return true;
            }
            continue;
        }
        let mut has_body = false;
        for index in 0..node.named_child_count() {
            let Some(child) = node.named_child(index) else {
                continue;
            };
            if node.child_by_field_name("condition") == Some(child)
                || child.kind() == "comment"
                || (child.kind().starts_with("preproc_") && !is_conditional_directive(child))
            {
                continue;
            }
            if node.child_by_field_name("alternative") != Some(child) {
                has_body = true;
            }
            stack.push(child);
        }
        if !has_body {
            return true;
        }
    }
    false
}

fn else_target(node: Node<'_>) -> Option<Node<'_>> {
    let parent = node.parent()?;
    (0..parent.child_count()).find_map(|index| {
        let child = parent.child(index)?;
        (child.start_byte() >= node.end_byte()
            && parent.field_name_for_child(index as u32) == Some("alternative"))
        .then_some(child)
    })
}

fn control_kind(
    node: Node<'_>,
    source: &str,
    direct_else_alternative: bool,
) -> Option<ControlKind> {
    Some(match node.kind() {
        "if_statement" => {
            if direct_else_alternative {
                ControlKind::Else
            } else {
                ControlKind::If
            }
        }
        "else" => {
            if else_target(node).is_some_and(has_non_if_alternative) {
                ControlKind::Else
            } else {
                return None;
            }
        }
        "for_statement" | "foreach_statement" | "while_statement" | "do_statement" => {
            ControlKind::Loop
        }
        "switch_statement" | "switch_expression" => ControlKind::Switch,
        "catch_clause" => ControlKind::Catch,
        "conditional_expression" => ControlKind::Ternary,
        "goto_statement" => ControlKind::Jump,
        "binary_expression" => {
            let operator = boolean_operator(node, source)?;
            let mut parent = node.parent();
            while parent.is_some_and(|parent| parent.kind() == "parenthesized_expression") {
                parent = parent.and_then(|parent| parent.parent());
            }
            if parent.and_then(|parent| boolean_operator(parent, source)) == Some(operator) {
                return None;
            }
            ControlKind::BooleanSequence
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::complexity::{CognitiveComplexity, ComplexityAnalyzer};

    fn parse(source: &str) -> FileFacts {
        CSharpParser::new()
            .unwrap()
            .parse("Example.cs".into(), source)
            .unwrap()
    }

    #[test]
    fn nesting_else_if_and_boolean_sequences_have_explicit_costs() {
        let file = parse("class C { void M(bool a, bool b, bool c) { if (a && b && c) { while (b) { if (c) {} } } else if (b) {} else {} } }");
        // if=1, boolean sequence=1, while=2, nested if=3, else-if=1, else=1.
        assert_eq!(CognitiveComplexity.analyze(&file).max_method, 9);
    }

    #[test]
    fn nested_callables_are_scored_independently() {
        let file = parse("class C { int P { get { if (true) return 1; return 0; } } void M() { if (true) { void Local() { if (true) {} } System.Action a = () => { if (true) {} }; } } }");
        let metrics = CognitiveComplexity.analyze(&file);
        assert_eq!(metrics.methods.len(), 4);
        assert!(metrics.methods.iter().all(|method| method.score == 1));
        assert_eq!(metrics.total, 4);
    }

    #[test]
    fn rejects_incomplete_syntax_instead_of_scoring_it() {
        let error = CSharpParser::new()
            .unwrap()
            .parse("Broken.cs".into(), "class C { void M( {")
            .unwrap_err();
        assert!(error.contains("syntax"));
    }

    #[test]
    fn unsupported_conditional_directives_inside_expressions_fail_explicitly() {
        // Valid C#, but unsupported by the bundled grammar. Do not silently strip
        // directives or publish metrics for an incomplete tree.
        let source = "class C { int M() => F(41\n#if FEATURE\n + 4\n#endif\n); }";
        let error = CSharpParser::new()
            .unwrap()
            .parse("Conditional.cs".into(), source)
            .unwrap_err();
        assert!(error.contains("unsupported or malformed C# syntax"));
        assert!(error.contains("line"));
    }

    #[test]
    fn records_switch_ternary_catch_and_boolean_operator_changes() {
        let file = parse("class C { int M(bool a, bool b, bool c) { try { switch (a) { case true: return b ? 1 : 2; default: return 0; } } catch (System.Exception) { if (a && b || c) return 3; throw; } } }");
        // switch=1, nested ternary=2, catch=1, nested if=2, && and ||=2.
        assert_eq!(CognitiveComplexity.analyze(&file).max_method, 8);
    }

    #[test]
    fn top_level_statements_and_expression_properties_are_measured() {
        let file =
            parse("if (true) { while (true) { break; } } class C { int P => true ? 1 : 0; }");
        let metrics = CognitiveComplexity.analyze(&file);
        assert_eq!(metrics.max_method, 3);
        assert_eq!(metrics.total, 4);
    }

    #[test]
    fn expression_bodied_indexers_are_callables() {
        let file = parse("class C { int this[int i] => i > 0 ? 1 : 0; }");
        assert_eq!(CognitiveComplexity.analyze(&file).max_method, 1);
    }

    #[test]
    fn collection_expressions_accept_contextual_keyword_identifiers() {
        // `type` is a local variable, not an attribute target. This shape occurs
        // in Jellyfin's subtitle format discovery.
        let file = parse(
            "class C { object[] M(object type) { if (type == null) return []; return [type]; } }",
        );
        assert_eq!(CognitiveComplexity.analyze(&file).max_method, 1);
    }

    #[test]
    fn parenthesized_boolean_operands_are_not_misread_as_pointer_casts() {
        // Older grammars could accept this as a cast plus two address-of
        // operators, silently losing the logical sequence increment.
        let file = parse("class C { void M(bool a, int b) { if ((a) && (b > 0)) {} } }");
        assert_eq!(CognitiveComplexity.analyze(&file).max_method, 2);
    }

    #[test]
    fn list_slice_patterns_with_captures_are_supported() {
        let file = parse(
            "class C { int M(int[] values) => values is [1, .. var rest] ? rest.Length : 0; }",
        );
        assert_eq!(CognitiveComplexity.analyze(&file).max_method, 1);
    }

    #[test]
    fn unsafe_pointer_access_preserves_control_flow() {
        let file =
            parse("unsafe class C { void M(int* p, bool enabled) { if (enabled) { *p = 1; } } }");
        assert_eq!(CognitiveComplexity.analyze(&file).max_method, 1);
    }

    #[test]
    fn pointer_arithmetic_and_casts_preserve_control_flow() {
        let source = "unsafe class C { int M(int* p, int i, bool enabled) { if (enabled) { *(p + i) = i; } return *((int*)p + i); } }";
        let file = parse(source);
        assert_eq!(file.methods.len(), 1);
        assert_eq!(CognitiveComplexity.analyze(&file).max_method, 1);
    }

    #[test]
    fn pointer_patch_preserves_assignment_multiplication_and_boolean_tree_shapes() {
        let source = "unsafe class C { int M(int* p, int* q, int i, bool enabled) { *(p + i) = i; *(p = q) = i; int value = *p + i; value += i * i; value += *p++; (*p)++; if ((enabled) && (i > 0)) { value += *((int*)p + i); } return value; } }";
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_c_sharp::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();
        assert!(!tree.root_node().has_error());
        let find = |kind: &str, fragment: &str| {
            let mut stack = vec![tree.root_node()];
            while let Some(node) = stack.pop() {
                if node.kind() == kind && text(node, source) == fragment {
                    return node;
                }
                let mut cursor = node.walk();
                stack.extend(node.named_children(&mut cursor));
            }
            panic!("missing {kind}: {fragment}");
        };
        for fragment in ["*(p + i) = i", "*(p = q) = i"] {
            let assignment = find("assignment_expression", fragment);
            assert_eq!(
                assignment.child_by_field_name("left").unwrap().kind(),
                "prefix_unary_expression"
            );
        }
        let addition = find("binary_expression", "*p + i");
        assert_eq!(
            addition.child_by_field_name("left").unwrap().kind(),
            "prefix_unary_expression"
        );
        assert_eq!(
            boolean_operator(find("binary_expression", "(enabled) && (i > 0)"), source),
            Some("&&")
        );
        find("binary_expression", "i * i");
        find("cast_expression", "(int*)p");
        let dereference = find("prefix_unary_expression", "*p++");
        assert_eq!(
            dereference.named_child(0).unwrap().kind(),
            "postfix_unary_expression"
        );
        let increment = find("postfix_unary_expression", "(*p)++");
        assert_eq!(
            increment.named_child(0).unwrap().kind(),
            "parenthesized_expression"
        );
        let facts = parse(source);
        assert_eq!(CognitiveComplexity.analyze(&facts).max_method, 2);
    }

    #[test]
    fn modern_extensions_and_null_conditional_assignments_keep_callable_scores() {
        let file = parse("static class E { extension(C c) { public int Positive() { if (c.Value > 0) return 1; return 0; } } } class C { public int Value; void M(C other) { other?.Value = 1; } }");
        let scores = CognitiveComplexity.analyze(&file);
        assert_eq!(scores.max_method, 1);
        assert_eq!(scores.methods.len(), 2);
    }

    #[test]
    fn supported_conditional_statement_branches_preserve_source_lines() {
        let file = parse("class C {\nvoid M(bool value) {\n#if DEBUG && FEATURE\nif (value) {}\n#else\nwhile (value) {}\n#endif\n}\n}");
        assert_eq!(file.methods[0].start_line, 2);
        assert_eq!(
            file.methods[0]
                .events
                .iter()
                .map(|event| event.line)
                .collect::<Vec<_>>(),
            vec![4, 6]
        );
        assert_eq!(CognitiveComplexity.analyze(&file).max_method, 2);
    }

    #[test]
    fn conditional_list_tails_keep_all_branches_locations_and_callable_scopes() {
        let source = "class C {\nvoid M(bool value\n#if DEBUG && FEATURE\n, int extra = 0\n#elif TRACE\n, string label = null\n#endif\n) {\nUse(value\n#if DEBUG && FEATURE\n, value ? 1 : 0\n#elif TRACE\n, () => { if (value) {} }\n#else\n#if OTHER\n, value ? 2 : 3\n#endif\n#endif\n, value ? 4 : 5);\n}\n}";
        let file = parse(source);
        assert_eq!(file.methods.len(), 2);
        assert_eq!(
            (file.methods[0].start_line, file.methods[0].end_line),
            (2, 20)
        );
        assert_eq!(
            (file.methods[1].start_line, file.methods[1].end_line),
            (13, 13)
        );
        assert_eq!(
            file.methods[0]
                .events
                .iter()
                .map(|e| e.line)
                .collect::<Vec<_>>(),
            vec![11, 16, 19]
        );
        let scores = CognitiveComplexity.analyze(&file);
        // Three alternative/source ternaries in M; the lambda's if is independent.
        // Directive conditions and wrappers add no runtime decisions or nesting.
        assert_eq!(
            scores.methods.iter().map(|m| m.score).collect::<Vec<_>>(),
            vec![3, 1]
        );
        assert_eq!(scores.total, 4);
    }

    #[test]
    fn conditional_list_tails_do_not_make_commas_optional() {
        for source in [
            "class C { void M() { Use(first\n#if DEBUG\nsecond\n#endif\n); } }",
            "class C { void M(int first\n#if DEBUG\nint second\n#endif\n) {} }",
        ] {
            assert!(CSharpParser::new()
                .unwrap()
                .parse("Invalid.cs".into(), source)
                .is_err());
        }
    }

    #[test]
    fn nested_conditional_list_tails_visit_each_item_once() {
        let mut source = String::from("class C { void M(bool value) { Use(0\n");
        for _ in 0..64 {
            source.push_str("#if DEBUG && FEATURE\n, value ? 1 : 0\n");
        }
        for _ in 0..64 {
            source.push_str("#endif\n");
        }
        source.push_str("); } }");
        let file = parse(&source);
        assert_eq!(file.methods.len(), 1);
        assert_eq!(file.methods[0].events.len(), 64);
        assert_eq!(CognitiveComplexity.analyze(&file).max_method, 64);
    }
}
