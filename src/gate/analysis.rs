// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

use super::identity::Identities;
use crate::complexity::{CognitiveComplexity, ComplexityAnalyzer};
use crate::language::{BundledParser, SourceParser};
use crate::maintainability::{ApproximateMaintainability, MaintainabilityAnalyzer, UnitKind};
use crate::model::SourceLanguage;
use std::path::Path;

#[derive(Clone, Debug)]
pub(super) struct Unit {
    pub key: Option<String>, // Declaration identity; None means association is uncertain.
    pub name: String,        // Analyzer-provided display label, not used as identity alone.
    pub line: u32,           // One-based source location in this snapshot.
    pub value: f64,          // Unrounded analyzer output.
}

#[derive(Default)]
pub(super) struct Metric {
    pub units: Vec<Unit>,    // All measured scopes, never report-truncated.
    pub incomplete: bool,    // Missing observations prevent proving an unmatched scope is new.
    pub issues: Vec<String>, // Coverage failures for this metric.
}

#[derive(Default)]
pub(super) struct Analysis {
    pub cc: Metric, // Cognitive complexity observations.
    pub mi: Metric, // Maintainability observations.
}

pub(super) struct Analyzer {
    parser: BundledParser,                // Existing unmodified extraction engine.
    identity_parser: tree_sitter::Parser, // Separate gate-only metadata parser.
}

fn is_callable_scope(
    unit: &crate::model::MethodComplexity,
    identities: &Result<Identities, String>,
) -> bool {
    if matches!(unit.name.as_str(), "<file>" | "<module>" | "<top-level>") {
        return false;
    }
    let Some(span) = unit.span else {
        return true;
    };
    let Ok(identities) = identities else {
        return true;
    };
    // Keep uncertain associations so the existing coverage diagnostics can report them.
    identities.is_callable(span.start, span.end).unwrap_or(true)
}

impl Analyzer {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            parser: BundledParser::new()?,
            identity_parser: tree_sitter::Parser::new(),
        })
    }

    pub fn analyze(
        &mut self,
        path: &Path,
        language: SourceLanguage,
        bytes: &[u8],
    ) -> Result<Analysis, String> {
        let source = crate::source::decode(bytes)?;
        let parsed = self.parser.parse_analysis(path.to_path_buf(), &source)?;
        let identities = Identities::parse(&mut self.identity_parser, language, &source);
        let cc = CognitiveComplexity.analyze(&parsed.file);
        let mut analysis = Analysis::default();
        if cc.has_unscored_scopes {
            analysis.cc.incomplete = true;
            analysis
                .cc
                .issues
                .push("cognitive profile leaves some source scopes unscored".into());
        }
        for unit in cc.methods {
            if !is_callable_scope(&unit, &identities) {
                continue;
            }
            let key = unit.span.and_then(|span| {
                identities
                    .as_ref()
                    .ok()?
                    .key(&source, span.start, span.end, &unit.name)
            });
            analysis.cc.units.push(Unit {
                key,
                name: unit.name,
                line: unit.start_line.max(1),
                value: f64::from(unit.score),
            });
        }
        if let Some(facts) = parsed.maintainability {
            let mi = ApproximateMaintainability.analyze(&facts);
            if mi.unscored_units > 0 {
                analysis.mi.incomplete = true;
                analysis.mi.issues.push(format!(
                    "{} maintainability scopes have no defined score",
                    mi.unscored_units
                ));
            }
            for unit in mi.units {
                let facts = unit.facts;
                if facts.kind != UnitKind::Callable {
                    continue;
                }
                let key = identities.as_ref().ok().and_then(|identities| {
                    identities.key(&source, facts.start_byte, facts.end_byte, &facts.name)
                });
                analysis.mi.units.push(Unit {
                    key,
                    name: facts.name,
                    line: facts.start_line.max(1),
                    value: unit.index,
                });
            }
        } else {
            analysis.mi.incomplete = true;
            analysis
                .mi
                .issues
                .push("maintainability inputs are unavailable".into());
        }
        if let Err(error) = identities {
            analysis.cc.incomplete = true;
            analysis.mi.incomplete = true;
            analysis.cc.issues.push(error.clone());
            analysis.mi.issues.push(error);
        }
        Ok(analysis)
    }
}
