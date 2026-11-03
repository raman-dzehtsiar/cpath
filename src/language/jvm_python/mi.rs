// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT
mod python;

use super::{name, text};
use crate::maintainability::{SyntaxCounts, UnitFacts, UnitKind};
use crate::metrics::jvm_python::{
    Language, MaintainabilityFacts, MaintainabilityUnit, ReferenceParse,
};
use std::collections::HashSet;
use tree_sitter::Node;

#[derive(Default)]
struct Counts {
    operators: u64,
    operands: u64,
    operator_kinds: HashSet<(u16, bool)>,
    operand_values: HashSet<String>,
    cyclomatic: u64,
}

fn java_operator(kind: &str) -> bool {
    matches!(
        kind,
        "if" | "else"
            | "switch"
            | "case"
            | "try"
            | "catch"
            | "throw"
            | "throws"
            | "for"
            | "while"
            | "continue"
            | "break"
            | "do"
            | "finally"
            | "new"
            | "return"
            | "default"
            | "abstract"
            | "assert"
            | "instanceof"
            | "extends"
            | "final"
            | "implements"
            | "transient"
            | "synchronized"
            | "super"
            | "this"
            | "void_type"
            | ";"
            | ","
            | "::"
            | "{"
            | "["
            | "("
            | "="
            | "<"
            | ">"
            | "!"
            | "~"
            | "?"
            | ":"
            | "=="
            | "<="
            | ">="
            | "!="
            | "&&"
            | "||"
            | "++"
            | "--"
            | "+"
            | "-"
            | "*"
            | "/"
            | "&"
            | "|"
            | "^"
            | "%"
            | "<<"
            | ">>"
            | ">>>"
            | "+="
            | "-="
            | "*="
            | "/="
            | "&="
            | "|="
            | "^="
            | "%="
            | "<<="
            | ">>="
            | ">>>="
            | "int"
            | "float"
    )
}

fn java_operand(kind: &str) -> bool {
    matches!(
        kind,
        "identifier"
            | "null_literal"
            | "class_literal"
            | "string_literal"
            | "character_literal"
            | "hex_integer_literal"
            | "octal_integer_literal"
            | "binary_integer_literal"
            | "decimal_integer_literal"
            | "hex_floating_point_literal"
            | "decimal_floating_point_literal"
    )
}

fn java_scope(node: Node<'_>) -> Option<UnitKind> {
    match node.kind() {
        "program" => Some(UnitKind::TopLevel),
        "class_declaration" | "interface_declaration" => Some(UnitKind::Container),
        "method_declaration" | "constructor_declaration" => Some(UnitKind::Callable),
        _ => None,
    }
}

fn merge(parent: &mut Counts, child: Counts) {
    parent.operators += child.operators;
    parent.operands += child.operands;
    parent.cyclomatic += child.cyclomatic;
    parent.operator_kinds.extend(child.operator_kinds);
    parent.operand_values.extend(child.operand_values);
}

fn requires_reference_grammar(node: Node<'_>, source: &str) -> bool {
    if node.kind() == "string_literal" {
        let value = text(node, source);
        return value.starts_with("\"\"\"") && value.contains("/*");
    }
    if node.kind() != "record_pattern" {
        return false;
    }
    let mut cursor = node.walk();
    let qualified = node
        .named_children(&mut cursor)
        .any(|child| child.kind() == "scoped_type_identifier");
    qualified
}

fn java_counts(
    root: Node<'_>,
    source: &str,
    check_reference: bool,
) -> (MaintainabilityFacts, bool) {
    let mut reference_needed = false;
    let mut scope_nodes = vec![root];
    let mut parents = vec![None];
    let mut counts = vec![Counts {
        cyclomatic: 1,
        ..Counts::default()
    }];
    let mut pending = vec![(root, 0_usize)];
    while let Some((node, mut scope)) = pending.pop() {
        if check_reference && !reference_needed && requires_reference_grammar(node, source) {
            reference_needed = true;
        }
        if node.id() != root.id() && java_scope(node).is_some() {
            let child = counts.len();
            parents.push(Some(scope));
            scope_nodes.push(node);
            counts.push(Counts {
                cyclomatic: 1,
                ..Counts::default()
            });
            scope = child;
        }
        let current = &mut counts[scope];
        if java_operator(node.kind()) {
            current.operators += 1;
            current
                .operator_kinds
                .insert((node.kind_id(), node.is_named()));
        }
        if java_operand(node.kind()) {
            current.operands += 1;
            current.operand_values.insert(text(node, source).to_owned());
        }
        if matches!(
            node.kind(),
            "if" | "for" | "while" | "case" | "catch" | "ternary_expression" | "&&" | "||"
        ) {
            current.cyclomatic += 1;
        }
        let mut cursor = node.walk();
        let children: Vec<_> = node.children(&mut cursor).collect();
        pending.extend(children.into_iter().rev().map(|child| (child, scope)));
    }
    let mut units = Vec::with_capacity(counts.len());
    for index in (0..counts.len()).rev() {
        let count = std::mem::take(&mut counts[index]);
        let node = scope_nodes[index];
        let lines =
            (node.end_position().row - node.start_position().row + usize::from(index != 0)) as u64;
        units.push(MaintainabilityUnit {
            facts: UnitFacts {
                name: name(node, source),
                kind: java_scope(node).unwrap_or(UnitKind::TopLevel),
                start_byte: node.start_byte(),
                end_byte: node.end_byte(),
                start_line: (node.start_position().row + 1) as u32,
                end_line: (node.end_position().row + 1) as u32,
                counts: SyntaxCounts {
                    operators: count.operators,
                    operands: count.operands,
                    distinct_operators: count.operator_kinds.len() as u64,
                    distinct_operands: count.operand_values.len() as u64,
                    decisions: count.cyclomatic.saturating_sub(1),
                    ..SyntaxCounts::default()
                },
            },
            cyclomatic: count.cyclomatic,
            formula_lines: lines,
            physical_source_lines: 0,
            comment_lines: 0,
        });
        if let Some(parent) = parents[index] {
            merge(&mut counts[parent], count);
        }
    }
    units.reverse();
    (
        MaintainabilityFacts {
            language: Language::Java,
            units,
            reference_parse: ReferenceParse::NotNeeded,
        },
        reference_needed,
    )
}

fn java(root: Node<'_>, source: &str) -> MaintainabilityFacts {
    let (mut facts, reference_needed) = java_counts(root, source, true);
    if !reference_needed {
        return facts;
    }
    // RCA publishes metrics for its pinned grammar, including its text-block
    // quirk. This separate parser never changes the reusable modern CC parser.
    let mut reference = tree_sitter::Parser::new();
    facts.reference_parse = ReferenceParse::Rejected;
    if reference
        .set_language(&tree_sitter_java::LANGUAGE_RCA.into())
        .is_err()
    {
        return facts;
    }
    let Some(tree) = reference.parse(source, None) else {
        return facts;
    };
    if tree.root_node().has_error() {
        return facts;
    }
    let (mut facts, _) = java_counts(tree.root_node(), source, false);
    facts.reference_parse = ReferenceParse::Accepted;
    facts
}

pub(super) fn extract(root: Node<'_>, source: &str, language: Language) -> MaintainabilityFacts {
    match language {
        Language::Java => java(root, source),
        Language::Python => python::extract(root, source),
    }
}
