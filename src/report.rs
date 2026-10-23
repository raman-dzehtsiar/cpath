// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

use std::io::{self, Write};
use std::path::Path;

use crate::pipeline::RankedFile;

/// Coverage warnings are deliberately separate from the score/path stream.
pub fn write_diagnostics(
    mut writer: impl Write,
    report: &crate::pipeline::ScanReport,
) -> io::Result<()> {
    if report.unscored_files == 0 {
        return Ok(());
    }
    writeln!(writer, "cpath: warning: incomplete scan: analyzed {} files; {} selected files unscored. Rankings cover analyzed files only.", report.files.len(), report.unscored_files)?;
    for diagnostic in &report.diagnostics {
        writeln!(
            writer,
            "  {}: {}",
            diagnostic.path.display(),
            diagnostic.message
        )?;
    }
    let omitted = report
        .unscored_files
        .saturating_sub(report.diagnostics.len());
    if omitted > 0 {
        writeln!(writer, "  {omitted} additional file diagnostics omitted")?;
    }
    Ok(())
}

/// Render only score/path rows. The caller passes already-ranked files.
pub fn write_top(
    mut writer: impl Write,
    root: &Path,
    files: &[RankedFile],
    top: usize,
) -> io::Result<()> {
    let mut rows = Vec::new();
    for file in files.iter().take(top) {
        let path = root.join(&file.path);
        let path = path.to_str().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "output path is not valid Unicode",
            )
        })?;
        if path.contains(['\n', '\r']) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "output path contains a line break",
            ));
        }
        rows.push((file.score.maximum, path.to_owned()));
    }
    for (score, path) in rows {
        writeln!(writer, "{score} {path}")?;
    }
    Ok(())
}

#[cfg(all(test, any(unix, windows)))]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::path::PathBuf;

    #[test]
    fn non_unicode_later_path_fails_before_writing_any_rows() {
        #[cfg(unix)]
        let invalid = {
            use std::os::unix::ffi::OsStringExt;
            OsString::from_vec(b"Z\xff.cs".to_vec())
        };
        #[cfg(windows)]
        let invalid = {
            use std::os::windows::ffi::OsStringExt;
            OsString::from_wide(&[b'Z' as u16, 0xd800, b'.' as u16, b'c' as u16, b's' as u16])
        };
        // Construct paths in memory: APFS cannot create the invalid Unix filename.
        let files = [PathBuf::from("A\u{fffd}.cs"), PathBuf::from(invalid)]
            .into_iter()
            .enumerate()
            .map(|(file_index, path)| RankedFile {
                path,
                file_index,
                score: Default::default(),
            })
            .collect::<Vec<_>>();
        let mut output = Vec::new();
        write_top(&mut output, Path::new("root"), &files, 1).unwrap();
        assert!(!output.is_empty());

        output.clear();
        let error = write_top(&mut output, Path::new("root"), &files, 2).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("Unicode"));
        assert!(output.is_empty());
    }
}
