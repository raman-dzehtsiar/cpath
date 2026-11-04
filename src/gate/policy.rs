// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

use super::analysis::{Metric, Unit};
use super::Report;
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn grouped(metric: &Metric) -> BTreeMap<&str, Vec<&Unit>> {
    let mut groups: BTreeMap<&str, Vec<&Unit>> = BTreeMap::new();
    for unit in &metric.units {
        if let Some(key) = &unit.key {
            groups.entry(key).or_default().push(unit);
        }
    }
    groups
}

fn coverage(report: &mut Report, file: &str, side: &str, name: &str, metric: &Metric) {
    for issue in &metric.issues {
        report.issue(file, side, Some(name), None, issue);
    }
    for unit in &metric.units {
        if unit.key.is_none() {
            report.issue(
                file,
                side,
                Some(name),
                Some(unit),
                "scope identity could not be associated reliably",
            );
        }
    }
    for units in grouped(metric).values().filter(|units| units.len() > 1) {
        for unit in units {
            report.issue(
                file,
                side,
                Some(name),
                Some(unit),
                "scope identity is ambiguous",
            );
        }
    }
}

pub(super) fn compare(
    report: &mut Report,
    file: &str,
    name: &str,
    before: Option<&Metric>,
    after: &Metric,
    limit: f64,
) {
    coverage(report, file, "to", name, after);
    if let Some(before) = before {
        coverage(report, file, "from", name, before);
    }
    let old_groups = before.map(grouped).unwrap_or_default();
    let new_groups = grouped(after);
    for unit in &after.units {
        let Some(key) = unit.key.as_deref() else {
            continue;
        };
        if new_groups[key].len() != 1 {
            continue;
        }
        let Some(before) = before else {
            report.issue(
                file,
                "from",
                Some(name),
                Some(unit),
                "baseline analysis is unavailable; scope is not assumed new",
            );
            continue;
        };
        let previous = match old_groups.get(key) {
            Some(units) if units.len() == 1 => Some(units[0].value),
            Some(_) => {
                report.issue(
                    file,
                    "from",
                    Some(name),
                    Some(unit),
                    "baseline scope identity is ambiguous",
                );
                continue;
            }
            None => {
                if before.incomplete || before.units.iter().any(|unit| unit.key.is_none()) {
                    report.issue(
                        file,
                        "from",
                        Some(name),
                        Some(unit),
                        "incomplete baseline cannot establish whether this scope is new",
                    );
                    continue;
                }
                None
            }
        };
        evaluate(report, file, name, unit, previous, limit);
    }
}

fn metric_number(value: f64) -> Value {
    if value >= 0.0 && value < u64::MAX as f64 && value.fract() == 0.0 {
        return json!(value as u64);
    }
    json!(value)
}

fn evaluate(
    report: &mut Report,
    file: &str,
    metric: &str,
    unit: &Unit,
    previous: Option<f64>,
    limit: f64,
) {
    if !unit.value.is_finite() || previous.is_some_and(|value| !value.is_finite()) {
        report.issue(
            file,
            "to",
            Some(metric),
            Some(unit),
            "metric has no finite value",
        );
        return;
    }
    report.checked_scopes += 1;
    let actual = unit.value.round();
    let previous = previous.map(f64::round);
    let baseline = previous.unwrap_or(limit);
    let expected = if metric == "CC" {
        baseline.max(limit)
    } else {
        baseline.min(limit)
    };
    let failed = if metric == "CC" {
        actual > expected
    } else {
        actual < expected
    };
    if !failed {
        return;
    }
    report.violations.push(json!({
        "file": file, "line": unit.line, "scope": unit.name,
        "classification": if previous.is_some() { "existing" } else { "new" },
        "metric": metric, "expected": {"operator": if metric == "CC" { "<=" } else { ">=" }, "value": metric_number(expected)},
        "actual": metric_number(actual), "previous": previous.map(metric_number),
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gate::Options;

    fn report() -> Report {
        Report::new(&Options {
            from: "a".into(),
            to: Some("b".into()),
            cc_limit: None,
            mi_limit: 20.0,
            pretty: false,
            debug: false,
        })
    }

    #[test]
    fn policy_allows_healthy_changes_and_prevents_worsening_existing_debt() {
        for (metric, previous, actual, limit, fails) in [
            ("CC", Some(5.0), 6.0, 15.0, false),
            ("CC", Some(5.0), 15.0, 15.0, false),
            ("CC", Some(5.0), 16.0, 15.0, true),
            ("CC", Some(30.0), 30.0, 15.0, false),
            ("CC", Some(30.0), 25.0, 15.0, false),
            ("CC", Some(19.0), 23.0, 15.0, true),
            ("CC", Some(19.0), 23.0, 25.0, false),
            ("CC", None, 15.0, 15.0, false),
            ("CC", None, 16.0, 15.5, true),
            ("MI", Some(70.0), 56.0, 20.0, false),
            ("MI", Some(70.0), 19.49, 20.0, true),
            ("MI", Some(70.0), 19.5, 20.0, false),
            ("MI", Some(19.6), 20.4, 20.0, false),
            ("MI", Some(12.1), 11.9, 20.0, false),
            ("MI", Some(12.5), 12.49, 20.0, true),
            ("MI", Some(12.02), 6.78, 20.0, true),
            ("MI", Some(5.0), 5.0, 20.0, false),
            ("MI", Some(5.0), 6.0, 20.0, false),
            ("MI", None, 20.0, 20.0, false),
            ("MI", None, 20.0, 20.5, true),
            ("MI", Some(70.0), 20.4, 20.5, true),
            ("MI", Some(70.0), 20.5, 20.5, false),
        ] {
            let mut report = report();
            let unit = Unit {
                key: Some("f".into()),
                name: "f".into(),
                line: 1,
                value: actual,
            };
            evaluate(&mut report, "f.c", metric, &unit, previous, limit);
            assert_eq!(
                !report.violations.is_empty(),
                fails,
                "{metric} {previous:?} -> {actual}"
            );
            if fails {
                let violation = &report.violations[0];
                assert_eq!(violation["actual"].as_u64(), Some(actual.round() as u64));
                assert_eq!(
                    violation["previous"].as_u64(),
                    previous.map(|value| value.round() as u64)
                );
                let baseline = previous.map(f64::round).unwrap_or(limit);
                let expected = if metric == "CC" {
                    baseline.max(limit)
                } else {
                    baseline.min(limit)
                };
                assert_eq!(violation["expected"]["value"].as_f64(), Some(expected));
            }
        }
    }

    #[test]
    fn ambiguous_and_missing_baselines_are_never_new_code() {
        let unit = Unit {
            key: Some("f".into()),
            name: "f".into(),
            line: 1,
            value: 99.0,
        };
        let after = Metric {
            units: vec![unit.clone()],
            ..Default::default()
        };
        let before = Metric {
            units: vec![unit.clone(), unit],
            ..Default::default()
        };
        let mut report = report();
        compare(&mut report, "f.c", "CC", Some(&before), &after, 15.0);
        compare(&mut report, "f.c", "CC", None, &after, 15.0);
        assert!(report.violations.is_empty());
        assert!(!report.complete);
    }
}
