// SPDX-License-Identifier: MIT

//! Bounded, stateless C# research scans; no production ranking/output changes.
mod local_review_obligations;
mod syntax_signals;

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Instant;

use cpath::complexity::{CognitiveComplexity, ComplexityAnalyzer};
use cpath::language::CSharpParser;
use cpath::model::{FileFacts, SourceLanguage};
use cpath::pipeline::ScanOptions;
use cpath::selection::{language_for_path, Selection, SelectionStats, SkipReason};
use crossbeam_channel::bounded;
use serde_json::{json, Value};
use syntax_signals::MethodSignals;

const VARIANTS: [&str; 12] = [
    "C", "B", "K", "CT", "KT", "CH", "KH", "CTH", "KTH", "KQH", "KTHA", "CS",
];
const RAW_FEATURES: [&str; 25] = [
    "baseline",
    "event_count",
    "nesting_sum",
    "peak_ancestry",
    "peak_nesting",
    "span_lines",
    "body_span_lines",
    "parameter_count",
    "statement_count",
    "awaits",
    "locks",
    "usings",
    "finallys",
    "yields",
    "throws",
    "catches",
    "returns",
    "writes",
    "distinct_write_targets",
    "repeated_writes",
    "explicit_this_writes",
    "explicit_this_repeats",
    "interlocked_calls",
    "volatile_calls",
    "monitor_calls",
];

fn ceil_sqrt(value: u32) -> u32 {
    let (mut low, mut high) = (0u32, 65_536u32);
    while low < high {
        let middle = low + (high - low) / 2;
        if u64::from(middle) * u64::from(middle) < u64::from(value) {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    low
}

fn obligations(m: &MethodSignals) -> u32 {
    [m.awaits, m.locks, m.usings, m.finallys, m.yields, m.throws]
        .into_iter()
        .map(|n| u32::from(n > 0))
        .sum()
}

fn api_names(m: &MethodSignals) -> u32 {
    [m.interlocked_calls, m.volatile_calls, m.monitor_calls]
        .into_iter()
        .map(|n| u32::from(n > 0))
        .sum()
}

fn formulas(m: &MethodSignals) -> [u32; 12] {
    let c = m.baseline;
    let b = m.nesting_sum.saturating_add(ceil_sqrt(m.event_count));
    let k = m
        .peak_ancestry
        .saturating_add(ceil_sqrt(c.saturating_sub(m.peak_ancestry)));
    let t = ceil_sqrt(m.explicit_this_repeats);
    let q = ceil_sqrt(m.repeated_writes);
    let h = obligations(m);
    let a = api_names(m);
    let add = u32::saturating_add;
    [
        c,
        b,
        k,
        add(c, t),
        add(k, t),
        add(c, h),
        add(k, h),
        add(add(c, t), h),
        add(add(k, t), h),
        add(add(k, q), h),
        add(add(add(k, t), h), a),
        add(c, ceil_sqrt(m.statement_count)),
    ]
}

fn raw_features(m: &MethodSignals) -> [u32; 25] {
    [
        m.baseline,
        m.event_count,
        m.nesting_sum,
        m.peak_ancestry,
        m.peak_nesting,
        m.span_lines,
        m.body_span_lines,
        m.parameter_count,
        m.statement_count,
        m.awaits,
        m.locks,
        m.usings,
        m.finallys,
        m.yields,
        m.throws,
        m.catches,
        m.returns,
        m.writes,
        m.distinct_write_targets,
        m.repeated_writes,
        m.explicit_this_writes,
        m.explicit_this_repeats,
        m.interlocked_calls,
        m.volatile_calls,
        m.monitor_calls,
    ]
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Score {
    maximum: u32,
    burden: u64,
}

impl Score {
    fn observe(&mut self, score: u32) {
        self.maximum = self.maximum.max(score);
        self.burden = self
            .burden
            .saturating_add(u64::from(score.saturating_sub(15)));
    }
}

struct Evidence {
    name: String,
    start: u32,
    end: u32,
    signals: MethodSignals,
    lowc: Option<Box<local_review_obligations::MethodInteractions>>,
}

impl Evidence {
    fn lowc_priority(&self) -> (u8, usize) {
        match &self.lowc {
            None if self.signals.baseline <= 5 && self.signals.span_lines <= 40 => (1, 0),
            Some(facts)
                if self.signals.baseline <= 5
                    && self.signals.span_lines <= 40
                    && !facts.incomplete() =>
            {
                (
                    if facts.has_findings() { 2 } else { 1 },
                    facts.counts.iter().filter(|n| **n > 0).count(),
                )
            }
            _ => (0, 0),
        }
    }
}

#[derive(Default)]
struct LowCStats {
    eligible_methods: usize,
    eligible_with_findings: usize,
    eligible_incomplete: usize,
    incomplete_methods: usize,
    rule_methods: [usize; 6],
    abstentions: BTreeMap<&'static str, usize>,
}

impl LowCStats {
    fn json(&self) -> Value {
        json!({"eligible_methods":self.eligible_methods,"eligible_with_findings":self.eligible_with_findings,
            "eligible_incomplete":self.eligible_incomplete,"incomplete_methods":self.incomplete_methods,
            "rule_methods":local_review_obligations::RULES.into_iter().zip(self.rule_methods).collect::<BTreeMap<_,_>>(),
            "abstentions":self.abstentions})
    }
}

struct FileRecord {
    path: String,
    bytes: u64,
    lines: u32,
    method_count: usize,
    baseline_max: u32,
    baseline_total: u32,
    scores: [Score; 12],
    feature_maxima: [u32; 25],
    max_obligations: u32,
    max_api_names: u32,
    methods: Vec<Evidence>,
    lowc: Option<Box<LowCStats>>,
}

impl FileRecord {
    fn json(&self) -> Value {
        let scores: BTreeMap<_, _> = VARIANTS
            .into_iter()
            .zip(self.scores)
            .map(|(name, s)| (name, json!({"maximum":s.maximum,"burden":s.burden})))
            .collect();
        let mut features: BTreeMap<_, _> =
            RAW_FEATURES.into_iter().zip(self.feature_maxima).collect();
        for (alias, index) in [("E", 1), ("arity", 7), ("S", 8), ("T", 21), ("Q", 19)] {
            features.insert(alias, self.feature_maxima[index]);
        }
        features.insert("H", self.max_obligations);
        features.insert("A", self.max_api_names);
        let mut result = json!({
            "path":self.path,"bytes":self.bytes,"lines":self.lines,
            "method_count":self.method_count,"baseline_max":self.baseline_max,
            "baseline_total":self.baseline_total,"scores":scores,"features":features,
            "methods":self.methods.iter().map(|m| {
                let mut value = json!({
                "name":m.name,"start":m.start,"end":m.end,"signals":m.signals.json(),
                "scores":VARIANTS.into_iter().zip(formulas(&m.signals)).collect::<BTreeMap<_,_>>()
                });
                if self.lowc.is_some() {
                    value["lowc"] = m.lowc.as_ref().map_or_else(|| local_review_obligations::MethodInteractions::default().json(), |facts| facts.json());
                }
                value
            }).collect::<Vec<_>>()
        });
        if let Some(facts) = &self.lowc {
            result["lowc"] = facts.json();
        }
        result
    }

    fn retained_bytes(&self) -> usize {
        // Conservative typed-summary bookkeeping; no claim of exact allocator RSS.
        // Fixed additive vector headroom covers initial allocation and growth.
        // Charging current spare capacity would make near-limit success depend
        // on which large/small records finish first. Include path conversion,
        // BTree/allocator overhead, location buffers, and method-name capacity.
        let mut size = std::mem::size_of::<Self>()
            .saturating_mul(4)
            .saturating_add(self.path.capacity().saturating_mul(4))
            .saturating_add(
                self.methods
                    .capacity()
                    .saturating_mul(std::mem::size_of::<Evidence>()),
            )
            .saturating_add(128);
        for m in &self.methods {
            size = size.saturating_add(m.name.capacity()).saturating_add(64);
            if let Some(facts) = &m.lowc {
                size = size
                    .saturating_add(facts.retained_bytes())
                    .saturating_add(64);
            }
            for (name, lines) in &m.signals.locations {
                size = size
                    .saturating_add(128 + name.len())
                    .saturating_add(lines.capacity().saturating_mul(std::mem::size_of::<u32>()));
            }
        }
        if let Some(facts) = &self.lowc {
            size = size
                .saturating_add(std::mem::size_of::<LowCStats>())
                .saturating_add(facts.abstentions.len() * 128 + 64);
        }
        size
    }
}

fn summarize(
    facts: FileFacts,
    signals: Vec<MethodSignals>,
    bytes: u64,
    evidence: usize,
) -> Result<FileRecord, String> {
    summarize_with_lowc(facts, signals, bytes, evidence, None)
}

fn summarize_with_lowc(
    facts: FileFacts,
    signals: Vec<MethodSignals>,
    bytes: u64,
    evidence: usize,
    interactions: Option<Vec<local_review_obligations::MethodInteractions>>,
) -> Result<FileRecord, String> {
    let metric = CognitiveComplexity.analyze(&facts);
    if facts.methods.len() != signals.len() || metric.methods.len() != signals.len() {
        return Err("research extraction changed callable count".into());
    }
    if interactions
        .as_ref()
        .is_some_and(|items| items.len() != signals.len())
    {
        return Err("low-C extraction changed callable count".into());
    }
    let lowc_enabled = interactions.is_some();
    let mut interactions = interactions.map(Vec::into_iter);
    let mut record = FileRecord {
        path: normalized(&facts.path)?,
        bytes,
        lines: facts.lines,
        method_count: signals.len(),
        baseline_max: metric.max_method,
        baseline_total: metric.total,
        scores: [Score::default(); 12],
        feature_maxima: [0; 25],
        max_obligations: 0,
        max_api_names: 0,
        methods: Vec::with_capacity(5),
        lowc: lowc_enabled.then(|| Box::new(LowCStats::default())),
    };
    for ((facts, metric), signals) in facts.methods.into_iter().zip(metric.methods).zip(signals) {
        if metric.score != signals.baseline
            || signals.baseline != signals.event_count.saturating_add(signals.nesting_sum)
            || signals.peak_ancestry > signals.baseline
        {
            return Err(format!(
                "research cognitive parity failed at {}:{}",
                record.path, facts.start_line
            ));
        }
        let scores = formulas(&signals);
        for (aggregate, score) in record.scores.iter_mut().zip(scores) {
            aggregate.observe(score);
        }
        for (maximum, value) in record.feature_maxima.iter_mut().zip(raw_features(&signals)) {
            *maximum = (*maximum).max(value);
        }
        record.max_obligations = record.max_obligations.max(obligations(&signals));
        record.max_api_names = record.max_api_names.max(api_names(&signals));
        let lowc = interactions.as_mut().and_then(Iterator::next).map(Box::new);
        if let (Some(summary), Some(facts)) = (&mut record.lowc, &lowc) {
            summary.incomplete_methods += usize::from(facts.incomplete());
            for (reason, count) in &facts.abstentions {
                *summary.abstentions.entry(reason).or_default() += *count as usize;
            }
            if signals.baseline <= 5 && signals.span_lines <= 40 {
                summary.eligible_methods += 1;
                summary.eligible_incomplete += usize::from(facts.incomplete());
                if !facts.incomplete() {
                    summary.eligible_with_findings += usize::from(facts.has_findings());
                    for (count, hits) in summary.rule_methods.iter_mut().zip(facts.counts) {
                        *count += usize::from(hits > 0);
                    }
                }
            }
        }
        // Empty complete facts have an implicit zero representation. This keeps
        // negative-control evidence without a heap allocation per retained method.
        let lowc = lowc.filter(|facts| facts.has_findings() || !facts.abstentions.is_empty());
        record.methods.push(Evidence {
            name: facts.name,
            start: facts.start_line,
            end: facts.end_line,
            signals,
            lowc,
        });
        record.methods.sort_by(|a, b| {
            if lowc_enabled {
                return b
                    .lowc_priority()
                    .cmp(&a.lowc_priority())
                    .then(a.start.cmp(&b.start))
                    .then(a.name.cmp(&b.name))
                    .then(a.end.cmp(&b.end));
            }
            formulas(&b.signals)[evidence]
                .cmp(&formulas(&a.signals)[evidence])
                .then(a.start.cmp(&b.start))
                .then(a.name.cmp(&b.name))
                .then(a.end.cmp(&b.end))
        });
        record.methods.truncate(5);
    }
    record.methods.shrink_to_fit();
    Ok(record)
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Diagnostic {
    path: String,
    kind: &'static str,
    message: String,
}

enum Outcome {
    Scored(Box<FileRecord>),
    Skipped(SkipReason),
    Unscored(Diagnostic),
    Fatal(String),
}

fn analyze_file(
    path: &Path,
    root: &Path,
    selection: &Selection,
    options: &ScanOptions,
    parser: &mut CSharpParser,
    evidence: usize,
    lowc: bool,
) -> Outcome {
    let relative = path.strip_prefix(root).unwrap_or(path).to_path_buf();
    let normalized_relative = match normalized(&relative) {
        Ok(path) => path,
        Err(error) => return Outcome::Fatal(error),
    };
    let read = (|| {
        let metadata = std::fs::symlink_metadata(path).map_err(|e| ("Read", e.to_string()))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err((
                "Read",
                "selected file changed into a symlink or non-regular file".into(),
            ));
        }
        if metadata.len() > options.max_file_bytes {
            return Err((
                "Size",
                format!("exceeds {}-byte source limit", options.max_file_bytes),
            ));
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        File::open(path)
            .and_then(|file| {
                file.take(options.max_file_bytes + 1)
                    .read_to_end(&mut bytes)
            })
            .map_err(|e| ("Read", e.to_string()))?;
        if bytes.len() as u64 > options.max_file_bytes {
            return Err(("Size", "source grew beyond byte limit".into()));
        }
        Ok(bytes)
    })();
    let unscored = |kind, message| {
        Outcome::Unscored(Diagnostic {
            path: normalized_relative.clone(),
            kind,
            message,
        })
    };
    let bytes = match read {
        Ok(bytes) => bytes,
        Err((kind, message)) => return unscored(kind, message),
    };
    let source = match cpath::source::decode(&bytes) {
        Ok(source) => source,
        Err(message) => return unscored("Encoding", message),
    };
    if let Some(reason) = selection.classify_content(&relative, source.as_bytes()) {
        return match reason {
            SkipReason::Binary => {
                unscored("Binary", format!("unsupported source content: {reason:?}"))
            }
            SkipReason::InvalidUtf8 => unscored(
                "Encoding",
                format!("unsupported source content: {reason:?}"),
            ),
            _ => Outcome::Skipped(reason),
        };
    }
    if language_for_path(&relative) != Some(SourceLanguage::CSharp) {
        return unscored(
            "ResearchUnsupportedLanguage",
            "syntax signals support C# only".into(),
        );
    }
    let mut collector = local_review_obligations::Collector::default();
    let extracted = if lowc {
        syntax_signals::extract_observed(parser, relative.clone(), &source, |node, owner| {
            collector.observe(node, owner, &source)
        })
    } else {
        syntax_signals::extract(parser, relative.clone(), &source)
    };
    let (facts, signals) = match extracted {
        Ok(result) => result,
        Err(message) => return unscored("Parse", message),
    };
    let result = if lowc {
        let interactions = collector.finish(facts.methods.len());
        summarize_with_lowc(
            facts,
            signals,
            bytes.len() as u64,
            evidence,
            Some(interactions),
        )
    } else {
        summarize(facts, signals, bytes.len() as u64, evidence)
    };
    match result {
        Ok(record) => Outcome::Scored(Box::new(record)),
        Err(error) => Outcome::Fatal(error),
    }
}

fn normalized(path: &Path) -> Result<String, String> {
    path.to_str()
        .map(|path| path.replace(std::path::MAIN_SEPARATOR, "/"))
        .ok_or_else(|| "research JSON requires UTF-8 paths; refusing a lossy path identity".into())
}

struct Report {
    root: PathBuf,
    files: Vec<FileRecord>,
    unscored: usize,
    failure_counts: BTreeMap<&'static str, usize>,
    diagnostics: BTreeSet<Diagnostic>,
    selection: SelectionStats,
    content_skips: BTreeMap<SkipReason, usize>,
    seconds: f64,
    lowc: bool,
}

fn scan(root: &Path, options: &ScanOptions, evidence: usize) -> Result<Report, String> {
    scan_mode(root, options, evidence, false)
}

fn scan_mode(
    root: &Path,
    options: &ScanOptions,
    evidence: usize,
    lowc: bool,
) -> Result<Report, String> {
    if !(1..=2).contains(&options.workers) {
        return Err("research workers must be 1 or 2".into());
    }
    if evidence >= VARIANTS.len() {
        return Err("unknown evidence formula".into());
    }
    if options.max_file_bytes == 0 || options.max_file_bytes >= usize::MAX as u64 {
        return Err("invalid source byte budget".into());
    }
    let start = Instant::now();
    let selection = Selection::new(root, &options.includes, &options.excludes)?;
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    normalized(&root)?;
    let (jobs_tx, jobs_rx) = bounded::<PathBuf>(2 * options.workers);
    let (results_tx, results_rx) = bounded(2 * options.workers);
    let mut files = Vec::new();
    let mut diagnostics = BTreeSet::new();
    let mut unscored = 0usize;
    let mut failure_counts = BTreeMap::new();
    let mut content_skips = BTreeMap::new();
    let mut summary_bytes = 0usize;
    let mut analyzed = 0usize;
    let mut fatal = None;
    let discovery = thread::scope(|scope| {
        let selection_ref = &selection;
        let producer = scope.spawn(move || {
            selection_ref.walk(|path| {
                jobs_tx
                    .send(path)
                    .map_err(|_| "research workers stopped".into())
            })
        });
        let mut workers = Vec::new();
        for _ in 0..options.workers {
            let receiver = jobs_rx.clone();
            let sender = results_tx.clone();
            let root = &root;
            let selection = &selection;
            workers.push(scope.spawn(move || {
                let mut parser = match CSharpParser::new() {
                    Ok(parser) => parser,
                    Err(error) => {
                        let _ = sender.send(Outcome::Fatal(error));
                        return;
                    }
                };
                for path in receiver {
                    if sender
                        .send(analyze_file(
                            &path,
                            root,
                            selection,
                            options,
                            &mut parser,
                            evidence,
                            lowc,
                        ))
                        .is_err()
                    {
                        break;
                    }
                }
            }));
        }
        drop(jobs_rx);
        drop(results_tx);
        // Always drain before joining, including after aggregate/fatal failure.
        for result in results_rx {
            match result {
                Outcome::Scored(record) => {
                    analyzed = analyzed.saturating_add(1);
                    summary_bytes = summary_bytes.saturating_add(record.retained_bytes());
                    if analyzed > options.max_files || summary_bytes > options.max_summary_bytes {
                        fatal.get_or_insert_with(|| {
                            "research scan exceeds aggregate summary budget".into()
                        });
                        files.clear();
                    } else if fatal.is_none() {
                        files.push(*record);
                    }
                }
                Outcome::Skipped(reason) => *content_skips.entry(reason).or_default() += 1,
                Outcome::Unscored(diagnostic) => {
                    unscored += 1;
                    *failure_counts.entry(diagnostic.kind).or_default() += 1;
                    diagnostics.insert(diagnostic);
                    if diagnostics.len() > 32 {
                        diagnostics.pop_last();
                    }
                }
                Outcome::Fatal(error) => {
                    fatal.get_or_insert(error);
                    files.clear();
                }
            }
        }
        for worker in workers {
            if worker.join().is_err() {
                fatal.get_or_insert_with(|| "research worker panicked".into());
            }
        }
        producer
            .join()
            .map_err(|_| "research discovery worker panicked".to_string())?
    });
    if let Some(error) = fatal {
        return Err(error);
    }
    let selection = discovery?;
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Report {
        root,
        files,
        unscored,
        failure_counts,
        diagnostics,
        selection,
        content_skips,
        seconds: start.elapsed().as_secs_f64(),
        lowc,
    })
}

fn write_report(
    report: &Report,
    options: &ScanOptions,
    evidence: usize,
    output: &mut impl Write,
) -> Result<(), String> {
    let mut header = json!({
        "schema":"cpath.research.signals.v1",
        "formula_family":"cpath.research.phase2-formulas.v1","formula_names":VARIANTS,
        "root":report.root,"workers":options.workers,
        "includes":options.includes,"excludes":options.excludes,
        "evidence_formula":VARIANTS[evidence],"analyzed_files":report.files.len(),
        "unscored_files":report.unscored,"failure_counts":report.failure_counts,
        "diagnostics_omitted":report.unscored.saturating_sub(report.diagnostics.len()),
        "diagnostics":report.diagnostics.iter().map(|d| json!({"path":d.path,"kind":d.kind,"message":d.message})).collect::<Vec<_>>(),
        "selection":{"candidates":report.selection.candidates,"selected":report.selection.selected,
            "skipped":report.selection.skipped.iter().map(|(k,v)|(format!("{k:?}"),v)).collect::<BTreeMap<_,_>>()},
        "content_skips":report.content_skips.iter().map(|(k,v)|(format!("{k:?}"),v)).collect::<BTreeMap<_,_>>(),
        "seconds":{"total":report.seconds,"file_pipeline":report.seconds},
        "syntax_notes":"P is AST ancestry, not an execution path; T is explicit this.member spelling; Q is unresolved simple target spelling; H is syntax-kind presence; A is unresolved invocation-receiver spelling. Evidence contains only five callables; all scores/features use every callable."
    });
    if report.lowc {
        header["lowc_contract"] = json!("cpath.research.lowc-obligations.v1");
        header["evidence_formula"] = Value::Null;
        header["evidence_selection"] = json!("At most five callables: complete C<=5/span<=40 first, rule hits then distinct rule count then source location; all formula scores and low-C totals computed before truncation.");
    }
    let mut prefix = serde_json::to_string(&header).map_err(|e| e.to_string())?;
    prefix.pop(); // The object header ends in }, replaced by its streamed files member.
    output
        .write_all(prefix.as_bytes())
        .map_err(|e| e.to_string())?;
    output
        .write_all(b",\"files\":[")
        .map_err(|e| e.to_string())?;
    for (i, file) in report.files.iter().enumerate() {
        if i > 0 {
            output.write_all(b",").map_err(|e| e.to_string())?;
        }
        serde_json::to_writer(&mut *output, &file.json()).map_err(|e| e.to_string())?;
    }
    output
        .write_all(b"]}\n")
        .and_then(|()| output.flush())
        .map_err(|e| e.to_string())
}

fn main() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let root = PathBuf::from(args.next().ok_or("usage: syntax_signal_experiment REPOSITORY WORKERS [--evidence VARIANT | --lowc] [--include GLOB | --exclude GLOB]...")?);
    let workers = args.next().ok_or("WORKERS is required")?;
    let mut scan_args = vec![
        root.clone().into_os_string(),
        "--top".into(),
        "10".into(),
        "--threads".into(),
        workers,
    ];
    let mut evidence = 0;
    let mut evidence_seen = false;
    let mut lowc = false;
    while let Some(arg) = args.next() {
        if arg == "--lowc" {
            if lowc {
                return Err("--lowc may appear once".into());
            }
            lowc = true;
        } else if arg == "--evidence" {
            if evidence_seen {
                return Err("--evidence may appear once".into());
            }
            evidence_seen = true;
            let value = args.next().ok_or("--evidence requires a variant")?;
            evidence = VARIANTS
                .iter()
                .position(|name| value == *name)
                .ok_or("unknown evidence variant")?;
        } else {
            scan_args.push(arg);
        }
    }
    let cpath::cli::Command::Scan { options, .. } = cpath::cli::parse(scan_args)? else {
        return Err("expected scan options".into());
    };
    if lowc && evidence_seen {
        return Err("--lowc owns evidence selection; do not combine with --evidence".into());
    }
    let report = if lowc {
        scan_mode(&root, &options, evidence, true)?
    } else {
        scan(&root, &options, evidence)?
    };
    write_report(&report, &options, evidence, &mut std::io::stdout().lock())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowc_evidence_keeps_complete_scores_and_counts_before_reduction() {
        let source = "class C { void A(){ } void B(){ } void Cc(){ } void D(){ } void E(){ } void F(){ } void Rare(){ try{}catch{} } void High(bool a){if(a){if(a){if(a){if(a){}}}}} }";
        let mut parser = CSharpParser::new().unwrap();
        let mut collector = local_review_obligations::Collector::default();
        let (facts, signals) = syntax_signals::extract_observed(
            &mut parser,
            "sample.cs".into(),
            source,
            |node, owner| collector.observe(node, owner, source),
        )
        .unwrap();
        let interactions = collector.finish(facts.methods.len());
        let record =
            summarize_with_lowc(facts, signals, source.len() as u64, 0, Some(interactions))
                .unwrap();
        assert_eq!(record.method_count, 8);
        assert_eq!(record.methods.len(), 5);
        assert_eq!(record.methods[0].name, "Rare");
        assert_eq!(record.scores[0].maximum, 10);
        let summary = record.lowc.as_ref().unwrap();
        assert_eq!(summary.eligible_methods, 7);
        assert_eq!(summary.eligible_with_findings, 1);
        assert_eq!(summary.rule_methods, [0, 0, 0, 1, 0, 0]);
        let (facts, signals) =
            syntax_signals::extract(&mut parser, "sample.cs".into(), source).unwrap();
        let ordinary = summarize(facts, signals, source.len() as u64, 0).unwrap();
        assert_eq!(ordinary.scores, record.scores);
        assert!(ordinary.json().get("lowc").is_none());
    }

    #[test]
    fn integer_square_root_boundaries_and_formula_hand_counts() {
        for (value, expected) in [
            (0, 0),
            (1, 1),
            (2, 2),
            (4, 2),
            (5, 3),
            (16, 4),
            (17, 5),
            (u32::MAX, 65_536),
        ] {
            assert_eq!(ceil_sqrt(value), expected);
        }
        let deep = MethodSignals {
            baseline: 10,
            event_count: 4,
            nesting_sum: 6,
            peak_ancestry: 10,
            ..Default::default()
        };
        assert_eq!(&formulas(&deep)[..3], &[10, 8, 10]);
        let repeat = MethodSignals {
            baseline: 48,
            event_count: 32,
            nesting_sum: 16,
            peak_ancestry: 3,
            ..Default::default()
        };
        assert_eq!(&formulas(&repeat)[..3], &[48, 22, 10]);
        let state = MethodSignals {
            explicit_this_repeats: 5,
            repeated_writes: 9,
            awaits: 9,
            locks: 2,
            interlocked_calls: 4,
            ..Default::default()
        };
        assert_eq!(formulas(&state), [0, 0, 0, 3, 3, 2, 2, 5, 5, 5, 6, 0]);
        let mut accumulated = Score::default();
        accumulated.observe(u32::MAX);
        accumulated.observe(u32::MAX);
        assert_eq!(accumulated.maximum, u32::MAX);
        assert_eq!(accumulated.burden, 2 * u64::from(u32::MAX - 15));
    }

    #[test]
    fn all_callables_contribute_before_evidence_reduction_and_winner_can_change() {
        let source = "class C { int x; void A(){ if(x>0){} if(x>1){} } void B(){ if(x>0){} if(x>1){} } void Cc(){ if(x>0){} if(x>1){} } void D(){ if(x>0){} if(x>1){} } void E(){ if(x>0){} if(x>1){} } void State(){ this.x=1; this.x=2; this.x=3; this.x=4; this.x=5; this.x=6; this.x=7; this.x=8; this.x=9; this.x=10; } }";
        let mut parser = CSharpParser::new().unwrap();
        let (facts, signals) =
            syntax_signals::extract(&mut parser, "input.cs".into(), source).unwrap();
        let record = summarize(facts, signals, source.len() as u64, 0).unwrap();
        assert_eq!(record.method_count, 6);
        assert_eq!(record.methods.len(), 5);
        assert!(record.methods.iter().all(|m| m.name != "State"));
        assert_eq!(record.scores[0].maximum, 2);
        assert_eq!(record.scores[3].maximum, 3);
        assert_eq!(record.feature_maxima[21], 9);
        let (facts, signals) =
            syntax_signals::extract(&mut parser, "input.cs".into(), source).unwrap();
        let alternate = summarize(facts, signals, source.len() as u64, 3).unwrap();
        assert_eq!(alternate.scores, record.scores);
        assert_eq!(alternate.methods[0].name, "State");
    }

    #[test]
    fn empty_file_has_zero_scores_and_parity_failure_is_fatal() {
        let facts = FileFacts {
            path: "empty.cs".into(),
            ..Default::default()
        };
        let record = summarize(facts, Vec::new(), 0, 0).unwrap();
        assert_eq!(record.scores, [Score::default(); 12]);
        let mut parser = CSharpParser::new().unwrap();
        let (facts, mut signals) =
            syntax_signals::extract(&mut parser, "bad.cs".into(), "class C { void M() {} }")
                .unwrap();
        signals[0].baseline = 1;
        assert!(summarize(facts, signals, 0, 0)
            .err()
            .unwrap()
            .contains("parity"));
    }

    #[test]
    fn bounded_scan_reports_unsupported_language_and_drains_after_budget_failure() {
        let directory = tempfile::tempdir().unwrap();
        for i in 0..40 {
            std::fs::write(
                directory.path().join(format!("{i:02}.cs")),
                "class C { void M() {} }",
            )
            .unwrap();
        }
        std::fs::write(directory.path().join("other.c"), "int main() { return 0; }").unwrap();
        let options = ScanOptions {
            workers: 2,
            ..Default::default()
        };
        let report = scan(directory.path(), &options, 0).unwrap();
        assert_eq!(report.files.len(), 40);
        assert_eq!(report.unscored, 1);
        assert_eq!(report.failure_counts["ResearchUnsupportedLanguage"], 1);
        let mut bytes = Vec::new();
        write_report(&report, &options, 0, &mut bytes).unwrap();
        let parsed: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed["analyzed_files"], 40);
        assert_eq!(parsed["files"].as_array().unwrap().len(), 40);
        let required_bytes = report.files.iter().map(FileRecord::retained_bytes).sum();
        for workers in [1, 2] {
            let exact = ScanOptions {
                workers,
                max_summary_bytes: required_bytes,
                ..ScanOptions::default()
            };
            assert_eq!(scan(directory.path(), &exact, 0).unwrap().files.len(), 40);
            let too_small = ScanOptions {
                max_summary_bytes: required_bytes - 1,
                ..exact
            };
            assert!(scan(directory.path(), &too_small, 0)
                .err()
                .unwrap()
                .contains("budget"));
        }
        let limited = ScanOptions {
            max_files: 1,
            ..options
        };
        assert!(scan(directory.path(), &limited, 0)
            .err()
            .unwrap()
            .contains("budget"));
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_paths_fail_before_json_instead_of_colliding() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let directory = tempfile::tempdir().unwrap();
        for suffix in [0xfe, 0xff] {
            let name = OsString::from_vec(vec![b'a', suffix, b'.', b'c', b's']);
            std::fs::write(directory.path().join(name), "class C {}").unwrap();
        }
        for workers in [1, 2] {
            let options = ScanOptions {
                workers,
                ..Default::default()
            };
            assert!(scan(directory.path(), &options, 0)
                .err()
                .unwrap()
                .contains("UTF-8 paths"));
        }
        let root = directory.path().join(OsString::from_vec(vec![0xfe]));
        std::fs::create_dir(&root).unwrap();
        let options = ScanOptions {
            workers: 1,
            ..Default::default()
        };
        assert!(scan(&root, &options, 0)
            .err()
            .unwrap()
            .contains("UTF-8 paths"));
    }
}
