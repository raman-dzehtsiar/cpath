// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

use crate::model::ComplexityMetrics;

/// Experimental ordering, not a defect probability or calibrated review cost.
pub const FORMULA: &str = "cpath.complexity-ranking.v1";
/// Provisional cutoff for the secondary burden signal, not an established limit.
pub const BURDEN_THRESHOLD: u32 = 15;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct ComplexityScore {
    /// Primary ordering key and the integer printed by the CLI.
    pub maximum: u32,
    /// Secondary key: sum of each callable's complexity above the threshold.
    pub burden: u64,
}

pub trait Scorer: Send + Sync {
    fn score(&self, complexity: &ComplexityMetrics) -> ComplexityScore;
}

pub struct ComplexityFirst;

impl Scorer for ComplexityFirst {
    fn score(&self, complexity: &ComplexityMetrics) -> ComplexityScore {
        ComplexityScore {
            maximum: complexity.max_method,
            burden: complexity.methods.iter().fold(0u64, |sum, method| {
                sum.saturating_add(u64::from(method.score.saturating_sub(BURDEN_THRESHOLD)))
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::MethodComplexity;

    fn score(values: &[u32]) -> ComplexityScore {
        ComplexityFirst.score(&ComplexityMetrics {
            max_method: values.iter().copied().max().unwrap_or(0),
            total: values.iter().copied().fold(0, u32::saturating_add),
            methods: values
                .iter()
                .map(|&score| MethodComplexity {
                    name: "M".into(),
                    start_line: 1,
                    end_line: 1,
                    score,
                })
                .collect(),
        })
    }

    #[test]
    fn burden_counts_only_excess_above_threshold() {
        assert_eq!(score(&[]), ComplexityScore::default());
        assert_eq!(
            score(&[0, 14, 15, 16, 40]),
            ComplexityScore {
                maximum: 40,
                burden: 26
            }
        );
    }

    #[test]
    fn concentration_breaks_ties_without_penalizing_many_simple_methods() {
        let mut large_simple_file = vec![40];
        large_simple_file.extend([2; 90]);
        assert_eq!(score(&large_simple_file).burden, 25);
        assert_eq!(score(&[40; 5]).burden, 125);
        assert!(score(&[40; 5]) > score(&large_simple_file));
        assert!(score(&[41]) > score(&[40; 5]));
    }

    #[test]
    fn burden_can_exceed_u32_without_overflow() {
        assert_eq!(score(&[u32::MAX; 2]).burden, 2 * u64::from(u32::MAX - 15));
    }
}
