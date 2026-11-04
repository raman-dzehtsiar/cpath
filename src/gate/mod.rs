// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

//! Endpoint quality gate, isolated from ordinary scanning and metric computation.

mod analysis;
mod git;
mod identity;
mod options;
mod policy;
mod working_tree;

pub use options::{Options, HELP};

use analysis::{Analysis, Analyzer, Unit};
use serde::Serialize;
use serde_json::{json, Value};
use std::ffi::OsString;
use std::io::Write;
use std::path::Path;

const EXIT_OK: u8 = 0;
const EXIT_VIOLATION: u8 = 1;
const EXIT_USAGE: u8 = 2;

struct Report {
    root: Option<String>,    // Discovered repository root, when setup succeeded.
    from: Option<String>,    // Resolved starting commit ID.
    to: Option<String>,      // Resolved target commit ID, or WORKTREE for current files.
    limits: Value,           // Effective new-code defaults and overrides.
    complete: bool,          // False when any eligible comparison could not be assessed.
    changed_files: usize,    // Endpoint tree changes, including excluded/deleted files.
    analyzed_files: usize,   // Target source files successfully parsed for metrics.
    checked_scopes: usize,   // Number of evaluated scope/metric pairs.
    exclusions: usize,       // Intentional exclusions, not missing eligible coverage.
    violations: Vec<Value>,  // Confirmed metric failures in target coordinates.
    diagnostics: Vec<Value>, // Coverage gaps, exclusions, or setup errors.
    setup_error: bool,       // Invalid invocation or inability to resolve endpoints.
    debug: bool,             // Include optional coverage and exclusion diagnostics.
}

impl Report {
    fn new(options: &Options) -> Self {
        Self {
            root: None,
            from: None,
            to: None,
            limits: json!({"CC": {"c_cpp": options.cc_limit.unwrap_or(options::DEFAULT_C_FAMILY_CC_LIMIT), "other": options.cc_limit.unwrap_or(options::DEFAULT_CC_LIMIT)}, "MI": options.mi_limit}),
            complete: true,
            changed_files: 0,
            analyzed_files: 0,
            checked_scopes: 0,
            exclusions: 0,
            violations: Vec::new(),
            diagnostics: Vec::new(),
            setup_error: false,
            debug: options.debug,
        }
    }

    fn issue(
        &mut self,
        file: &str,
        side: &str,
        metric: Option<&str>,
        unit: Option<&Unit>,
        reason: &str,
    ) {
        self.complete = false;
        if !self.debug {
            return;
        }
        self.diagnostics.push(json!({
            "kind": "unassessed", "file": file, "side": side, "metric": metric,
            "scope": unit.map(|unit| unit.name.as_str()), "line": unit.map(|unit| unit.line), "reason": reason,
        }));
    }

    fn exclude(&mut self, file: &str, reason: &str) {
        self.exclusions += 1;
        if !self.debug {
            return;
        }
        self.diagnostics
            .push(json!({"kind": "excluded", "file": file, "side": "to", "reason": reason}));
    }

    fn error(&mut self, reason: &str) {
        self.setup_error = true;
        self.complete = false;
        self.diagnostics
            .push(json!({"kind": "error", "reason": reason}));
    }

    fn exit_code(&self) -> u8 {
        if self.setup_error {
            return EXIT_USAGE;
        }
        if !self.violations.is_empty() {
            return EXIT_VIOLATION;
        }
        EXIT_OK
    }

    fn document(mut self) -> Value {
        let exit_code = self.exit_code();
        let status = match exit_code {
            EXIT_USAGE => "error",
            EXIT_VIOLATION => "failed",
            _ if !self.complete => "incomplete",
            _ => "passed",
        };
        self.violations.sort_by_key(sort_key);
        self.diagnostics
            .sort_by_key(|value| (sort_key(value), value.to_string()));
        let mut document = json!({
            "cpath_version": env!("CARGO_PKG_VERSION"),
            "repository_root": self.root, "from": self.from, "to": self.to,
            "status": status, "limits": self.limits, "exit_code": exit_code,
            "counts": {"changed_files": self.changed_files, "analyzed_files": self.analyzed_files,
                "checked_scope_metrics": self.checked_scopes, "exclusions": self.exclusions},
            "violations": self.violations, "diagnostics": self.diagnostics,
        });
        if self.debug {
            document["complete"] = json!(self.complete);
        }
        document
    }
}

fn sort_key(value: &Value) -> (String, u64, String, String) {
    (
        value["file"].as_str().unwrap_or_default().to_owned(),
        value["line"].as_u64().unwrap_or_default(),
        value["scope"].as_str().unwrap_or_default().to_owned(),
        value["metric"].as_str().unwrap_or_default().to_owned(),
    )
}

fn analyze_source(
    analyzer: &mut Analyzer,
    file: &str,
    bytes: &[u8],
    language: crate::model::SourceLanguage,
) -> Result<Analysis, String> {
    // Only this gate adapter catches ordinary unwinding failures. The scanner is unchanged.
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        analyzer.analyze(Path::new(file), language, bytes)
    })) {
        Ok(result) => result,
        Err(_) => {
            *analyzer = Analyzer::new()?;
            Err("analysis panicked; this file was not assessed".into())
        }
    }
}

fn target_bytes(
    report: &mut Report,
    repository: &git::Repository,
    change: &git::Change,
    file: &str,
    blobs: &mut git::Blobs,
) -> Option<Vec<u8>> {
    let result = if repository.to.is_some() {
        if change.new_mode == "000000" {
            report.exclude(file, "deleted file has no target scope to gate");
            return None;
        }
        if !git::regular(&change.new_mode) {
            report.exclude(file, "symlink, submodule, or other non-regular tree entry");
            return None;
        }
        blobs.read(&change.new_blob)
    } else {
        if change.new_mode == "160000" {
            report.exclude(file, "submodule contents are not gated");
            return None;
        }
        match working_tree::inspect(&repository.root, file) {
            Ok(working_tree::Entry::File(path)) => working_tree::read(&path),
            Ok(working_tree::Entry::Missing) if change.new_mode == "000000" => {
                report.exclude(file, "deleted file has no target scope to gate");
                return None;
            }
            Ok(working_tree::Entry::Missing) => Err(
                "working-tree source is missing; rerun the gate if files changed during scanning"
                    .into(),
            ),
            Ok(working_tree::Entry::Excluded) => {
                report.exclude(
                    file,
                    "symlink, directory, or other non-regular working-tree entry",
                );
                return None;
            }
            Err(error) => Err(error),
        }
    };
    match result {
        Ok(bytes) => Some(bytes),
        Err(error) => {
            report.issue(file, "to", None, None, &error);
            None
        }
    }
}

fn process_change(
    report: &mut Report,
    options: &Options,
    repository: &git::Repository,
    change: git::Change,
    blobs: &mut git::Blobs,
    analyzer: &mut Analyzer,
) {
    let file = match std::str::from_utf8(&change.path) {
        Ok(file) => file,
        Err(_) => {
            let escaped = change
                .path
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            report.issue(
                &format!("git-path-hex:{escaped}"),
                "to",
                None,
                None,
                "Git path is not valid Unicode; exact path bytes are shown as hexadecimal",
            );
            return;
        }
    };
    let Some(language) = crate::selection::language_for_path(Path::new(file)) else {
        report.exclude(file, "unsupported source language");
        return;
    };
    let Some(bytes) = target_bytes(report, repository, &change, file, blobs) else {
        return;
    };
    let after = match analyze_source(analyzer, file, &bytes, language) {
        Ok(analysis) => analysis,
        Err(error) => {
            report.issue(file, "to", None, None, &error);
            return;
        }
    };
    drop(bytes);
    report.analyzed_files += 1;
    let before = if git::regular(&change.old_mode) {
        match blobs
            .read(&change.old_blob)
            .and_then(|bytes| analyze_source(analyzer, file, &bytes, language))
        {
            Ok(analysis) => Some(analysis),
            Err(error) => {
                report.issue(file, "from", None, None, &error);
                None
            }
        }
    } else {
        Some(Analysis::default())
    };
    let default_cc = if matches!(
        language,
        crate::model::SourceLanguage::C
            | crate::model::SourceLanguage::Cpp
            | crate::model::SourceLanguage::CHeader
    ) {
        options::DEFAULT_C_FAMILY_CC_LIMIT
    } else {
        options::DEFAULT_CC_LIMIT
    };
    policy::compare(
        report,
        file,
        "CC",
        before.as_ref().map(|analysis| &analysis.cc),
        &after.cc,
        options.cc_limit.unwrap_or(default_cc),
    );
    policy::compare(
        report,
        file,
        "MI",
        before.as_ref().map(|analysis| &analysis.mi),
        &after.mi,
        options.mi_limit,
    );
}

fn inspect(
    report: &mut Report,
    repository: &git::Repository,
    options: &Options,
) -> Result<(), String> {
    let changes = repository.changes()?;
    report.changed_files = changes.len();
    if changes.is_empty() {
        return Ok(());
    }
    let mut blobs = repository.blobs()?;
    let mut analyzer = Analyzer::new()?;
    for change in changes {
        process_change(
            report,
            options,
            repository,
            change,
            &mut blobs,
            &mut analyzer,
        );
    }
    Ok(())
}

/// Compare a commit with current files or an explicit commit without changing the index or worktree.
pub fn evaluate(directory: &Path, options: &Options) -> Value {
    let mut report = Report::new(options);
    let repository = match git::Repository::open(directory, &options.from, options.to.as_deref()) {
        Ok(repository) => repository,
        Err(error) => {
            report.error(&error);
            return report.document();
        }
    };
    report.root = Some(repository.root.to_string_lossy().into_owned());
    report.from = Some(repository.from.clone());
    report.to = Some(repository.to.clone().unwrap_or_else(|| "WORKTREE".into()));
    if let Err(error) = inspect(&mut report, &repository, options) {
        report.issue("", "comparison", None, None, &error);
    }
    report.document()
}

fn write_json(document: &Value, pretty: bool) -> std::io::Result<()> {
    let mut writer = std::io::stdout().lock();
    if pretty {
        let formatter = serde_json::ser::PrettyFormatter::with_indent(b"    ");
        let mut serializer = serde_json::Serializer::with_formatter(&mut writer, formatter);
        document
            .serialize(&mut serializer)
            .map_err(std::io::Error::other)?;
    } else {
        serde_json::to_writer(&mut writer, document)?;
    }
    writeln!(writer)
}

/// Dedicated CLI boundary keeps gate usage errors and output separate from scans.
pub fn run(arguments: &[OsString]) -> u8 {
    if arguments.len() == 1 && matches!(arguments[0].to_str(), Some("--help" | "-h")) {
        println!("{HELP}");
        return EXIT_OK;
    }
    let pretty = arguments.iter().any(|argument| argument == "--pretty");
    let debug = arguments.iter().any(|argument| argument == "--debug");
    let document = match options::parse(arguments) {
        Ok(options) => match std::env::current_dir() {
            Ok(directory) => evaluate(&directory, &options),
            Err(error) => error_document(
                &format!("Failed to locate current directory: {error}"),
                debug,
            ),
        },
        Err(error) => error_document(&error, debug),
    };
    let exit_code = document["exit_code"].as_u64().unwrap_or(u64::from(EXIT_OK)) as u8;
    if let Err(error) = write_json(&document, pretty) {
        eprintln!("Failed to write gate JSON: {error}");
    }
    exit_code
}

fn error_document(error: &str, debug: bool) -> Value {
    let options = Options {
        from: String::new(),
        to: None,
        cc_limit: None,
        mi_limit: options::DEFAULT_MI_LIMIT,
        pretty: false,
        debug,
    };
    let mut report = Report::new(&options);
    report.error(error);
    report.document()
}
