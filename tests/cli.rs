// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cpath"))
        .args(args)
        .output()
        .expect("the compiled cpath binary should run")
}

#[test]
fn help_contains_only_usage_options_and_exit_status() {
    let expected =
        "Usage: cpath <repository> [--metrics CC,MI|all] [--include GLOB] [--exclude GLOB] [--threads N]
       cpath gate [<commit> | <from> <to>] [--cc-limit VALUE] [--mi-limit VALUE] [--pretty]

Options:
  --metrics LIST  Metrics to calculate: CC, MI, or all (default: all)
  --pretty        Indent JSON output by four spaces
  --include GLOB  Root-relative include; overrides defaults/ignores (repeatable)
  --exclude GLOB  Root-relative exclusion; wins over includes (repeatable)
  --threads N     File-analysis workers, 1..=64 (default: available CPUs, at most 64)
  -h, --help      Show this help
  -v, --version   Show the version

Possible scan exit codes: 0 complete, 1 fatal error, 2 invalid arguments, 3 incomplete scan.
Gate: 0 no confirmed violations, 1 violations, 2 usage/setup error. See cpath gate --help.
";
    for args in [vec![], vec!["--help"], vec!["-h"]] {
        let output = run(&args);
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let stdout = String::from_utf8(output.stdout).expect("help should be UTF-8");
        assert_eq!(stdout, expected);
    }
    let output = run(&["gate", "--help"]);
    assert!(output.status.success());
    assert!(!String::from_utf8(output.stdout)
        .unwrap()
        .contains("--debug"));
}

#[test]
fn version_matches_the_package() {
    for flag in ["--version", "-v"] {
        let output = run(&[flag]);
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            concat!(env!("CARGO_PKG_VERSION"), "\n")
        );
    }
}

#[test]
fn invalid_options_return_argument_errors_as_json() {
    for arguments in [
        vec!["--unknown"],
        vec!["-V"],
        vec!["--help", "extra"],
        vec!["--version", "extra"],
        vec!["repo", "--top", "1"],
        vec!["repo", "extra"],
        vec!["--pretty"],
        vec!["repo", "--pretty", "--pretty"],
        vec!["repo", "--threads"],
        vec!["repo", "--threads", "0"],
        vec!["repo", "--threads", "65"],
        vec!["repo", "--threads", "-1"],
        vec!["repo", "--threads", "1.5"],
        vec!["repo", "--threads", "many"],
        vec!["repo", "--threads", "18446744073709551616"],
        vec!["repo", "--threads", "1", "--threads", "2"],
        vec!["repo", "--include"],
        vec!["repo", "--exclude"],
        vec!["repo", "--metrics"],
        vec!["repo", "--metrics="],
        vec!["repo", "--metrics", "cc"],
        vec!["repo", "--metrics", "LOC"],
        vec!["repo", "--metrics", "CC,"],
        vec!["repo", "--metrics", ",MI"],
        vec!["repo", "--metrics", "CC, MI"],
        vec!["repo", "--metrics", "all,CC"],
        vec!["repo", "--metrics=CC", "--metrics", "MI"],
        vec!["repo", "--metrics", "MI", "--metrics=CC"],
    ] {
        let output = run(&arguments);
        assert_eq!(output.status.code(), Some(2), "{arguments:?}: {output:?}");
        assert!(output.stderr.is_empty());
        let document: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(document["exit_code"], 2);
        assert_eq!(document["cpath_version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(document["repository_root"], serde_json::Value::Null);
        assert_eq!(document["scanned_files"], serde_json::json!([]));
        assert_eq!(document["skipped_files"], serde_json::json!([]));
        assert!(!document["error"].as_str().unwrap().is_empty());
    }
}

#[test]
fn parser_accepts_metric_forms_and_options_on_either_side_of_the_root() {
    use cpath::cli::{parse, Command};
    use std::ffi::OsString;

    for (value, cognitive_complexity, maintainability) in [
        ("CC", true, false),
        ("MI", false, true),
        ("CC,MI", true, true),
        ("MI,CC", true, true),
        ("CC,CC", true, false),
        ("all", true, true),
        ("ALL", true, true),
    ] {
        for equals in [false, true] {
            let metric_arguments = if equals {
                vec![format!("--metrics={value}")]
            } else {
                vec!["--metrics".into(), value.into()]
            };
            let arguments = metric_arguments
                .into_iter()
                .chain([
                    "--include".into(),
                    "src/**".into(),
                    "repository with spaces".into(),
                    "--include".into(),
                    "tests/**".into(),
                    "--exclude".into(),
                    "**/generated/**".into(),
                    "--exclude".into(),
                    "**/vendor/**".into(),
                    "--threads".into(),
                    "64".into(),
                    "--pretty".into(),
                ])
                .map(OsString::from);
            let Command::Scan {
                root,
                options,
                pretty,
            } = parse(arguments).unwrap()
            else {
                panic!("expected a scan");
            };
            assert_eq!(root, std::path::Path::new("repository with spaces"));
            assert_eq!(options.metrics.cognitive_complexity, cognitive_complexity);
            assert_eq!(options.metrics.maintainability, maintainability);
            assert_eq!(options.workers, 64);
            assert_eq!(options.includes, ["src/**", "tests/**"]);
            assert_eq!(options.excludes, ["**/generated/**", "**/vendor/**"]);
            assert!(pretty);
        }
    }
}

#[test]
fn parser_defaults_to_all_metrics_and_bounded_workers() {
    let cpath::cli::Command::Scan {
        options, pretty, ..
    } = cpath::cli::parse([std::ffi::OsString::from(".")]).unwrap()
    else {
        panic!("expected a scan");
    };
    assert_eq!(options.metrics, cpath::pipeline::MetricSet::all());
    assert!((1..=64).contains(&options.workers));
    assert!(options.includes.is_empty());
    assert!(options.excludes.is_empty());
    assert!(!pretty);
}

#[test]
fn scan_is_a_repository_name_rather_than_a_subcommand() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("scan")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_cpath"))
        .current_dir(directory.path())
        .arg("scan")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let document: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        document["repository_root"],
        serde_json::json!(directory.path().join("scan").canonicalize().unwrap())
    );
    assert_eq!(document["scanned_files"], serde_json::json!([]));
}

#[cfg(unix)]
#[test]
fn non_unicode_option_values_are_rejected_but_repository_paths_are_preserved() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let invalid = OsString::from_vec(b"repository-\xff".to_vec());
    for flag in ["--include", "--exclude", "--metrics", "--threads"] {
        let result =
            cpath::cli::parse([OsString::from("."), OsString::from(flag), invalid.clone()]);
        assert!(result.is_err(), "{flag}");
    }
    let cpath::cli::Command::Scan { root, .. } = cpath::cli::parse([invalid.clone()]).unwrap()
    else {
        panic!("expected a scan");
    };
    assert_eq!(root.as_os_str(), invalid);
}
