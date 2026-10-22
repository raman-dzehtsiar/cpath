// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::bounded;

use crate::complexity::{CognitiveComplexity, ComplexityAnalyzer};
use crate::language::{BundledParser, SourceParser};
use crate::model::{ComplexitySummary, FileSummary};
use crate::scoring::{ComplexityFirst, ComplexityScore, Scorer};
use crate::selection::{Selection, SelectionStats, SkipReason};

#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub includes: Vec<String>,
    pub excludes: Vec<String>,
    pub workers: usize,
    pub max_file_bytes: u64,
    /// Bound retained summaries, not total scanned source size or exact RSS.
    pub max_summary_bytes: usize,
    pub max_files: usize,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            includes: Vec::new(),
            excludes: Vec::new(),
            workers: thread::available_parallelism()
                .map_or(1, usize::from)
                .min(8),
            max_file_bytes: 8 * 1024 * 1024,
            max_summary_bytes: 128 * 1024 * 1024,
            max_files: 100_000,
        }
    }
}

#[derive(Debug)]
pub struct RankedFile {
    pub path: PathBuf,
    /// Index into ScanReport.files and metric arrays.
    pub file_index: usize,
    pub score: ComplexityScore,
}

#[derive(Debug, Default)]
pub struct StageTimes {
    pub files: Duration,
    pub ranking: Duration,
    pub total: Duration,
}

/// Evidence is available to tests/developer tooling, not printed by the CLI.
pub struct ScanReport {
    pub root: PathBuf,
    pub files: Vec<FileSummary>,
    pub complexity: Vec<ComplexitySummary>,
    pub unscored_files: usize,
    /// Exact counts for every failed file, independent of diagnostic truncation.
    pub failure_counts: BTreeMap<FailureKind, usize>,
    pub diagnostics: Vec<FileDiagnostic>,
    pub ranked: Vec<RankedFile>,
    pub selection: SelectionStats,
    pub content_skips: BTreeMap<SkipReason, usize>,
    pub times: StageTimes,
}

/// The composition point. Algorithms can be replaced without changing scheduling.
pub struct Analyzers<'a> {
    pub complexity: &'a dyn ComplexityAnalyzer,
    pub scorer: &'a dyn Scorer,
}

pub fn scan(root: &Path, options: &ScanOptions) -> Result<ScanReport, String> {
    scan_with(
        root,
        options,
        Analyzers {
            complexity: &CognitiveComplexity,
            scorer: &ComplexityFirst,
        },
    )
}

pub fn scan_with(
    root: &Path,
    options: &ScanOptions,
    analyzers: Analyzers<'_>,
) -> Result<ScanReport, String> {
    scan_observed(root, options, analyzers, &mut |_| Ok(()))
}

/// Developer observability without retaining all diagnostics or changing CLI output.
/// The observer runs once per unscored file on the aggregation thread, in completion
/// order, and must not assume a stable order. Slow observers apply backpressure.
/// A fatal scan can still have delivered earlier per-file diagnostics.
/// Observer errors or panics fail the scan after draining the bounded queues.
pub fn scan_with_diagnostics(
    root: &Path,
    options: &ScanOptions,
    observer: &mut dyn FnMut(&FileDiagnostic) -> Result<(), String>,
) -> Result<ScanReport, String> {
    scan_observed(
        root,
        options,
        Analyzers {
            complexity: &CognitiveComplexity,
            scorer: &ComplexityFirst,
        },
        observer,
    )
}

fn scan_observed(
    root: &Path,
    options: &ScanOptions,
    analyzers: Analyzers<'_>,
    observer: &mut dyn FnMut(&FileDiagnostic) -> Result<(), String>,
) -> Result<ScanReport, String> {
    let start = Instant::now();
    if !(1..=64).contains(&options.workers) {
        return Err("worker count must be between 1 and 64".into());
    }
    if options.max_file_bytes == 0
        || options.max_file_bytes == u64::MAX
        || options.max_file_bytes > usize::MAX as u64
    {
        return Err("source byte limit must fit memory and allow a one-byte overflow check".into());
    }
    let selection = Selection::new(root, &options.includes, &options.excludes)?;
    let root = root
        .canonicalize()
        .map_err(|e| format!("cannot resolve {}: {e}", root.display()))?;
    let capacity = options.workers * 2;
    let (jobs_tx, jobs_rx) = bounded(capacity);
    let (results_tx, results_rx) = bounded(capacity);
    let mut parsed = Vec::new();
    let mut diagnostics = BTreeSet::new();
    let mut unscored_files = 0usize;
    let mut failure_counts = BTreeMap::new();
    let mut observer_failed = false;
    let mut fatal = None;
    let mut content_skips = BTreeMap::new();
    let mut summary_bytes = 0usize;
    let mut retained_files = 0usize;
    let mut budget_exceeded = false;
    let discovery = thread::scope(|scope| {
        let selection_ref = &selection;
        let producer = scope.spawn(move || {
            selection_ref.walk(|path| {
                jobs_tx
                    .send(path)
                    .map_err(|_| "analysis workers stopped".into())
            })
        });
        let mut workers = Vec::new();
        for _ in 0..options.workers {
            let receiver = jobs_rx.clone();
            let sender = results_tx.clone();
            let root = &root;
            let selection = &selection;
            let complexity = analyzers.complexity;
            let scorer = analyzers.scorer;
            workers.push(scope.spawn(move || {
                let mut parser = match BundledParser::new() {
                    Ok(parser) => parser,
                    Err(error) => {
                        let _ = sender.send(Outcome::Fatal(error));
                        return;
                    }
                };
                for path in receiver {
                    let result = analyze_file(
                        &path,
                        root,
                        selection,
                        options,
                        &mut parser,
                        complexity,
                        scorer,
                    );
                    if sender.send(result).is_err() {
                        break;
                    }
                }
            }));
        }
        drop(jobs_rx);
        drop(results_tx);
        for result in results_rx {
            match result {
                Outcome::Parsed(file, metric, score) => {
                    summary_bytes = summary_bytes.saturating_add(summary_size(&file, &metric));
                    retained_files = retained_files.saturating_add(1);
                    if summary_bytes > options.max_summary_bytes
                        || retained_files > options.max_files
                    {
                        budget_exceeded = true;
                        parsed.clear();
                    } else if !budget_exceeded {
                        parsed.push((file, metric, score));
                    }
                }
                Outcome::Skipped(reason) => *content_skips.entry(reason).or_default() += 1,
                Outcome::Unscored(diagnostic) => {
                    unscored_files += 1;
                    *failure_counts.entry(diagnostic.kind).or_default() += 1;
                    if !observer_failed {
                        let observed =
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                observer(&diagnostic)
                            }));
                        let error = match observed {
                            Ok(Ok(())) => None,
                            Ok(Err(error)) => Some(format!("diagnostic observer failed: {error}")),
                            Err(_) => Some("diagnostic observer panicked".into()),
                        };
                        if let Some(error) = error {
                            // Drain bounded queues before joining workers even when tooling
                            // fails. Unwinding the aggregation loop could block their sends.
                            observer_failed = true;
                            fatal = Some(error);
                        }
                    }
                    diagnostics.insert(diagnostic);
                    if diagnostics.len() > 32 {
                        diagnostics.pop_last();
                    }
                }
                Outcome::Fatal(error) => fatal = Some(error),
            }
        }
        for worker in workers {
            if worker.join().is_err() {
                fatal = Some("file analysis worker panicked".into());
            }
        }
        producer
            .join()
            .map_err(|_| "file discovery worker panicked".to_string())?
    });
    if let Some(error) = fatal {
        return Err(error);
    }
    let selection = discovery?;
    if budget_exceeded {
        return Err(format!(
            "scan exceeds summary budget ({} bytes or {} files); narrow with --include/--exclude",
            options.max_summary_bytes, options.max_files
        ));
    }
    parsed.sort_by_cached_key(|entry| (normalized_path(&entry.0.path), entry.0.path.clone()));
    let file_time = start.elapsed();
    let ranking_start = Instant::now();
    let mut files = Vec::with_capacity(parsed.len());
    let mut complexity = Vec::with_capacity(parsed.len());
    let mut ranked = Vec::with_capacity(parsed.len());
    for (file, metric, score) in parsed {
        ranked.push(RankedFile {
            path: file.path.clone(),
            file_index: files.len(),
            score,
        });
        files.push(file);
        complexity.push(metric);
    }
    // File indexes already follow normalized path order, independent of worker completion.
    ranked.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.file_index.cmp(&b.file_index))
    });
    let ranking_time = ranking_start.elapsed();
    Ok(ScanReport {
        root,
        files,
        complexity,
        unscored_files,
        failure_counts,
        diagnostics: diagnostics.into_iter().collect(),
        ranked,
        selection,
        content_skips,
        times: StageTimes {
            files: file_time,
            ranking: ranking_time,
            total: start.elapsed(),
        },
    })
}

/// Failure stage, not a claim that the source itself is invalid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FailureKind {
    Read,
    Size,
    Encoding,
    Binary,
    Parse,
}

/// A selected file that could not be scored; never assigned a zero score.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct FileDiagnostic {
    pub path: PathBuf,
    pub message: String,
    pub kind: FailureKind,
}

enum Outcome {
    Parsed(FileSummary, ComplexitySummary, ComplexityScore),
    Skipped(SkipReason),
    Unscored(FileDiagnostic),
    Fatal(String),
}

fn analyze_file(
    path: &Path,
    root: &Path,
    selection: &Selection,
    options: &ScanOptions,
    parser: &mut dyn SourceParser,
    complexity: &dyn ComplexityAnalyzer,
    scorer: &dyn Scorer,
) -> Outcome {
    let relative = path.strip_prefix(root).unwrap_or(path).to_path_buf();
    let result = (|| {
        let metadata =
            std::fs::symlink_metadata(path).map_err(|e| (FailureKind::Read, e.to_string()))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err((
                FailureKind::Read,
                "selected file changed into a symlink or non-regular file".into(),
            ));
        }
        if metadata.len() > options.max_file_bytes {
            return Err((
                FailureKind::Size,
                format!("exceeds {}-byte source limit", options.max_file_bytes),
            ));
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        File::open(path)
            .and_then(|file| {
                file.take(options.max_file_bytes + 1)
                    .read_to_end(&mut bytes)
            })
            .map_err(|e| (FailureKind::Read, e.to_string()))?;
        if bytes.len() as u64 > options.max_file_bytes {
            return Err((FailureKind::Size, "source grew beyond byte limit".into()));
        }
        let source = crate::source::decode(&bytes).map_err(|e| (FailureKind::Encoding, e))?;
        if let Some(reason) = selection.classify_content(&relative, source.as_bytes()) {
            if matches!(reason, SkipReason::Binary | SkipReason::InvalidUtf8) {
                let kind = if reason == SkipReason::Binary {
                    FailureKind::Binary
                } else {
                    FailureKind::Encoding
                };
                return Err((kind, format!("unsupported source content: {reason:?}")));
            }
            return Ok(Outcome::Skipped(reason));
        }
        let facts = parser
            .parse(relative.clone(), &source)
            .map_err(|e| (FailureKind::Parse, e))?;
        let mut metric = complexity.analyze(&facts);
        // Score the complete method set before reducing evidence. Syntax events,
        // source buffers and trees stay worker-local and die at this boundary.
        let score = scorer.score(&metric);
        metric
            .methods
            .sort_by(|a, b| b.score.cmp(&a.score).then(a.start_line.cmp(&b.start_line)));
        metric.methods.truncate(5);
        metric.methods.shrink_to_fit();
        let file = FileSummary {
            path: facts.path,
            bytes: bytes.len() as u64,
            lines: facts.lines,
        };
        let summary = ComplexitySummary {
            max_method: metric.max_method,
            total: metric.total,
            methods: metric.methods,
        };
        Ok(Outcome::Parsed(file, summary, score))
    })();
    result.unwrap_or_else(|(kind, message)| {
        Outcome::Unscored(FileDiagnostic {
            path: relative,
            message,
            kind,
        })
    })
}

fn summary_size(file: &FileSummary, metric: &ComplexitySummary) -> usize {
    // Conservative bookkeeping estimate for the two stored path copies and
    // bounded method evidence. Allocator overhead/parser working memory excluded.
    let methods = metric.methods.iter().fold(0usize, |sum, m| {
        sum.saturating_add(std::mem::size_of_val(m).saturating_add(m.name.len()))
    });
    std::mem::size_of::<FileSummary>()
        + std::mem::size_of::<ComplexitySummary>()
        + std::mem::size_of::<RankedFile>()
        + file.path.as_os_str().len().saturating_mul(4)
        + methods
}

fn normalized_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_removed_after_selection_is_a_read_failure() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let selection = Selection::new(root, &[], &[]).unwrap();
        let mut parser = BundledParser::new().unwrap();
        let outcome = analyze_file(
            &root.join("removed.cs"),
            root,
            &selection,
            &ScanOptions::default(),
            &mut parser,
            &CognitiveComplexity,
            &ComplexityFirst,
        );
        let Outcome::Unscored(diagnostic) = outcome else {
            panic!("removed input must be unscored");
        };
        assert_eq!(diagnostic.kind, FailureKind::Read);
        assert_eq!(diagnostic.path, Path::new("removed.cs"));
    }
}
