// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT
//! Self-contained native metric and parser regression cases.

use cpath::language::{NativeParser, SourceParser};
use cpath::maintainability::{ApproximateMaintainability, MaintainabilityAnalyzer};
use cpath::metrics::MaintainabilityInput;
use serde_json::Value;

fn number(value: &Value) -> f64 {
    value.as_f64().expect("expected numeric value")
}

fn close(actual: f64, expected: &Value, context: &str) {
    let expected = number(expected);
    assert!(
        (actual - expected).abs() <= 1e-10,
        "{context}: expected {expected}, measured {actual}"
    );
}

#[test]
fn native_deep_expressions_fit_an_ordinary_worker_stack() {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let chain = std::iter::repeat_n("value", 20_000)
                .collect::<Vec<_>>()
                .join(" && ");
            let deep = format!("{}value{}", "(".repeat(20_000), ")".repeat(20_000));
            for language in ["c", "cpp", "rust"] {
                let mut parser = match language {
                    "c" => NativeParser::c(),
                    "cpp" => NativeParser::cpp(),
                    "rust" => NativeParser::rust(),
                    _ => unreachable!(),
                }
                .unwrap();
                for (expression, expected) in [(&chain, 1), (&deep, 0)] {
                    let source = if language == "rust" {
                        format!("fn check(value: bool) -> bool {{ {expression} }}\n")
                    } else {
                        format!("int check(int value) {{ return {expression}; }}\n")
                    };
                    let analysis = parser
                        .parse_analysis(format!("deep.{language}").into(), &source)
                        .unwrap();
                    let cognitive = cpath::metrics::analyze_cognitive(
                        analysis.file.profiled_cognitive.as_ref().unwrap(),
                    );
                    assert_eq!(cognitive.total, expected, "{language}");
                    assert_eq!(cognitive.methods.len(), 1, "{language}");
                    assert_eq!(cognitive.methods[0].name, "check", "{language}");
                }
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn native_wide_bodies_and_nested_scopes_preserve_counts() {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            for language in ["c", "cpp", "rust"] {
                let mut parser = match language {
                    "c" => NativeParser::c(),
                    "cpp" => NativeParser::cpp(),
                    "rust" => NativeParser::rust(),
                    _ => unreachable!(),
                }
                .unwrap();
                let body = "value = value;\n".repeat(20_000);
                let source = if language == "rust" {
                    format!("fn wide(mut value: i32) {{ {body} }}\n")
                } else {
                    format!("void wide(int value) {{ {body} }}\n")
                };
                let analysis = parser
                    .parse_analysis(format!("wide.{language}").into(), &source)
                    .unwrap();
                let cognitive = cpath::metrics::analyze_cognitive(
                    analysis.file.profiled_cognitive.as_ref().unwrap(),
                );
                assert_eq!(cognitive.total, 0, "{language}");
                assert_eq!(cognitive.methods.len(), 1, "{language}");
            }
            let depth = 1_000;
            let mut source = String::new();
            for index in 0..depth {
                source.push_str(&format!("fn function_{index}() {{\n"));
            }
            source.push_str(&"}\n".repeat(depth));
            let analysis = NativeParser::rust()
                .unwrap()
                .parse_analysis("nested.rs".into(), &source)
                .unwrap();
            let facts = analysis.maintainability.unwrap();
            let MaintainabilityInput::Native(facts) = facts.profile.unwrap() else {
                unreachable!()
            };
            assert_eq!(facts.units.len(), depth + 1);
            assert_eq!(facts.units[0].source.counts.distinct_operands, depth as u64);
            assert_eq!(
                facts.units.last().unwrap().source.counts.distinct_operands,
                1
            );
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn native_cast_helpers_fit_an_ordinary_worker_stack() {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let expressions = [
                format!("{}(BYTE)+3", "~".repeat(20_000)),
                format!("({}BYTE)+3", "/**/".repeat(20_000)),
            ];
            let mut parser = NativeParser::cpp().unwrap();
            for expression in expressions {
                let source =
                    format!("typedef unsigned char BYTE; int f() {{ return {expression}; }}\n");
                parser.parse_analysis("cast.cpp".into(), &source).unwrap();
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn native_metrics_match_expected_values() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/native-metric-probes.json")).unwrap();
    let compatibility: Value =
        serde_json::from_str(include_str!("fixtures/native-cpp-compat.json")).unwrap();
    let token_cases: Value =
        serde_json::from_str(include_str!("fixtures/native-mi-tokens.json")).unwrap();
    let nested_token_cases: Value =
        serde_json::from_str(include_str!("fixtures/native-mi-token-nesting.json")).unwrap();
    let role_cases: Value =
        serde_json::from_str(include_str!("fixtures/native-mi-role-context.json")).unwrap();
    let requires_role_cases: Value =
        serde_json::from_str(include_str!("fixtures/native-mi-requires-role.json")).unwrap();
    let reserved_role_cases: Value =
        serde_json::from_str(include_str!("fixtures/native-mi-reserved-roles.json")).unwrap();
    let scope_role_cases: Value =
        serde_json::from_str(include_str!("fixtures/native-mi-scope-roles.json")).unwrap();
    let declaration_role_cases: Value =
        serde_json::from_str(include_str!("fixtures/native-mi-declaration-roles.json")).unwrap();
    for case in fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .chain(compatibility["cases"].as_array().unwrap())
        .chain(token_cases["cases"].as_array().unwrap())
        .chain(nested_token_cases["cases"].as_array().unwrap())
        .chain(role_cases["cases"].as_array().unwrap())
        .chain(requires_role_cases["cases"].as_array().unwrap())
        .chain(reserved_role_cases["cases"].as_array().unwrap())
        .chain(scope_role_cases["cases"].as_array().unwrap())
        .chain(declaration_role_cases["cases"].as_array().unwrap())
    {
        let language = case["language"].as_str().unwrap();
        let name = case["name"].as_str().unwrap();
        let context = format!("{language}/{name}");
        let source = case["source"].as_str().unwrap();
        let mut parser = match language {
            "c" => NativeParser::c(),
            "cpp" => NativeParser::cpp(),
            "rust" => NativeParser::rust(),
            _ => unreachable!(),
        }
        .unwrap();
        let analysis = parser.parse_analysis(format!("probe.{language}").into(), source);
        if case["status"] != "ok" {
            assert!(analysis.is_err(), "{context}: invalid input became scored");
            continue;
        }
        let analysis = analysis.unwrap_or_else(|error| panic!("{context}: {error}"));
        let cognitive =
            cpath::metrics::analyze_cognitive(analysis.file.profiled_cognitive.as_ref().unwrap());
        if let Some(expected) = case["cognitive_has_unscored_scopes"].as_bool() {
            assert_eq!(cognitive.has_unscored_scopes, expected, "{context}");
        }
        if case["cognitive_status"] == "unscored" {
            assert!(case["cognitive_units"].is_null(), "{context}");
            assert!(case["file_cognitive"].is_null(), "{context}");
        } else {
            close(cognitive.total as f64, &case["file_cognitive"], &context);
            let expected_cognitive = case["cognitive_units"].as_array().unwrap();
            assert_eq!(
                cognitive.methods.len(),
                expected_cognitive.len(),
                "{context}: cognitive scope coverage changed"
            );
            for (actual, expected) in cognitive.methods.iter().zip(expected_cognitive) {
                assert_eq!(actual.name, expected["name"].as_str().unwrap(), "{context}");
                close(actual.score as f64, &expected["score"], &context);
                if let Some(start) = expected["start_byte"].as_u64() {
                    let span = actual.span.expect("exact native callable span");
                    assert_eq!(span.start, start as usize, "{context}");
                    assert_eq!(
                        span.end,
                        expected["end_byte"].as_u64().unwrap() as usize,
                        "{context}"
                    );
                }
            }
        }
        if case["mi_status"] == "oracle_unvalidated" {
            assert!(case["mi_units"].is_null(), "{context}");
            continue;
        }
        let facts = analysis.maintainability.unwrap();
        let MaintainabilityInput::Native(raw) = facts.profile.as_ref().unwrap() else {
            panic!("{context}: wrong MI profile")
        };
        let metrics = ApproximateMaintainability.analyze(&facts);
        let all_units = case["mi_units"].as_array().unwrap();
        let expected_units: Vec<_> = all_units
            .iter()
            .filter(|unit| !unit["mi"].is_null())
            .collect();
        assert_eq!(
            metrics.units.len(),
            expected_units.len(),
            "{context}: MI scope coverage changed"
        );
        assert_eq!(
            metrics.unscored_units,
            all_units.len() - expected_units.len(),
            "{context}"
        );
        for (actual, expected) in metrics.units.iter().zip(expected_units) {
            let raw = raw
                .units
                .iter()
                .find(|unit| {
                    unit.source.start_byte == actual.facts.start_byte
                        && unit.source.end_byte == actual.facts.end_byte
                        && unit.source.kind == actual.facts.kind
                })
                .unwrap();
            let counts = actual.facts.counts;
            assert_eq!(
                actual.facts.name,
                expected["name"].as_str().unwrap(),
                "{context}"
            );
            close(actual.index, &expected["mi"], &context);
            close(
                actual.cyclomatic_complexity as f64,
                &expected["cyclomatic"],
                &context,
            );
            close(raw.source_lines as f64, &expected["source_lines"], &context);
            for (count, field) in [
                (counts.operators, "N1"),
                (counts.operands, "N2"),
                (counts.distinct_operators, "n1"),
                (counts.distinct_operands, "n2"),
            ] {
                close(count as f64, &expected["halstead"][field], &context);
            }
            if expected["halstead"]["volume"].is_null() {
                assert!(
                    actual.halstead_volume.is_nan(),
                    "{context}: preserve undefined raw volume"
                );
            } else {
                close(
                    actual.halstead_volume,
                    &expected["halstead"]["volume"],
                    &context,
                );
            }
        }
    }
}

#[test]
fn native_token_syntax_is_accepted() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/native-mi-tokens.json")).unwrap();
    for case in fixture["parser_cases"].as_array().unwrap() {
        NativeParser::cpp()
            .unwrap()
            .parse_analysis("tokens.cpp".into(), case["source"].as_str().unwrap())
            .unwrap();
    }
}

#[test]
fn native_unsupported_reserved_syntax_is_rejected() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/native-mi-reserved-roles.json")).unwrap();
    for case in fixture["rejected_cases"].as_array().unwrap() {
        assert!(NativeParser::c()
            .unwrap()
            .parse_analysis("reserved.c".into(), case["source"].as_str().unwrap())
            .is_err());
    }
}

#[test]
fn native_rust_commented_else_has_expected_file_totals() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/native-rust-commented-else.json")).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 4);
    let mut parser = NativeParser::rust().unwrap();
    for case in cases {
        let source = case["source"].as_str().unwrap();
        let name = case["name"].as_str().unwrap();
        let analysis = parser
            .parse_analysis("commented-else.rs".into(), source)
            .unwrap();
        let cognitive =
            cpath::metrics::analyze_cognitive(analysis.file.profiled_cognitive.as_ref().unwrap());
        assert!(!cognitive.has_unscored_scopes, "{name}");
        close(
            cognitive.total as f64,
            &case["expected_file_cognitive"],
            name,
        );
    }
}

#[test]
fn native_aligned_structure_is_not_a_callable() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/native-mi-scope-roles.json")).unwrap();
    for case in fixture["parser_cases"].as_array().unwrap() {
        let analysis = NativeParser::cpp()
            .unwrap()
            .parse_analysis("aligned.cpp".into(), case["source"].as_str().unwrap())
            .unwrap();
        let cognitive =
            cpath::metrics::analyze_cognitive(analysis.file.profiled_cognitive.as_ref().unwrap());
        assert!(
            cognitive.methods.is_empty(),
            "an aligned structure is not a callable"
        );
    }
}

#[test]
fn native_decimal_include_fragments_have_an_explicit_gap() {
    for mut parser in [NativeParser::c().unwrap(), NativeParser::cpp().unwrap()] {
        for source in [
            "1,2,",
            " /* table */ - /* sign */ .5, +2E-3, 1., // end\r\n",
            "0, 1e9999, /* large exponent */",
        ] {
            let error = parser
                .parse_analysis("table.h".into(), source)
                .err()
                .unwrap();
            assert!(error.contains("standalone decimal initializer fragment"));
        }
        for source in [
            "1,",
            "1,2",
            "1,2;",
            "1e+,2,",
            "1-2,3,",
            "--1,2,",
            "0x1,2,",
            "1f,2,",
            "NaN,2,",
            "\"1\",2,",
            "1,2,/* unterminated",
        ] {
            if let Err(error) = parser.parse_analysis("other.h".into(), source) {
                assert!(
                    !error.contains("standalone decimal initializer fragment"),
                    "{source}"
                );
            }
        }
        let fragment = "-12.345, 6.789e-2,\n".repeat(30_000);
        let error = parser
            .parse_analysis("large.h".into(), &fragment)
            .err()
            .unwrap();
        assert!(error.contains("standalone decimal initializer fragment"));
        let declaration = format!("const double values[] = {{\n{fragment}}};\n");
        assert!(parser
            .parse_analysis("array.h".into(), &declaration)
            .is_ok());
    }
}

#[test]
fn native_declaration_fragments_are_accepted() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/native-mi-declaration-roles.json")).unwrap();
    let gaps = fixture["parser_cases"].as_array().unwrap();
    assert_eq!(gaps.len(), 2);
    for case in gaps {
        let source = case["source"].as_str().unwrap();
        let mut parser = match case["language"].as_str().unwrap() {
            "c" => NativeParser::c().unwrap(),
            "cpp" => NativeParser::cpp().unwrap(),
            _ => unreachable!(),
        };
        assert!(parser.parse_analysis("values.h".into(), source).is_ok());
    }
}
