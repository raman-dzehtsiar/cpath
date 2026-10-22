// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

use std::ffi::OsString;
use std::path::PathBuf;

use crate::pipeline::ScanOptions;

pub const HELP: &str = "cpath - Find the code that needs human attention.

Offline C#, C, and C++ analysis of cognitive complexity.
Experimental prototype: approximate metrics, not a probability of failure.

Usage: cpath <repository> --top N [--include GLOB] [--exclude GLOB] [--threads N]

Options:
  --top N         Maximum number of files to print (required, positive integer)
  --include GLOB  Root-relative include; overrides defaults/ignores (repeatable)
  --exclude GLOB  Root-relative exclusion; wins over includes (repeatable)
  --threads N     File-analysis workers, 1..=64 (default: available CPUs, at most 8)
  -h, --help      Show this help
  -V, --version   Show the version

Exit status: 0 complete, 1 fatal error, 2 invalid arguments, 3 incomplete scan.
Incomplete scans print available rankings and coverage warnings to stderr.

Scan stdout contains only: maximum_function_complexity /absolute/path/to/file";

pub enum Command {
    Help,
    Version,
    Scan {
        root: PathBuf,
        top: usize,
        options: ScanOptions,
    },
}

pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Command, String> {
    let args: Vec<_> = args.into_iter().collect();
    if args.is_empty() || (args.len() == 1 && (args[0] == "--help" || args[0] == "-h")) {
        return Ok(Command::Help);
    }
    if args.len() == 1 && (args[0] == "--version" || args[0] == "-V") {
        return Ok(Command::Version);
    }
    let mut root = None;
    let mut top = None;
    let mut options = ScanOptions::default();
    let mut threads_seen = false;
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.to_str() {
            Some("--top") => {
                if top.is_some() {
                    return Err("--top must be supplied once".into());
                }
                top = Some(positive(iter.next(), "--top")?);
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
        top: top.ok_or("--top N is required")?,
        options,
    })
}

fn positive(value: Option<OsString>, flag: &str) -> Result<usize, String> {
    value
        .and_then(|v| v.into_string().ok())
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .ok_or_else(|| format!("{flag} requires a positive integer"))
}
