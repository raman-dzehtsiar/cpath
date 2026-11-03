// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

use std::io::{self, Write};
use std::path::Path;

use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::pipeline::ScanReport;

pub fn write_json(
    mut writer: impl Write,
    version: &str,
    report: &ScanReport,
    exit_code: u8,
    pretty: bool,
) -> io::Result<()> {
    let scanned_files = report
        .files
        .iter()
        .enumerate()
        .map(|(index, file)| scanned_file(report, index, &file.path))
        .collect::<Vec<_>>();
    let skipped_files = report
        .diagnostics
        .iter()
        .map(|diagnostic| json!({"path": normalized_path(&diagnostic.path), "reason": diagnostic.message}))
        .collect::<Vec<_>>();
    let document = json!({
        "cpath_version": version,
        "repository_root": report.root,
        "scanned_files": scanned_files,
        "skipped_files": skipped_files,
        "exit_code": exit_code,
    });
    if pretty {
        let formatter = serde_json::ser::PrettyFormatter::with_indent(b"    ");
        let mut serializer = serde_json::Serializer::with_formatter(&mut writer, formatter);
        document
            .serialize(&mut serializer)
            .map_err(|error| io::Error::other(error.to_string()))?;
    } else {
        serde_json::to_writer(&mut writer, &document)
            .map_err(|error| io::Error::other(error.to_string()))?;
    }
    writeln!(writer)
}

pub fn write_error_json(
    mut writer: impl Write,
    version: &str,
    exit_code: u8,
    error: &str,
) -> io::Result<()> {
    let document = json!({
        "cpath_version": version,
        "repository_root": Value::Null,
        "scanned_files": [],
        "skipped_files": [],
        "exit_code": exit_code,
        "error": error,
    });
    serde_json::to_writer(&mut writer, &document)
        .map_err(|write_error| io::Error::other(write_error.to_string()))?;
    writeln!(writer)
}

fn scanned_file(report: &ScanReport, index: usize, path: &Path) -> Value {
    let mut row = Map::new();
    row.insert("path".into(), Value::String(normalized_path(path)));
    if report.metrics.cognitive_complexity {
        row.insert("CC".into(), json!(report.complexity[index].max_method));
    }
    if report.metrics.maintainability {
        row.insert(
            "MI".into(),
            report.maintainability[index]
                .as_ref()
                .and_then(|summary| summary.minimum)
                .map_or(Value::Null, |value| json!(value)),
        );
    }
    Value::Object(row)
}

fn normalized_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}
