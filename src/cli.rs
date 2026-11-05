// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

use std::ffi::OsString;
use std::path::PathBuf;

use crate::pipeline::{MetricSet, ScanOptions};

pub const HELP: &str =
    "Usage: cpath <repository> [--metrics CC,MI|all] [--include GLOB] [--exclude GLOB] [--threads N]
       cpath gate [<commit> | <from> <to>] [--cc-limit VALUE] [--mi-limit VALUE] [--pretty]

Options:
  --metrics LIST  Metrics to calculate: CC, MI, or all (default: all)
  --pretty        Indent JSON output by four spaces
  --include GLOB  Root-relative include; overrides defaults/ignores (repeatable)
  --exclude GLOB  Root-relative exclusion; wins over includes (repeatable)
  --threads N     File-analysis workers, 1..=64 (default: available CPUs, at most 64)
  -h, --help      Show this help
  -v, --version   Show the version

Possible scan exit codes: 0 complete, 1 fatal error, 2 invalid arguments, 3 incomplete scan.
Gate: 0 no confirmed violations, 1 violations, 2 usage/setup error. See cpath gate --help.";

pub enum Command {
    Help,
    Version,
    Gate(Vec<OsString>),
    Scan {
        root: PathBuf,
        options: ScanOptions,
        pretty: bool,
    },
}

pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Command, String> {
    let args: Vec<_> = args.into_iter().collect();
    if args.first().is_some_and(|argument| argument == "gate") {
        return Ok(Command::Gate(args.into_iter().skip(1).collect()));
    }
    if args.is_empty() || (args.len() == 1 && (args[0] == "--help" || args[0] == "-h")) {
        return Ok(Command::Help);
    }
    if args.len() == 1 && (args[0] == "--version" || args[0] == "-v") {
        return Ok(Command::Version);
    }
    let mut root = None;
    let mut options = ScanOptions::default();
    let mut threads_seen = false;
    let mut metrics_seen = false;
    let mut pretty = false;
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.to_str() {
            Some("--metrics") => {
                if metrics_seen {
                    return Err("--metrics must be supplied once".into());
                }
                options.metrics = parse_metrics(iter.next())?;
                metrics_seen = true;
            }
            Some(flag) if flag.starts_with("--metrics=") => {
                if metrics_seen {
                    return Err("--metrics must be supplied once".into());
                }
                options.metrics = parse_metrics(Some(OsString::from(&flag[10..])))?;
                metrics_seen = true;
            }
            Some("--threads") => {
                if threads_seen {
                    return Err("--threads must be supplied once".into());
                }
                options.workers = positive(iter.next(), "--threads")?;
                if options.workers > 64 {
                    return Err("--threads must be between 1 and 64".into());
                }
                threads_seen = true;
            }
            Some("--pretty") => {
                if pretty {
                    return Err("--pretty must be supplied once".into());
                }
                pretty = true;
            }
            Some("--include" | "--exclude") => {
                let value = iter
                    .next()
                    .ok_or("missing glob pattern")?
                    .into_string()
                    .map_err(|_| "glob must be valid Unicode")?;
                if arg == "--include" {
                    options.includes.push(value);
                } else {
                    options.excludes.push(value);
                }
            }
            Some(flag) if flag.starts_with('-') => return Err(format!("unknown option: {flag}")),
            _ => {
                if root.replace(PathBuf::from(arg)).is_some() {
                    return Err("provide exactly one repository path".into());
                }
            }
        }
    }
    Ok(Command::Scan {
        root: root.ok_or("repository path is required")?,
        options,
        pretty,
    })
}

fn parse_metrics(value: Option<OsString>) -> Result<MetricSet, String> {
    let value = value
        .ok_or("--metrics requires CC, MI, or all")?
        .into_string()
        .map_err(|_| "metric list must be valid Unicode")?;
    if value.eq_ignore_ascii_case("all") {
        return Ok(MetricSet::all());
    }
    let mut metrics = MetricSet::none();
    for metric in value.split(',') {
        match metric {
            "CC" => metrics.cognitive_complexity = true,
            "MI" => metrics.maintainability = true,
            _ => return Err("--metrics accepts CC, MI, or all".into()),
        }
    }
    if !metrics.cognitive_complexity && !metrics.maintainability {
        return Err("--metrics requires at least one metric".into());
    }
    Ok(metrics)
}

fn positive(value: Option<OsString>, flag: &str) -> Result<usize, String> {
    value
        .and_then(|v| v.into_string().ok())
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .ok_or_else(|| format!("{flag} requires a positive integer"))
}
