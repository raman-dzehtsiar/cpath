// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

use super::{name, text};
use crate::metrics::jvm_python::{
    CognitiveEvent, CognitiveFacts, CognitiveScope, Language, Nesting,
};
use tree_sitter::Node;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScopeKind {
    File,
    Class,
    Function,
    Lambda,
    Initializer,
}

struct Scope<'tree> {
    node: Node<'tree>,
    kind: ScopeKind,
    eligible_members: bool,
    local_class_ancestor: bool,
}

#[derive(Clone, Copy)]
struct Frame<'tree> {
    node: Node<'tree>,
    parent: Option<Node<'tree>>,
    grandparent: Option<Node<'tree>>,
    boolean_parent: bool,
    scope: usize,
    nesting: u32,
}

fn scope_kind(node: Node<'_>, parent: Option<Node<'_>>, language: Language) -> Option<ScopeKind> {
    match (language, node.kind()) {
        (Language::Python, "decorated_definition")
            if node
                .child_by_field_name("definition")
                .is_some_and(|definition| definition.kind() == "function_definition") =>
        {
            Some(ScopeKind::Function)
        }
        (Language::Python, "decorated_definition")
            if node
                .child_by_field_name("definition")
                .is_some_and(|definition| definition.kind() == "class_definition") =>
        {
            Some(ScopeKind::Class)
        }
        (Language::Python, "function_definition")
            if !parent.is_some_and(|parent| parent.kind() == "decorated_definition") =>
        {
            Some(ScopeKind::Function)
        }
        (Language::Python, "class_definition")
            if !parent.is_some_and(|parent| parent.kind() == "decorated_definition") =>
        {
            Some(ScopeKind::Class)
        }
        (
            Language::Java,
            "method_declaration"
            | "constructor_declaration"
            | "compact_constructor_declaration"
            | "annotation_type_element_declaration",
        ) => Some(ScopeKind::Function),
        (
            Language::Java,
            "class_declaration"
            | "interface_declaration"
            | "enum_declaration"
            | "record_declaration"
            | "annotation_type_declaration",
        ) => Some(ScopeKind::Class),
        (Language::Java, "class_body")
            if parent.is_some_and(|parent| {
                matches!(
                    parent.kind(),
                    "object_creation_expression" | "enum_constant"
                )
            }) =>
        {
            Some(ScopeKind::Class)
        }
        (Language::Java, "lambda_expression")
            if !parent.is_some_and(|parent| parent.kind() == "switch_label") =>
        {
            Some(ScopeKind::Lambda)
        }
        (Language::Java, "static_initializer") => Some(ScopeKind::Initializer),
        (Language::Java, "block")
            if parent.is_some_and(|parent| {
                matches!(parent.kind(), "class_body" | "enum_body_declarations")
            }) =>
        {
            Some(ScopeKind::Initializer)
        }
        _ => None,
    }
}

fn wrapper_function(parent: Node<'_>, child: Node<'_>) -> bool {
    let Some(body) = parent.child_by_field_name("body") else {
        return false;
    };
    let mut cursor = body.walk();
    let is_wrapper = body
        .named_children(&mut cursor)
        .filter(|statement| statement.kind() != "comment")
        .all(|statement| {
            let candidate = if statement.kind() == "decorated_definition" {
                statement
                    .child_by_field_name("definition")
                    .unwrap_or(statement)
            } else {
                statement
            };
            if candidate.id() == child.id() {
                return true;
            }
            candidate.kind() == "return_statement"
                && candidate.named_child_count() == 1
                && candidate
                    .named_child(0)
                    .is_some_and(|value| value.kind() == "identifier")
        });
    is_wrapper
}

fn scope_facts(node: Node<'_>, source: &str, report_unit: bool, eligible: bool) -> CognitiveScope {
    CognitiveScope {
        name: name(node, source),
        start_line: (node.start_position().row + 1) as u32,
        end_line: (node.end_position().row + 1) as u32,
        start_byte: node.start_byte(),
        end_byte: node.end_byte(),
        events: Vec::new(),
        report_unit,
        standalone_eligible: eligible,
    }
}

fn enter_scope<'tree>(
    frame: &mut Frame<'tree>,
    kind: ScopeKind,
    facts: &mut CognitiveFacts,
    scopes: &mut Vec<Scope<'tree>>,
    source: &str,
) {
    let declaration = if frame.node.kind() == "decorated_definition" {
        frame
            .node
            .child_by_field_name("definition")
            .unwrap_or(frame.node)
    } else {
        frame.node
    };
    let parent = &scopes[frame.scope];
    let eligible = kind == ScopeKind::Initializer
        || facts.language == Language::Python
        || (parent.eligible_members && frame.node.child_by_field_name("body").is_some());
    let inheritance = match (facts.language, kind) {
        (Language::Python, ScopeKind::Class) => Nesting::Reset,
        (Language::Python, ScopeKind::Function) if parent.kind == ScopeKind::Function => {
            Nesting::Inherit(frame.nesting + u32::from(!wrapper_function(parent.node, declaration)))
        }
        (Language::Python, _) => Nesting::Reset,
        (Language::Java, ScopeKind::Class | ScopeKind::Lambda) => {
            Nesting::Inherit(frame.nesting + 1)
        }
        (Language::Java, _) => Nesting::Inherit(frame.nesting),
    };
    let local_class_ancestor = parent.local_class_ancestor
        || (kind == ScopeKind::Class
            && matches!(parent.kind, ScopeKind::Function | ScopeKind::Lambda));
    let eligible_members = if kind == ScopeKind::Class {
        frame.node.kind() != "class_body" && !local_class_ancestor
    } else {
        parent.eligible_members
    };
    let index = facts.scopes.len();
    facts.scopes[frame.scope]
        .events
        .push(CognitiveEvent::Child {
            scope: index,
            nesting: inheritance,
        });
    facts.scopes.push(scope_facts(
        frame.node,
        source,
        matches!(kind, ScopeKind::Function | ScopeKind::Initializer),
        eligible,
    ));
    facts.scopes[index].name = name(declaration, source);
    if facts.language == Language::Java
        && ((kind == ScopeKind::Function && eligible) || kind == ScopeKind::Initializer)
    {
        facts.file_roots.push(index);
    }
    scopes.push(Scope {
        node: declaration,
        kind,
        eligible_members,
        local_class_ancestor,
    });
    frame.scope = index;
    frame.nesting = 0;
}

fn boolean_operator<'a>(node: Node<'_>, source: &'a str, language: Language) -> Option<&'a str> {
    let kind = if language == Language::Java {
        "binary_expression"
    } else {
        "boolean_operator"
    };
    if node.kind() != kind {
        return None;
    }
    let operator = node.child_by_field_name("operator")?;
    let value = text(operator, source);
    matches!(value, "&&" | "||" | "and" | "or").then_some(value)
}

fn unwrap_parentheses(mut node: Node<'_>, language: Language) -> Node<'_> {
    if language == Language::Java {
        while node.kind() == "parenthesized_expression" {
            let mut cursor = node.walk();
            let Some(child) = node
                .named_children(&mut cursor)
                .find(|child| !child.is_extra())
            else {
                break;
            };
            node = child;
        }
    }
    node
}

fn boolean_sequences(root: Node<'_>, source: &str, language: Language) -> u32 {
    let mut pending = vec![(root, false)];
    let mut previous = None;
    let mut count = 0;
    while let Some((node, emit)) = pending.pop() {
        let node = unwrap_parentheses(node, language);
        let Some(operator) = boolean_operator(node, source, language) else {
            continue;
        };
        if emit {
            if previous != Some(operator) {
                count += 1;
            }
            previous = Some(operator);
            continue;
        }
        if let Some(right) = node.child_by_field_name("right") {
            pending.push((right, false));
        }
        pending.push((node, true));
        if let Some(left) = node.child_by_field_name("left") {
            pending.push((left, false));
        }
    }
    count
}

fn else_if(node: Node<'_>, parent: Option<Node<'_>>) -> bool {
    parent.is_some_and(|parent| {
        parent.kind() == "if_statement"
            && parent
                .child_by_field_name("alternative")
                .is_some_and(|alternative| alternative.id() == node.id())
    })
}

fn switch_statement(
    node: Node<'_>,
    parent: Option<Node<'_>>,
    grandparent: Option<Node<'_>>,
) -> bool {
    node.kind() == "switch_expression"
        && parent.is_some_and(|parent| {
            if parent.kind() == "expression_statement"
                && grandparent.is_some_and(|parent| parent.kind() == "switch_rule")
            {
                return false;
            }
            matches!(
                parent.kind(),
                "block"
                    | "constructor_body"
                    | "switch_block_statement_group"
                    | "labeled_statement"
                    | "expression_statement"
            ) || matches!(
                parent.kind(),
                "if_statement"
                    | "for_statement"
                    | "enhanced_for_statement"
                    | "while_statement"
                    | "do_statement"
            ) && ["body", "consequence", "alternative"]
                .into_iter()
                .any(|field| {
                    parent
                        .child_by_field_name(field)
                        .is_some_and(|child| child.id() == node.id())
                })
        })
}

fn add_events(frame: Frame<'_>, facts: &mut CognitiveFacts, source: &str) {
    let node = frame.node;
    let language = facts.language;
    let events = &mut facts.scopes[frame.scope].events;
    let structural = match language {
        Language::Python => matches!(
            node.kind(),
            "if_statement"
                | "for_statement"
                | "while_statement"
                | "except_clause"
                | "conditional_expression"
        ),
        Language::Java => {
            matches!(
                node.kind(),
                "if_statement"
                    | "for_statement"
                    | "enhanced_for_statement"
                    | "while_statement"
                    | "do_statement"
                    | "catch_clause"
                    | "ternary_expression"
            ) || switch_statement(node, frame.parent, frame.grandparent)
        }
    };
    if structural {
        if language == Language::Java
            && node.kind() == "if_statement"
            && else_if(node, frame.parent)
        {
            events.push(CognitiveEvent::Flat { increments: 1 });
        } else {
            events.push(CognitiveEvent::Structural {
                nesting: frame.nesting,
            });
        }
    }
    if language == Language::Python && matches!(node.kind(), "elif_clause" | "else_clause") {
        events.push(CognitiveEvent::Flat { increments: 1 });
    }
    if language == Language::Java {
        if node.kind() == "if_statement"
            && node
                .child_by_field_name("alternative")
                .is_some_and(|alternative| alternative.kind() != "if_statement")
        {
            events.push(CognitiveEvent::Flat { increments: 1 });
        }
        if matches!(node.kind(), "break_statement" | "continue_statement")
            && node.named_child_count() > 0
        {
            events.push(CognitiveEvent::Flat { increments: 1 });
        }
    }
    if boolean_operator(node, source, language).is_some() && !frame.boolean_parent {
        events.push(CognitiveEvent::Flat {
            increments: boolean_sequences(node, source, language),
        });
    }
}

fn switch_lambda_nesting(rule: Node<'_>, child: Node<'_>) -> u32 {
    if child.kind() == "switch_label" {
        return 0;
    }
    let mut cursor = rule.walk();
    let Some(label) = rule
        .named_children(&mut cursor)
        .find(|node| node.kind() == "switch_label")
    else {
        return 0;
    };
    let mut cursor = label.walk();
    let Some(lambda) = label
        .named_children(&mut cursor)
        .find(|node| node.kind() == "lambda_expression")
    else {
        return 0;
    };
    // The grammar can consume `A -> v` as a label lambda in `case A -> v -> body`.
    // Its prefix belongs to the rule; the actual lambda body is the rule's sibling.
    let Some(mut prefix) = lambda.child_by_field_name("body") else {
        return 0;
    };
    let mut nesting = 1;
    loop {
        prefix = unwrap_parentheses(prefix, Language::Java);
        let next = match prefix.kind() {
            "ternary_expression" => {
                nesting += 1;
                prefix.child_by_field_name("alternative")
            }
            "lambda_expression" => {
                nesting += 1;
                prefix.child_by_field_name("body")
            }
            "cast_expression" => prefix.child_by_field_name("value"),
            "assignment_expression" => prefix.child_by_field_name("right"),
            _ => None,
        };
        let Some(next) = next else {
            return nesting;
        };
        prefix = next;
    }
}

fn child_nesting(
    node: Node<'_>,
    parent: Option<Node<'_>>,
    grandparent: Option<Node<'_>>,
    child: Node<'_>,
    nesting: u32,
    language: Language,
) -> u32 {
    if language == Language::Python {
        if node.kind() == "conditional_expression" {
            return nesting + 1;
        }
        if node.kind() == "block" {
            let excluded = parent.is_some_and(|parent| {
                matches!(
                    parent.kind(),
                    "module"
                        | "try_statement"
                        | "finally_clause"
                        | "class_definition"
                        | "function_definition"
                        | "with_statement"
                        | "match_statement"
                )
            });
            return nesting + u32::from(!excluded);
        }
        return nesting;
    }
    match node.kind() {
        "switch_rule" => nesting + switch_lambda_nesting(node, child),
        "if_statement" => {
            let consequence = node
                .child_by_field_name("consequence")
                .is_some_and(|branch| branch.id() == child.id());
            let alternative = node
                .child_by_field_name("alternative")
                .is_some_and(|branch| branch.id() == child.id() && child.kind() != "if_statement");
            nesting + u32::from(consequence || alternative)
        }
        "for_statement"
        | "enhanced_for_statement"
        | "while_statement"
        | "do_statement"
        | "ternary_expression" => nesting + 1,
        "switch_expression" if switch_statement(node, parent, grandparent) => nesting + 1,
        "catch_clause" if child.kind() == "block" => nesting + 1,
        _ => nesting,
    }
}

pub(super) fn extract(root: Node<'_>, source: &str, language: Language) -> CognitiveFacts {
    let mut file_scope = scope_facts(root, source, false, true);
    file_scope.start_byte = 0;
    file_scope.end_byte = source.len();
    file_scope.start_line = 1;
    file_scope.end_line = source.lines().count().max(1).try_into().unwrap_or(u32::MAX);
    let mut facts = CognitiveFacts {
        language,
        scopes: vec![file_scope],
        file_roots: Vec::new(),
    };
    let mut scopes = vec![Scope {
        node: root,
        kind: ScopeKind::File,
        eligible_members: language == Language::Python,
        local_class_ancestor: false,
    }];
    let mut pending = vec![Frame {
        node: root,
        parent: None,
        grandparent: None,
        boolean_parent: false,
        scope: 0,
        nesting: 0,
    }];
    while let Some(mut frame) = pending.pop() {
        if let Some(kind) = scope_kind(frame.node, frame.parent, language) {
            enter_scope(&mut frame, kind, &mut facts, &mut scopes, source);
        }
        add_events(frame, &mut facts, source);
        let mut cursor = frame.node.walk();
        let children: Vec<_> = frame.node.named_children(&mut cursor).collect();
        let boolean_parent =
            if language == Language::Java && frame.node.kind() == "parenthesized_expression" {
                frame.boolean_parent
            } else {
                boolean_operator(frame.node, source, language).is_some()
            };
        for child in children.into_iter().rev() {
            pending.push(Frame {
                node: child,
                parent: Some(frame.node),
                grandparent: frame.parent,
                boolean_parent,
                scope: frame.scope,
                nesting: child_nesting(
                    frame.node,
                    frame.parent,
                    frame.grandparent,
                    child,
                    frame.nesting,
                    language,
                ),
            });
        }
    }
    facts
}
