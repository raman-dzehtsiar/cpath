// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

//! Research-only failure census. Reuses production selection/decoding/parsing,
//! then rereads failed inputs for bounded contextual evidence. Heuristic labels
//! describe nearby text, never prove a failure's cause or source validity.
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use cpath::model::SourceLanguage;
use cpath::pipeline::{scan_with_diagnostics, FailureKind, FileDiagnostic, ScanOptions};
use serde_json::{json, Value};

const SAMPLE_LIMIT: usize = 8;
const CONTEXT_CHARS: usize = 1000;

#[derive(Default)]
struct Bucket {
    files: usize,
    bytes: u64,
    unknown_size_files: usize,
    // Bottom-k stable hashes avoid retaining only alphabetically early paths.
    samples: BTreeMap<(u64, String), Value>,
}

impl Bucket {
    fn add(&mut self, path: &str, bytes: Option<u64>, sample: Value) {
        self.files += 1;
        if let Some(bytes) = bytes {
            self.bytes = self.bytes.saturating_add(bytes);
        } else {
            self.unknown_size_files += 1;
        }
        self.samples
            .insert((stable_hash(path), path.to_owned()), sample);
        if self.samples.len() > SAMPLE_LIMIT {
            self.samples.pop_last();
        }
    }

    fn json(&self) -> Value {
        json!({"files":self.files,"bytes":self.bytes,"unknown_size_files":self.unknown_size_files,"samples":self.samples.values().collect::<Vec<_>>()})
    }
}

#[derive(Default)]
struct Audit {
    // Keys come exclusively from fixed scope/category lists, never source names.
    buckets: BTreeMap<(&'static str, &'static str), Bucket>,
    labels: BTreeMap<(&'static str, &'static str), usize>,
    observed: usize,
    reread_failures: usize,
}

impl Audit {
    fn observe(
        &mut self,
        root: &Path,
        limit: u64,
        diagnostic: &FileDiagnostic,
        details: Option<&mut dyn Write>,
    ) -> Result<(), String> {
        self.observed += 1;
        let path = diagnostic.path.to_string_lossy().replace('\\', "/");
        let scope = scope(&diagnostic.path);
        let absolute = root.join(&diagnostic.path);
        let mut bytes = None;
        let mut context = String::new();
        let mut line = None;
        let mut labels = BTreeSet::new();
        let mut category = kind_name(diagnostic.kind);
        if diagnostic.kind == FailureKind::Parse {
            if let Ok((source_bytes, source)) = read_context_source(&absolute, limit) {
                bytes = Some(source_bytes);
                line = diagnostic_line(&diagnostic.message);
                let near = line.map_or_else(String::new, |line| error_context(&source, line));
                let csharp = cpath::selection::language_for_path(&diagnostic.path)
                    == Some(SourceLanguage::CSharp);
                labels = labels_for(&source, &near, csharp);
                if ambiguous_structure_diagnostic(&diagnostic.message) {
                    labels.insert("ambiguous_structure_diagnostic");
                }
                category = primary_category(&labels);
                context = near;
            } else {
                self.reread_failures += 1;
                category = "context_read_failed";
            }
        } else {
            bytes = std::fs::symlink_metadata(&absolute)
                .ok()
                .filter(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
                .map(|metadata| metadata.len());
        }
        for label in &labels {
            *self.labels.entry((scope, label)).or_default() += 1;
        }
        let record = json!({
            "path":path,"bytes":bytes,"kind":kind_name(diagnostic.kind),
            "message":diagnostic.message,"line":line,"context":context,"labels":labels,
            "scope":scope,"category":category,
        });
        if let Some(details) = details {
            write_record(details, &record)?;
        }
        self.buckets
            .entry((scope, category))
            .or_default()
            .add(&path, bytes, record);
        Ok(())
    }

    fn json(&self) -> Value {
        json!({
            "observed_files":self.observed,"context_read_failures":self.reread_failures,
            "buckets":self.buckets.iter().map(|((scope, category), bucket)| json!({
                "scope":scope,"category":category,"data":bucket.json(),
            })).collect::<Vec<_>>(),
            "overlapping_labels":self.labels.iter().map(|((scope,label),files)| json!({
                "scope":scope,"label":label,"files":files,
            })).collect::<Vec<_>>(),
        })
    }
}

fn kind_name(kind: FailureKind) -> &'static str {
    match kind {
        FailureKind::Read => "read",
        FailureKind::Size => "size",
        FailureKind::Encoding => "encoding",
        FailureKind::Binary => "binary",
        FailureKind::Parse => "parse",
    }
}

fn read_context_source(path: &Path, limit: u64) -> Result<(u64, String), String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("context input changed into a symlink or non-regular file".into());
    }
    if metadata.len() > limit {
        return Err("context input exceeds byte limit".into());
    }
    let mut content = Vec::with_capacity(metadata.len() as usize);
    File::open(path)
        .and_then(|file| file.take(limit + 1).read_to_end(&mut content))
        .map_err(|error| error.to_string())?;
    if content.len() as u64 > limit {
        return Err("context input grew beyond byte limit".into());
    }
    let source = cpath::source::decode(&content)?.into_owned();
    Ok((content.len() as u64, source))
}

fn scope(path: &Path) -> &'static str {
    if path.components().any(|component| {
        matches!(
            component.as_os_str().to_str(),
            Some(
                "test"
                    | "tests"
                    | "Test"
                    | "Tests"
                    | "testing"
                    | "unittests"
                    | "UnitTests"
                    | "benchmarks"
            )
        )
    }) {
        "test_or_benchmark_path"
    } else {
        "other_path"
    }
}

fn diagnostic_line(message: &str) -> Option<usize> {
    // Ambiguous .h files report both grammars; the final C++ diagnostic is the
    // useful alternative for C++ headers, not the C rejection of `namespace`.
    message
        .rsplit_once(" at line ")?
        .1
        .split(',')
        .next()?
        .parse()
        .ok()
}

fn ambiguous_structure_diagnostic(message: &str) -> bool {
    [
        "unsupported or ambiguous C callable declaration",
        "unsupported or ambiguous C++ callable declaration",
        "unsupported or ambiguous C file-scope block",
        "unsupported or ambiguous C++ file-scope block",
    ]
    .iter()
    .any(|prefix| message.contains(*prefix))
}

fn error_context(source: &str, line: usize) -> String {
    source
        .lines()
        .skip(line.saturating_sub(4))
        .take(7)
        .flat_map(|line| line.chars().chain(std::iter::once('\n')))
        .take(CONTEXT_CHARS)
        .collect()
}

fn labels_for(source: &str, near: &str, csharp: bool) -> BTreeSet<&'static str> {
    let mut labels = BTreeSet::new();
    if source.lines().any(|line| {
        matches!(
            line.split_whitespace().next(),
            Some("#if" | "#ifdef" | "#ifndef" | "#elif" | "#else" | "#endif")
        )
    }) {
        labels.insert("conditional_directive_in_file");
    }
    if near.lines().any(|line| {
        matches!(
            line.split_whitespace().next(),
            Some("#if" | "#ifdef" | "#ifndef" | "#elif" | "#else" | "#endif")
        )
    }) {
        labels.insert("conditional_directive_near_error");
    }
    if near.lines().any(|line| line.trim_start().starts_with('#')) {
        labels.insert("preprocessor_near_error");
    }
    if source.contains("expected-error")
        || source.contains("expected-warning")
        || source.contains("expected-note")
    {
        labels.insert("compiler_diagnostic_expectation_in_file");
    }
    if csharp {
        if near.contains("[..")
            || near.contains("..]")
            || near.contains(" is ")
            || near.contains(" not ")
            || near.contains(" and ")
            || near.contains(" or ")
        {
            labels.insert("pattern_or_boolean_text_near_error");
        }
    } else {
        // Lexical clues only. Uppercase names can be types, constants or prose;
        // these are candidates for source review, not verified macro expansion.
        for (index, character) in near.char_indices() {
            if !character.is_ascii_uppercase() && character != '_' {
                continue;
            }
            if index > 0
                && (near.as_bytes()[index - 1].is_ascii_alphanumeric()
                    || near.as_bytes()[index - 1] == b'_')
            {
                continue;
            }
            let suffix = &near[index..];
            let end = suffix
                .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .unwrap_or(suffix.len());
            let word = &suffix[..end];
            if word.len() < 3
                || !word.contains('_')
                || !word.bytes().any(|byte| byte.is_ascii_uppercase())
                || !word
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
            {
                continue;
            }
            labels.insert("uppercase_identifier_near_error");
            if suffix[end..].trim_start().starts_with('(') {
                labels.insert("uppercase_call_near_error");
            }
            if word.ends_with("_ABI")
                || word.ends_with("_EXPORT")
                || word.ends_with("_API")
                || word.ends_with("_METHOD")
                || word.ends_with("_VISIBILITY")
                || word.ends_with("_VIS")
                || word.starts_with("LLVM_ATTRIBUTE_")
            {
                labels.insert("annotation_name_near_error");
            }
        }
        if near.contains("template") || near.contains("requires") || near.contains("concept") {
            labels.insert("template_or_constraint_text_near_error");
        }
    }
    labels
}

fn primary_category(labels: &BTreeSet<&str>) -> &'static str {
    for category in [
        "ambiguous_structure_diagnostic",
        "annotation_name_near_error",
        "uppercase_call_near_error",
        "conditional_directive_near_error",
        "preprocessor_near_error",
        "uppercase_identifier_near_error",
        "template_or_constraint_text_near_error",
        "pattern_or_boolean_text_near_error",
    ] {
        if labels.contains(category) {
            return category;
        }
    }
    "parse_other"
}

fn stable_hash(text: &str) -> u64 {
    text.as_bytes()
        .iter()
        .fold(0xcbf29ce484222325, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        })
}

fn create_details(path: &Path) -> Result<BufWriter<File>, String> {
    if !path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("jsonl"))
    {
        return Err(
            "diagnostic report must use .jsonl extension so it cannot become selected source"
                .into(),
        );
    }
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map(BufWriter::new)
        .map_err(|error| {
            format!(
                "cannot create diagnostic report {}: {error}",
                path.display()
            )
        })
}

fn write_record(writer: &mut dyn Write, record: &Value) -> Result<(), String> {
    serde_json::to_writer(&mut *writer, record)
        .map_err(|error| format!("cannot write diagnostic report: {error}"))?;
    writer
        .write_all(b"\n")
        .map_err(|error| format!("cannot write diagnostic report: {error}"))
}

fn flush_details(writer: &mut dyn Write) -> Result<(), String> {
    writer
        .flush()
        .map_err(|error| format!("cannot flush diagnostic report: {error}"))
}

fn main() -> Result<(), String> {
    let mut arguments = std::env::args_os().skip(1);
    let root = PathBuf::from(
        arguments
            .next()
            .ok_or("usage: failure_audit REPOSITORY [WORKERS [DIAGNOSTICS.jsonl]]")?,
    );
    let workers = arguments
        .next()
        .map(|value| {
            value
                .to_str()
                .ok_or("invalid workers")?
                .parse::<usize>()
                .map_err(|_| "invalid workers")
        })
        .transpose()?
        .unwrap_or(8);
    let details_path = arguments.next().map(PathBuf::from);
    if arguments.next().is_some() {
        return Err("usage: failure_audit REPOSITORY [WORKERS [DIAGNOSTICS.jsonl]]".into());
    }
    let options = ScanOptions {
        workers,
        ..Default::default()
    };
    let mut details = details_path.as_deref().map(create_details).transpose()?;
    let mut audit = Audit::default();
    let report = scan_with_diagnostics(&root, &options, &mut |diagnostic| {
        audit.observe(
            &root,
            options.max_file_bytes,
            diagnostic,
            details.as_mut().map(|writer| writer as &mut dyn Write),
        )
    })?;
    if let Some(details) = &mut details {
        flush_details(details)?;
    }
    let output = json!({
        "schema":"cpath.failure-audit.v2","root":root,"workers":workers,
        "diagnostic_report":details_path,
        "selected":report.selection.selected,"scored":report.files.len(),"unscored":report.unscored_files,
        "failure_counts":report.failure_counts.iter().map(|(kind, count)|
            (kind_name(*kind), count)).collect::<BTreeMap<_,_>>(),
        "methodology":{
            "coverage":"Every unscored file is observed; first parser diagnostic only, not every syntax error.",
            "classification":"Lexical context clues, not established root causes. Labels overlap; buckets use fixed precedence.",
            "structural_diagnostics":"ambiguous_structure_diagnostic means the diagnostic contains a C or C++ callable-declaration or file-scope-block guard. A combined header diagnostic may contain more than one grammar rejection.",
            "scope":"Exact test/benchmark path components are research labels; other_path is not proof of production code.",
            "samples":"Eight smallest FNV-1a path hashes per scope/category, with deterministic path tie break; not a statistical random sample.",
            "source":"Failed inputs are reread under the source size limit. A concurrent edit can change context; use pinned clean inputs.",
            "reread":"Symlinks and non-regular files are rejected before opening, but a replacement between metadata check and open remains a TOCTOU limitation. Unknown sizes are counted separately, not invented as zero-byte files.",
            "headers":"For .h diagnostics rejecting both C and C++, context uses the last C++ syntax-error location; the original dual diagnostic is preserved. This is a C++-oriented triage choice, not language detection.",
            "limits":"At most 8 samples per fixed scope/category and 1000 context characters per sample. No full source is retained.",
            "diagnostic_report":"Optional explicit JSONL output streams all failure records in completion order; compare by path, not line order. A failed run may leave a partial report and emits no successful summary. It is never reused as scan state.",
        },
        "audit":audit.json(),
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&output).map_err(|error| error.to_string())?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_overlapping_clues_and_header_context_uses_last_error() {
        let text = "#ifdef CONFIG\nLLVM_ABI void f();\nDECLARE_NODES(X)\n#endif";
        let labels = labels_for(text, text, false);
        assert!(labels.contains("annotation_name_near_error"));
        assert!(labels.contains("uppercase_call_near_error"));
        assert!(labels.contains("conditional_directive_near_error"));
        assert!(labels_for("", "_LIBCPP_BEGIN_NAMESPACE_STD", false)
            .contains("uppercase_identifier_near_error"));
        assert!(labels_for("", "class LLVM_LIBRARY_VISIBILITY C {};", false)
            .contains("annotation_name_near_error"));
        assert_eq!(primary_category(&labels), "annotation_name_near_error");
        assert_eq!(
            diagnostic_line("C syntax at line 1, column 1; C++ syntax at line 9, column 2"),
            Some(9)
        );
        assert_eq!(diagnostic_line("no tree"), None);
        assert_eq!(
            diagnostic_line(
                "C syntax at line 1, column 1; C++ callable declaration at line 12, column 4"
            ),
            Some(12)
        );
        assert_eq!(
            scope(Path::new("llvm/test/invalid.cpp")),
            "test_or_benchmark_path"
        );
        assert_eq!(scope(Path::new("llvm/lib/Contest.cpp")), "other_path");
    }

    #[test]
    fn structural_diagnostic_clue_recognizes_both_languages_and_guard_forms() {
        for form in [
            "C callable declaration",
            "C++ callable declaration",
            "C file-scope block",
            "C++ file-scope block",
        ] {
            assert!(ambiguous_structure_diagnostic(&format!(
                "unsupported or ambiguous {form} at line 2, column 1"
            )));
        }
        assert!(!ambiguous_structure_diagnostic(
            "unsupported or malformed C++ syntax at line 2, column 1"
        ));
    }

    #[test]
    fn bottom_hash_samples_are_bounded_and_independent_of_completion_order() {
        let paths: Vec<_> = (0..100).map(|n| format!("file{n}.cpp")).collect();
        let mut forward = Bucket::default();
        let mut backward = Bucket::default();
        for path in &paths {
            forward.add(path, Some(2), json!({"path":path}));
        }
        for path in paths.iter().rev() {
            backward.add(path, Some(2), json!({"path":path}));
        }
        assert_eq!(forward.json(), backward.json());
        assert_eq!(forward.samples.len(), SAMPLE_LIMIT);
        assert_eq!(forward.files, 100);
        assert_eq!(forward.bytes, 200);
    }

    #[test]
    fn context_is_bounded_before_building_a_string_even_for_long_unicode_lines() {
        let source = "λ".repeat(20_000);
        let context = error_context(&source, 1);
        assert_eq!(context.chars().count(), CONTEXT_CHARS);
        assert_eq!(context.len(), CONTEXT_CHARS * 2);
    }

    #[test]
    fn explicit_diagnostic_reports_never_overwrite_and_cannot_become_source() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("failures.jsonl");
        let record = json!({"path":"a path.cs","message":"line one\nline two"});
        let mut writer = create_details(&path).unwrap();
        write_record(&mut writer, &record).unwrap();
        flush_details(&mut writer).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(bytes.iter().filter(|byte| **byte == b'\n').count(), 1);
        assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), record);
        assert!(create_details(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert!(create_details(&directory.path().join("report.cs")).is_err());
    }

    #[test]
    fn diagnostic_write_and_flush_errors_are_propagated() {
        struct BrokenWriter {
            fail_on_flush: bool,
        }
        impl Write for BrokenWriter {
            fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
                if self.fail_on_flush {
                    Ok(buffer.len())
                } else {
                    Err(std::io::Error::other("deliberate write error"))
                }
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Err(std::io::Error::other("deliberate flush error"))
            }
        }
        let mut writer = BrokenWriter {
            fail_on_flush: false,
        };
        assert!(write_record(&mut writer, &json!({"path":"A.cs"}))
            .unwrap_err()
            .contains("cannot write"));
        let mut writer = BrokenWriter {
            fail_on_flush: true,
        };
        write_record(&mut writer, &json!({"path":"A.cs"})).unwrap();
        assert!(flush_details(&mut writer)
            .unwrap_err()
            .contains("cannot flush"));
    }

    #[cfg(unix)]
    #[test]
    fn replacement_symlinks_are_not_followed_during_context_rereading() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("target.cs");
        std::fs::write(&target, "class Secret {}").unwrap();
        let path = directory.path().join("replaced.cs");
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(read_context_source(&path, 100)
            .unwrap_err()
            .contains("symlink"));
        let diagnostic = FileDiagnostic {
            path: "replaced.cs".into(),
            kind: FailureKind::Parse,
            message: "unsupported C# syntax at line 1, column 1".into(),
        };
        let mut audit = Audit::default();
        audit
            .observe(directory.path(), 100, &diagnostic, None)
            .unwrap();
        assert_eq!(audit.reread_failures, 1);
        let bucket = &audit.buckets[&("other_path", "context_read_failed")];
        assert_eq!(bucket.unknown_size_files, 1);
        assert_eq!(bucket.samples.values().next().unwrap()["context"], "");
    }

    #[test]
    fn context_reads_remain_bounded_and_require_regular_files() {
        let directory = tempfile::tempdir().unwrap();
        assert!(read_context_source(directory.path(), 100).is_err());
        let path = directory.path().join("large.cs");
        std::fs::write(&path, [b'a'; 101]).unwrap();
        assert!(read_context_source(&path, 100)
            .unwrap_err()
            .contains("byte limit"));
    }
}
