// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use cpath::pipeline::{scan, ScanOptions, ScanReport};
use cpath::selection::SkipReason;
use tempfile::TempDir;

struct Fixture(TempDir);

impl Fixture {
    fn new() -> Self {
        Self(
            tempfile::Builder::new()
                .prefix("cpath test ")
                .tempdir()
                .unwrap(),
        )
    }

    fn root(&self) -> &Path {
        self.0.path()
    }

    fn write(&self, path: &str, content: impl AsRef<[u8]>) {
        let path = self.root().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn run(&self, arguments: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_cpath"))
            .arg(self.root())
            .args(arguments)
            .output()
            .unwrap()
    }

    fn scan(&self, workers: usize) -> ScanReport {
        scan(
            self.root(),
            &ScanOptions {
                workers,
                ..Default::default()
            },
        )
        .unwrap()
    }
}

fn failure(report: Result<ScanReport, String>) -> String {
    match report {
        Ok(_) => panic!("scan unexpectedly succeeded"),
        Err(error) => error,
    }
}

fn ranked(report: &ScanReport) -> Vec<(PathBuf, u32)> {
    report
        .ranked
        .iter()
        .map(|file| (file.path.clone(), file.score.maximum))
        .collect()
}

#[test]
fn mixed_language_scans_rank_all_files_deterministically_and_reread_edits() {
    let f = Fixture::new();
    f.write(
        "Native file.c",
        "int f(int n) { while (n) { if (n) --n; } return 0; }",
    );
    f.write("Object.cpp", "int f(int n) { if (n) return 1; return 0; }");
    f.write("Managed.cs", "class C { void M() { if (true) {} } }");
    f.write(
        "api.h",
        "namespace N { inline int f(int n) { return n ? 1 : 0; } }",
    );
    f.write("declarations.hpp", "int prototype(int);");
    // Encoding support also applies before native parser dispatch.
    let utf16: Vec<_> = [0xff, 0xfe]
        .into_iter()
        .chain(
            "int f(void) { return 0; }"
                .encode_utf16()
                .flat_map(u16::to_le_bytes),
        )
        .collect();
    f.write("encoded.c", utf16);
    let expected = vec![
        ("Native file.c".into(), 3),
        ("Managed.cs".into(), 1),
        ("Object.cpp".into(), 1),
        ("api.h".into(), 1),
        ("declarations.hpp".into(), 0),
        ("encoded.c".into(), 0),
    ];
    for workers in [1, 4, 2] {
        let report = f.scan(workers);
        assert_eq!(report.unscored_files, 0);
        assert_eq!(ranked(&report), expected);
        assert_eq!(report.files.len(), 6);
    }
    let output = f.run(&["--top", "1", "--threads", "4"]);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!(
            "3 {}\n",
            f.root()
                .canonicalize()
                .unwrap()
                .join("Native file.c")
                .display()
        )
    );
    f.write("Native file.c", "int f(void) { return 0; }");
    assert_eq!(f.scan(2).ranked[0].path, Path::new("Managed.cs"));
}

#[test]
fn native_failures_preserve_partial_coverage_even_when_top_row_is_valid() {
    let f = Fixture::new();
    f.write("Good.cpp", "int f(int n) { if (n) return 1; return 0; }");
    f.write("Broken.c", "int broken( {");
    f.write("Broken.h", "int broken( {");
    f.write("Binary.hpp", b"a\0b");
    let one = f.run(&["--top", "1", "--threads", "1"]);
    let many = f.run(&["--top", "1", "--threads", "4"]);
    assert_eq!(one.status.code(), Some(3));
    assert_eq!(many.status.code(), Some(3));
    assert_eq!(one.stdout, many.stdout);
    assert_eq!(one.stderr, many.stderr);
    assert_eq!(
        String::from_utf8(one.stdout).unwrap(),
        format!(
            "1 {}\n",
            f.root().canonicalize().unwrap().join("Good.cpp").display()
        )
    );
    let report = f.scan(2);
    assert_eq!(report.files.len(), 1);
    assert_eq!(report.unscored_files, 3);
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|d| d.path.as_path())
            .collect::<Vec<_>>(),
        [
            Path::new("Binary.hpp"),
            Path::new("Broken.c"),
            Path::new("Broken.h")
        ]
    );
    let unscored = f.run(&["--top", "10", "--exclude", "Good.cpp"]);
    assert_eq!(unscored.status.code(), Some(3));
    assert!(unscored.stdout.is_empty());
}

#[test]
fn cli_prints_only_top_scores_absolute_paths_and_preserves_spaces() {
    let f = Fixture::new();
    f.write(
        "Deep file.cs",
        "class Deep { void M(bool a, bool b) { if(a) { if(b) {} } } }",
    );
    f.write("One.cs", "class One { void M(bool a) { if(a) {} } }");
    f.write("A.cs", "class A {}");
    f.write("Z.cs", "class Z {}");
    let output = f.run(&["--top", "2", "--threads", "2"]);
    assert!(output.status.success(), "{:?}", output.stderr);
    assert!(output.stderr.is_empty());
    let root = f.root().canonicalize().unwrap();
    let expected = format!(
        "3 {}\n1 {}\n",
        root.join("Deep file.cs").display(),
        root.join("One.cs").display()
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);

    let report = f.scan(1);
    assert_eq!(
        ranked(&report),
        vec![
            ("Deep file.cs".into(), 3),
            ("One.cs".into(), 1),
            ("A.cs".into(), 0),
            ("Z.cs".into(), 0)
        ]
    );
    let all = f.run(&["--top", "100"]);
    assert!(all.status.success());
    assert_eq!(String::from_utf8(all.stdout).unwrap().lines().count(), 4);
}

#[test]
fn maximum_then_excess_burden_then_path_determine_order() {
    let f = Fixture::new();
    // Each sequential if contributes one point; method names do not affect it.
    let method = |name: &str, points: usize| {
        format!("void {name}() {{ {} }}", "if (true) {} ".repeat(points))
    };
    let many_simple = (0..90)
        .map(|i| method(&format!("Simple{i}"), 2))
        .collect::<String>();
    f.write(
        "A.cs",
        format!("class A {{ {} {many_simple} }}", method("M", 40)),
    );
    let concentrated = (0..5)
        .map(|i| method(&format!("M{i}"), 40))
        .collect::<String>();
    f.write("B.cs", format!("class B {{ {concentrated} }}"));
    f.write("C.cs", format!("class C {{ {concentrated} }}"));
    f.write("D.cs", format!("class D {{ {} }}", method("M", 41)));
    for workers in [1, 4] {
        let report = f.scan(workers);
        assert_eq!(
            ranked(&report),
            vec![
                ("D.cs".into(), 41),
                ("B.cs".into(), 40),
                ("C.cs".into(), 40),
                ("A.cs".into(), 40),
            ]
        );
        assert_eq!(report.ranked[1].score.burden, 125);
        assert_eq!(report.ranked[3].score.burden, 25);
        assert!(report.complexity[0].total > report.complexity[1].total);
    }
    let output = f.run(&["--top", "1"]);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!(
            "41 {}\n",
            f.root().canonicalize().unwrap().join("D.cs").display()
        )
    );
}

#[test]
fn repeated_simple_code_does_not_increase_scores() {
    let f = Fixture::new();
    let body = "void M() { int x = 0; x += 1; x += 2; x += 3; x += 4; x += 5; x += 6; x += 7; x += 8; x += 9; x += 10; }";
    f.write("A.cs", format!("class A {{ {body} }}"));
    f.write("B.cs", format!("class B {{ {body} }}"));
    f.write("Z.cs", "class Z { void M() { if(true) {} } }");
    let report = f.scan(2);
    assert_eq!(report.files.len(), 3);
    assert_eq!(
        ranked(&report),
        vec![("Z.cs".into(), 1), ("A.cs".into(), 0), ("B.cs".into(), 0),]
    );
}

#[test]
fn worker_counts_preserve_rankings_and_complexity_evidence() {
    let f = Fixture::new();
    let body = "void M(int x) { if(x > 0) { x += 1; x += 2; x += 3; x += 4; x += 5; x += 6; x += 7; x += 8; x += 9; x += 10; } }";
    f.write("A.cs", format!("class A {{ B item; {body} }}"));
    f.write("B.cs", format!("class B {{ A item; {body} }}"));
    f.write("Unused.cs", "class Unused {}");
    let serial = f.scan(1);
    for count in [2, 4] {
        let parallel = f.scan(count);
        assert_eq!(ranked(&serial), ranked(&parallel));
        assert_eq!(serial.diagnostics, parallel.diagnostics);
        assert_eq!(serial.selection, parallel.selection);
        assert_eq!(
            format!("{:?}", serial.complexity),
            format!("{:?}", parallel.complexity)
        );
    }
}

#[test]
fn every_scan_reads_changed_source_and_creates_no_scan_state() {
    let f = Fixture::new();
    f.write("A.cs", "class A {}");
    assert_eq!(f.scan(1).ranked[0].score.maximum, 0);
    f.write("A.cs", "class A { void M(bool a) { if(a) {} } }");
    assert_eq!(f.scan(2).ranked[0].score.maximum, 1);
    let names = fs::read_dir(f.root())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    assert_eq!(names, ["A.cs"]);
}

#[test]
fn shared_selection_policy_filters_paths_and_content_with_explicit_overrides() {
    let f = Fixture::new();
    f.write("Keep.cs", "class Keep {}");
    f.write("Generated.cs", "// <auto-generated/>\nclass Generated {}");
    f.write("bin/Output.g.cs", "class Output {}");
    f.write("ignored/Hidden.cs", "class Hidden {}");
    f.write(".gitignore", "ignored/\n");
    let default = f.scan(2);
    assert_eq!(
        default
            .files
            .iter()
            .map(|f| f.path.clone())
            .collect::<Vec<_>>(),
        [PathBuf::from("Keep.cs")]
    );
    assert_eq!(default.content_skips[&SkipReason::GeneratedHeader], 1);
    let options = ScanOptions {
        includes: vec!["**/*.cs".into()],
        excludes: vec!["ignored/**".into()],
        workers: 2,
        ..Default::default()
    };
    let explicit = scan(f.root(), &options).unwrap();
    assert_eq!(
        explicit
            .files
            .iter()
            .map(|f| f.path.clone())
            .collect::<Vec<_>>(),
        ["Generated.cs", "Keep.cs", "bin/Output.g.cs"].map(PathBuf::from)
    );
    let output = f.run(&[
        "--top",
        "10",
        "--include",
        "**/*.cs",
        "--exclude",
        "ignored/**",
    ]);
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap().lines().count(), 3);
}

#[test]
fn empty_and_unsupported_inputs_produce_no_results() {
    let f = Fixture::new();
    for step in 0..2 {
        if step == 1 {
            f.write("Readme.txt", "documentation");
        }
        let output = f.run(&["--top", "5"]);
        assert!(output.status.success());
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
        assert!(f.scan(1).ranked.is_empty());
    }
}

#[test]
fn binary_and_invalid_encodings_make_coverage_incomplete_without_inventing_scores() {
    let f = Fixture::new();
    f.write("Binary.cs", b"class X {}\0");
    f.write("Encoding.cs", [0xff, 0xfe, 0xff]);
    f.write("Valid.cs", "class Valid {}");
    let report = f.scan(2);
    assert_eq!(report.files.len(), 1);
    assert_eq!(report.files[0].path, Path::new("Valid.cs"));
    assert_eq!(report.unscored_files, 2);
    assert_eq!(report.diagnostics.len(), 2);
    assert!(report.content_skips.is_empty());
    let output = f.run(&["--top", "10"]);
    assert_eq!(output.status.code(), Some(3));
    let diagnostics = String::from_utf8(output.stderr).unwrap();
    assert!(diagnostics.contains("incomplete scan: analyzed 1 files; 2 selected files unscored."));
    assert_eq!(String::from_utf8(output.stdout).unwrap().lines().count(), 1);
}

#[test]
fn parse_failure_returns_scored_rows_with_explicit_incomplete_status() {
    let f = Fixture::new();
    f.write("A.cs", "class A {}");
    f.write("Broken.cs", "class Broken { void M( {");
    let report = f.scan(2);
    assert_eq!(ranked(&report), vec![("A.cs".into(), 0)]);
    assert_eq!(report.unscored_files, 1);
    assert_eq!(report.diagnostics[0].path, Path::new("Broken.cs"));
    assert!(report.diagnostics[0].message.contains("syntax"));
    let output = f.run(&["--top", "10"]);
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(String::from_utf8(output.stdout).unwrap().lines().count(), 1);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("analyzed 1 files; 1 selected files"));
    assert!(stderr.contains("Broken.cs"));
    fs::remove_file(f.root().join("A.cs")).unwrap();
    let output = f.run(&["--top", "10"]);
    assert_eq!(output.status.code(), Some(3));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("analyzed 0 files"));
}

#[test]
fn failures_drain_bounded_queues_and_report_deterministically() {
    let f = Fixture::new();
    // More files than either bounded queue can hold. Mix success and failure so
    // aggregation must keep draining while discovery and workers are active.
    for index in 0..40 {
        f.write(
            &format!("good/G{index:02}.cs"),
            format!("class G{index} {{}}"),
        );
        f.write(&format!("bad/B{index:02}.cs"), "class Broken { void M( {");
    }
    let first = f.scan(1);
    let parallel = f.scan(4);
    assert_eq!(first.diagnostics, parallel.diagnostics);
    assert_eq!(first.unscored_files, 40);
    assert_eq!(parallel.unscored_files, 40);
    assert_eq!(first.files.len(), 40);
    assert_eq!(ranked(&first), ranked(&parallel));
    assert_eq!(first.diagnostics.len(), 32);
    assert_eq!(first.diagnostics[0].path, Path::new("bad/B00.cs"));
    let serial = f.run(&["--top", "10", "--threads", "1"]);
    let parallel = f.run(&["--top", "10", "--threads", "4"]);
    assert_eq!(serial.status.code(), Some(3));
    assert_eq!(serial.stdout, parallel.stdout);
    assert_eq!(serial.stderr, parallel.stderr);
    assert!(String::from_utf8(serial.stderr)
        .unwrap()
        .contains("8 additional"));
}

#[test]
fn malformed_arguments_and_roots_fail_without_stdout() {
    let f = Fixture::new();
    for arguments in [
        vec![],
        vec!["--top"],
        vec!["--top", "0"],
        vec!["--top", "-1"],
        vec!["--top", "abc"],
        vec!["--top", "1", "--top", "2"],
        vec!["--top", "1", "--threads", "0"],
        vec!["--top", "1", "--threads", "65"],
        vec!["--top", "1", "--include"],
        vec!["--top", "1", "--include", "["],
        vec!["--top", "1", "--unknown"],
    ] {
        let output = f.run(&arguments);
        assert!(!output.status.success(), "accepted {arguments:?}");
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
    f.write("File.cs", "class File {}");
    for path in [f.root().join("File.cs"), f.root().join("missing")] {
        let output = Command::new(env!("CARGO_BIN_EXE_cpath"))
            .arg(path)
            .args(["--top", "1"])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn explicit_resource_budgets_fail_instead_of_truncating_analysis() {
    let f = Fixture::new();
    f.write("A.cs", "class A {}");
    let file_budget = ScanOptions {
        max_file_bytes: 4,
        ..Default::default()
    };
    let report = scan(f.root(), &file_budget).unwrap();
    assert_eq!(report.unscored_files, 1);
    assert!(report.files.is_empty());
    assert!(report.diagnostics[0].message.contains("4-byte"));
    let bytes_budget = ScanOptions {
        max_summary_bytes: 4,
        ..Default::default()
    };
    assert!(failure(scan(f.root(), &bytes_budget)).contains("4 bytes"));
    // Empty valid sources still consume per-file bookkeeping.
    f.write("A.cs", "");
    f.write("B.cs", "");
    let files_budget = ScanOptions {
        max_files: 1,
        ..Default::default()
    };
    assert!(failure(scan(f.root(), &files_budget)).contains("1 files"));
    for workers in [0, 65] {
        assert!(failure(scan(
            f.root(),
            &ScanOptions {
                workers,
                ..Default::default()
            }
        ))
        .contains("worker count"));
    }
    // A public library caller can supply values beyond the CLI surface.
    for max_file_bytes in [0, u64::MAX] {
        assert!(failure(scan(
            f.root(),
            &ScanOptions {
                max_file_bytes,
                ..Default::default()
            }
        ))
        .contains("source byte limit"));
    }
}

#[cfg(unix)]
#[test]
fn unrenderable_later_path_fails_before_printing_any_rows() {
    let f = Fixture::new();
    f.write("A.cs", "class A { void M(bool a) { if(a) {} } }");
    f.write("Z\nInvalid.cs", "class Z {}");
    let output = f.run(&["--top", "10"]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("line break"));
}

// APFS rejects invalid UTF-8 filenames before the scanner can read them.
#[cfg(all(unix, not(target_vendor = "apple")))]
#[test]
fn non_unicode_paths_have_deterministic_internal_order_and_fail_cleanly_at_output() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let f = Fixture::new();
    let names = [
        OsString::from("A\u{fffd}.cs"),
        OsString::from_vec(b"A\xfe.cs".to_vec()),
        OsString::from_vec(b"A\xff.cs".to_vec()),
    ];
    for (index, name) in names.iter().enumerate() {
        fs::write(f.root().join(name), format!("class Unique{index} {{}}")).unwrap();
    }
    let expected = names.into_iter().map(PathBuf::from).collect::<Vec<_>>();
    for workers in [1, 4] {
        assert_eq!(
            f.scan(workers)
                .files
                .iter()
                .map(|file| file.path.clone())
                .collect::<Vec<_>>(),
            expected
        );
    }
    let output = f.run(&["--top", "10"]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("Unicode"));
}

#[test]
fn bounded_method_evidence_does_not_truncate_burden() {
    let f = Fixture::new();
    let methods = (0..9)
        .map(|i| format!("void M{i}() {{ {} }}", "if(true) {}".repeat(40)))
        .collect::<String>();
    f.write("Many.cs", format!("class Many {{ {methods} }}"));
    let report = f.scan(2);
    assert_eq!(report.complexity[0].methods.len(), 5);
    assert_eq!(report.complexity[0].total, 360);
    assert_eq!(report.ranked[0].score.maximum, 40);
    assert_eq!(report.ranked[0].score.burden, 225);
}

#[test]
fn bom_marked_sources_are_decoded_before_selection_and_parsing() {
    let f = Fixture::new();
    let source = "class Cafe { void M() { if(true) {} } }\r\n";
    let encode = |text: &str, little: bool| {
        let mut bytes = if little {
            vec![0xff, 0xfe]
        } else {
            vec![0xfe, 0xff]
        };
        for unit in text.encode_utf16() {
            bytes.extend(if little {
                unit.to_le_bytes()
            } else {
                unit.to_be_bytes()
            });
        }
        bytes
    };
    f.write("Little.cs", encode(source, true));
    f.write("Big.cs", encode(source, false));
    f.write(
        "Bom.cs",
        [b"\xef\xbb\xbf".as_slice(), source.as_bytes()].concat(),
    );
    f.write(
        "Generated.cs",
        encode("// <auto-generated/>\nclass G {}", true),
    );
    let report = f.scan(2);
    assert_eq!(report.unscored_files, 0);
    assert_eq!(report.files.len(), 3);
    assert!(report.ranked.iter().all(|r| r.score.maximum == 1));
    assert_eq!(report.content_skips[&SkipReason::GeneratedHeader], 1);
    let explicit = scan(
        f.root(),
        &ScanOptions {
            includes: vec!["Generated.cs".into()],
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(explicit.files.len(), 1);
    assert_eq!(explicit.unscored_files, 0);
}

#[test]
fn worker_panics_fail_scan_and_release_bounded_discovery_queue() {
    use cpath::complexity::ComplexityAnalyzer;
    use cpath::model::{ComplexityMetrics, FileFacts};
    use cpath::pipeline::{scan_with, Analyzers};
    use cpath::scoring::ComplexityFirst;
    struct BrokenAnalyzer;
    impl ComplexityAnalyzer for BrokenAnalyzer {
        fn analyze(&self, _: &FileFacts) -> ComplexityMetrics {
            panic!("intentional analyzer failure");
        }
    }
    let f = Fixture::new();
    for i in 0..40 {
        f.write(&format!("F{i}.cs"), "class C {}");
    }
    let error = failure(scan_with(
        f.root(),
        &ScanOptions {
            workers: 2,
            ..Default::default()
        },
        Analyzers {
            complexity: &BrokenAnalyzer,
            scorer: &ComplexityFirst,
        },
    ));
    assert!(error.contains("worker panicked"));
}
