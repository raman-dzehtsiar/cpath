// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

use std::collections::BTreeMap;
use std::fs;

use cpath::pipeline::{scan_with_diagnostics, FailureKind, ScanOptions};

#[test]
fn all_failures_are_observed_and_counted_beyond_the_display_limit() {
    let root = tempfile::tempdir().unwrap();
    for index in 0..70 {
        fs::write(root.path().join(format!("broken{index:02}.cs")), "class {").unwrap();
    }
    fs::write(root.path().join("encoding.cs"), [0xff]).unwrap();
    fs::write(root.path().join("binary.cs"), [0]).unwrap();
    fs::write(root.path().join("large.cs"), [b' '; 101]).unwrap();
    fs::write(root.path().join("valid.cs"), "class C {}").unwrap();

    let mut previous = None;
    for workers in [1, 4] {
        let mut observed = BTreeMap::new();
        let report = scan_with_diagnostics(
            root.path(),
            &ScanOptions {
                workers,
                max_file_bytes: 100,
                ..Default::default()
            },
            &mut |diagnostic| {
                assert!(observed
                    .insert(diagnostic.path.clone(), diagnostic.kind)
                    .is_none());
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(report.files.len(), 1);
        assert_eq!(report.unscored_files, 73);
        assert_eq!(report.diagnostics.len(), 32);
        assert_eq!(observed.len(), 73);
        assert_eq!(
            report.failure_counts,
            BTreeMap::from([
                (FailureKind::Parse, 70),
                (FailureKind::Encoding, 1),
                (FailureKind::Binary, 1),
                (FailureKind::Size, 1),
            ])
        );
        if let Some(previous) = &previous {
            assert_eq!(&observed, previous);
        }
        previous = Some(observed);
    }
}

#[test]
fn observer_failure_drains_queues_and_returns_a_fatal_error() {
    let root = tempfile::tempdir().unwrap();
    for index in 0..40 {
        fs::write(root.path().join(format!("broken{index}.cs")), "class {").unwrap();
    }
    for panic in [false, true] {
        let mut called = 0;
        let result = scan_with_diagnostics(root.path(), &ScanOptions::default(), &mut |_| {
            called += 1;
            if panic {
                panic!("deliberate observer panic");
            }
            Err("deliberate observer error".into())
        });
        let error = result.err().expect("observer failure must be fatal");
        assert!(error.contains("diagnostic observer"));
        assert_eq!(called, 1);
    }
}
