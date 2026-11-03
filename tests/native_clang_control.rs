// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT
//! Expected scores for direct, braced and nested control-flow bodies.

use cpath::language::{NativeParser, SourceParser};
use serde_json::Value;

#[test]
fn control_bodies_have_expected_scores() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/native-clang-control-probes.json")).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 2);
    let mut comparisons = 0;
    for case in cases {
        let language = case["language"].as_str().unwrap();
        let source = case["source"].as_str().unwrap();
        let mut parser = match language {
            "c" => NativeParser::c(),
            "cpp" => NativeParser::cpp(),
            _ => panic!("unexpected fixture language"),
        }
        .unwrap();
        let analysis = parser
            .parse_analysis(format!("control.{language}").into(), source)
            .unwrap();
        let metrics =
            cpath::metrics::analyze_cognitive(analysis.file.profiled_cognitive.as_ref().unwrap());
        assert!(!metrics.has_unscored_scopes, "{language}");
        let expected = case["units"].as_array().unwrap();
        assert_eq!(metrics.methods.len(), expected.len(), "{language}");
        for unit in expected {
            let start = unit["start_byte"].as_u64().unwrap() as usize;
            let end = unit["end_byte"].as_u64().unwrap() as usize;
            let name = unit["name"].as_str().unwrap();
            let measured = metrics
                .methods
                .iter()
                .find(|method| {
                    method.name == name
                        && method
                            .span
                            .is_some_and(|span| span.start == start && span.end == end)
                })
                .unwrap_or_else(|| panic!("missing {language} scope {name} at {start}..{end}"));
            assert_eq!(
                measured.score as u64,
                unit["score"].as_u64().unwrap(),
                "{language}: {name} at {start}..{end}"
            );
            comparisons += 1;
        }
    }
    assert_eq!(comparisons, 34);
}
