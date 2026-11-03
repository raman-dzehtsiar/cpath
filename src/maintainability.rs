// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

//! Toolchain-free maintainability approximation. Extraction and scoring are separate.
//! See docs/maintainability.md for the complete, versioned input contract.

pub const FORMULA: &str = "cpath-maintainability-v2";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitKind {
    Callable,
    Initializer,
    TopLevel,
    /// A separately measured class, namespace, trait, impl or analogous scope.
    Container,
}

/// Counts contain no source buffers, trees, tokens or symbol-resolution claims.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyntaxCounts {
    pub operators: u64,
    pub operands: u64,
    pub distinct_operators: u64,
    pub distinct_operands: u64,
    /// C# statement observations; other profiles use their own typed line inputs.
    pub logical_statements: u64,
    pub decisions: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitFacts {
    pub name: String,
    pub kind: UnitKind,
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: u32,
    pub end_line: u32,
    pub counts: SyntaxCounts,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MaintainabilityFacts {
    pub units: Vec<UnitFacts>,
    /// None preserves the original Microsoft-calibrated C# input contract.
    pub profile: Option<crate::metrics::MaintainabilityInput>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UnitMaintainability {
    pub facts: UnitFacts,
    /// Higher is easier to maintain under this approximation; range 0..=100.
    pub index: f64,
    pub halstead_volume: f64,
    pub cyclomatic_complexity: u64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MaintainabilityMetrics {
    pub profile: Option<&'static str>,
    /// Inputs for which the selected formula has no finite, defined result.
    pub unscored_units: usize,
    /// None means no source unit has a scored result, not a perfect score.
    pub minimum: Option<f64>,
    pub units: Vec<UnitMaintainability>,
}

/// Complete aggregates plus bounded evidence. Never reaggregate from `worst`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MaintainabilitySummary {
    pub profile: Option<&'static str>,
    pub unscored_units: usize,
    pub minimum: Option<f64>,
    pub unit_count: usize,
    /// Only profiles that measure this counter expose it; None is unmeasured.
    pub logical_statements: Option<u64>,
    pub worst: Vec<UnitMaintainability>,
}

pub trait MaintainabilityAnalyzer: Send + Sync {
    fn analyze(&self, facts: &MaintainabilityFacts) -> MaintainabilityMetrics;
}

#[derive(Default)]
pub struct ApproximateMaintainability;

impl ApproximateMaintainability {
    pub fn measure(counts: SyntaxCounts) -> (f64, f64, u64) {
        let length = counts.operators as f64 + counts.operands as f64;
        let vocabulary = counts.distinct_operators as f64 + counts.distinct_operands as f64;
        let volume = length * vocabulary.max(1.0).log2();
        let cyclomatic = counts.decisions.saturating_add(1);
        let index = if length == 0.0 && counts.logical_statements == 0 && counts.decisions == 0 {
            100.0
        } else {
            ((171.0
                - 5.2 * volume.max(1.0).ln()
                - 0.23 * cyclomatic as f64
                - 16.2 * (counts.logical_statements.max(1) as f64).ln())
                * 100.0
                / 171.0)
                .clamp(0.0, 100.0)
                .round_ties_even()
        };
        (index, volume, cyclomatic)
    }
}

impl MaintainabilityAnalyzer for ApproximateMaintainability {
    fn analyze(&self, facts: &MaintainabilityFacts) -> MaintainabilityMetrics {
        if let Some(profile) = &facts.profile {
            return crate::metrics::analyze_maintainability(profile);
        }
        let units: Vec<_> = facts
            .units
            .iter()
            .map(|facts| {
                let (index, halstead_volume, cyclomatic_complexity) = Self::measure(facts.counts);
                UnitMaintainability {
                    facts: facts.clone(),
                    index,
                    halstead_volume,
                    cyclomatic_complexity,
                }
            })
            .collect();
        MaintainabilityMetrics {
            profile: Some(FORMULA),
            unscored_units: 0,
            minimum: units.iter().map(|u| u.index).min_by(f64::total_cmp),
            units,
        }
    }
}

impl MaintainabilityMetrics {
    pub fn into_summary(mut self) -> MaintainabilitySummary {
        let unit_count = self.units.len();
        let logical_statements = (self.profile == Some(FORMULA)).then(|| {
            self.units.iter().fold(0_u64, |sum, unit| {
                sum.saturating_add(unit.facts.counts.logical_statements)
            })
        });
        let compare = |a: &UnitMaintainability, b: &UnitMaintainability| {
            a.index
                .total_cmp(&b.index)
                .then(a.facts.start_byte.cmp(&b.facts.start_byte))
                .then(a.facts.end_byte.cmp(&b.facts.end_byte))
        };
        if self.units.len() > 5 {
            self.units.select_nth_unstable_by(5, compare);
            self.units.truncate(5);
        }
        self.units.sort_by(compare);
        self.units.truncate(5);
        self.units.shrink_to_fit();
        MaintainabilitySummary {
            profile: self.profile,
            unscored_units: self.unscored_units,
            minimum: self.minimum,
            unit_count,
            logical_statements,
            worst: self.units,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formula_matches_independent_arithmetic_and_bounds() {
        let (index, volume, cc) = ApproximateMaintainability::measure(SyntaxCounts {
            operators: 2,
            operands: 2,
            distinct_operators: 2,
            distinct_operands: 2,
            logical_statements: 1,
            decisions: 0,
        });
        assert_eq!(volume, 8.0);
        assert_eq!(cc, 1);
        assert_eq!(
            index,
            ((171.0 - 5.2 * 8.0_f64.ln() - 0.23) * 100.0 / 171.0).round_ties_even()
        );
        assert_eq!(
            ApproximateMaintainability::measure(SyntaxCounts::default()).0,
            100.0
        );
        let huge = SyntaxCounts {
            operators: u64::MAX,
            operands: u64::MAX,
            distinct_operators: u64::MAX,
            distinct_operands: u64::MAX,
            logical_statements: u64::MAX,
            decisions: u64::MAX,
        };
        let (index, volume, _) = ApproximateMaintainability::measure(huge);
        assert_eq!(index, 0.0);
        assert!(volume.is_finite());
        assert_eq!(
            ApproximateMaintainability
                .analyze(&MaintainabilityFacts::default())
                .minimum,
            None
        );
    }
}
