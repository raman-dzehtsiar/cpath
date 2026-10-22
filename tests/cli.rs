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
fn help_explains_the_purpose_and_current_limits() {
    for args in [vec![], vec!["--help"], vec!["-h"]] {
        let output = run(&args);
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let stdout = String::from_utf8(output.stdout).expect("help should be UTF-8");
        assert!(stdout.contains("Find the code that needs human attention."));
        assert!(stdout.contains("Offline C#, C, and C++ analysis of cognitive complexity."));
        assert!(stdout.contains("Experimental prototype"));
    }
}

#[test]
fn version_matches_the_package() {
    for flag in ["--version", "-V"] {
        let output = run(&[flag]);
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().trim(),
            concat!("cpath ", env!("CARGO_PKG_VERSION"))
        );
    }
}

#[test]
fn unsupported_commands_and_extra_arguments_fail_with_guidance() {
    for args in [
        vec!["scan"],
        vec!["--unknown"],
        vec!["--help", "extra"],
        vec!["--version", "extra"],
    ] {
        let output = run(&args);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8(output.stderr).expect("errors should be UTF-8");
        assert!(stderr.contains("cpath --help"));
    }
}
