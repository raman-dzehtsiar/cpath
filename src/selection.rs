// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

//! The single source-selection policy. Explicit excludes win; explicit includes
//! form an allow-list and override ignores and generated/build heuristics.
//! Metadata, symlinks, unsupported languages and invalid content never qualify.
//! Ignore rules are checkout-local `.gitignore` and `.ignore` files only.
//! Include patterns use `/`, `*` within a component and `**` across components.
//! An explicit exclusion of a directory excludes every descendant. Includes
//! disable heuristic directory pruning and ignore evaluation, then filter files.
//! Generated-name and leading-comment detection are heuristics, not proof that
//! a file is generated. Includes opt out of both. Discovery fails beyond 256
//! nested directories rather than risk a stack overflow or silently omit files.
//! Default build directories: bin, obj, .vs, TestResults, build, CMakeFiles. Default vendor
//! directories: vendor, vendors, vendored, third_party, third-party, thirdparty,
//! packages, node_modules. These are exact, case-insensitive directory names,
//! not ownership detection. Tests, migrations and ambiguous `external` directories
//! remain eligible. Explicit includes opt into these directories as well.

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::model::SourceLanguage;

/// Deterministic extension policy; no compiler flags, include resolution, or
/// filesystem context. Uppercase `.C` conventionally denotes C++.
pub fn language_for_path(path: &Path) -> Option<SourceLanguage> {
    let extension = path.extension()?.to_str()?;
    if extension == "C" {
        return Some(SourceLanguage::Cpp);
    }
    match extension.to_ascii_lowercase().as_str() {
        "cs" => Some(SourceLanguage::CSharp),
        "c" => Some(SourceLanguage::C),
        "h" => Some(SourceLanguage::CHeader),
        "cc" | "cpp" | "cxx" | "c++" | "hh" | "hpp" | "hxx" | "h++" | "ipp" | "tpp" | "inl" => {
            Some(SourceLanguage::Cpp)
        }
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SkipReason {
    Metadata,
    Symlink,
    Excluded,
    NotIncluded,
    Ignored,
    BuildDirectory,
    VendorDirectory,
    UnsupportedLanguage,
    GeneratedName,
    GeneratedHeader,
    Binary,
    InvalidUtf8,
    SpecialFile,
}

/// Counts observed files and selection decisions. A pruned directory contributes
/// one skip; its descendants are not enumerated or included in `candidates`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SelectionStats {
    pub candidates: usize,
    pub selected: usize,
    pub skipped: BTreeMap<SkipReason, usize>,
}

impl SelectionStats {
    fn skip(&mut self, reason: SkipReason) {
        *self.skipped.entry(reason).or_default() += 1;
    }
}

pub struct Selection {
    root: PathBuf,
    includes: GlobSet,
    excludes: GlobSet,
}

impl Selection {
    pub fn new(root: &Path, includes: &[String], excludes: &[String]) -> Result<Self, String> {
        let metadata = fs::symlink_metadata(root)
            .map_err(|error| format!("cannot inspect {}: {error}", root.display()))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(format!(
                "scan root must be a directory, not a symlink: {}",
                root.display()
            ));
        }
        let root = root
            .canonicalize()
            .map_err(|error| format!("cannot resolve {}: {error}", root.display()))?;
        Ok(Self {
            root,
            includes: compile_globs(includes)?,
            excludes: compile_globs(excludes)?,
        })
    }

    /// Stream selected absolute paths to the reader. Callback failures stop
    /// traversal immediately so downstream cancellation does not leave it blocked.
    pub fn walk(
        &self,
        mut visit: impl FnMut(PathBuf) -> Result<(), String>,
    ) -> Result<SelectionStats, String> {
        let mut stats = SelectionStats::default();
        self.walk_directory(&self.root, 0, &mut Vec::new(), &mut stats, &mut visit)?;
        Ok(stats)
    }

    fn walk_directory(
        &self,
        directory: &Path,
        depth: usize,
        ignores: &mut Vec<Gitignore>,
        stats: &mut SelectionStats,
        visit: &mut impl FnMut(PathBuf) -> Result<(), String>,
    ) -> Result<(), String> {
        if depth > 256 {
            return Err(format!(
                "directory nesting exceeds supported depth of 256: {}",
                directory.display()
            ));
        }
        let has_rules = self.includes.is_empty();
        if has_rules {
            ignores.push(load_ignores(directory)?);
        }
        let mut entries = fs::read_dir(directory)
            .map_err(|error| format!("cannot read directory {}: {error}", directory.display()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("cannot enumerate {}: {error}", directory.display()))?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let relative = path
                .strip_prefix(&self.root)
                .expect("directory entry is inside root");
            let file_type = entry
                .file_type()
                .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
            let is_directory = file_type.is_dir();
            if file_type.is_file() {
                stats.candidates += 1;
            }
            let skip = if entry.file_name() == ".git" {
                Some(SkipReason::Metadata)
            } else if file_type.is_symlink() {
                Some(SkipReason::Symlink)
            } else if self.excludes.is_match(relative) {
                Some(SkipReason::Excluded)
            } else if !is_directory && !file_type.is_file() {
                Some(SkipReason::SpecialFile)
            } else if has_rules && is_ignored(ignores, &path, is_directory) {
                Some(SkipReason::Ignored)
            } else if is_directory {
                if has_rules {
                    classify_directory(&entry.file_name().to_string_lossy())
                } else {
                    None
                }
            } else {
                self.classify_path(relative)
            };
            if let Some(reason) = skip {
                stats.skip(reason);
            } else if is_directory {
                self.walk_directory(&path, depth + 1, ignores, stats, visit)?;
            } else {
                visit(path)?;
                stats.selected += 1;
            }
        }
        if has_rules {
            ignores.pop();
        }
        Ok(())
    }

    fn classify_path(&self, relative: &Path) -> Option<SkipReason> {
        if language_for_path(relative).is_none() {
            return Some(SkipReason::UnsupportedLanguage);
        }
        if !self.includes.is_empty() {
            return (!self.includes.is_match(relative)).then_some(SkipReason::NotIncluded);
        }
        let name = relative.file_name()?.to_string_lossy().to_ascii_lowercase();
        if language_for_path(relative) != Some(SourceLanguage::CSharp) {
            let stem = relative.file_stem()?.to_string_lossy().to_ascii_lowercase();
            return [".generated", ".autogen", ".pb"]
                .iter()
                .any(|suffix| stem.ends_with(suffix))
                .then_some(SkipReason::GeneratedName);
        }
        [
            ".g.cs",
            ".g.i.cs",
            ".generated.cs",
            ".designer.cs",
            ".autogen.cs",
        ]
        .iter()
        .any(|suffix| name.ends_with(suffix))
        .then_some(SkipReason::GeneratedName)
    }

    /// Called by the reader with decoded UTF-8 bytes subsequently passed to
    /// parsing. The reader owns BOM/encoding decoding; this policy defensively
    /// rejects NUL/invalid UTF-8 and applies generated-header classification.
    pub fn classify_content(&self, relative: &Path, bytes: &[u8]) -> Option<SkipReason> {
        if bytes.contains(&0) {
            return Some(SkipReason::Binary);
        }
        let Ok(source) = std::str::from_utf8(bytes) else {
            return Some(SkipReason::InvalidUtf8);
        };
        if self.includes.is_match(relative) {
            return None;
        }
        generated_header(source).then_some(SkipReason::GeneratedHeader)
    }
}

fn compile_globs(patterns: &[String]) -> Result<GlobSet, String> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        // '/' is the portable separator. '*' stays within a path component;
        // '**' spans directories, including zero directories in '**/*.cs'.
        let glob = GlobBuilder::new(pattern)
            .literal_separator(true)
            .backslash_escape(false)
            .build()
            .map_err(|error| format!("invalid selection glob {pattern:?}: {error}"))?;
        builder.add(glob);
    }
    builder
        .build()
        .map_err(|error| format!("cannot compile selection patterns: {error}"))
}

fn load_ignores(directory: &Path) -> Result<Gitignore, String> {
    let mut builder = GitignoreBuilder::new(directory);
    for name in [".gitignore", ".ignore"] {
        let path = directory.join(name);
        match fs::symlink_metadata(&path) {
            // Never follow a symlink even for policy files.
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                if let Some(error) = builder.add(&path) {
                    return Err(format!(
                        "cannot load ignore rules {}: {error}",
                        path.display()
                    ));
                }
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "cannot inspect ignore rules {}: {error}",
                    path.display()
                ))
            }
        }
    }
    builder.build().map_err(|error| {
        format!(
            "cannot compile ignore rules in {}: {error}",
            directory.display()
        )
    })
}

fn is_ignored(ignores: &[Gitignore], path: &Path, is_directory: bool) -> bool {
    for rules in ignores.iter().rev() {
        let matched = rules.matched_path_or_any_parents(path, is_directory);
        if !matched.is_none() {
            return matched.is_ignore();
        }
    }
    false
}

fn classify_directory(name: &str) -> Option<SkipReason> {
    if ["bin", "obj", ".vs", "TestResults", "build", "CMakeFiles"]
        .iter()
        .any(|excluded| name.eq_ignore_ascii_case(excluded))
    {
        Some(SkipReason::BuildDirectory)
    } else if [
        "vendor",
        "vendors",
        "vendored",
        "third_party",
        "third-party",
        "thirdparty",
        "packages",
        "node_modules",
    ]
    .iter()
    .any(|excluded| name.eq_ignore_ascii_case(excluded))
    {
        Some(SkipReason::VendorDirectory)
    } else {
        None
    }
}

fn generated_header(source: &str) -> bool {
    // Only inspect leading comments, never strings or arbitrary source text.
    // Limit inspection to 4 KiB to bound work on large headers.
    let mut end = source.len().min(4096);
    while !source.is_char_boundary(end) {
        end -= 1;
    }
    let mut remaining = source[..end].trim_start_matches('\u{feff}').trim_start();
    let mut generator_attribution = false;
    let mut edit_warning = false;
    loop {
        let (comment, rest) = if let Some(line) = remaining.strip_prefix("//") {
            line.split_once('\n').unwrap_or((line, ""))
        } else if let Some(block) = remaining.strip_prefix("/*") {
            block.split_once("*/").unwrap_or((block, ""))
        } else {
            return generator_attribution && edit_warning;
        };
        for line in comment.lines() {
            // Anchor declarations at a comment-line start, after conventional
            // block/doc-comment decoration. Narrative mentions are not markers.
            let line = line
                .trim()
                .trim_start_matches(['*', '/'])
                .trim()
                .to_ascii_lowercase();
            if is_generated_tag(&line) || line == "this code was generated by a tool." {
                return true;
            }
            let attribution = [
                "generated by ",
                "code generated by ",
                "this file was generated by ",
                "this file was automatically generated by ",
            ]
            .iter()
            .any(|prefix| {
                line.strip_prefix(prefix)
                    .is_some_and(|tool| !tool.trim().is_empty())
            });
            generator_attribution |= attribution;
            // An attribution alone is too weak. Require an explicit no-edit
            // warning on its line or a standalone warning elsewhere in header.
            edit_warning |= ["do not edit", "do not edit.", "do not edit!"]
                .iter()
                .any(|warning| {
                    line == *warning
                        || attribution
                            && line
                                .strip_suffix(warning)
                                .is_some_and(|prefix| prefix.ends_with(' '))
                });
        }
        remaining = rest.trim_start();
    }
}

fn is_generated_tag(line: &str) -> bool {
    ["<auto-generated", "<autogenerated"].iter().any(|prefix| {
        line.strip_prefix(prefix).is_some_and(|suffix| {
            (suffix.starts_with('>')
                || suffix.starts_with("/>")
                || suffix.starts_with(char::is_whitespace))
                && suffix.contains('>')
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "cpath-selection-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }
        fn write(&self, path: &str, content: &str) {
            let path = self.0.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }
        fn selection(&self, includes: &[&str], excludes: &[&str]) -> Selection {
            Selection::new(
                &self.0,
                &includes.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                &excludes.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            )
            .unwrap()
        }
        fn paths(&self, includes: &[&str], excludes: &[&str]) -> (Vec<String>, SelectionStats) {
            let selection = self.selection(includes, excludes);
            let mut result = Vec::new();
            let stats = selection
                .walk(|path| {
                    result.push(
                        path.strip_prefix(&selection.root)
                            .unwrap()
                            .to_string_lossy()
                            .replace('\\', "/"),
                    );
                    Ok(())
                })
                .unwrap();
            (result, stats)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn native_extensions_are_centralized_and_uppercase_c_means_cpp() {
        for (extension, language) in [
            ("c", SourceLanguage::C),
            ("C", SourceLanguage::Cpp),
            ("h", SourceLanguage::CHeader),
            ("H", SourceLanguage::CHeader),
            ("cs", SourceLanguage::CSharp),
            ("CS", SourceLanguage::CSharp),
        ] {
            assert_eq!(
                language_for_path(Path::new(&format!("file.{extension}"))),
                Some(language)
            );
        }
        for extension in [
            "cc", "cpp", "cxx", "c++", "hh", "hpp", "hxx", "h++", "ipp", "tpp", "inl", "CPP", "HPP",
        ] {
            assert_eq!(
                language_for_path(Path::new(&format!("file.{extension}"))),
                Some(SourceLanguage::Cpp)
            );
        }
        for name in [
            "file",
            "file.rs",
            "file.m",
            "file.mm",
            "file.cu",
            "file.cpp.bak",
        ] {
            assert_eq!(language_for_path(Path::new(name)), None);
        }
    }

    #[test]
    fn native_selection_defaults_and_overrides_share_existing_precedence() {
        let f = Fixture::new();
        for path in [
            "src/main.c",
            "include/api.h",
            "build/output.cpp",
            "CMakeFiles/id.c",
            "vendor/lib.hpp",
            "ignored/keep.cc",
            "src/messages.pb.h",
            "src/model.generated.cpp",
            "src/model.AUTOGEN.H",
            "src/no.rs",
            ".git/no.c",
            "Tests/check.cpp",
        ] {
            f.write(path, "");
        }
        f.write(".gitignore", "ignored/\n");
        assert_eq!(
            f.paths(&[], &[]).0,
            ["Tests/check.cpp", "include/api.h", "src/main.c"]
        );
        let (selected, stats) = f.paths(&["**/*"], &["vendor", "src/main.c"]);
        assert_eq!(
            selected,
            [
                "CMakeFiles/id.c",
                "Tests/check.cpp",
                "build/output.cpp",
                "ignored/keep.cc",
                "include/api.h",
                "src/messages.pb.h",
                "src/model.AUTOGEN.H",
                "src/model.generated.cpp"
            ]
        );
        assert_eq!(stats.skipped[&SkipReason::UnsupportedLanguage], 2); // .gitignore and .rs
        assert_eq!(f.paths(&["build/output.cpp"], &[]).0, ["build/output.cpp"]);
        let ordinary = f.selection(&[], &[]);
        let explicit = f.selection(&["**/*.cpp"], &[]);
        let path = Path::new("src/generated.cpp");
        let content = b"// Generated by Example. DO NOT EDIT!\nint f() { return 0; }";
        assert_eq!(
            ordinary.classify_content(path, content),
            Some(SkipReason::GeneratedHeader)
        );
        assert_eq!(explicit.classify_content(path, content), None);
        assert_eq!(
            explicit.classify_content(path, b"a\0b"),
            Some(SkipReason::Binary)
        );
    }

    #[test]
    fn defaults_prune_build_ignored_metadata_and_generated() {
        let f = Fixture::new();
        for path in [
            "z.cs",
            "nested/a.cs",
            "bin/output.cs",
            "obj/output.cs",
            ".git/hidden.cs",
            "ignored/no.cs",
            "file.g.cs",
            "file.G.I.cs",
            "file.Designer.cs",
            "text.txt",
        ] {
            f.write(path, "");
        }
        f.write(".gitignore", "ignored/\n");
        let (paths, stats) = f.paths(&[], &[]);
        assert_eq!(paths, ["nested/a.cs", "z.cs"]);
        assert_eq!(stats.selected, 2);
        assert_eq!(stats.skipped[&SkipReason::Ignored], 1);
        assert_eq!(stats.skipped[&SkipReason::BuildDirectory], 2);
        assert_eq!(stats.skipped[&SkipReason::GeneratedName], 3);
    }

    #[test]
    fn explicit_includes_override_defaults_but_excludes_win() {
        let f = Fixture::new();
        for path in [
            "root.cs",
            "bin/output.g.cs",
            "ignored/keep.cs",
            "ignored/no.cs",
            ".git/no.cs",
        ] {
            f.write(path, "");
        }
        f.write(".gitignore", "ignored/\n");
        let (paths, _) = f.paths(&["**/*.cs"], &["ignored/no.cs"]);
        assert_eq!(paths, ["bin/output.g.cs", "ignored/keep.cs", "root.cs"]);
        assert_eq!(f.paths(&["ignored/keep.cs"], &[]).0, ["ignored/keep.cs"]);
        assert_eq!(
            f.paths(&["**/*.cs"], &["ignored"]).0,
            ["bin/output.g.cs", "root.cs"]
        );
    }

    #[test]
    fn nested_ignore_negation_and_local_ignore_files() {
        let f = Fixture::new();
        f.write(".gitignore", "*.cs\n!nested/\n");
        f.write("nested/.gitignore", "!keep.cs\n");
        f.write("nested/keep.cs", "");
        f.write("nested/drop.cs", "");
        assert_eq!(f.paths(&[], &[]).0, ["nested/keep.cs"]);
        f.write("nested/.ignore", "keep.cs\n");
        assert!(f.paths(&[], &[]).0.is_empty());
    }

    #[test]
    fn content_classification_is_header_only_and_never_overrides_invalid_content() {
        let f = Fixture::new();
        let policy = f.selection(&[], &[]);
        let path = Path::new("a.cs");
        assert_eq!(
            policy.classify_content(path, b"// <auto-generated/>\nclass A {}"),
            Some(SkipReason::GeneratedHeader)
        );
        assert_eq!(
            policy.classify_content(
                path,
                b"/* license */\n/* This code was generated by a tool. */"
            ),
            Some(SkipReason::GeneratedHeader)
        );
        assert_eq!(
            policy.classify_content(path, b"class A { string x = \"<auto-generated>\"; }"),
            None
        );
        assert_eq!(
            policy.classify_content(path, b"class A {}\n// <auto-generated>"),
            None
        );
        let override_policy = f.selection(&["**/*.cs"], &[]);
        assert_eq!(
            override_policy.classify_content(path, b"// <auto-generated/>"),
            None
        );
        assert_eq!(
            override_policy.classify_content(path, b"a\0b"),
            Some(SkipReason::Binary)
        );
        assert_eq!(
            override_policy.classify_content(path, &[255]),
            Some(SkipReason::InvalidUtf8)
        );
    }

    #[test]
    fn autogen_suffix_is_case_insensitive_and_explicitly_overridable() {
        let f = Fixture::new();
        f.write("File.autogen.cs", "");
        f.write("Other.AUTOGEN.CS", "");
        f.write("Autogen.cs", "");
        f.write("File.autogenerated.cs", "");
        let (paths, _) = f.paths(&[], &[]);
        assert_eq!(paths, ["Autogen.cs", "File.autogenerated.cs"]);
        assert_eq!(f.paths(&["File.autogen.cs"], &[]).0, ["File.autogen.cs"]);
        assert!(f
            .paths(&["File.autogen.cs"], &["File.autogen.cs"])
            .0
            .is_empty());
    }

    #[test]
    fn vendor_heuristics_are_specific_and_do_not_exclude_tests_or_migrations() {
        let f = Fixture::new();
        for directory in [
            "vendor",
            "VENDORS",
            "Vendored",
            "Third_Party",
            "third-party",
            "ThirdParty",
            "node_modules",
            "packages",
        ] {
            f.write(&format!("{directory}/Dependency.cs"), "");
        }
        for directory in ["Tests", "Migrations", "External", "VendorLogic", "src"] {
            f.write(&format!("{directory}/Keep.cs"), "");
        }
        let (paths, stats) = f.paths(&[], &[]);
        assert_eq!(stats.skipped[&SkipReason::VendorDirectory], 8);
        assert_eq!(
            paths,
            [
                "External/Keep.cs",
                "Migrations/Keep.cs",
                "Tests/Keep.cs",
                "VendorLogic/Keep.cs",
                "src/Keep.cs"
            ]
        );
        assert_eq!(
            f.paths(&["Third_Party/**/*.cs"], &[]).0,
            ["Third_Party/Dependency.cs"]
        );
        assert!(f
            .paths(&["Third_Party/**/*.cs"], &["Third_Party/**"])
            .0
            .is_empty());
    }

    #[test]
    fn credible_generated_headers_support_bom_casing_and_tool_warnings() {
        let f = Fixture::new();
        let policy = f.selection(&[], &[]);
        let explicit = f.selection(&["**/*.cs"], &[]);
        let path = Path::new("File.cs");
        for source in [
            "\u{feff}// <AuTo-GeNeRaTeD />\nclass A {}",
            "/*\n * <autogenerated>\n */\nclass A {}",
            "/// <auto-generated tool=\"Example\">\nclass A {}",
            "// This code was generated by a tool.\nclass A {}",
            "// Generated by ExampleGenerator. DO NOT EDIT!\nclass A {}",
            "// Code generated by ExampleGenerator.\n// DO NOT EDIT.\nclass A {}",
        ] {
            assert_eq!(
                policy.classify_content(path, source.as_bytes()),
                Some(SkipReason::GeneratedHeader),
                "{source}"
            );
            assert_eq!(
                explicit.classify_content(path, source.as_bytes()),
                None,
                "{source}"
            );
        }
    }

    #[test]
    fn mentions_of_generated_code_are_not_generated_file_declarations() {
        let f = Fixture::new();
        let policy = f.selection(&[], &[]);
        for source in [
            "// Detects the <auto-generated> header in other files.\nclass Detector {}",
            "// Example: This code was generated by a tool.\nclass Example {}",
            "// \"This code was generated by a tool.\" is a recognized marker.\nclass A {}",
            "// <auto-generated-parser> documentation\nclass A {}",
            "// Generated by ExampleGenerator is a phrase we match.\nclass A {}",
            "// DO NOT EDIT configuration before reading the guide.\nclass A {}",
            "class A { string text = \"Generated by X. DO NOT EDIT!\"; }",
            "// Normal header\nclass A {}\n// <auto-generated>",
        ] {
            assert_eq!(
                policy.classify_content(Path::new("File.cs"), source.as_bytes()),
                None,
                "{source}"
            );
        }
    }

    #[test]
    fn generated_header_inspection_is_bounded_and_utf8_safe() {
        let f = Fixture::new();
        let policy = f.selection(&[], &[]);
        let source = format!("// {}\n// <auto-generated>\nclass A {{}}", "é".repeat(3000));
        assert_eq!(
            policy.classify_content(Path::new("File.cs"), source.as_bytes()),
            None
        );
        assert_eq!(
            policy.classify_content(Path::new("File.cs"), "\u{feff}class A {}".as_bytes()),
            None
        );
    }

    #[test]
    fn invalid_glob_root_and_callback_failures_are_errors() {
        let f = Fixture::new();
        f.write("a.cs", "");
        assert!(Selection::new(&f.0, &["[".into()], &[]).is_err());
        assert!(Selection::new(&f.0.join("a.cs"), &[], &[]).is_err());
        assert!(Selection::new(&f.0.join("missing"), &[], &[]).is_err());
        assert_eq!(
            f.selection(&[], &[]).walk(|_| Err("cancelled".into())),
            Err("cancelled".into())
        );
    }

    #[test]
    fn directory_depth_is_bounded_even_when_includes_disable_ignore_stack() {
        let f = Fixture::new();
        let policy = f.selection(&["**/*.cs"], &[]);
        let error = policy
            .walk_directory(
                &policy.root,
                257,
                &mut Vec::new(),
                &mut SelectionStats::default(),
                &mut |_| Ok(()),
            )
            .unwrap_err();
        assert!(error.contains("depth of 256"));
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_not_followed_even_with_explicit_includes() {
        let f = Fixture::new();
        f.write("real/a.cs", "");
        std::os::unix::fs::symlink(f.0.join("real"), f.0.join("link")).unwrap();
        std::os::unix::fs::symlink(f.0.join("real/a.cs"), f.0.join("linked.cs")).unwrap();
        let (paths, stats) = f.paths(&["**/*.cs"], &[]);
        assert_eq!(paths, ["real/a.cs"]);
        assert_eq!(stats.skipped[&SkipReason::Symlink], 2);
    }
}
