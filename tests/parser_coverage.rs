// SPDX-License-Identifier: MIT

//! Small source-level coverage experiments, not a repository coverage benchmark.

use std::collections::BTreeMap;
use std::path::PathBuf;

use cpath::complexity::{CognitiveComplexity, ComplexityAnalyzer};
use cpath::language::{BundledParser, SourceParser};
use cpath::pipeline::{scan, FailureKind, ScanOptions};

#[test]
fn complete_source_branches_score_while_unsupported_shapes_remain_unscored() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/parser_coverage");
    let report = scan(
        &root,
        &ScanOptions {
            workers: 1,
            ..ScanOptions::default()
        },
    )
    .unwrap();
    let scores: BTreeMap<_, _> = report
        .ranked
        .iter()
        .map(|rank| {
            (
                report.files[rank.file_index]
                    .path
                    .to_string_lossy()
                    .into_owned(),
                rank.score.maximum,
            )
        })
        .collect();
    // Hand counts: direct conditional else-if alternatives add 1 each to the
    // outer if; separate #if branch decisions add 1 each; annotation metadata
    // adds nothing; the allocation bound contains exactly one ternary.
    assert_eq!(
        scores,
        BTreeMap::from([
            ("conditional_else.cs".to_owned(), 3),
            ("complete_branches.cs".to_owned(), 2),
            ("explicit_annotation.cpp".to_owned(), 1),
            ("pointer_array.cpp".to_owned(), 1),
            ("macro_hidden.c".to_owned(), 0),
            ("macro_expanded.c".to_owned(), 3),
        ])
    );
    assert_eq!(report.unscored_files, 7);
    assert_eq!(report.failure_counts.get(&FailureKind::Parse), Some(&7));
    assert_eq!(report.diagnostics.len(), 7);
    assert!(report
        .diagnostics
        .iter()
        .all(|diagnostic| { !scores.contains_key(diagnostic.path.to_string_lossy().as_ref()) }));
}

#[test]
fn macro_expansion_is_outside_scored_source_syntax_coverage() {
    let mut parser = BundledParser::new().unwrap();
    let hidden = parser
        .parse(
            "hidden.c".into(),
            include_str!("fixtures/parser_coverage/macro_hidden.c"),
        )
        .unwrap();
    let expanded = parser
        .parse(
            "expanded.c".into(),
            include_str!("fixtures/parser_coverage/macro_expanded.c"),
        )
        .unwrap();
    assert_eq!(hidden.methods.len(), 1);
    assert_eq!(expanded.methods.len(), 1);
    assert_eq!(hidden.methods[0].name, "f");
    assert_eq!(hidden.methods[0].start_line, 2);
    assert!(hidden.methods[0].events.is_empty());
    // The ordinary macro call contributes no visible control-flow events. The
    // spelled-out expansion contains a do-loop (1) and its nested if (2).
    assert_eq!(CognitiveComplexity.analyze(&hidden).max_method, 0);
    assert_eq!(CognitiveComplexity.analyze(&expanded).max_method, 3);
}
