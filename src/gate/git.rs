// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

pub(super) const MAX_BLOB_BYTES: usize = 64 * 1024 * 1024;

pub(super) struct Repository {
    pub root: PathBuf,      // Worktree root; all Git commands are anchored here.
    pub from: String,       // Resolved immutable starting commit ID.
    pub to: Option<String>, // Resolved target commit ID; None selects working-tree files.
}

pub(super) struct Change {
    pub path: Vec<u8>,    // Exact Git path bytes; never decoded lossily for matching.
    pub old_mode: String, // Starting tree entry mode, zero for an addition.
    pub new_mode: String, // Target tree entry mode, zero for a deletion.
    pub old_blob: String, // Starting object ID.
    pub new_blob: String, // Target object ID.
}

fn command(root: &Path) -> Command {
    let mut command = Command::new("git");
    command.args(["--no-pager", "-C"]).arg(root);
    command.args([
        "-c",
        "diff.autoRefreshIndex=false",
        "-c",
        "core.fsmonitor=false",
    ]);
    command.env("GIT_NO_REPLACE_OBJECTS", "1");
    command.env("GIT_NO_LAZY_FETCH", "1");
    command.env("GIT_TERMINAL_PROMPT", "0");
    command.env("GIT_OPTIONAL_LOCKS", "0");
    command.stdin(Stdio::null());
    command
}

fn output(root: &Path, arguments: &[&str]) -> Result<Vec<u8>, String> {
    let output = command(root)
        .args(arguments)
        .output()
        .map_err(|error| format!("Failed to run Git: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "Git {} failed ({}): {}",
            arguments[0],
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(output.stdout)
}

fn resolve(root: &Path, revision: &str) -> Result<String, String> {
    let revision = format!("{revision}^{{commit}}");
    let bytes = output(
        root,
        &["rev-parse", "--verify", "--end-of-options", &revision],
    )?;
    let oid = String::from_utf8(bytes).map_err(|_| "Git returned a non-ASCII commit ID")?;
    let oid = oid.trim();
    if !valid_oid(oid) {
        return Err("Git returned an invalid commit ID".into());
    }
    Ok(oid.to_owned())
}

fn valid_oid(oid: &str) -> bool {
    matches!(oid.len(), 40 | 64) && oid.bytes().all(|byte| byte.is_ascii_hexdigit())
}

impl Repository {
    pub fn open(directory: &Path, from: &str, to: Option<&str>) -> Result<Self, String> {
        let bytes = output(directory, &["rev-parse", "--show-toplevel"])?;
        let root = String::from_utf8(bytes).map_err(|_| "repository root must be valid Unicode")?;
        let root = PathBuf::from(
            root.strip_suffix('\n')
                .unwrap_or(&root)
                .trim_end_matches('\r'),
        );
        Ok(Self {
            from: resolve(&root, from)?,
            to: to.map(|revision| resolve(&root, revision)).transpose()?,
            root,
        })
    }

    pub fn changes(&self) -> Result<Vec<Change>, String> {
        let mut arguments = vec![
            "diff",
            "--raw",
            "--no-abbrev",
            "-z",
            "--no-renames",
            "--no-ext-diff",
            "--no-textconv",
            "--ignore-submodules=none",
            &self.from,
        ];
        if let Some(to) = &self.to {
            arguments.push(to);
        } else {
            // Raw diff alone trusts stale index stat data. Numstat verifies content
            // without refreshing the index, including in shared Windows/WSL trees.
            arguments.push("--numstat");
        }
        arguments.push("--");
        let bytes = output(&self.root, &arguments)?;
        let changes = parse_changes(&bytes, self.to.is_none())?;
        if self.to.is_some() {
            return Ok(changes);
        }
        self.with_untracked(changes)
    }

    fn with_untracked(&self, changes: Vec<Change>) -> Result<Vec<Change>, String> {
        let paths = output(
            &self.root,
            &["ls-files", "--others", "--exclude-standard", "-z"],
        )?;
        let mut by_path: BTreeMap<_, _> = changes
            .into_iter()
            .map(|change| (change.path.clone(), change))
            .collect();
        for path in paths
            .split(|byte| *byte == 0)
            .filter(|path| !path.is_empty())
        {
            // A staged deletion may reappear as an untracked file. Keep its baseline.
            by_path.entry(path.to_vec()).or_insert_with(|| Change {
                path: path.to_vec(),
                old_mode: "000000".into(),
                new_mode: "100644".into(),
                old_blob: String::new(),
                new_blob: String::new(),
            });
        }
        Ok(by_path.into_values().collect())
    }

    pub fn blobs(&self) -> Result<Blobs, String> {
        let mut child = command(&self.root)
            .args(["cat-file", "--batch"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("Failed to start Git blob reader: {error}"))?;
        let input = child.stdin.take().ok_or("Git blob input is unavailable")?;
        let output = BufReader::new(
            child
                .stdout
                .take()
                .ok_or("Git blob output is unavailable")?,
        );
        Ok(Blobs {
            child,
            input,
            output,
            broken: false,
        })
    }
}

fn parse_changes(bytes: &[u8], verify_content: bool) -> Result<Vec<Change>, String> {
    let mut records = bytes.split(|byte| *byte == 0);
    let mut changes = Vec::new();
    let mut confirmed_paths = BTreeSet::new();
    while let Some(header) = records.next().filter(|header| !header.is_empty()) {
        if verify_content && !header.starts_with(b":") {
            let path = header
                .splitn(3, |byte| *byte == b'\t')
                .nth(2)
                .filter(|path| !path.is_empty())
                .ok_or("invalid Git numstat record")?;
            confirmed_paths.insert(path);
            continue;
        }
        let header = std::str::from_utf8(header).map_err(|_| "invalid Git diff header")?;
        let fields: Vec<_> = header
            .strip_prefix(':')
            .ok_or("invalid Git diff record")?
            .split(' ')
            .collect();
        if fields.len() != 5 || !valid_oid(fields[2]) || !valid_oid(fields[3]) {
            return Err("invalid Git diff fields".into());
        }
        let path = records
            .next()
            .filter(|path| !path.is_empty())
            .ok_or("missing Git diff path")?;
        changes.push(Change {
            path: path.to_vec(),
            old_mode: fields[0].into(),
            new_mode: fields[1].into(),
            old_blob: fields[2].into(),
            new_blob: fields[3].into(),
        });
    }
    if verify_content {
        changes.retain(|change| confirmed_paths.contains(change.path.as_slice()));
    }
    Ok(changes)
}

pub(super) fn regular(mode: &str) -> bool {
    matches!(mode, "100644" | "100755")
}

pub(super) struct Blobs {
    child: Child,                   // Owned persistent Git subprocess.
    input: ChildStdin,              // Object requests, flushed individually.
    output: BufReader<ChildStdout>, // Length-delimited object responses.
    broken: bool,                   // Prevents reuse after a framing/I/O error.
}

impl Blobs {
    pub fn read(&mut self, oid: &str) -> Result<Vec<u8>, String> {
        if self.broken {
            return Err("Git blob reader is unavailable after an earlier failure".into());
        }
        // A failed read may leave the protocol between objects. Do not retry on it.
        self.broken = true;
        let result = self.read_object(oid);
        result.map_err(|error| format!("Failed to read Git blob {oid}: {error}"))
    }

    fn read_object(&mut self, oid: &str) -> Result<Vec<u8>, String> {
        writeln!(self.input, "{oid}").map_err(|error| error.to_string())?;
        self.input.flush().map_err(|error| error.to_string())?;
        let mut header = String::new();
        self.output
            .read_line(&mut header)
            .map_err(|error| error.to_string())?;
        if header.trim_end() == format!("{oid} missing") {
            self.broken = false;
            return Err("object is missing; no automatic fetch was attempted".into());
        }
        let fields: Vec<_> = header.split_whitespace().collect();
        if fields.len() != 3 || fields[0] != oid || fields[1] != "blob" {
            return Err("invalid blob response".into());
        }
        let size: usize = fields[2].parse().map_err(|_| "invalid blob size")?;
        if size > MAX_BLOB_BYTES {
            // Drain without allocating the object so subsequent files can still run.
            let mut remaining = (&mut self.output).take(size as u64 + 1);
            let count = std::io::copy(&mut remaining, &mut std::io::sink())
                .map_err(|error| error.to_string())?;
            self.broken = count != size as u64 + 1;
            return Err(format!(
                "blob exceeds the {MAX_BLOB_BYTES}-byte analysis budget"
            ));
        }
        let mut bytes = vec![0; size];
        self.output
            .read_exact(&mut bytes)
            .map_err(|error| error.to_string())?;
        let mut delimiter = [0];
        self.output
            .read_exact(&mut delimiter)
            .map_err(|error| error.to_string())?;
        if delimiter != *b"\n" {
            return Err("invalid blob terminator".into());
        }
        self.broken = false;
        Ok(bytes)
    }
}

impl Drop for Blobs {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
