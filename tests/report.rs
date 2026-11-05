// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

use std::io::{self, Write};

use cpath::pipeline::{scan, ScanOptions};
use cpath::report::{write_error_json, write_json};

struct FailingWriter {
    remaining_bytes: usize,
}

impl Write for FailingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.remaining_bytes == 0 {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed output"));
        }
        let written = bytes.len().min(self.remaining_bytes);
        self.remaining_bytes -= written;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn report_propagates_output_failures_including_the_final_newline() {
    let directory = tempfile::tempdir().unwrap();
    let report = scan(
        directory.path(),
        &ScanOptions {
            workers: 1,
            ..Default::default()
        },
    )
    .unwrap();
    for pretty in [false, true] {
        let mut complete = Vec::new();
        write_json(&mut complete, "test", &report, 0, pretty).unwrap();
        for remaining_bytes in [0, 10, complete.len() - 1] {
            let result = write_json(
                FailingWriter { remaining_bytes },
                "test",
                &report,
                0,
                pretty,
            );
            assert!(
                result.is_err(),
                "accepted output failure after {remaining_bytes} bytes"
            );
        }
    }
    let mut complete = Vec::new();
    write_error_json(&mut complete, "test", 1, "unreadable input").unwrap();
    for remaining_bytes in [0, 10, complete.len() - 1] {
        assert!(write_error_json(
            FailingWriter { remaining_bytes },
            "test",
            1,
            "unreadable input"
        )
        .is_err());
    }
}

#[test]
fn metric_output_rounds_mi_to_unsigned_integers_without_changing_calculated_values() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("source.cs"),
        "class C { int F() { return 1; } }",
    )
    .unwrap();
    let mut report = scan(directory.path(), &ScanOptions::default()).unwrap();
    for (value, expected) in [
        (0.0, Some(0)),
        (12.02, Some(12)),
        (6.78, Some(7)),
        (19.5, Some(20)),
        (100.0, Some(100)),
        (f64::NAN, None),
        (f64::INFINITY, None),
        (-1.0, None),
    ] {
        report.maintainability[0].as_mut().unwrap().minimum = Some(value);
        let mut output = Vec::new();
        write_json(&mut output, "test", &report, 0, false).unwrap();
        let document: serde_json::Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(document["scanned_files"][0]["MI"].as_u64(), expected);
        assert!(document["scanned_files"][0]["CC"].is_u64());
        let retained = report.maintainability[0].as_ref().unwrap().minimum.unwrap();
        assert_eq!(retained.to_bits(), value.to_bits());
    }
}

#[test]
fn fatal_error_json_escapes_diagnostics() {
    let error = "cannot read \"café\":\npermission denied\t\\";
    let mut output = Vec::new();
    write_error_json(&mut output, "test", 1, error).unwrap();
    let document: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(
        document,
        serde_json::json!({
            "cpath_version": "test", "repository_root": null,
            "scanned_files": [], "skipped_files": [], "exit_code": 1, "error": error
        })
    );
    assert_eq!(String::from_utf8(output).unwrap().lines().count(), 1);
}
