// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

//! Independent Java and Python profiles, with parser-free scope facts.
use crate::maintainability::{MaintainabilityMetrics, UnitFacts, UnitMaintainability};
use crate::model::{ComplexityMetrics, MethodComplexity};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Java,
    Python,
}

#[derive(Debug, Clone, Copy)]
pub enum Nesting {
    Inherit(u32),
    Reset,
}

#[derive(Debug, Clone)]
pub enum CognitiveEvent {
    Structural { nesting: u32 },
    Flat { increments: u32 },
    Child { scope: usize, nesting: Nesting },
}

#[derive(Debug, Clone)]
pub struct CognitiveScope {
    pub name: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: u32,
    pub end_line: u32,
    pub events: Vec<CognitiveEvent>,
    pub report_unit: bool,
    pub standalone_eligible: bool,
}

#[derive(Debug, Clone)]
pub struct CognitiveFacts {
    pub language: Language,
    /// Preorder graph: every child follows its parent.
    pub scopes: Vec<CognitiveScope>,
    /// Java sums eligible methods and initializers; Python visits scope zero.
    pub file_roots: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MaintainabilityUnit {
    pub facts: UnitFacts,
    pub cyclomatic: u64,
    pub formula_lines: u64,
    /// Python's measured comment-adjustment inputs; unused and zero for Java.
    pub physical_source_lines: u64,
    pub comment_lines: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ReferenceParse {
    NotNeeded,
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MaintainabilityFacts {
    pub language: Language,
    pub units: Vec<MaintainabilityUnit>,
    pub reference_parse: ReferenceParse,
}

#[derive(Clone, Copy, Default)]
struct Contribution {
    base: u32,
    nesting_sensitive: u32,
}

fn scope_contribution(
    scope: &CognitiveScope,
    values: &[Contribution],
    index: usize,
) -> Contribution {
    let mut value = Contribution::default();
    for event in &scope.events {
        match *event {
            CognitiveEvent::Structural { nesting } => {
                value.base = value.base.saturating_add(nesting.saturating_add(1));
                value.nesting_sensitive = value.nesting_sensitive.saturating_add(1);
            }
            CognitiveEvent::Flat { increments } => {
                value.base = value.base.saturating_add(increments)
            }
            CognitiveEvent::Child {
                scope: child,
                nesting,
            } => {
                debug_assert!(child > index);
                let contribution = values[child];
                value.base = value.base.saturating_add(contribution.base);
                if let Nesting::Inherit(extra) = nesting {
                    value.base = value
                        .base
                        .saturating_add(contribution.nesting_sensitive.saturating_mul(extra));
                    value.nesting_sensitive = value
                        .nesting_sensitive
                        .saturating_add(contribution.nesting_sensitive);
                }
            }
        }
    }
    value
}

fn scope_values(facts: &CognitiveFacts, omit_functions: bool) -> Vec<Contribution> {
    let mut values = vec![Contribution::default(); facts.scopes.len()];
    for (index, scope) in facts.scopes.iter().enumerate().rev() {
        if omit_functions && scope.report_unit {
            continue;
        }
        values[index] = scope_contribution(scope, &values, index);
    }
    values
}

fn python_module(facts: &CognitiveFacts) -> Option<MethodComplexity> {
    if facts.language != Language::Python {
        return None;
    }
    let scope = facts.scopes.first()?;
    let values = scope_values(facts, true);
    let score = values.first()?.base;
    if score == 0 {
        return None;
    }
    Some(MethodComplexity {
        name: "<module>".into(),
        start_line: scope.start_line,
        end_line: scope.end_line,
        span: Some(crate::model::cognitive::Span {
            start: scope.start_byte,
            end: scope.end_byte,
        }),
        score,
    })
}

pub fn analyze_cognitive(facts: &CognitiveFacts) -> ComplexityMetrics {
    let values = scope_values(facts, false);
    let mut methods: Vec<_> = facts
        .scopes
        .iter()
        .zip(&values)
        .filter(|(scope, _)| scope.report_unit)
        .map(|(scope, value)| MethodComplexity {
            name: scope.name.clone(),
            start_line: scope.start_line,
            end_line: scope.end_line,
            span: Some(crate::model::cognitive::Span {
                start: scope.start_byte,
                end: scope.end_byte,
            }),
            score: if scope.standalone_eligible {
                value.base
            } else {
                0
            },
        })
        .collect();
    methods.extend(python_module(facts));
    let total = match facts.language {
        Language::Python => values.first().map_or(0, |value| value.base),
        Language::Java => facts
            .file_roots
            .iter()
            .fold(0_u32, |sum, index| sum.saturating_add(values[*index].base)),
    };
    ComplexityMetrics {
        max_method: methods.iter().map(|unit| unit.score).max().unwrap_or(0),
        total,
        methods,
        has_unscored_scopes: false,
    }
}

fn index(language: Language, unit: &MaintainabilityUnit, volume: f64) -> f64 {
    let lines = unit.formula_lines as f64;
    if language == Language::Python && (volume <= 0.0 || lines <= 0.0) {
        return 100.0;
    }
    let mut raw = 171.0 - 5.2 * volume.ln() - 0.23 * unit.cyclomatic as f64 - 16.2 * lines.ln();
    if language == Language::Python {
        let comments = if unit.physical_source_lines == 0 {
            0.0
        } else {
            100.0 * unit.comment_lines as f64 / unit.physical_source_lines as f64
        };
        raw += 50.0 * (2.46 * comments.to_radians()).sqrt().sin();
    }
    let scaled = raw * 100.0 / 171.0;
    match language {
        Language::Python => scaled.clamp(0.0, 100.0),
        Language::Java => scaled.max(0.0),
    }
}

pub fn analyze_maintainability(facts: &MaintainabilityFacts) -> MaintainabilityMetrics {
    let profile = match facts.language {
        Language::Java => "cpath-java-mi-rca-v1",
        Language::Python => "cpath-python-mi-radon-v1",
    };
    let mut result = MaintainabilityMetrics {
        profile: Some(profile),
        ..MaintainabilityMetrics::default()
    };
    for unit in &facts.units {
        let counts = unit.facts.counts;
        let vocabulary = counts
            .distinct_operators
            .saturating_add(counts.distinct_operands) as f64;
        let length = counts.operators.saturating_add(counts.operands) as f64;
        // RCA preserves IEEE undefined raw volume; its published VS score uses max(0).
        let volume = if facts.language == Language::Python && vocabulary == 0.0 {
            0.0
        } else {
            length * vocabulary.log2()
        };
        let index = index(facts.language, unit, volume);
        if !index.is_finite() {
            result.unscored_units += 1;
            continue;
        }
        result.units.push(UnitMaintainability {
            facts: unit.facts.clone(),
            index,
            halstead_volume: volume,
            cyclomatic_complexity: unit.cyclomatic,
        });
    }
    result.minimum = result
        .units
        .iter()
        .map(|unit| unit.index)
        .min_by(f64::total_cmp);
    result
}
