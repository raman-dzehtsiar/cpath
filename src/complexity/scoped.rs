// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

//! Cognitive v2 scoring over parser-independent C# scope and expression facts.
use crate::model::cognitive::{CognitiveFacts, Event, ScopeKind, Span};
use crate::model::{ComplexityMetrics, ControlKind, MethodComplexity};
use std::collections::HashSet;

#[derive(Clone, Copy)]
struct Frame {
    scope: usize,
    event: usize,
    nesting: u32,
}

fn scope_nesting(kind: ScopeKind) -> u32 {
    u32::from(matches!(kind, ScopeKind::Lambda | ScopeKind::Local { .. }))
}

fn score(facts: &CognitiveFacts, root: usize) -> u32 {
    let mut result = 0_u32;
    let mut ignored: HashSet<Span> = HashSet::new();
    let mut current_method = (facts.scopes[root].kind == ScopeKind::Method).then_some(root);
    let mut recursive = false;
    let mut stack = vec![Frame {
        scope: root,
        event: 0,
        nesting: scope_nesting(facts.scopes[root].kind),
    }];
    // Sonar's file metric visits globals once explicitly and once as file
    // members. The top-level callable itself is measured only once.
    if root == 0 {
        if let Some(scope) = facts.top_level {
            stack.push(Frame {
                scope,
                event: 0,
                nesting: 0,
            });
        }
    }
    while let Some(frame) = stack.last_mut() {
        let scope = &facts.scopes[frame.scope];
        let Some(event) = scope.events.get(frame.event) else {
            if scope.kind == ScopeKind::Method && recursive {
                result = result.saturating_add(1);
                recursive = false;
            }
            stack.pop();
            continue;
        };
        frame.event += 1;
        match event {
            Event::Control { kind, nesting } => {
                let increment = if matches!(kind, ControlKind::Else | ControlKind::BooleanSequence)
                {
                    1
                } else {
                    frame.nesting.saturating_add(*nesting).saturating_add(1)
                };
                result = result.saturating_add(increment);
            }
            Event::Logical {
                span,
                same_left,
                same_right,
            } => {
                if !ignored.contains(span) {
                    if !same_left {
                        result = result.saturating_add(1);
                    }
                    if let Some(right) = same_right {
                        ignored.insert(*right);
                    }
                }
            }
            Event::Invocation(call) => {
                if current_method
                    .and_then(|index| facts.scopes[index].signature.as_ref())
                    .is_some_and(|method| {
                        method.name == call.name && method.arguments == call.arguments
                    })
                {
                    recursive = true;
                }
            }
            Event::Child { index, nesting } => {
                let child = &facts.scopes[*index];
                if child.kind == (ScopeKind::Local { is_static: true }) {
                    continue;
                }
                let nesting = frame
                    .nesting
                    .saturating_add(*nesting)
                    .saturating_add(scope_nesting(child.kind));
                if child.kind == ScopeKind::Method {
                    current_method = Some(*index);
                }
                stack.push(Frame {
                    scope: *index,
                    event: 0,
                    nesting,
                });
            }
        }
    }
    result
}

pub(super) fn analyze(facts: &CognitiveFacts) -> ComplexityMetrics {
    let methods: Vec<_> = facts
        .scopes
        .iter()
        .enumerate()
        .skip(1)
        .map(|(index, scope)| MethodComplexity {
            span: Some(scope.span),
            name: scope.name.clone(),
            start_line: scope.start_line,
            end_line: scope.end_line,
            score: score(facts, index),
        })
        .collect();
    ComplexityMetrics {
        has_unscored_scopes: false,
        max_method: methods.iter().map(|method| method.score).max().unwrap_or(0),
        total: score(facts, 0),
        methods,
    }
}
