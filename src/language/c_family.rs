// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

//! C and C++ share control-flow syntax, but use distinct bundled grammars.
//! Preprocessor branches are source alternatives, never build configurations.

use std::path::PathBuf;

use tree_sitter::{Language, Node, Parser};

use super::{line, text, SourceParser};
use crate::model::{ComplexityEvent, ControlKind, FileFacts, MethodFacts};

pub struct CFamilyParser {
    parser: Parser,
    label: &'static str,
}

impl CFamilyParser {
    pub fn c() -> Result<Self, String> {
        Self::new(tree_sitter_c::LANGUAGE.into(), "C")
    }

    pub fn cpp() -> Result<Self, String> {
        Self::new(tree_sitter_cpp::LANGUAGE.into(), "C++")
    }

    fn new(language: Language, label: &'static str) -> Result<Self, String> {
        let mut parser = Parser::new();
        parser
            .set_language(&language)
            .map_err(|error| format!("cannot initialize bundled {label} grammar: {error}"))?;
        Ok(Self { parser, label })
    }
}

impl SourceParser for CFamilyParser {
    fn parse(&mut self, path: PathBuf, source: &str) -> Result<FileFacts, String> {
        let tree = self
            .parser
            .parse(source, None)
            .ok_or_else(|| format!("{} parser did not produce a syntax tree", self.label))?;
        let root = tree.root_node();
        if root.has_error() {
            return Err(syntax_error(root, self.label));
        }
        let mut facts = FileFacts {
            path,
            bytes: source.len() as u64,
            lines: source.lines().count().try_into().unwrap_or(u32::MAX),
            ..FileFacts::default()
        };
        // Iterative traversal keeps deeply nested source off the Rust call stack.
        let mut stack = vec![(root, None, 0_u32)];
        while let Some((node, mut method, mut nesting)) = stack.pop() {
            let kind = node.kind();
            if matches!(
                kind,
                "comment"
                    | "preproc_def"
                    | "preproc_function_def"
                    | "preproc_call"
                    | "ms_declspec_modifier"
            ) {
                continue;
            }
            if kind == "compound_statement" && is_file_scope_block(node) {
                return Err(format!(
                    "unsupported or ambiguous {} file-scope block at line {}, column {}: a callable declaration may have been parsed incompletely",
                    self.label,
                    line(node),
                    node.start_position().column + 1,
                ));
            }
            if matches!(kind, "function_definition" | "lambda_expression") {
                // The grammars can accept `struct API Name { ... }` as a
                // function returning `struct API`, with a bare identifier as its
                // declarator. That error-free tree is not trustworthy callable
                // evidence. Do not guess whether API is an annotation macro.
                if kind == "function_definition" && !has_function_declarator(node) {
                    return Err(format!(
                        "unsupported or ambiguous {} callable declaration at line {}, column {}: function parameter list is missing; an unresolved macro may alter the declaration",
                        self.label,
                        line(node),
                        node.start_position().column + 1,
                    ));
                }
                method = Some(facts.methods.len());
                nesting = 0;
                facts.methods.push(MethodFacts {
                    name: callable_name(node, source),
                    start_line: line(node),
                    end_line: node.end_position().row as u32 + 1,
                    events: vec![],
                });
            }
            let control = control_kind(node, source);
            if let (Some(method), Some(kind)) = (method, control) {
                facts.methods[method].events.push(ComplexityEvent {
                    kind,
                    nesting,
                    line: line(node),
                });
            }
            let nests = kind == "if_statement"
                || matches!(
                    control,
                    Some(
                        ControlKind::Loop
                            | ControlKind::Switch
                            | ControlKind::Catch
                            | ControlKind::Ternary
                    )
                );
            for index in (0..node.child_count()).rev() {
                if let Some(child) = node.child(index) {
                    if kind.starts_with("preproc_")
                        && node.child_by_field_name("condition") == Some(child)
                    {
                        continue;
                    }
                    // C/C++ wrap the alternative in else_clause. Flatten only
                    // a direct else-if; `else { if (...) }` remains nested.
                    let else_if = kind == "if_statement"
                        && child.kind() == "else_clause"
                        && else_if_body(child).is_some();
                    stack.push((
                        child,
                        method,
                        if nests && !else_if {
                            nesting.saturating_add(1)
                        } else {
                            nesting
                        },
                    ));
                }
            }
        }
        Ok(facts)
    }
}

fn is_file_scope_block(node: Node<'_>) -> bool {
    let mut parent = node.parent();
    while let Some(current) = parent {
        match current.kind() {
            "translation_unit" => return true,
            "preproc_if" | "preproc_ifdef" | "preproc_else" | "preproc_elif"
            | "preproc_elifdef" => parent = current.parent(),
            _ => return false,
        }
    }
    false
}

fn has_function_declarator(node: Node<'_>) -> bool {
    let mut declarator = node.child_by_field_name("declarator");
    while let Some(current) = declarator {
        if matches!(
            current.kind(),
            "function_declarator" | "abstract_function_declarator"
        ) {
            return true;
        }
        // Follow only the declarator chain, never parameter types, attributes,
        // initializers, or nested bodies which may themselves contain functions.
        declarator = current.child_by_field_name("declarator").or_else(|| {
            if matches!(
                current.kind(),
                "parenthesized_declarator"
                    | "abstract_parenthesized_declarator"
                    | "reference_declarator"
                    | "abstract_reference_declarator"
                    | "attributed_declarator"
            ) {
                first_non_comment(current)
            } else if current.kind() == "qualified_identifier" {
                current.child_by_field_name("name")
            } else {
                None
            }
        });
    }
    false
}

fn else_if_body(node: Node<'_>) -> Option<Node<'_>> {
    first_non_comment(node).filter(|child| child.kind() == "if_statement")
}

fn first_non_comment(node: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = node.walk();
    let child = node
        .named_children(&mut cursor)
        .find(|child| child.kind() != "comment");
    child
}

fn callable_name(node: Node<'_>, source: &str) -> String {
    if node.kind() == "lambda_expression" {
        return format!("<lambda@{}>", line(node));
    }
    let mut declarator = node.child_by_field_name("declarator");
    while let Some(current) = declarator {
        if matches!(
            current.kind(),
            "identifier"
                | "field_identifier"
                | "qualified_identifier"
                | "destructor_name"
                | "operator_name"
        ) {
            return text(current, source).to_owned();
        }
        if current.kind() == "operator_cast" {
            if let Some(target) = current.child_by_field_name("type") {
                return format!("operator {}", text(target, source));
            }
        }
        declarator = current.child_by_field_name("declarator").or_else(|| {
            // Parenthesized/reference declarators may expose an unnamed child.
            if matches!(
                current.kind(),
                "parenthesized_declarator" | "reference_declarator"
            ) {
                first_non_comment(current)
            } else {
                None
            }
        });
    }
    format!("<function@{}>", line(node))
}

fn boolean_operator(node: Node<'_>, source: &str) -> Option<&'static str> {
    if node.kind() != "binary_expression" {
        return None;
    }
    match text(node.child_by_field_name("operator")?, source) {
        "&&" | "and" => Some("&&"),
        "||" | "or" => Some("||"),
        _ => None,
    }
}

fn control_kind(node: Node<'_>, source: &str) -> Option<ControlKind> {
    Some(match node.kind() {
        "if_statement"
            if node
                .parent()
                .is_some_and(|parent| parent.kind() == "else_clause") =>
        {
            ControlKind::Else
        }
        "if_statement" => ControlKind::If,
        "else_clause" if else_if_body(node).is_none() => ControlKind::Else,
        "for_statement" | "for_range_loop" | "while_statement" | "do_statement" => {
            ControlKind::Loop
        }
        "switch_statement" => ControlKind::Switch,
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

fn syntax_error(root: Node<'_>, label: &str) -> String {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.is_error() || node.is_missing() {
            return format!(
                "unsupported or malformed {label} syntax at line {}, column {}",
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
    format!("unsupported or malformed {label} syntax")
}
