// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

//! Developer inspection utility for validating scanner evidence and rankings.
use std::path::PathBuf;

use cpath::pipeline::{scan, ScanOptions};
use serde_json::json;

fn main() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let root = PathBuf::from(
        args.next()
            .ok_or("usage: inspect REPOSITORY [WORKERS [--include GLOB | --exclude GLOB]...]")?,
    );
    let mut options = ScanOptions::default();
    if let Some(workers) = args.next() {
        options.workers = workers
            .to_str()
            .ok_or("invalid workers")?
            .parse()
            .map_err(|_| "invalid workers")?;
    }
    // Reuse product option validation for scoped evaluations without adding a
    // product output mode. Existing REPOSITORY [WORKERS] usage stays supported.
    let scan_args = [
        root.clone().into_os_string(),
        "--top".into(),
        "10".into(),
        "--threads".into(),
        options.workers.to_string().into(),
    ]
    .into_iter()
    .chain(args);
    let cpath::cli::Command::Scan { options, .. } = cpath::cli::parse(scan_args)? else {
        return Err("expected scan arguments".into());
    };
    let report = scan(&root, &options)?;
    let rows: Vec<_> = report.ranked.iter().map(|rank| {
        let i = rank.file_index;
        let file = &report.files[i];
        let complexity = &report.complexity[i];
        let mut methods: Vec<_> = complexity.methods.iter().collect();
        methods.sort_by(|a, b| b.score.cmp(&a.score).then(a.start_line.cmp(&b.start_line)));
        json!({
            "path":file.path, "score":rank.score.maximum,
            "bytes":file.bytes,"lines":file.lines,
            "complexity":complexity.max_method,"complexity_total":complexity.total,"complexity_burden":rank.score.burden,
            "methods":methods.iter().take(5).map(|method| json!({"name":method.name,"start":method.start_line,"end":method.end_line,"complexity":method.score})).collect::<Vec<_>>()
        })
    }).collect();
    let output = json!({
        "formula":cpath::scoring::FORMULA,
        "includes":options.includes,"excludes":options.excludes,
        "workers":options.workers,"files":report.files.len(),
        "bytes":report.files.iter().map(|f|f.bytes).sum::<u64>(),
        "lines":report.files.iter().map(|f|u64::from(f.lines)).sum::<u64>(),
        "unscored_files":report.unscored_files,
        "failure_counts":report.failure_counts.iter().map(|(kind,count)|(format!("{kind:?}"),*count)).collect::<std::collections::BTreeMap<_,_>>(),
        "diagnostics":report.diagnostics.iter().map(|d|json!({"path":d.path,"message":d.message,"kind":format!("{:?}",d.kind)})).collect::<Vec<_>>(),
        "path_selection":format!("{:?}",report.selection),"content_skips":format!("{:?}",report.content_skips),
        "seconds":{"file_pipeline":report.times.files.as_secs_f64(),"ranking":report.times.ranking.as_secs_f64(),"total":report.times.total.as_secs_f64()},
        "ranking":rows,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&output).map_err(|e| e.to_string())?
    );
    Ok(())
}
