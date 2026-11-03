// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

//! Native-language profiles over parser-independent metric observations.

use crate::maintainability::{MaintainabilityMetrics, UnitFacts, UnitMaintainability};
use crate::model::{ComplexityMetrics, MethodComplexity};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    C,
    Cpp,
    Rust,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CognitiveEvent {
    pub nesting: u32,     // Enclosing structures prescribed by the selected profile.
    pub structural: bool, // Whether nesting contributes to this increment.
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicalOperator {
    And,
    Or,
}

impl LogicalOperator {
    pub(crate) fn index(self) -> usize {
        match self {
            Self::And => 0,
            Self::Or => 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CognitiveUnit {
    pub name: String,      // Source name, or an explicit anonymous callable label.
    pub start_byte: usize, // Inclusive original UTF-8 source position.
    pub end_byte: usize,   // Exclusive original UTF-8 source position.
    pub start_line: u32,   // One-based source location.
    pub end_line: u32,     // One-based source location.
    pub events: Vec<CognitiveEvent>, // Owned events; no syntax nodes or text retained.
    pub parent: Option<usize>, // Enclosing callable, when the definition aggregates children.
    pub parent_nesting: u32, // Additional C-family nesting when viewed from that callable.
    pub parent_logical: Option<LogicalOperator>, // Logical operator surrounding this definition.
    pub inherits_parent_logical: bool, // A call expression blocks inherited logical context.
    pub logical_roots: [u32; 2], // Root increments removed when that same operator surrounds the unit.
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CognitiveFacts {
    pub language: Language, // Independent C-family and Rust cognitive definitions.
    pub source_length: usize, // Complete decoded UTF-8 file range for derived module scopes.
    pub source_lines: u32,  // Physical lines, excluding the empty line after a final newline.
    pub units: Vec<CognitiveUnit>, // Callable results, including nested callables.
    pub global_events: Vec<CognitiveEvent>, // Events outside represented callables.
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaintainabilityUnit {
    pub source: UnitFacts, // Stable source identity and measured lexical counts.
    pub source_lines: u64, // Physical span lines, including comments and blank lines.
    pub cyclomatic: u64,   // Includes the base of each nested metric scope.
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaintainabilityFacts {
    pub language: Language, // Versioned language-specific token/scope policy.
    pub units: Vec<MaintainabilityUnit>, // File, container and callable observations.
}

fn cognitive_score(events: &[CognitiveEvent]) -> u32 {
    events.iter().fold(0_u32, |total, event| {
        let increment = if event.structural {
            event.nesting.saturating_add(1)
        } else {
            1
        };
        total.saturating_add(increment)
    })
}

pub fn analyze_cognitive(facts: &CognitiveFacts) -> ComplexityMetrics {
    let mut scores: Vec<_> = facts
        .units
        .iter()
        .map(|unit| cognitive_score(&unit.events))
        .collect();
    let own_total = scores
        .iter()
        .fold(cognitive_score(&facts.global_events), |total, score| {
            total.saturating_add(*score)
        });
    let mut structural: Vec<u32> = facts
        .units
        .iter()
        .map(|unit| {
            unit.events
                .iter()
                .filter(|event| event.structural)
                .count()
                .try_into()
                .unwrap_or(u32::MAX)
        })
        .collect();
    let mut logical_roots: Vec<_> = facts.units.iter().map(|unit| unit.logical_roots).collect();
    for child in (0..facts.units.len()).rev() {
        if let Some(parent) = facts.units[child].parent {
            let extra = if facts.language == Language::Rust {
                0
            } else {
                structural[child].saturating_mul(facts.units[child].parent_nesting)
            };
            let reduction = if facts.language == Language::Rust {
                0
            } else {
                facts.units[child]
                    .parent_logical
                    .map_or(0, |operator| logical_roots[child][operator.index()])
            };
            scores[parent] = scores[parent]
                .saturating_add(scores[child].saturating_sub(reduction))
                .saturating_add(extra);
            structural[parent] = structural[parent].saturating_add(structural[child]);
            if facts.units[child].parent_logical.is_none()
                && facts.units[child].inherits_parent_logical
            {
                let inherited = logical_roots[child];
                for (target, value) in logical_roots[parent].iter_mut().zip(inherited) {
                    *target = target.saturating_add(value);
                }
            }
        }
    }
    let mut methods: Vec<_> = facts
        .units
        .iter()
        .zip(scores)
        .map(|(unit, score)| MethodComplexity {
            name: unit.name.clone(),
            start_line: unit.start_line,
            end_line: unit.end_line,
            span: Some(crate::model::cognitive::Span {
                start: unit.start_byte,
                end: unit.end_byte,
            }),
            score,
        })
        .collect();
    let global_score = cognitive_score(&facts.global_events);
    if facts.language == Language::Rust && global_score > 0 {
        // RCA's file sum minus outermost callable sums is this non-callable
        // contribution. It is a derived module scope, not a teacher method.
        methods.push(MethodComplexity {
            name: "<module>".into(),
            start_line: 1,
            end_line: facts.source_lines.max(1),
            span: Some(crate::model::cognitive::Span {
                start: 0,
                end: facts.source_length,
            }),
            score: global_score,
        });
    }
    ComplexityMetrics {
        max_method: methods.iter().map(|method| method.score).max().unwrap_or(0),
        total: if facts.language == Language::Rust {
            own_total
        } else {
            methods
                .iter()
                .fold(0u32, |total, method| total.saturating_add(method.score))
        },
        methods,
        has_unscored_scopes: facts.language != Language::Rust && global_score > 0,
    }
}

pub fn analyze_maintainability(facts: &MaintainabilityFacts) -> MaintainabilityMetrics {
    let profile = match facts.language {
        Language::C => "cpath-mi-c-rca-v1",
        Language::Cpp => "cpath-mi-cpp-rca-v1",
        Language::Rust => "cpath-mi-rust-rca-v1",
    };
    let mut result = MaintainabilityMetrics {
        profile: Some(profile),
        ..Default::default()
    };
    for unit in &facts.units {
        let counts = unit.source.counts;
        let length = counts.operators as f64 + counts.operands as f64;
        let vocabulary = counts.distinct_operators as f64 + counts.distinct_operands as f64;
        let volume = length * vocabulary.log2();
        let original = 171.0
            - 5.2 * volume.ln()
            - 0.23 * unit.cyclomatic as f64
            - 16.2 * (unit.source_lines as f64).ln();
        // RCA's published VS profile maps NaN to zero through floating-point max.
        // Preserve that selected metric while retaining the undefined raw volume.
        let index = (original * 100.0 / 171.0).max(0.0);
        if !index.is_finite() {
            result.unscored_units += 1;
            continue;
        }
        result.minimum = Some(result.minimum.map_or(index, |minimum| minimum.min(index)));
        result.units.push(UnitMaintainability {
            facts: unit.source.clone(),
            index,
            halstead_volume: volume,
            cyclomatic_complexity: unit.cyclomatic,
        });
    }
    result
}
