// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

//! Versioned, syntax-based approximation of cognitive complexity.

use crate::model::{ComplexityMetrics, ControlKind, FileFacts, MethodComplexity};

pub trait ComplexityAnalyzer: Send + Sync {
    fn analyze(&self, file: &FileFacts) -> ComplexityMetrics;
}

/// `cpath-cognitive-v1`: structural increments include nesting; other increments do not.
#[derive(Default)]
pub struct CognitiveComplexity;

impl ComplexityAnalyzer for CognitiveComplexity {
    fn analyze(&self, file: &FileFacts) -> ComplexityMetrics {
        let methods: Vec<_> = file
            .methods
            .iter()
            .map(|method| {
                let score = method.events.iter().fold(0_u32, |sum, event| {
                    let increment = match event.kind {
                        ControlKind::If
                        | ControlKind::Loop
                        | ControlKind::Switch
                        | ControlKind::Catch
                        | ControlKind::Ternary => event.nesting.saturating_add(1),
                        ControlKind::Else | ControlKind::BooleanSequence | ControlKind::Jump => 1,
                    };
                    sum.saturating_add(increment)
                });
                MethodComplexity {
                    name: method.name.clone(),
                    start_line: method.start_line,
                    end_line: method.end_line,
                    score,
                }
            })
            .collect();
        ComplexityMetrics {
            max_method: methods.iter().map(|method| method.score).max().unwrap_or(0),
            total: methods
                .iter()
                .fold(0_u32, |sum, method| sum.saturating_add(method.score)),
            methods,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ComplexityEvent, MethodFacts};

    #[test]
    fn aggregates_maximum_and_total_without_averaging_away_difficult_methods() {
        let file = FileFacts {
            path: "Example.cs".into(),
            bytes: 0,
            lines: 0,
            methods: vec![
                MethodFacts {
                    name: "Difficult".into(),
                    start_line: 1,
                    end_line: 10,
                    events: vec![
                        ComplexityEvent {
                            kind: ControlKind::If,
                            nesting: 0,
                            line: 2,
                        },
                        ComplexityEvent {
                            kind: ControlKind::Loop,
                            nesting: 1,
                            line: 3,
                        },
                        ComplexityEvent {
                            kind: ControlKind::BooleanSequence,
                            nesting: 2,
                            line: 3,
                        },
                        ComplexityEvent {
                            kind: ControlKind::Else,
                            nesting: 2,
                            line: 4,
                        },
                        ComplexityEvent {
                            kind: ControlKind::Jump,
                            nesting: 2,
                            line: 5,
                        },
                    ],
                },
                MethodFacts {
                    name: "Simple".into(),
                    start_line: 11,
                    end_line: 12,
                    events: vec![],
                },
            ],
        };
        let metrics = CognitiveComplexity.analyze(&file);
        assert_eq!(metrics.max_method, 6);
        assert_eq!(metrics.total, 6);
        assert_eq!(metrics.methods[0].score, 6);
        assert_eq!(metrics.methods[1].score, 0);
    }
}
