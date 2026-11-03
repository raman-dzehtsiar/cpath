// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

//! Independent web/Go metric profiles over compact, parser-free facts.

use crate::maintainability::{MaintainabilityMetrics, UnitFacts, UnitMaintainability};
use crate::model::{ComplexityMetrics, MethodComplexity};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    JavaScript,
    TypeScript,
    Go,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CognitiveEvent {
    pub structural: bool,
    pub own_nesting: u32,
    pub file_nesting: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CognitiveUnit {
    pub name: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: u32,
    pub end_line: u32,
    pub top_level: usize,
    pub nested: bool,
    pub react_component: bool,
    pub recursive: bool,
    pub events: Vec<CognitiveEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CognitiveFacts {
    pub language: Language,
    pub source_length: usize,
    pub source_lines: u32,
    pub units: Vec<CognitiveUnit>,
    pub global_events: Vec<CognitiveEvent>,
    /// Go's named-declaration teacher does not score these literal bodies.
    pub unscored_global_function_literals: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaintainabilityFacts {
    pub language: Language,
    pub units: Vec<MaintainabilityUnit>,
    /// Corrected cognitive syntax required the unchanged RCA grammar for MI.
    pub used_compatibility_grammar: bool,
    /// A source root rejected by that independent MI grammar remains unscored.
    pub unscored_roots: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaintainabilityUnit {
    pub facts: UnitFacts,
    pub formula_lines: u64,
    pub cyclomatic: u64,
}

fn increment(event: &CognitiveEvent, nesting: u32) -> u32 {
    if event.structural {
        nesting.saturating_add(1)
    } else {
        1
    }
}

pub fn analyze_cognitive(facts: &CognitiveFacts) -> ComplexityMetrics {
    let mut methods: Vec<_> = facts
        .units
        .iter()
        .map(|unit| {
            let score = unit
                .events
                .iter()
                .fold(u32::from(unit.recursive), |sum, event| {
                    sum.saturating_add(increment(event, event.own_nesting))
                });
            MethodComplexity {
                name: unit.name.clone(),
                start_line: unit.start_line,
                end_line: unit.end_line,
                score,
                span: Some(crate::model::cognitive::Span {
                    start: unit.start_byte,
                    end: unit.end_byte,
                }),
            }
        })
        .collect();
    let global = facts.global_events.iter().fold(0_u32, |sum, event| {
        sum.saturating_add(increment(event, event.file_nesting))
    });
    let total = if facts.language == Language::Go {
        methods
            .iter()
            .fold(0_u32, |sum, unit| sum.saturating_add(unit.score))
    } else {
        facts.units.iter().fold(global, |sum, unit| {
            let top = &facts.units[unit.top_level];
            let nested_surcharge = u32::from(
                unit.nested
                    && !top.react_component
                    && top.events.iter().any(|event| event.structural),
            );
            unit.events.iter().fold(sum, |sum, event| {
                sum.saturating_add(increment(
                    event,
                    event.file_nesting.saturating_add(nested_surcharge),
                ))
            })
        })
    };
    if facts.language != Language::Go && global > 0 {
        methods.push(MethodComplexity {
            name: "<module>".into(),
            start_line: 1,
            end_line: facts.source_lines.max(1),
            span: Some(crate::model::cognitive::Span {
                start: 0,
                end: facts.source_length,
            }),
            score: global,
        });
    }
    ComplexityMetrics {
        max_method: methods.iter().map(|unit| unit.score).max().unwrap_or(0),
        total,
        methods,
        has_unscored_scopes: facts.language == Language::Go
            && (facts.unscored_global_function_literals > 0 || !facts.global_events.is_empty()),
    }
}

pub fn analyze_maintainability(facts: &MaintainabilityFacts) -> MaintainabilityMetrics {
    let profile = Some(match facts.language {
        Language::JavaScript => "cpath-mi-javascript-rca-v1",
        Language::TypeScript => "cpath-mi-typescript-rca-v1",
        Language::Go => "cpath-mi-go-maintidx-v1",
    });
    let mut result = MaintainabilityMetrics {
        profile,
        unscored_units: facts.unscored_roots,
        ..MaintainabilityMetrics::default()
    };
    for unit in &facts.units {
        let counts = unit.facts.counts;
        let length = counts.operators.saturating_add(counts.operands) as f64;
        let vocabulary = counts
            .distinct_operators
            .saturating_add(counts.distinct_operands) as f64;
        let volume = length * vocabulary.log2();
        let cyclomatic = unit.cyclomatic;
        let index = (171.0
            - 5.2 * volume.ln()
            - 0.23 * cyclomatic as f64
            - 16.2 * (unit.formula_lines as f64).ln())
            * 100.0
            / 171.0;
        let index = index.max(0.0);
        if !index.is_finite() {
            result.unscored_units += 1;
            continue;
        }
        let index = if facts.language == Language::Go {
            index.trunc()
        } else {
            index
        };
        result.minimum = Some(result.minimum.map_or(index, |current| current.min(index)));
        result.units.push(UnitMaintainability {
            facts: unit.facts.clone(),
            index,
            halstead_volume: volume,
            cyclomatic_complexity: cyclomatic,
        });
    }
    result
}
