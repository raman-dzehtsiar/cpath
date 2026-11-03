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
