// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT
//! A syntax retry must preserve original bytes and restore the primary parser.

use cpath::language::{NativeParser, SourceParser};
use serde_json::Value;

fn accepts(grammar: tree_sitter::Language, source: &str) -> bool {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&grammar).unwrap();
    !parser.parse(source, None).unwrap().root_node().has_error()
}

#[test]
fn cpp_compatibility_preserves_dense_templates_and_parser_reuse() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/native-cpp-compat.json")).unwrap();
    let source = fixture["cases"][0]["source"].as_str().unwrap();
    assert!(!accepts(tree_sitter_cpp::LANGUAGE.into(), source));
    assert!(accepts(tree_sitter_cpp_compat::LANGUAGE.into(), source));

    let primary_only = "struct Owner { int value; }; using Pointer = int Owner::*;\n";
    assert!(accepts(tree_sitter_cpp::LANGUAGE.into(), primary_only));
    assert!(!accepts(
        tree_sitter_cpp_compat::LANGUAGE.into(),
        primary_only
    ));
    let unsupported_combination = format!("{source}using Pointer = int Owner::*;\n");
    assert!(!accepts(
        tree_sitter_cpp::LANGUAGE.into(),
        &unsupported_combination
    ));
    assert!(!accepts(
        tree_sitter_cpp_compat::LANGUAGE.into(),
        &unsupported_combination
    ));

    let mut parser = NativeParser::cpp().unwrap();
    for _ in 0..3 {
        parser.parse("member.cpp".into(), primary_only).unwrap();
        let analysis = parser.parse_analysis("dense.cpp".into(), source).unwrap();
        let cognitive =
            cpath::metrics::analyze_cognitive(analysis.file.profiled_cognitive.as_ref().unwrap());
        let expected = &fixture["cases"][0]["cognitive_units"][0];
        assert_eq!(cognitive.methods.len(), 1);
        assert_eq!(
            cognitive.methods[0].score as u64,
            expected["score"].as_u64().unwrap()
        );
        let span = cognitive.methods[0].span.unwrap();
        assert_eq!(span.start as u64, expected["start_byte"].as_u64().unwrap());
        assert_eq!(span.end as u64, expected["end_byte"].as_u64().unwrap());
        assert!(parser
            .parse("mixed.cpp".into(), &unsupported_combination)
            .is_err());
        assert!(parser.parse("invalid.cpp".into(), "int f( {").is_err());
        parser.parse("restored.cpp".into(), primary_only).unwrap();
    }
}
