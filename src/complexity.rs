// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

//! Versioned, syntax-based approximation of cognitive complexity.

use crate::model::{ComplexityMetrics, ControlKind, FileFacts, MethodComplexity};

mod scoped;

pub trait ComplexityAnalyzer: Send + Sync {
    fn analyze(&self, file: &FileFacts) -> ComplexityMetrics;
}

/// C# uses `cpath-cognitive-v2`, calibrated against the pinned Sonar C# engine.
/// Other languages carry independent facts for their pinned-tool profiles.
#[derive(Default)]
pub struct CognitiveComplexity;

impl ComplexityAnalyzer for CognitiveComplexity {
    fn analyze(&self, file: &FileFacts) -> ComplexityMetrics {
        if let Some(facts) = &file.profiled_cognitive {
            return crate::metrics::analyze_cognitive(facts);
        }
        if let Some(facts) = &file.cognitive {
            return scoped::analyze(facts);
        }
        IndependentCognitiveComplexity.analyze(file)
    }
}

/// Original v1 profile, retained for its independent hand-audited corpus and
/// for explicitly historical syntax-observation experiments.
#[derive(Default)]
pub struct IndependentCognitiveComplexity;

impl ComplexityAnalyzer for IndependentCognitiveComplexity {
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
                    span: None,
                    start_line: method.start_line,
                    end_line: method.end_line,
                    score,
                }
            })
            .collect();
        ComplexityMetrics {
            has_unscored_scopes: false,
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
            cognitive: None,
            profiled_cognitive: None,
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
