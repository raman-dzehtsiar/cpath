// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

//! C# lexical scopes, control events and logical-expression edges from one walk.
use crate::model::cognitive::{CognitiveFacts, Event, Scope, ScopeKind, Signature, Span};
use crate::model::ControlKind;
use std::collections::HashMap;
use tree_sitter::Node;

pub(super) struct Builder {
    facts: CognitiveFacts,
    boolean_regions: HashMap<(usize, Span), Vec<BooleanEdge>>,
}

struct BooleanEdge {
    event: usize,
    position: usize,
    precedence: usize,
    left_group: Option<Span>,
    right_group: Option<Span>,
}

impl Builder {
    pub fn new(root: Node<'_>) -> Self {
        Self {
            facts: CognitiveFacts {
                scopes: vec![scope(root, ScopeKind::File, "<file>".into(), None)],
                top_level: None,
            },
            boolean_regions: HashMap::new(),
        }
    }

    pub fn enter(&mut self, node: Node<'_>, parent: usize, nesting: u32, source: &str) -> usize {
        if node.kind() == "global_statement" {
            if let Some(index) = self.facts.top_level {
                self.facts.scopes[index].span.end = node.end_byte();
                self.facts.scopes[index].end_line = node.end_position().row as u32 + 1;
                return index;
            }
            let index = self.add(
                node,
                ScopeKind::TopLevel,
                "<top-level>".into(),
                None,
                parent,
                nesting,
            );
            self.facts.top_level = Some(index);
            return index;
        }
        if !super::is_callable(node) && node.kind() != "field_declaration" {
            return parent;
        }
        let kind = match node.kind() {
            "method_declaration" => ScopeKind::Method,
            "lambda_expression" => ScopeKind::Lambda,
            "local_function_statement" => ScopeKind::Local {
                is_static: (0..node.child_count()).any(|i| {
                    node.child(i)
                        .is_some_and(|child| super::text(child, source) == "static")
                }),
            },
            "field_declaration" => ScopeKind::Field,
            _ => ScopeKind::Other,
        };
        let name = node
            .child_by_field_name("name")
            .or_else(|| node.child_by_field_name("operator"))
            .map(|name| super::text(name, source).to_owned())
            .unwrap_or_else(|| format!("<{}@{}>", node.kind(), super::line(node)));
        let signature = (kind == ScopeKind::Method).then(|| Signature {
            name: identifier_value(&name),
            arguments: count_children(node.child_by_field_name("parameters"), "parameter"),
        });
        self.add(node, kind, name, signature, parent, nesting)
    }

    fn add(
        &mut self,
        node: Node<'_>,
        kind: ScopeKind,
        name: String,
        signature: Option<Signature>,
        parent: usize,
        nesting: u32,
    ) -> usize {
        let index = self.facts.scopes.len();
        self.facts.scopes.push(scope(node, kind, name, signature));
        self.facts.scopes[parent]
            .events
            .push(Event::Child { index, nesting });
        index
    }

    pub fn observe(
        &mut self,
        node: Node<'_>,
        index: usize,
        nesting: u32,
        control: Option<ControlKind>,
        source: &str,
    ) {
        let events = &mut self.facts.scopes[index].events;
        if let Some(operator) = logical_operator(node, source) {
            let raw_left = node.child_by_field_name("left");
            let raw_right = node.child_by_field_name("right");
            let left = node.child_by_field_name("left").map(unparenthesized);
            let right = node.child_by_field_name("right").map(unparenthesized);
            {
                let region = boolean_region(node, source);
                self.boolean_regions
                    .entry((index, span(region)))
                    .or_default()
                    .push(BooleanEdge {
                        event: events.len(),
                        position: (0..node.child_count())
                            .filter_map(|index| node.child(index))
                            .find(|child| {
                                !child.is_named() && super::text(*child, source) == operator
                            })
                            .map_or(node.start_byte(), |operator| operator.start_byte()),
                        precedence: match operator {
                            "||" => 0,
                            "&&" => 1,
                            "or" => 2,
                            "and" => 3,
                            _ => unreachable!(),
                        },
                        left_group: raw_left.and_then(parenthesized_group),
                        right_group: raw_right.and_then(parenthesized_group),
                    });
            }
            events.push(Event::Logical {
                span: span(node),
                same_left: left.and_then(|left| logical_operator(left, source)) == Some(operator),
                same_right: right
                    .filter(|right| logical_operator(*right, source) == Some(operator))
                    .map(span),
            });
        } else if let Some(kind) = control.filter(|_| !is_variable_foreach(node)) {
            events.push(Event::Control { kind, nesting });
        }
        if node.kind() == "invocation_expression" {
            if let Some(function) = node
                .child_by_field_name("function")
                .filter(|function| function.kind() == "identifier")
            {
                events.push(Event::Invocation(Signature {
                    name: identifier_value(super::text(function, source)),
                    arguments: count_children(node.child_by_field_name("arguments"), "argument"),
                }));
            }
        }
    }

    pub fn finish(mut self) -> CognitiveFacts {
        // Recover each parenthesized region's logical root from the operator
        // facts already collected by the parser walk. Wrapping pattern and
        // conditional-access nodes do not change C# operator precedence.
        let roots: HashMap<_, _> = self
            .boolean_regions
            .iter()
            .map(|(key, edges)| {
                let root = edges
                    .iter()
                    .min_by_key(|edge| (edge.precedence, std::cmp::Reverse(edge.position)))
                    .expect("nonempty logical region");
                let Event::Logical { span, .. } = self.facts.scopes[key.0].events[root.event]
                else {
                    unreachable!()
                };
                (*key, (root.precedence, span))
            })
            .collect();
        for ((scope, _), mut edges) in self.boolean_regions {
            edges.sort_unstable_by_key(|edge| edge.position);
            let mut seen = [false; 4];
            for (index, edge) in edges.iter().enumerate() {
                let Event::Logical {
                    same_left,
                    same_right,
                    ..
                } = &mut self.facts.scopes[scope].events[edge.event]
                else {
                    unreachable!()
                };
                let left = edge.left_group.and_then(|group| roots.get(&(scope, group)));
                let right = edge
                    .right_group
                    .and_then(|group| roots.get(&(scope, group)));
                *same_left = seen[edge.precedence]
                    || (!seen[edge.precedence + 1..].iter().any(|seen| *seen)
                        && left.is_some_and(|(precedence, _)| *precedence == edge.precedence));
                *same_right = right
                    .filter(|(precedence, _)| {
                        *precedence == edge.precedence
                            && edges
                                .get(index + 1)
                                .is_none_or(|next| next.precedence <= edge.precedence)
                    })
                    .map(|(_, span)| *span);
                seen[edge.precedence] = true;
                seen[edge.precedence + 1..].fill(false);
            }
        }
        self.facts
    }
}

fn boolean_region<'tree>(mut node: Node<'tree>, source: &str) -> Node<'tree> {
    while let Some(parent) = node.parent() {
        let transparent = match parent.kind() {
            "invocation_expression" => parent.child_by_field_name("function") == Some(node),
            "element_access_expression" => parent.child_by_field_name("expression") == Some(node),
            "binary_expression" => parent
                .child_by_field_name("operator")
                .is_some_and(|operator| super::text(operator, source) != "??"),
            "conditional_access_expression"
            | "member_access_expression"
            | "member_binding_expression"
            | "is_expression"
            | "is_pattern_expression"
            | "constant_pattern"
            | "negated_pattern"
            | "relational_pattern"
            | "and_pattern"
            | "or_pattern"
            | "as_expression"
            | "prefix_unary_expression"
            | "postfix_unary_expression"
            | "cast_expression"
            | "await_expression" => true,
            _ => false,
        };
        if !transparent {
            break;
        }
        node = parent;
    }
    node
}

fn parenthesized_group(node: Node<'_>) -> Option<Span> {
    matches!(
        node.kind(),
        "parenthesized_expression" | "parenthesized_pattern"
    )
    .then(|| span(unparenthesized(node)))
}

/// Roslyn represents a deconstructing enumeration as ForEachVariableStatement,
/// distinct from ForEachStatement. The pinned Sonar visitor does not increment
/// or increase nesting for the former. Keep v1's ordinary loop facts intact.
pub(super) fn is_variable_foreach(node: Node<'_>) -> bool {
    node.kind() == "foreach_statement"
        && node.child_by_field_name("left").is_some_and(|left| {
            matches!(
                left.kind(),
                "tuple_pattern"
                    | "tuple_expression"
                    | "declaration_expression"
                    | "parenthesized_expression"
            )
        })
}

fn span(node: Node<'_>) -> Span {
    Span {
        start: node.start_byte(),
        end: node.end_byte(),
    }
}

fn scope(node: Node<'_>, kind: ScopeKind, name: String, signature: Option<Signature>) -> Scope {
    Scope {
        kind,
        name,
        signature,
        syntax_kind: node.kind().into(),
        span: span(node),
        start_line: super::line(node),
        end_line: node.end_position().row as u32 + 1,
        events: vec![],
    }
}

fn count_children(node: Option<Node<'_>>, kind: &str) -> usize {
    node.map_or(0, |node| {
        (0..node.child_count())
            .filter(|index| {
                node.child(*index).is_some_and(|child| {
                    child.kind() == kind
                        || (kind == "parameter"
                            && (child.kind() == "__arglist"
                                || node.field_name_for_child(*index as u32) == Some("name")))
                })
            })
            .count()
    })
}

fn unparenthesized(mut node: Node<'_>) -> Node<'_> {
    while matches!(
        node.kind(),
        "parenthesized_expression" | "parenthesized_pattern"
    ) {
        let Some(child) = (0..node.named_child_count())
            .filter_map(|index| node.named_child(index))
            .find(|child| child.kind() != "comment")
        else {
            break;
        };
        node = child;
    }
    node
}

fn logical_operator<'source>(node: Node<'_>, source: &'source str) -> Option<&'source str> {
    match node.kind() {
        "and_pattern" => Some("and"),
        "or_pattern" => Some("or"),
        _ => super::boolean_operator(node, source),
    }
}

fn identifier_value(name: &str) -> String {
    let name = name.strip_prefix('@').unwrap_or(name);
    if !name.contains('\\') {
        return name.into();
    }
    let mut result = String::new();
    let mut chars = name.chars();
    while let Some(character) = chars.next() {
        if character != '\\' {
            result.push(character);
            continue;
        }
        let digits = match chars.next() {
            Some('u') => 4,
            Some('U') => 8,
            _ => return name.into(),
        };
        let hex: String = chars.by_ref().take(digits).collect();
        let Some(value) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) else {
            return name.into();
        };
        result.push(value);
    }
    result
}
