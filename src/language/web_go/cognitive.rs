// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

use super::{callable, name, text};
use crate::metrics::web_go::{CognitiveEvent, CognitiveFacts, CognitiveUnit, Language};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

#[derive(Clone, Copy)]
struct Frame<'tree> {
    node: Node<'tree>,
    parent: Option<Node<'tree>>,
    method_beginning: Option<Node<'tree>>,
    unit: Option<usize>,
    own_nesting: u32,
    file_nesting: u32,
    function_depth: u32,
    controlled_callable: bool,
    exit_controlled_callable: bool,
    jsx_logical_root: Option<usize>,
}

fn logical_operator<'a>(node: Node<'_>, source: &'a str) -> Option<&'a str> {
    if node.kind() != "binary_expression" {
        return None;
    }
    let operator = node.child_by_field_name("operator")?;
    let value = text(operator, source);
    matches!(value, "&&" | "||" | "??").then_some(value)
}

fn skip_parentheses(mut node: Node<'_>) -> Node<'_> {
    while node.kind() == "parenthesized_expression" {
        let mut cursor = node.walk();
        let inner = node
            .named_children(&mut cursor)
            .find(|child| child.kind() != "comment");
        let Some(inner) = inner else {
            break;
        };
        node = inner;
    }
    node
}

fn assertion_logical_left<'tree>(mut node: Node<'tree>, source: &str) -> Option<Node<'tree>> {
    let mut assertion = false;
    loop {
        match node.kind() {
            "non_null_expression" | "as_expression" | "satisfies_expression" => {
                let child = node.named_child(0)?;
                assertion = true;
                node = child;
            }
            "member_expression" | "subscript_expression" => {
                node = node.child_by_field_name("object")?;
            }
            "call_expression" => {
                node = node.child_by_field_name("function")?;
            }
            "binary_expression" => {
                if logical_operator(node, source).is_some() {
                    return assertion.then_some(node);
                }
                node = node.child_by_field_name("left")?;
            }
            _ => return None,
        }
    }
}

fn jsx_logical_root(node: Node<'_>, source: &str) -> Option<usize> {
    let mut cursor = node.walk();
    let expression = node
        .named_children(&mut cursor)
        .find(|child| child.kind() != "comment")?;
    let expression = skip_parentheses(expression);
    if logical_operator(expression, source).is_some() {
        Some(expression.id())
    } else {
        assertion_logical_left(expression, source).map(|expression| expression.id())
    }
}

fn logical_runs(
    node: Node<'_>,
    source: &str,
    go: bool,
    jsx_root: Option<usize>,
    visited: &mut HashSet<usize>,
) -> u32 {
    if visited.contains(&node.id()) {
        return 0;
    }
    let mut operators = Vec::new();
    let mut contains_ternary = false;
    let mut stack = vec![(node, false)];
    while let Some((node, emit)) = stack.pop() {
        let node = skip_parentheses(node);
        let node = if !go && logical_operator(node, source).is_none() {
            assertion_logical_left(node, source).unwrap_or(node)
        } else {
            node
        };
        let operator = if go && node.kind() == "binary_expression" {
            node.child_by_field_name("operator")
                .map(|operator| text(operator, source))
        } else {
            logical_operator(node, source)
        };
        let Some(operator) = operator else {
            contains_ternary |= node.kind() == "ternary_expression";
            continue;
        };
        if matches!(operator, "&&" | "||" | "??") {
            visited.insert(node.id());
        }
        if emit {
            if matches!(operator, "&&" | "||" | "??") {
                operators.push(operator);
            }
            continue;
        }
        if let Some(right) = node.child_by_field_name("right") {
            stack.push((right, false));
        }
        stack.push((node, true));
        if let Some(left) = node.child_by_field_name("left") {
            stack.push((left, false));
        }
    }
    let jsx_short_circuit = !go
        && !contains_ternary
        && jsx_root == Some(node.id())
        && operators
            .first()
            .is_some_and(|first| operators.iter().all(|operator| operator == first));
    if jsx_short_circuit {
        return 0;
    }
    let mut previous = None;
    let mut count = 0;
    for operator in operators {
        if previous != Some(operator) && (go || operator == "&&") {
            count += 1;
        }
        previous = Some(operator);
    }
    count
}

fn is_else_if(node: Node<'_>, parent: Option<Node<'_>>) -> bool {
    let Some(parent) = parent else {
        return false;
    };
    if parent.kind() == "else_clause" {
        return true;
    }
    parent.kind() == "if_statement" && parent.child_by_field_name("alternative") == Some(node)
}

fn has_label(node: Node<'_>, source: &str, go: bool) -> bool {
    let mut cursor = node.walk();
    let label = node.child_by_field_name("label").or_else(|| {
        node.named_children(&mut cursor)
            .find(|child| matches!(child.kind(), "statement_identifier" | "label_name"))
    });
    label.is_some_and(|label| {
        go || !source[node.start_byte()..label.start_byte()]
            .contains(['\n', '\r', '\u{2028}', '\u{2029}'])
    })
}

fn calls_self(
    call: Node<'_>,
    declaration: Node<'_>,
    source: &str,
    bindings: &super::mi::GoBindings,
) -> bool {
    let Some(function_name) = declaration.child_by_field_name("name") else {
        return false;
    };
    let Some(mut target) = call
        .child_by_field_name("function")
        .or_else(|| call.child_by_field_name("type"))
    else {
        return false;
    };
    loop {
        target = skip_parentheses(target);
        if !matches!(target.kind(), "index_expression" | "generic_type") {
            break;
        }
        let Some(inner) = target
            .child_by_field_name("operand")
            .or_else(|| target.child_by_field_name("type"))
        else {
            break;
        };
        target = inner;
    }
    if declaration.kind() == "function_declaration" {
        return matches!(target.kind(), "identifier" | "type_identifier")
            && text(target, source) == text(function_name, source)
            && super::mi::identifier_binding(target, source, bindings) != Some(false);
    }
    if !matches!(target.kind(), "selector_expression" | "qualified_type") {
        return false;
    }
    let Some(field) = target
        .child_by_field_name("field")
        .or_else(|| target.child_by_field_name("name"))
    else {
        return false;
    };
    let Some(operand) = target
        .child_by_field_name("operand")
        .or_else(|| target.child_by_field_name("package"))
    else {
        return false;
    };
    if !matches!(operand.kind(), "identifier" | "package_identifier")
        || text(field, source) != text(function_name, source)
    {
        return false;
    }
    let Some(receiver) = declaration.child_by_field_name("receiver") else {
        return false;
    };
    let mut cursor = receiver.walk();
    let receiver_matches = receiver.named_children(&mut cursor).any(|parameter| {
        parameter
            .child_by_field_name("name")
            .is_some_and(|name| text(name, source) == text(operand, source))
    });
    receiver_matches
}

fn react_name<'tree>(node: Node<'tree>, parent: Option<Node<'tree>>, source: &str) -> bool {
    let identifier = if node.kind() != "method_definition" {
        node.child_by_field_name("name")
    } else {
        None
    }
    .or_else(|| {
        parent
            .filter(|parent| parent.kind() == "variable_declarator")
            .and_then(|parent| parent.child_by_field_name("name"))
    });
    identifier
        .filter(|identifier| identifier.kind() == "identifier")
        .and_then(|identifier| text(identifier, source).chars().next())
        .is_some_and(|first| first.to_uppercase().eq(std::iter::once(first)))
}

fn final_syntax<'tree>(
    mut node: Node<'tree>,
    cache: &mut HashMap<usize, Node<'tree>>,
) -> Node<'tree> {
    let mut visited = Vec::new();
    let ending = loop {
        if let Some(ending) = cache.get(&node.id()) {
            break *ending;
        }
        visited.push(node.id());
        let child = (0..node.child_count())
            .rev()
            .filter_map(|index| node.child(index))
            .find(|child| child.kind() != "comment");
        let Some(child) = child else {
            break node;
        };
        node = child;
    };
    for identifier in visited {
        cache.insert(identifier, ending);
    }
    ending
}

fn bare_class_modifier<'a>(node: Node<'_>, source: &'a str) -> Option<&'a str> {
    if !matches!(node.kind(), "field_definition" | "public_field_definition") {
        return None;
    }
    let mut cursor = node.walk();
    let mut children = node
        .children(&mut cursor)
        .filter(|child| child.kind() != "comment");
    let token = children.next()?;
    if children.next().is_some() {
        return None;
    }
    Some(text(token, source))
}

fn method_start<'tree>(
    node: Node<'tree>,
    parent: Node<'tree>,
    previous: &[Node<'tree>],
    source: &str,
) -> Node<'tree> {
    let mut cursor = node.walk();
    let children: Vec<_> = node
        .children(&mut cursor)
        .filter(|child| child.kind() != "comment")
        .collect();
    let object = parent.kind() == "object";
    if object
        && children
            .first()
            .is_some_and(|child| matches!(child.kind(), "get" | "set"))
    {
        return node.child_by_field_name("parameters").unwrap_or(node);
    }
    if !object
        && children.len() >= 2
        && children[0].kind() == "async"
        && source[children[0].end_byte()..children[1].start_byte()].contains('\n')
    {
        return children[1];
    }
    let mut beginning = node;
    let mut accessor = children
        .iter()
        .any(|child| matches!(child.kind(), "get" | "set"));
    let mut static_seen = children.iter().any(|child| child.kind() == "static");
    let accessor_allowed = !children
        .iter()
        .any(|child| matches!(child.kind(), "async" | "*"));
    for sibling in previous.iter().rev().copied() {
        if sibling.kind() == "comment" {
            continue;
        }
        let modifier = if object {
            None
        } else {
            bare_class_modifier(sibling, source)
        };
        if sibling.kind() == "decorator" {
            beginning = sibling;
        } else if !static_seen
            && !accessor
            && accessor_allowed
            && matches!(modifier, Some("get" | "set"))
        {
            accessor = true;
            beginning = sibling;
        } else if !static_seen && modifier == Some("static") {
            static_seen = true;
            beginning = sibling;
        } else {
            break;
        }
    }
    beginning
}

fn nesting_child(parent: Node<'_>, child: Node<'_>, go: bool) -> bool {
    match parent.kind() {
        "if_statement" => {
            parent.child_by_field_name("consequence") == Some(child)
                || (parent.child_by_field_name("alternative") == Some(child)
                    && !matches!(child.kind(), "if_statement" | "else_clause"))
        }
        "else_clause" => child.is_named() && child.kind() != "if_statement",
        "for_statement" | "for_in_statement" | "while_statement" | "do_statement"
        | "catch_clause" => parent.child_by_field_name("body") == Some(child),
        "switch_statement" => parent.child_by_field_name("body") == Some(child),
        "expression_switch_statement" | "type_switch_statement" | "select_statement" if go => {
            matches!(
                child.kind(),
                "expression_case" | "type_case" | "communication_case" | "default_case"
            )
        }
        "ternary_expression" => {
            parent.child_by_field_name("consequence") == Some(child)
                || parent.child_by_field_name("alternative") == Some(child)
        }
        _ => false,
    }
}

pub(super) fn extract(
    root: Node<'_>,
    source: &str,
    language: Language,
    bindings: &super::mi::GoBindings,
) -> CognitiveFacts {
    let go = language == Language::Go;
    let mut facts = CognitiveFacts {
        language,
        source_length: source.len(),
        source_lines: (root.end_position().row + 1).try_into().unwrap_or(u32::MAX),
        units: Vec::new(),
        global_events: Vec::new(),
        unscored_global_function_literals: 0,
    };
    let mut declarations = Vec::new();
    let mut react_candidates = Vec::new();
    let mut own_offsets = Vec::<u32>::new();
    let mut file_offset = 0_u32;
    let mut logical_nodes = HashSet::new();
    let mut final_nodes = HashMap::new();
    let mut stack = vec![Frame {
        node: root,
        parent: None,
        method_beginning: None,
        unit: None,
        own_nesting: 0,
        file_nesting: 0,
        function_depth: 0,
        controlled_callable: false,
        exit_controlled_callable: false,
        jsx_logical_root: None,
    }];
    while let Some(mut frame) = stack.pop() {
        if frame.exit_controlled_callable {
            file_offset += 1;
            continue;
        }
        let node = frame.node;
        let kind = node.kind();
        if !go && kind == "jsx_expression" {
            frame.jsx_logical_root = jsx_logical_root(node, source);
        }
        if kind == "comment" {
            continue;
        }
        let enclosing_frame = frame;
        if callable(node, language) {
            // Sonar's selector ordering retains the enclosing function's control
            // increment when a branch itself is a function expression. A third-
            // level branch function also retains one file nesting increment.
            if !go && frame.controlled_callable {
                if let Some(parent) = frame.unit {
                    own_offsets[parent] += 1;
                }
                if frame.function_depth >= 2 {
                    stack.push(Frame {
                        exit_controlled_callable: true,
                        ..frame
                    });
                }
            }
            let index = facts.units.len();
            let top_level = frame
                .unit
                .map_or(index, |parent| facts.units[parent].top_level);
            let nested = frame.unit.is_some();
            declarations.push(node);
            react_candidates.push(!go && react_name(node, frame.parent, source));
            own_offsets.push(0);
            let beginning = if !go && kind == "method_definition" {
                frame.method_beginning.unwrap_or(node)
            } else {
                node
            };
            let ending = if go {
                node
            } else {
                final_syntax(node, &mut final_nodes)
            };
            facts.units.push(CognitiveUnit {
                name: name(node, frame.parent, source),
                start_byte: beginning.start_byte(),
                end_byte: ending.end_byte(),
                start_line: beginning.start_position().row as u32 + 1,
                end_line: ending.end_position().row as u32 + 1,
                top_level,
                nested,
                react_component: false,
                recursive: false,
                events: Vec::new(),
            });
            frame.unit = Some(index);
            frame.own_nesting = 0;
            if frame.function_depth >= 2 {
                frame.file_nesting += 1;
            }
            frame.function_depth += 1;
        } else if go && kind == "func_literal" {
            if frame.unit.is_none() {
                facts.unscored_global_function_literals += 1;
            }
            frame.own_nesting += 1;
            frame.file_nesting += 1;
        }
        if !go && kind == "return_statement" && frame.function_depth == 1 {
            if let Some(index) = frame.unit {
                let jsx = node.named_child(0).is_some_and(|child| {
                    matches!(
                        skip_parentheses(child).kind(),
                        "jsx_element" | "jsx_self_closing_element"
                    )
                });
                facts.units[index].react_component |= react_candidates[index] && jsx;
            }
        }
        if go && matches!(kind, "call_expression" | "type_conversion_expression") {
            if let Some(index) = frame.unit {
                facts.units[index].recursive |=
                    calls_self(node, declarations[index], source, bindings);
            }
        }
        let structural = match kind {
            "if_statement" => Some(!is_else_if(node, frame.parent)),
            "for_statement"
            | "for_in_statement"
            | "while_statement"
            | "do_statement"
            | "switch_statement"
            | "expression_switch_statement"
            | "type_switch_statement"
            | "select_statement"
            | "catch_clause"
            | "ternary_expression" => Some(true),
            "break_statement" | "continue_statement" | "goto_statement"
                if has_label(node, source, go) =>
            {
                Some(false)
            }
            "else"
                if frame.parent.is_some_and(|parent| {
                    if parent.kind() == "else_clause" {
                        let mut cursor = parent.walk();
                        let final_else = parent
                            .named_children(&mut cursor)
                            .find(|child| child.kind() != "comment")
                            .is_none_or(|child| child.kind() != "if_statement");
                        final_else
                    } else {
                        parent
                            .child_by_field_name("alternative")
                            .is_none_or(|child| child.kind() != "if_statement")
                    }
                }) =>
            {
                Some(false)
            }
            _ => None,
        };
        let events = if let Some(index) = frame.unit {
            &mut facts.units[index].events
        } else {
            &mut facts.global_events
        };
        if let Some(structural) = structural {
            events.push(CognitiveEvent {
                structural,
                own_nesting: frame.own_nesting + frame.unit.map_or(0, |index| own_offsets[index]),
                file_nesting: frame.file_nesting + file_offset,
            });
        }
        if logical_operator(node, source).is_some() {
            for _ in 0..logical_runs(node, source, go, frame.jsx_logical_root, &mut logical_nodes) {
                events.push(CognitiveEvent {
                    structural: false,
                    own_nesting: frame.own_nesting,
                    file_nesting: frame.file_nesting,
                });
            }
        }
        let mut cursor = node.walk();
        let children: Vec<_> = node.children(&mut cursor).collect();
        for (child_index, child) in children.iter().copied().enumerate().rev() {
            let frame = if !go
                && kind == "method_definition"
                && (node.child_by_field_name("name") == Some(child) || child.kind() == "decorator")
            {
                enclosing_frame
            } else {
                frame
            };
            let increment = u32::from(nesting_child(node, child, go));
            let controlled_callable = increment != 0
                || (node.kind() == "parenthesized_expression" && frame.controlled_callable);
            stack.push(Frame {
                node: child,
                parent: if !go && kind == "parenthesized_expression" {
                    frame.parent
                } else {
                    Some(node)
                },
                method_beginning: (!go && child.kind() == "method_definition")
                    .then(|| method_start(child, node, &children[..child_index], source)),
                own_nesting: frame.own_nesting + increment,
                file_nesting: frame.file_nesting + increment,
                controlled_callable,
                ..frame
            });
        }
    }
    facts
}
