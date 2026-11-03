// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT
//! Self-contained metric and syntax regression tests.

use cpath::language::{SourceParser, WebGoParser};
use cpath::maintainability::{ApproximateMaintainability, MaintainabilityAnalyzer, UnitKind};
use serde_json::Value;

fn close(actual: f64, expected: &Value, context: &str) {
    let expected = expected.as_f64().expect("finite expected value");
    assert!(
        (actual - expected).abs() <= 1e-10,
        "{context}: expected {expected}, actual {actual}"
    );
}

fn spaces<'a>(space: &'a Value, result: &mut Vec<&'a Value>) {
    result.push(space);
    for child in space["spaces"].as_array().unwrap() {
        spaces(child, result);
    }
}

fn expected_units(record: &Value, go: bool) -> &[Value] {
    match record.get("units") {
        Some(Value::Array(units)) => units,
        None if go => &[],
        _ => panic!("Malformed expected unit list"),
    }
}

fn expected_span(unit: &Value) -> (u64, u64) {
    let end = unit["end_byte"].as_u64().unwrap();
    if let Some(function_end) = unit["function_end_byte"].as_u64() {
        if function_end != end {
            return (unit["function_start_byte"].as_u64().unwrap(), function_end);
        }
    }
    (unit["start_byte"].as_u64().unwrap(), end)
}

fn is_module(unit: &cpath::model::MethodComplexity, source: &str) -> bool {
    unit.name == "<module>"
        && unit
            .span
            .is_some_and(|span| span.start == 0 && span.end == source.len())
}

fn check_module(actual: &cpath::model::ComplexityMetrics, case: &Value, expected: Option<&Value>) {
    let context = case["id"].as_str().unwrap();
    let source = case["source"].as_str().unwrap();
    let modules: Vec<_> = actual
        .methods
        .iter()
        .filter(|unit| is_module(unit, source))
        .collect();
    let Some(expected) = expected else {
        assert_eq!(case["language"], "go", "{context}: missing module baseline");
        assert!(
            modules.is_empty(),
            "{context}: unsupported Go module must stay unscored"
        );
        return;
    };
    let unit = &expected["module_unit"];
    let score = unit["score"].as_u64().unwrap();
    assert_eq!(
        modules.len(),
        usize::from(score > 0),
        "{context}: module coverage"
    );
    if let Some(actual) = modules.first() {
        close(actual.score as f64, &unit["score"], context);
        let span = actual.span.unwrap();
        assert_eq!(span.start as u64, unit["start_byte"].as_u64().unwrap());
        assert_eq!(span.end as u64, unit["end_byte"].as_u64().unwrap());
        assert_eq!(
            actual.start_line as u64,
            unit["start_line"].as_u64().unwrap()
        );
        assert_eq!(actual.end_line as u64, unit["end_line"].as_u64().unwrap());
    }
}

#[test]
fn go_rejects_an_unterminated_string() {
    let mut parser = WebGoParser::go().unwrap();
    assert!(parser
        .parse_analysis(
            "broken.go".into(),
            "package sample\nconst value = \"unfinished\n"
        )
        .is_err());
}

#[test]
fn go_global_scopes_remain_explicitly_unscored() {
    let cases = [
        ("package p\nvar f=func()int{return 1}\n", 1, 0, true, 0),
        ("package p\nvar flag=true&&false\n", 0, 1, true, 0),
        (
            "package p\nvar f=func()func()int{return func()int{if true{return 1};return 0}}()\n",
            2,
            1,
            true,
            0,
        ),
        (
            "package p\nfunc f()int{g:=func()int{if true{return 1};return 0};return g()}\n",
            0,
            0,
            false,
            1,
        ),
    ];
    let mut parser = WebGoParser::go().unwrap();
    for (source, literals, events, partial, callables) in cases {
        let analysis = parser.parse_analysis("scope.go".into(), source).unwrap();
        let input = analysis.file.profiled_cognitive.as_ref().unwrap();
        let cpath::metrics::CognitiveInput::WebGo(facts) = input else {
            panic!("Incorrect Go cognitive profile");
        };
        assert_eq!(facts.unscored_global_function_literals, literals);
        assert_eq!(facts.global_events.len(), events);
        let metrics = cpath::metrics::analyze_cognitive(input);
        assert_eq!(metrics.has_unscored_scopes, partial);
        assert_eq!(metrics.methods.len(), callables);
        if callables == 0 {
            assert_eq!(metrics.total, 0);
            assert_eq!(metrics.max_method, 0);
        }
    }
}

#[test]
fn web_go_metrics_match_expected_values() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/web-go-metric-probes.json")).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 124);
    let module_fixture: Value =
        serde_json::from_str(include_str!("fixtures/web-go-module-probes.json")).unwrap();
    let module_cases = module_fixture["cases"].as_array().unwrap();
    assert_eq!(module_cases.len(), 73);
    check_metric_cases(cases, module_cases);
}

#[test]
fn typescript_mi_compatibility_uses_syntax_ownership() {
    let mut parser = WebGoParser::typescript().unwrap();
    for (source, compatibility) in [
        ("const value = left + right;", false),
        ("const text = 'factory<<T>(x:T)=>T>(identity)';", false),
        ("// left << right\nconst value = 1;", false),
        ("const f = (value:T): U => value;", false),
        ("const value = left << right;", false),
        (
            "const f = condition ? (value:T): U => value : fallback;",
            true,
        ),
        ("factory<<T>(value:T)=>T>(identity);", true),
    ] {
        let analysis = parser.parse_analysis("probe.ts".into(), source).unwrap();
        let Some(cpath::metrics::MaintainabilityInput::WebGo(facts)) =
            analysis.maintainability.unwrap().profile
        else {
            panic!("missing MI profile")
        };
        assert_eq!(facts.used_compatibility_grammar, compatibility, "{source}");
        assert_eq!(facts.unscored_roots, 0, "{source}");
    }
}

#[test]
fn corrected_typescript_does_not_fabricate_mi_for_unsupported_legacy_syntax() {
    let source = "const result = factory<<T extends U>(value:T)=>T>(identity);";
    let mut original = tree_sitter::Parser::new();
    original
        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        .unwrap();
    assert!(original
        .parse(source, None)
        .unwrap()
        .root_node()
        .has_error());
    let analysis = WebGoParser::typescript()
        .unwrap()
        .parse_analysis("probe.ts".into(), source)
        .unwrap();
    assert!(analysis.file.profiled_cognitive.is_some());
    let maintainability = analysis.maintainability.unwrap();
    let Some(cpath::metrics::MaintainabilityInput::WebGo(facts)) = &maintainability.profile else {
        panic!("missing MI profile")
    };
    assert!(facts.used_compatibility_grammar);
    assert_eq!(facts.unscored_roots, 1);
    let result = ApproximateMaintainability.analyze(&maintainability);
    assert!(result.units.is_empty());
    assert!(result.minimum.is_none());
    assert_eq!(result.unscored_units, 1);
}

fn check_metric_cases(cases: &[Value], module_cases: &[Value]) {
    for case in cases {
        let context = case["id"].as_str().unwrap();
        let source = case["source"].as_str().unwrap();
        let go = case["language"] == "go";
        let mut parser = match case["language"].as_str().unwrap() {
            "javascript" => WebGoParser::javascript(),
            "typescript" => WebGoParser::typescript(),
            "tsx" => WebGoParser::tsx(),
            "go" => WebGoParser::go(),
            _ => unreachable!(),
        }
        .unwrap();
        let analysis = parser
            .parse_analysis(context.into(), source)
            .unwrap_or_else(|error| panic!("{context}: {error}"));
        let expected_cc = &case["cognitive"];
        if expected_cc["status"] == "ok" {
            let actual = cpath::metrics::analyze_cognitive(
                analysis.file.profiled_cognitive.as_ref().unwrap(),
            );
            let units = expected_units(expected_cc, go);
            check_module(
                &actual,
                case,
                module_cases.iter().find(|entry| entry["id"] == case["id"]),
            );
            let mut callables: Vec<_> = actual
                .methods
                .iter()
                .filter(|unit| !is_module(unit, source))
                .collect();
            callables.sort_by_key(|unit| {
                let span = unit.span.unwrap();
                (span.start, span.end)
            });
            let mut expected_callables: Vec<_> = units.iter().collect();
            expected_callables.sort_by_key(|unit| expected_span(unit));
            assert!(
                expected_callables
                    .windows(2)
                    .all(|pair| expected_span(pair[0]) != expected_span(pair[1])),
                "{context}: ambiguous callable identity"
            );
            assert_eq!(
                callables.len(),
                units.len(),
                "{context}: cognitive unit coverage"
            );
            let total = if go {
                units
                    .iter()
                    .map(|unit| unit["cognitive"].as_u64().unwrap())
                    .sum()
            } else {
                expected_cc["file_score"].as_u64().unwrap()
            };
            assert_eq!(
                u64::from(actual.total),
                total,
                "{context}: file cognitive complexity"
            );
            assert_eq!(
                actual.max_method,
                actual
                    .methods
                    .iter()
                    .map(|unit| unit.score)
                    .max()
                    .unwrap_or(0)
            );
            for (actual, expected) in callables.iter().zip(expected_callables) {
                let span = actual.span.unwrap();
                let expected_span = expected_span(expected);
                assert_eq!(span.start as u64, expected_span.0, "{context}");
                assert_eq!(span.end as u64, expected_span.1, "{context}");
                close(
                    actual.score as f64,
                    &expected[if go { "cognitive" } else { "score" }],
                    context,
                );
            }
        }
        let expected_mi = &case["maintainability"];
        if expected_mi["status"] != "ok" {
            continue;
        }
        let actual = ApproximateMaintainability.analyze(analysis.maintainability.as_ref().unwrap());
        let mut units = Vec::new();
        if go {
            units.extend(expected_units(expected_mi, go));
        } else {
            spaces(&expected_mi["spaces"], &mut units);
        }
        let expected: Vec<_> = units
            .iter()
            .filter(|unit| {
                if go {
                    unit["maintainability"]
                        .as_f64()
                        .is_some_and(|value| (0.0..=100.0).contains(&value))
                } else {
                    unit["metrics"]["mi"]["mi_visual_studio"].is_number()
                }
            })
            .collect();
        assert_eq!(
            actual.units.len(),
            expected.len(),
            "{context}: MI unit coverage"
        );
        if units.is_empty() {
            assert!(
                actual.minimum.is_none(),
                "{context}: no callable has no MI score"
            );
        }
        assert_eq!(
            actual.unscored_units,
            units.len() - expected.len(),
            "{context}: undefined MI units"
        );
        for (actual, expected) in actual.units.iter().zip(expected) {
            if go {
                assert_eq!(
                    actual.facts.start_byte as u64,
                    expected["start_byte"].as_u64().unwrap(),
                    "{context}"
                );
                assert_eq!(
                    actual.facts.end_byte as u64,
                    expected["end_byte"].as_u64().unwrap(),
                    "{context}"
                );
                close(actual.index, &expected["maintainability"], context);
                close(
                    actual.halstead_volume,
                    &expected["halstead_volume"],
                    context,
                );
                close(
                    actual.cyclomatic_complexity as f64,
                    &expected["cyclomatic"],
                    context,
                );
            } else {
                let kind = match expected["kind"].as_str().unwrap() {
                    "unit" => UnitKind::TopLevel,
                    "function" => UnitKind::Callable,
                    "class" | "interface" => UnitKind::Container,
                    _ => unreachable!(),
                };
                assert_eq!(actual.facts.kind, kind, "{context}");
                assert_eq!(
                    actual.facts.start_line as u64,
                    expected["start_line"].as_u64().unwrap(),
                    "{context}"
                );
                let end = actual
                    .facts
                    .end_line
                    .saturating_sub(u32::from(kind == UnitKind::TopLevel));
                assert_eq!(
                    end as u64,
                    expected["end_line"].as_u64().unwrap(),
                    "{context}"
                );
                close(
                    actual.index,
                    &expected["metrics"]["mi"]["mi_visual_studio"],
                    context,
                );
                let halstead = &expected["metrics"]["halstead"];
                let counts = actual.facts.counts;
                for (field, value) in [
                    ("N1", counts.operators),
                    ("N2", counts.operands),
                    ("n1", counts.distinct_operators),
                    ("n2", counts.distinct_operands),
                ] {
                    close(value as f64, &halstead[field], context);
                }
                if halstead["volume"].is_number() {
                    close(actual.halstead_volume, &halstead["volume"], context);
                } else {
                    assert!(!actual.halstead_volume.is_finite(), "{context}");
                }
                close(
                    actual.cyclomatic_complexity as f64,
                    &expected["metrics"]["cyclomatic"]["sum"],
                    context,
                );
            }
        }
    }
}
