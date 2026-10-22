// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

//! Original source examples with independently hand-counted expectations.
//! Fixtures are in tests/fixtures/cognitive; expected scores and spans are below.
use cpath::complexity::{CognitiveComplexity, ComplexityAnalyzer};
use cpath::language::{CSharpParser, SourceParser};

fn verify(path: &str, source: &str, expected: &[(&str, u32, u32, u32)], maximum: u32, total: u32) {
    let facts = CSharpParser::new()
        .expect("bundled grammar must initialize")
        .parse(path.into(), source)
        .unwrap_or_else(|error| panic!("corpus fixture {path}: {error}"));
    let metrics = CognitiveComplexity.analyze(&facts);
    assert_eq!(
        metrics.methods.len(),
        expected.len(),
        "{path}: missing or extra callable: {:?}",
        metrics.methods
    );
    for (actual, (label, start, end, score)) in metrics.methods.iter().zip(expected) {
        assert_eq!(
            (actual.start_line, actual.end_line, actual.score),
            (*start, *end, *score),
            "{path}: hand-audited {label}, extracted name {}",
            actual.name
        );
    }
    assert_eq!(metrics.max_method, maximum, "{path}: maximum");
    assert_eq!(metrics.total, total, "{path}: total");
}

#[test]
fn control_flow_has_hand_counted_structural_and_sequence_costs() {
    verify(
        "control_flow.cs",
        include_str!("fixtures/cognitive/control_flow.cs"),
        &[
            // if 1 + for 2 + inner if 3 + else-if 1 + else 1.
            ("NestedBranches", 5, 16, 8),
            // switch 1 + inner if 2 + goto 1; cases add no points.
            ("SwitchCases", 18, 26, 4),
            // try-if 1 + catch 1 + filter && 1 + catch-if 2 + finally-if 1.
            ("TryCatch", 28, 37, 6),
            // && sequence, || sequence, separate && sequence.
            ("BooleanGroups", 39, 41, 3),
            // foreach 1 + nested while 2 + do 1 + for 1.
            ("LoopKinds", 43, 51, 5),
            // outer conditional 1 + inner conditional 2.
            ("TernaryChain", 53, 53, 3),
        ],
        8,
        29,
    );
}

#[test]
fn every_callable_kind_has_its_own_nesting_scope_and_source_location() {
    verify(
        "callable_scopes.cs",
        include_str!("fixtures/cognitive/callable_scopes.cs"),
        &[
            ("constructor", 6, 9, 1),
            ("getter", 12, 15, 1),
            ("setter", 16, 19, 1),
            ("expression property", 22, 22, 1),
            ("expression indexer", 23, 23, 1),
            // Nested callable decisions must not inflate their enclosing method.
            ("Outer", 25, 39, 1),
            // The local function starts at nesting zero despite the outer if.
            ("Local", 27, 33, 3),
            ("lambda", 34, 34, 1),
            ("anonymous delegate", 35, 35, 1),
            ("addition operator", 41, 42, 1),
            ("conversion operator", 44, 45, 1),
            ("destructor", 47, 49, 1),
        ],
        3,
        14,
    );
}

#[test]
fn modern_syntax_distinguishes_decisions_from_literals_and_unsupported_metrics() {
    verify(
        "modern_syntax.cs",
        include_str!("fixtures/cognitive/modern_syntax.cs"),
        &[
            // switch 1 + conditional inside an arm 2; guards add no points.
            ("Classify", 10, 13, 3),
            // Pattern 'and' is deliberately outside cpath-cognitive-v1.
            ("PatternOnly", 15, 18, 1),
            ("CountAsync", 20, 26, 3),
            ("LiteralOnly", 28, 28, 0),
            ("Interpolated", 30, 30, 1),
            // Null propagation/coalescing do not count as ternary expressions.
            ("NullSafe", 32, 32, 0),
        ],
        3,
        8,
    );
}

#[test]
fn top_level_code_and_its_local_function_are_counted_separately() {
    verify(
        "top_level.cs",
        include_str!("fixtures/cognitive/top_level.cs"),
        &[("top-level statements", 4, 13, 3), ("Measure", 10, 13, 1)],
        3,
        4,
    );
}
