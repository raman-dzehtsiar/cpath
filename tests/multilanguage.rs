// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT
//! Integration contracts, independent of the much larger differential corpora.
use cpath::complexity::{CognitiveComplexity, ComplexityAnalyzer};
use cpath::language::{BundledParser, SourceParser};
use cpath::maintainability::{ApproximateMaintainability, MaintainabilityAnalyzer};
use cpath::selection::language_for_path;
use std::path::Path;

fn simple_branch_cases() -> [(&'static str, &'static str); 8] {
    [
        ("example.c", "int f(int x) { if (x) return 1; return 0; }"),
        ("example.cpp", "int f(int x) { if (x) return 1; return 0; }"),
        (
            "example.rs",
            "fn f(x: bool) -> i32 { if x { return 1; } 0 }",
        ),
        (
            "Example.java",
            "class Example { int f(boolean x) { if (x) return 1; return 0; } }",
        ),
        (
            "example.py",
            "def f(x):\n    if x:\n        return 1\n    return 0\n",
        ),
        ("example.js", "function f(x) { if (x) return 1; return 0; }"),
        (
            "example.ts",
            "function f(x: boolean): number { if (x) return 1; return 0; }",
        ),
        (
            "example.go",
            "package example\nfunc f(x bool) int { if x { return 1 }; return 0 }",
        ),
    ]
}

#[test]
fn requested_languages_have_separate_profiles_and_one_simple_branch() {
    let mut parser = BundledParser::new().unwrap();
    for (path, source) in simple_branch_cases() {
        assert!(language_for_path(Path::new(path)).is_some(), "{path}");
        let analysis = parser.parse_analysis(path.into(), source).unwrap();
        assert!(
            analysis.file.cognitive.is_none(),
            "C# graph leaked into {path}"
        );
        assert!(analysis.file.profiled_cognitive.is_some(), "{path}");
        let complexity = CognitiveComplexity.analyze(&analysis.file);
        assert_eq!(complexity.max_method, 1, "{path}");
        assert!(
            complexity.methods.iter().all(|unit| unit.span.is_some()),
            "{path}"
        );
        let facts = analysis.maintainability.unwrap();
        assert!(facts.profile.is_some(), "{path}");
        let metrics = ApproximateMaintainability.analyze(&facts);
        assert_ne!(
            metrics.profile,
            Some(cpath::maintainability::FORMULA),
            "{path}"
        );
        assert!(!metrics.units.is_empty(), "{path}");
        assert!(
            metrics.units.iter().all(|unit| unit.index.is_finite()),
            "{path}"
        );
    }
}

#[test]
fn mixed_language_cli_is_parallel_deterministic_and_keeps_parse_failures_unscored() {
    let directory = tempfile::tempdir().unwrap();
    let cases: std::collections::BTreeMap<_, _> = simple_branch_cases()
        .into_iter()
        .chain([
            (
                "Example.cs",
                "class Example { int F(bool x) { if (x) return 1; return 0; } }",
            ),
            (
                "example.tsx",
                "function f(x: boolean) { if (x) return <div />; return null; }",
            ),
        ])
        .collect();
    for (name, source) in &cases {
        std::fs::write(directory.path().join(name), source).unwrap();
    }
    let run = |threads: &str| {
        std::process::Command::new(env!("CARGO_BIN_EXE_cpath"))
            .arg(directory.path())
            .args(["--threads", threads])
            .output()
            .unwrap()
    };
    let serial = run("1");
    let parallel = run("4");
    assert!(serial.status.success(), "{:?}", serial);
    assert!(parallel.status.success(), "{:?}", parallel);
    assert!(serial.stderr.is_empty());
    assert!(parallel.stderr.is_empty());
    assert_eq!(serial.stdout, parallel.stdout);
    let document: serde_json::Value = serde_json::from_slice(&serial.stdout).unwrap();
    assert_eq!(document["exit_code"], 0);
    assert_eq!(document["skipped_files"], serde_json::json!([]));
    let names: Vec<_> = document["scanned_files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| {
            assert_eq!(file["CC"], 1);
            assert!(file["MI"].is_number(), "{file}");
            let path = file["path"].as_str().unwrap();
            assert!(directory.path().join(path).is_file());
            path
        })
        .collect();
    assert_eq!(names, cases.keys().copied().collect::<Vec<_>>());

    std::fs::write(directory.path().join("broken.py"), "def broken(:\n").unwrap();
    let partial = run("4");
    assert_eq!(partial.status.code(), Some(3));
    assert!(partial.stderr.is_empty());
    let partial: serde_json::Value = serde_json::from_slice(&partial.stdout).unwrap();
    assert_eq!(partial["exit_code"], 3);
    assert_eq!(partial["scanned_files"], document["scanned_files"]);
    assert_eq!(partial["skipped_files"].as_array().unwrap().len(), 1);
    assert_eq!(partial["skipped_files"][0]["path"], "broken.py");
    assert!(partial["skipped_files"][0]["reason"]
        .as_str()
        .unwrap()
        .contains("Python syntax"));
}

#[test]
fn mixed_language_worker_reuse_does_not_change_csharp_metrics() {
    let mut parser = BundledParser::new().unwrap();
    let source = "class C { int F(bool x) { if (x) return 1; return 0; } }";
    let before = parser.parse_analysis("C.cs".into(), source).unwrap();
    parser
        .parse_analysis("x.py".into(), "def f(x):\n    return x + 1\n")
        .unwrap();
    parser
        .parse_analysis("x.rs".into(), "fn f(x: i32) -> i32 { x + 1 }")
        .unwrap();
    parser
        .parse_analysis("x.ts".into(), "const f = (x: number) => x + 1;")
        .unwrap();
    let after = parser.parse_analysis("C.cs".into(), source).unwrap();
    assert!(after.file.profiled_cognitive.is_none());
    assert!(after.maintainability.as_ref().unwrap().profile.is_none());
    assert_eq!(before.maintainability, after.maintainability);
    let first = CognitiveComplexity.analyze(&before.file);
    let second = CognitiveComplexity.analyze(&after.file);
    assert_eq!(
        (first.max_method, first.total),
        (second.max_method, second.total)
    );
    assert_eq!(
        ApproximateMaintainability.analyze(before.maintainability.as_ref().unwrap()),
        ApproximateMaintainability.analyze(after.maintainability.as_ref().unwrap())
    );
}

#[test]
fn unsupported_go_global_scopes_do_not_receive_optimistic_file_rankings() {
    let directory = tempfile::tempdir().unwrap();
    for (name, source) in [
        ("scored.go", "package p\nfunc f(x bool) { if x {} }"),
        (
            "literal.go",
            "package p\nvar f = func() int { return 1 }\nfunc g(x bool) { if x {} }",
        ),
        ("initializer.go", "package p\nvar a, b bool\nvar c = a && b"),
    ] {
        std::fs::write(directory.path().join(name), source).unwrap();
    }
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_cpath"))
        .arg(directory.path())
        .args(["--threads", "3"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    assert!(output.stderr.is_empty());
    let document: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(document["scanned_files"].as_array().unwrap().len(), 1);
    assert_eq!(document["scanned_files"][0]["path"], "scored.go");
    assert_eq!(document["scanned_files"][0]["CC"], 1);
    let skipped = document["skipped_files"].as_array().unwrap();
    assert_eq!(skipped.len(), 2);
    assert_eq!(skipped[0]["path"], "initializer.go");
    assert_eq!(skipped[1]["path"], "literal.go");
    assert!(skipped
        .iter()
        .all(|file| file["reason"].as_str().unwrap().contains("scopes unscored")));
}
