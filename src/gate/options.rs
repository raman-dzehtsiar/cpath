// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

use std::ffi::OsString;

pub(super) const DEFAULT_C_FAMILY_CC_LIMIT: f64 = 25.0;
pub(super) const DEFAULT_CC_LIMIT: f64 = 15.0;
pub(super) const DEFAULT_MI_LIMIT: f64 = 20.0;

pub const HELP: &str =
    "Usage: cpath gate [<from> [<to>]] [--cc-limit VALUE] [--mi-limit VALUE] [--pretty]

With no commits, compare HEAD with the working tree (staged, unstaged, and untracked files).
With one commit, compare that commit with the working tree.
With two commits, compare those snapshots and ignore uncommitted changes.
Gate callable scopes. Existing scopes may change within limits; existing debt must not worsen.
Renamed and cross-file/enclosing-scope moves are new. MI is rounded to the nearest integer.

Options:
  --cc-limit VALUE  CC maximum (default: 25 for C/C++, 15 otherwise)
  --mi-limit VALUE  MI minimum (default: 20)
  --pretty          Indent JSON output by four spaces
  -h, --help        Show this help

Exit codes: 0 no confirmed violations (possibly incomplete), 1 violations, 2 usage/setup error.";

#[derive(Debug, Clone)]
pub struct Options {
    pub from: String,          // Starting commit reference, defaulting to HEAD.
    pub to: Option<String>,    // Explicit target commit; None selects the working tree.
    pub cc_limit: Option<f64>, // Optional replacement for language-specific CC limits.
    pub mi_limit: f64,         // MI minimum, allowing unchanged or improved existing debt.
    pub pretty: bool,          // Four-space JSON indentation when enabled.
    pub debug: bool,           // Include unassessed and excluded diagnostics.
}

pub fn parse(args: &[OsString]) -> Result<Options, String> {
    let mut revisions = Vec::new();
    let mut cc_limit = None;
    let mut mi_limit = None;
    let mut pretty = false;
    let mut debug = false;
    let mut arguments = args.iter();
    while let Some(argument) = arguments.next() {
        let argument = argument
            .to_str()
            .ok_or("gate arguments must be valid Unicode")?;
        if argument == "--pretty" {
            if pretty {
                return Err("--pretty must be supplied once".into());
            }
            pretty = true;
            continue;
        }
        if argument == "--debug" {
            if debug {
                return Err("--debug must be supplied once".into());
            }
            debug = true;
            continue;
        }
        let (flag, inline) = argument
            .split_once('=')
            .map_or((argument, None), |(flag, value)| (flag, Some(value)));
        if matches!(flag, "--cc-limit" | "--mi-limit") {
            let target = if flag == "--cc-limit" {
                &mut cc_limit
            } else {
                &mut mi_limit
            };
            if target.is_some() {
                return Err(format!("{flag} must be supplied once"));
            }
            let value = inline.or_else(|| arguments.next().and_then(|value| value.to_str()));
            *target = Some(decimal(value, flag)?);
            continue;
        }
        if argument.starts_with('-') {
            return Err(format!("unknown gate option: {argument}"));
        }
        revisions.push(argument.to_owned());
    }
    if revisions.len() > 2 {
        return Err("gate accepts at most two commit arguments: [<from> [<to>]]".into());
    }
    Ok(Options {
        from: revisions.first().cloned().unwrap_or_else(|| "HEAD".into()),
        to: revisions.get(1).cloned(),
        cc_limit,
        mi_limit: mi_limit.unwrap_or(DEFAULT_MI_LIMIT),
        pretty,
        debug,
    })
}

fn decimal(value: Option<&str>, flag: &str) -> Result<f64, String> {
    let error = || format!("{flag} requires a finite unsigned decimal value");
    let value = value.ok_or_else(error)?;
    let mut parts = value.split('.');
    let integer = parts.next().unwrap_or_default();
    if integer.is_empty() || !integer.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(error());
    }
    if let Some(fraction) = parts.next() {
        if fraction.is_empty() || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(error());
        }
    }
    if parts.next().is_some() {
        return Err(error());
    }
    let number: f64 = value.parse().map_err(|_| error())?;
    if !number.is_finite() {
        return Err(error());
    }
    Ok(number)
}
