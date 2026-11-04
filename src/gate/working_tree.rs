// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

//! Read current source files without following symlinks or changing the index.

use super::git::MAX_BLOB_BYTES;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

pub(super) enum Entry {
    File(PathBuf), // Existing regular file under the repository root.
    Missing,       // A path or one of its parents no longer exists.
    Excluded,      // Symlink, directory, or other non-regular file.
}

pub(super) fn inspect(root: &Path, relative: &str) -> Result<Entry, String> {
    let mut path = root.to_path_buf();
    let mut components = Path::new(relative).components().peekable();
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            return Err("working-tree path is not a normal repository-relative path".into());
        };
        path.push(name);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Entry::Missing)
            }
            Err(error) => {
                return Err(format!(
                    "Failed to inspect working-tree file {relative}: {error}"
                ))
            }
        };
        if metadata.file_type().is_symlink() {
            return Ok(Entry::Excluded);
        }
        if components.peek().is_some() {
            if !metadata.is_dir() {
                return Ok(Entry::Excluded);
            }
        } else if !metadata.is_file() {
            return Ok(Entry::Excluded);
        }
    }
    Ok(Entry::File(path))
}

pub(super) fn read(path: &Path) -> Result<Vec<u8>, String> {
    let mut file =
        File::open(path).map_err(|error| format!("Failed to open working-tree source: {error}"))?;
    let before = file.metadata().map_err(|error| error.to_string())?;
    if !before.is_file() {
        return Err("working-tree source is no longer a regular file".into());
    }
    if before.len() > MAX_BLOB_BYTES as u64 {
        return Err(format!(
            "source exceeds the {MAX_BLOB_BYTES}-byte analysis budget"
        ));
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_BLOB_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    let after = file.metadata().map_err(|error| error.to_string())?;
    if bytes.len() > MAX_BLOB_BYTES
        || before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
    {
        return Err("working-tree source changed during reading; rerun the gate".into());
    }
    Ok(bytes)
}
