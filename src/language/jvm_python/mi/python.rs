// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

use super::super::text;
use crate::maintainability::{SyntaxCounts, UnitFacts, UnitKind};
use crate::metrics::jvm_python::{Language, MaintainabilityFacts, MaintainabilityUnit};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

mod literals;
use literals::{numeric, string_value};

#[derive(Default)]
struct Halstead {
    operators: u64,
    operands: u64,
    operator_kinds: HashSet<String>,
    operand_values: HashSet<(usize, String)>,
}

fn unwrap(mut node: Node<'_>) -> Node<'_> {
    while matches!(node.kind(), "parenthesized_expression" | "type") {
        let mut cursor = node.walk();
        let Some(child) = node
            .named_children(&mut cursor)
            .find(|child| !child.is_extra())
        else {
            break;
        };
        node = child;
    }
    node
}

fn operand(node: Node<'_>, source: &str) -> String {
    let node = unwrap(node);
    match node.kind() {
        "identifier" | "print" => format!("str:{}", text(node, source)),
        "attribute" => node.child_by_field_name("attribute").map_or_else(
            || format!("node:{}", node.id()),
            |attribute| format!("str:{}", text(attribute, source)),
        ),
        "integer" | "float" => numeric(text(node, source)).map_or_else(
            || format!("number:{}", text(node, source)),
            |value| format!("number:{value}"),
        ),
        "true" => "number:1".into(),
        "false" => "number:0".into(),
        "none" => "none".into(),
        "ellipsis" => "ellipsis".into(),
        "string" => {
            string_value(text(node, source)).unwrap_or_else(|| format!("node:{}", node.id()))
        }
        "concatenated_string" => {
            let mut cursor = node.walk();
            let values: Option<Vec<_>> = node
                .named_children(&mut cursor)
                .filter(|child| !child.is_extra())
                .map(|child| string_value(text(child, source)))
                .collect();
            values.filter(|values| !values.is_empty()).map_or_else(
                || format!("node:{}", node.id()),
                |values| {
                    let mut value = values[0].clone();
                    for next in values.iter().skip(1) {
                        value
                            .push_str(next.split_once(':').map_or(next.as_str(), |(_, text)| text));
                    }
                    value
                },
            )
        }
        _ => format!("node:{}", node.id()),
    }
}

fn operator_name(value: &str) -> &str {
    match value {
        "+" | "+=" => "Add",
        "-" | "-=" => "Sub",
        "*" | "*=" => "Mult",
        "/" | "/=" => "Div",
        "//" | "//=" => "FloorDiv",
        "%" | "%=" => "Mod",
        "**" | "**=" => "Pow",
        "@" | "@=" => "MatMult",
        "<<" | "<<=" => "LShift",
        ">>" | ">>=" => "RShift",
        "&" | "&=" => "BitAnd",
        "|" | "|=" => "BitOr",
        "^" | "^=" => "BitXor",
        "==" => "Eq",
        "!=" => "NotEq",
        "<" => "Lt",
        "<=" => "LtE",
        ">" => "Gt",
        ">=" => "GtE",
        "is" => "Is",
        "is not" => "IsNot",
        "in" => "In",
        "not in" => "NotIn",
        "and" => "And",
        "or" => "Or",
        "not" => "Not",
        "~" => "Invert",
        _ if value.split_whitespace().eq(["is", "not"]) => "IsNot",
        _ if value.split_whitespace().eq(["not", "in"]) => "NotIn",
        _ => value,
    }
}

fn boolean_values<'tree>(root: Node<'tree>, source: &str) -> Vec<Node<'tree>> {
    let operator = root
        .child_by_field_name("operator")
        .map(|node| text(node, source));
    let mut pending = vec![root];
    let mut values = Vec::new();
    while let Some(node) = pending.pop() {
        if node.kind() == "boolean_operator"
            && node
                .child_by_field_name("operator")
                .map(|node| text(node, source))
                == operator
        {
            if let Some(right) = node.child_by_field_name("right") {
                pending.push(right);
            }
            if let Some(left) = node.child_by_field_name("left") {
                pending.push(left);
            }
        } else {
            values.push(node);
        }
    }
    values
}

fn expression_counts(
    node: Node<'_>,
    parent: Option<Node<'_>>,
    source: &str,
    context: usize,
    counts: &mut Halstead,
) {
    let mut operands = Vec::new();
    let mut operators = Vec::new();
    let mut synthetic_operands = Vec::new();
    match node.kind() {
        "chevron" if parent.is_some_and(|parent| parent.kind() == "print_statement") => {
            operands.extend(parent.and_then(|parent| parent.child(0)));
            operands.extend(node.named_child(0));
            operators.push("RShift".to_owned());
        }
        "case_pattern" | "union_pattern" => {
            let mut cursor = node.walk();
            let mut negative = false;
            for child in node.children(&mut cursor) {
                if child.is_extra() {
                    continue;
                }
                if negative && matches!(child.kind(), "integer" | "float") {
                    operands.push(child);
                    operators.push("USub".to_owned());
                }
                negative = child.kind() == "-";
            }
        }
        "complex_pattern" => {
            let Some(left) = node.named_child(0) else {
                return;
            };
            let Some(right) = node.named_child(1) else {
                return;
            };
            operands.push(left);
            operands.push(right);
            operators.push(
                operator_name(
                    source
                        .get(left.end_byte()..right.start_byte())
                        .unwrap_or("")
                        .trim(),
                )
                .to_owned(),
            );
            if text(node, source).trim_start().starts_with('-') {
                operators.push("USub".to_owned());
                synthetic_operands.push(format!("negative-pattern:{}", node.id()));
            }
        }
        "union_type" => {
            let mut cursor = node.walk();
            operands.extend(
                node.named_children(&mut cursor)
                    .filter(|child| !child.is_extra()),
            );
            operators.extend((1..operands.len()).map(|_| "BitOr".to_owned()));
        }
        "binary_operator" | "augmented_assignment" => {
            operands.extend(node.child_by_field_name("left"));
            operands.extend(node.child_by_field_name("right"));
            operators.extend(
                node.child_by_field_name("operator")
                    .map(|operator| operator_name(text(operator, source)).to_owned()),
            );
        }
        "unary_operator" | "not_operator" => {
            operands.extend(
                node.child_by_field_name("argument")
                    .or_else(|| node.named_child(node.named_child_count().saturating_sub(1))),
            );
            let value = node
                .child_by_field_name("operator")
                .map_or("not", |operator| text(operator, source));
            operators.push(
                match value {
                    "+" => "UAdd",
                    "-" => "USub",
                    _ => operator_name(value),
                }
                .to_owned(),
            );
        }
        "boolean_operator" => {
            let operator = node
                .child_by_field_name("operator")
                .map(|operator| text(operator, source));
            if parent.is_some_and(|parent| {
                parent.kind() == "boolean_operator"
                    && parent
                        .child_by_field_name("operator")
                        .map(|operator| text(operator, source))
                        == operator
            }) {
                return;
            }
            operands = boolean_values(node, source);
            if let Some(operator) = operator {
                operators.push(operator_name(operator).to_owned());
            }
        }
        "comparison_operator" => {
            let mut cursor = node.walk();
            for (index, child) in node.children(&mut cursor).enumerate() {
                if child.is_extra() {
                    continue;
                }
                if node.field_name_for_child(index as u32) == Some("operators") || !child.is_named()
                {
                    operators.push(operator_name(text(child, source)).to_owned());
                } else {
                    operands.push(child);
                }
            }
        }
        _ => return,
    }
    counts.operators += operators.len() as u64;
    counts.operands += (operands.len() + synthetic_operands.len()) as u64;
    counts.operator_kinds.extend(operators);
    counts.operand_values.extend(
        operands
            .into_iter()
            .map(|node| (context, operand(node, source))),
    );
    counts
        .operand_values
        .extend(synthetic_operands.into_iter().map(|value| (context, value)));
}

fn halstead(root: Node<'_>, source: &str) -> Halstead {
    let mut counts = Halstead::default();
    let mut contexts: HashMap<&str, usize> = HashMap::from([("", 0)]);
    let mut pending = vec![(root, 0, None)];
    while let Some((node, context, parent)) = pending.pop() {
        if node.kind() == "decorated_definition"
            && node
                .child_by_field_name("definition")
                .is_some_and(|definition| definition.kind() == "function_definition")
        {
            if let Some(definition) = node.child_by_field_name("definition") {
                pending.push((definition, context, Some(node)));
            }
            continue;
        }
        if node.kind() == "function_definition" {
            if let Some(body) = node.child_by_field_name("body") {
                let name = node
                    .child_by_field_name("name")
                    .map_or("", |name| text(name, source));
                let next_context = contexts.len();
                let context = *contexts.entry(name).or_insert(next_context);
                pending.push((body, context, Some(node)));
            }
            continue;
        }
        expression_counts(node, parent, source, context, &mut counts);
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            pending.push((child, context, Some(node)));
        }
    }
    counts
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Context {
    Module,
    Class,
    Function,
}

fn cyclomatic(root: Node<'_>, source: &str) -> u64 {
    let mut total = 1;
    let mut pending = vec![(root, Context::Module, None)];
    while let Some((node, context, parent)) = pending.pop() {
        match node.kind() {
            "decorated_definition" => {
                if let Some(definition) = node.child_by_field_name("definition") {
                    pending.push((definition, context, Some(node)));
                }
                continue;
            }
            "function_definition" | "class_definition" => {
                let function = node.kind() == "function_definition";
                if context == Context::Function || (!function && context == Context::Class) {
                    continue;
                }
                if function && context == Context::Class {
                    total += 1;
                }
                if let Some(body) = node.child_by_field_name("body") {
                    pending.push((
                        body,
                        if function {
                            Context::Function
                        } else {
                            Context::Class
                        },
                        Some(node),
                    ));
                }
                continue;
            }
            "assert_statement" => {
                total += 1;
                continue;
            }
            "if_statement"
            | "elif_clause"
            | "conditional_expression"
            | "boolean_operator"
            | "for_in_clause" => total += 1,
            "if_clause" if !parent.is_some_and(|parent| parent.kind() == "case_clause") => {
                total += 1;
            }
            "for_statement" | "while_statement" => {
                total += 1 + u64::from(node.child_by_field_name("alternative").is_some());
            }
            "try_statement" => {
                let mut cursor = node.walk();
                let clauses: Vec<_> = node.named_children(&mut cursor).collect();
                let starred = clauses.iter().any(|clause| {
                    clause.kind() == "except_clause"
                        && text(*clause, source)
                            .strip_prefix("except")
                            .is_some_and(|rest| rest.trim_start().starts_with('*'))
                });
                if !starred {
                    total += clauses
                        .iter()
                        .filter(|child| matches!(child.kind(), "except_clause" | "else_clause"))
                        .count() as u64;
                }
            }
            "match_statement" => {
                let body = node.child_by_field_name("body").unwrap_or(node);
                let mut cursor = body.walk();
                let cases: Vec<_> = body
                    .named_children(&mut cursor)
                    .filter(|child| child.kind() == "case_clause")
                    .collect();
                let capture = cases.iter().any(|case| {
                    case.named_child(0).is_some_and(|pattern| {
                        let pattern = text(pattern, source).trim();
                        !pattern.is_empty()
                            && pattern
                                .chars()
                                .all(|character| character == '_' || character.is_alphanumeric())
                            && !matches!(pattern, "True" | "False" | "None")
                            && !pattern
                                .chars()
                                .next()
                                .is_some_and(|character| character.is_numeric())
                    })
                });
                total += (cases.len() as u64).saturating_sub(u64::from(capture));
            }
            _ => {}
        }
        let mut cursor = node.walk();
        pending.extend(
            node.named_children(&mut cursor)
                .map(|child| (child, context, Some(node))),
        );
    }
    total
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TokenKind {
    Comment,
    String,
    Other,
    Boundary,
}
struct Token<'a> {
    node: Node<'a>,
    kind: TokenKind,
}

fn tokens<'tree>(root: Node<'tree>, source: &str) -> Vec<Token<'tree>> {
    let mut output = Vec::new();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if node.kind() == "comment" {
            output.push(Token {
                node,
                kind: TokenKind::Comment,
            });
            continue;
        }
        if node.kind() == "string" {
            let value = text(node, source);
            let quote = value.find(['\'', '"']).unwrap_or(0);
            let prefix = value[..quote].to_ascii_lowercase();
            if !prefix.contains(['f', 't']) {
                output.push(Token {
                    node,
                    kind: TokenKind::String,
                });
                continue;
            }
            output.push(Token {
                node,
                kind: TokenKind::Boundary,
            });
        }
        if node.child_count() == 0 && node.end_byte() > node.start_byte() {
            output.push(Token {
                node,
                kind: TokenKind::Other,
            });
            continue;
        }
        let mut cursor = node.walk();
        let children: Vec<_> = node.children(&mut cursor).collect();
        pending.extend(children.into_iter().rev());
    }
    output
}

fn logical_lines(tokens: &[Token<'_>]) -> u64 {
    let filtered: Vec<_> = tokens
        .iter()
        .filter(|token| !matches!(token.kind, TokenKind::Comment | TokenKind::Boundary))
        .collect();
    let mut total = 0;
    let mut start = 0;
    for end in 0..=filtered.len() {
        let last = end == filtered.len();
        if !last && filtered[end].node.kind() != ";" {
            continue;
        }
        let segment = &filtered[start..end];
        if !segment.is_empty() {
            let colon = segment.iter().rposition(|token| token.node.kind() == ":");
            total += match colon {
                Some(position) if position + 1 + usize::from(!last) != segment.len() => 2,
                _ => 1,
            };
        }
        start = end + 1;
    }
    total
}

fn raw_counts(root: Node<'_>, source: &str) -> (u64, u64, u64) {
    let tokens = tokens(root, source);
    let lines: Vec<_> = source.lines().collect();
    let mut logical = 0;
    let mut physical = 0;
    let mut comments = 0;
    let mut line = 0;
    let mut token = 0;
    while line < lines.len() {
        let start_line = line;
        let start_token = token;
        let mut depth = 0_i64;
        let mut continued_group = false;
        loop {
            let mut spans_next = false;
            while token < tokens.len() && tokens[token].node.start_position().row <= line {
                let item = &tokens[token];
                if item.node.end_position().row > line {
                    spans_next = true;
                    break;
                }
                if item.kind == TokenKind::Comment {
                    comments += 1;
                }
                if item.kind == TokenKind::Other {
                    match item.node.kind() {
                        "(" | "[" | "{" => depth += 1,
                        ")" | "]" | "}" => depth -= 1,
                        _ => {}
                    }
                }
                token += 1;
            }
            let comment_at_end = tokens[start_token..token].last().is_some_and(|item| {
                item.kind == TokenKind::Comment && item.node.start_position().row == line
            });
            let continued = (lines[line].trim_end().ends_with('\\') && !comment_at_end)
                || (continued_group && lines[line].trim().is_empty());
            continued_group = continued;
            line += 1;
            if line == lines.len() || (!spans_next && depth <= 0 && !continued) {
                break;
            }
        }
        let group = &tokens[start_token..token];
        logical += logical_lines(group);
        let nonempty = lines[start_line..line]
            .iter()
            .filter(|line| !line.trim().is_empty())
            .count() as u64;
        if group.len() == 1 && group[0].kind == TokenKind::Comment {
            continue;
        }
        if group.len() == 1 && group[0].kind == TokenKind::String {
            if group[0].node.end_position().row > group[0].node.start_position().row {
                comments += nonempty;
            }
            continue;
        }
        physical += nonempty;
    }
    (logical, physical, comments)
}

pub(super) fn extract(root: Node<'_>, source: &str) -> MaintainabilityFacts {
    let counts = halstead(root, source);
    let cyclomatic = cyclomatic(root, source);
    let (logical, physical, comments) = raw_counts(root, source);
    MaintainabilityFacts {
        language: Language::Python,
        reference_parse: crate::metrics::jvm_python::ReferenceParse::NotNeeded,
        units: vec![MaintainabilityUnit {
            facts: UnitFacts {
                name: "<module>".into(),
                kind: UnitKind::TopLevel,
                start_byte: root.start_byte(),
                end_byte: root.end_byte(),
                start_line: (root.start_position().row + 1) as u32,
                end_line: (root.end_position().row + 1) as u32,
                counts: SyntaxCounts {
                    operators: counts.operators,
                    operands: counts.operands,
                    distinct_operators: counts.operator_kinds.len() as u64,
                    distinct_operands: counts.operand_values.len() as u64,
                    decisions: cyclomatic.saturating_sub(1),
                    ..SyntaxCounts::default()
                },
            },
            cyclomatic,
            formula_lines: logical,
            physical_source_lines: physical,
            comment_lines: comments,
        }],
    }
}
