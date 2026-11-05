// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

struct Repository {
    directory: TempDir, // Isolated Git repository; never modifies the checkout under test.
}

impl Repository {
    fn new() -> Self {
        let repository = Self {
            directory: tempfile::tempdir().unwrap(),
        };
        repository.git(&["init", "-q"]);
        repository.git(&["config", "user.name", "Gate test"]);
        repository.git(&["config", "user.email", "gate@example.invalid"]);
        repository.git(&["config", "core.autocrlf", "false"]);
        repository.git(&["config", "commit.gpgsign", "false"]);
        repository.commit();
        repository
    }

    fn path(&self) -> &Path {
        self.directory.path()
    }

    fn git(&self, arguments: &[&str]) -> String {
        let result = Command::new("git")
            .args(arguments)
            .current_dir(self.path())
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "git {arguments:?}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        String::from_utf8(result.stdout).unwrap().trim().to_owned()
    }

    fn write(&self, path: &str, source: &str) {
        let path = self.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }

    fn commit(&self) -> String {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-qm", "fixture", "--allow-empty"]);
        self.git(&["rev-parse", "HEAD"])
    }

    fn gate(&self, arguments: &[&str]) -> (Output, Value) {
        run(self.path(), arguments)
    }
}

fn run(directory: &Path, arguments: &[&str]) -> (Output, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_cpath"))
        .arg("gate")
        .args(arguments)
        .current_dir(directory)
        .output()
        .unwrap();
    let document: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{error}: {}", String::from_utf8_lossy(&output.stdout)));
    assert_eq!(document["cpath_version"], env!("CARGO_PKG_VERSION"));
    assert!(document.get("schema_version").is_none());
    assert_eq!(
        document.get("complete").is_some(),
        arguments.contains(&"--debug")
    );
    assert_eq!(
        output.status.code().unwrap() as i64,
        document["exit_code"].as_i64().unwrap()
    );
    (output, document)
}

fn failures<'a>(document: &'a Value, metric: &str) -> Vec<&'a Value> {
    document["violations"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|value| value["metric"] == metric)
        .collect()
}

#[test]
fn regression_reports_all_scopes_and_exact_expectations() {
    let repository = Repository::new();
    let before = (0..8)
        .map(|index| format!("int f{index}(int x) {{ return x; }}\n"))
        .collect::<String>();
    let after = (0..8)
        .map(|index| format!("int f{index}(int x) {{ if (x) return 1; return 0; }}\n"))
        .collect::<String>();
    repository.write("source.c", &before);
    let from = repository.commit();
    repository.write("source.c", &after);
    let to = repository.commit();
    let (_, healthy) = repository.gate(&[&from, &to]);
    assert_eq!(healthy["status"], "passed", "{healthy}");
    let (output, document) = repository.gate(&[&from, &to, "--cc-limit", "0", "--mi-limit", "100"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(document.get("complete").is_none());
    let cc = failures(&document, "CC");
    assert_eq!(cc.len(), 8, "{document}");
    for (index, failure) in cc.iter().enumerate() {
        assert_eq!(failure["classification"], "existing");
        assert_eq!(failure["scope"], format!("f{index}"));
        assert_eq!(failure["line"], index + 1);
        assert_eq!(failure["expected"]["value"], 0.0);
        assert_eq!(failure["expected"]["operator"], "<=");
        assert_eq!(failure["actual"], 1.0);
    }
    let mi = failures(&document, "MI");
    assert_eq!(mi.len(), 8);
    assert_eq!(document["counts"]["checked_scope_metrics"], 16);
    assert!(mi.iter().all(|value| value["actual"].is_u64()));
    assert!(mi.iter().all(|value| value["previous"].is_u64()));
}

#[test]
fn file_and_class_growth_do_not_add_aggregate_gate_failures() {
    let repository = Repository::new();
    let empty = repository.git(&["rev-parse", "HEAD"]);
    repository.write("source.cpp", "class C { int f0(int x) { return x; } };\n");
    let from = repository.commit();
    let methods = (0..80)
        .map(|index| format!("int f{index}(int x) {{ return x; }}\n"))
        .collect::<String>();
    repository.write("source.cpp", &format!("class C {{\n{methods}}};\n"));
    repository.commit();
    for baseline in [&empty, &from] {
        let (_, document) = repository.gate(&[baseline, "HEAD"]);
        assert_eq!(document["status"], "passed", "{document}");
        assert!(document.get("complete").is_none());
        assert_eq!(document["counts"]["checked_scope_metrics"], 160);
    }
}

#[test]
fn new_code_uses_decimal_limits_and_language_defaults() {
    let repository = Repository::new();
    let from = repository.git(&["rev-parse", "HEAD"]);
    repository.write(
        "source.cs",
        "class C { int F(int x) { if (x > 0) return x; return 0; } }",
    );
    repository.commit();
    let (_, passed) = repository.gate(&[&from, "HEAD"]);
    assert_eq!(passed["status"], "passed", "{passed}");
    assert_eq!(passed["limits"]["CC"]["c_cpp"], 25.0);
    assert_eq!(passed["limits"]["CC"]["other"], 15.0);
    let (_, failed) = repository.gate(&[&from, "HEAD", "--cc-limit=0.5", "--mi-limit", "100.5"]);
    assert_eq!(failed["status"], "failed");
    assert_eq!(failures(&failed, "CC")[0]["expected"]["value"], 0.5);
    assert_eq!(failures(&failed, "MI")[0]["expected"]["value"], 100.5);
    assert!(failed["violations"]
        .as_array()
        .unwrap()
        .iter()
        .all(|value| value["previous"].is_null()));
    let (_, boundary) = repository.gate(&[&from, "HEAD", "--cc-limit", "1.0", "--mi-limit=0"]);
    assert_eq!(boundary["status"], "passed", "{boundary}");
}

#[test]
fn renames_and_cross_file_or_container_moves_are_new() {
    for change in ["rename", "file", "class"] {
        let repository = Repository::new();
        repository.write(
            "source.cs",
            "class A { int F(int x) { if (x > 0) return x; return 0; } }",
        );
        let from = repository.commit();
        match change {
            "rename" => repository.write(
                "source.cs",
                "class A { int G(int x) { if (x > 0) return x; return 0; } }",
            ),
            "class" => repository.write(
                "source.cs",
                "class B { int F(int x) { if (x > 0) return x; return 0; } }",
            ),
            _ => fs::rename(
                repository.path().join("source.cs"),
                repository.path().join("moved.cs"),
            )
            .unwrap(),
        }
        repository.commit();
        let (_, document) = repository.gate(&[&from, "HEAD", "--cc-limit", "0", "--mi-limit", "0"]);
        let cc = failures(&document, "CC");
        assert_eq!(cc.len(), 1, "{change}: {document}");
        assert_eq!(cc[0]["classification"], "new");
        assert!(document.get("complete").is_none());
    }
}

#[test]
fn ordering_line_shifts_comments_and_overloads_preserve_identity() {
    let repository = Repository::new();
    repository.write("source.cs", "class A {\n int F(int x) { if (x > 0) return x; return 0; }\n int F(string x) { if (x == null) return 0; return 1; }\n}");
    let from = repository.commit();
    repository.write("source.cs", "\n// extra line\nclass A {\n int F(string x) { if (x == null) return 0; return 1; }\n int F( /* note */ int x) { if (x > 0) return x; return 0; }\n}");
    repository.commit();
    let (_, document) = repository.gate(&[&from, "HEAD", "--cc-limit", "0", "--mi-limit", "100"]);
    assert_eq!(document["status"], "passed", "{document}");
    assert!(document.get("complete").is_none());
}

#[test]
fn partial_scans_fail_only_when_a_confirmed_violation_exists() {
    let repository = Repository::new();
    let from = repository.git(&["rev-parse", "HEAD"]);
    repository.write("broken.cs", "class {");
    repository.commit();
    let (output, document) = repository.gate(&[&from, "HEAD", "--debug"]);
    assert!(output.status.success());
    assert_eq!(document["status"], "incomplete");
    assert_eq!(document["diagnostics"][0]["file"], "broken.cs");
    repository.write(
        "valid.cs",
        "class C { int F(int x) { if (x > 0) return x; return 0; } }",
    );
    repository.commit();
    let (output, document) = repository.gate(&[&from, "HEAD", "--cc-limit", "0"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(document["status"], "failed");
    assert!(document.get("complete").is_none());
    assert_eq!(failures(&document, "CC").len(), 1);
}

#[test]
fn missing_baseline_is_not_reclassified_as_new_code() {
    let repository = Repository::new();
    repository.write("source.cs", "class {");
    let from = repository.commit();
    repository.write(
        "source.cs",
        "class C { int F(int x) { if (x > 0) return x; return 0; } }",
    );
    repository.commit();
    let (output, document) = repository.gate(&[
        &from,
        "HEAD",
        "--cc-limit",
        "0",
        "--mi-limit",
        "100",
        "--debug",
    ]);
    assert!(output.status.success());
    assert_eq!(document["status"], "incomplete");
    assert!(document["violations"].as_array().unwrap().is_empty());
    assert!(document["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value["side"] == "from"));
}

#[test]
fn initializer_scopes_are_excluded_from_the_gate() {
    let repository = Repository::new();
    repository.write("source.cs", "class C { int value = true ? 1 : 2; }\n");
    let from = repository.commit();
    repository.write(
        "source.cs",
        "class C { int value = true ? (false ? 1 : 2) : 3; }\n",
    );
    repository.commit();
    let (_, document) = repository.gate(&[&from, "HEAD", "--cc-limit", "0", "--mi-limit", "100"]);
    assert!(document.get("complete").is_none());
    assert_eq!(document["status"], "passed");
    assert_eq!(document["counts"]["checked_scope_metrics"], 0);
}

#[test]
fn top_level_scopes_are_excluded_from_the_gate() {
    let repository = Repository::new();
    repository.write("source.cs", "int x = 1;\nSystem.Console.WriteLine(x);\n");
    let from = repository.commit();
    repository.write(
        "source.cs",
        "int x = 1;\nif (x > 0) System.Console.WriteLine(x);\n",
    );
    repository.commit();
    let (_, document) = repository.gate(&[&from, "HEAD", "--cc-limit", "0", "--mi-limit", "100"]);
    assert!(document.get("complete").is_none());
    assert_eq!(document["status"], "passed");
    assert_eq!(document["counts"]["checked_scope_metrics"], 0);
}

#[test]
fn endpoint_comparison_ignores_intermediate_commits_and_working_tree() {
    let repository = Repository::new();
    let simple = "class C { int F(int x) { return x; } }";
    repository.write("sub/source.cs", simple);
    let from = repository.commit();
    repository.write(
        "sub/source.cs",
        "class C { int F(int x) { if (x > 0) return x; return 0; } }",
    );
    repository.commit();
    repository.write("sub/source.cs", simple);
    let to = repository.commit();
    repository.write("sub/source.cs", "uncommitted broken content");
    repository.git(&["add", "sub/source.cs"]);
    repository.write("sub/source.cs", "different unstaged content");
    let status = repository.git(&["status", "--porcelain=v1"]);
    let index = repository.git(&["ls-files", "--stage"]);
    let (output, document) = run(&repository.path().join("sub"), &[&from, &to, "--pretty"]);
    assert!(output.status.success());
    assert_eq!(document["counts"]["changed_files"], 0);
    assert_eq!(document["to"], to);
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .contains("\n    \""));
    assert_eq!(repository.git(&["status", "--porcelain=v1"]), status);
    assert_eq!(repository.git(&["ls-files", "--stage"]), index);
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), to);
    assert_eq!(
        fs::read_to_string(repository.path().join("sub/source.cs")).unwrap(),
        "different unstaged content"
    );
}

#[test]
fn every_supported_language_uses_existing_analysis_with_complete_identity_metadata() {
    let examples = [
        ("c", "int f(int x) { if (x) return 1; return 0; }"),
        ("h", "int f(int x) { if (x) return 1; return 0; }"),
        (
            "cpp",
            "class C { public: int f(int x) { if (x) return 1; return 0; } };",
        ),
        (
            "cs",
            "class C { int F(int x) { if (x > 0) return 1; return 0; } }",
        ),
        (
            "java",
            "class C { int f(int x) { if (x > 0) return 1; return 0; } }",
        ),
        ("rs", "fn f(x: i32) -> i32 { if x > 0 { 1 } else { 0 } }"),
        (
            "py",
            "def f(x):\n    if x > 0:\n        return 1\n    return 0\n",
        ),
        ("js", "function f(x) { if (x > 0) return 1; return 0; }"),
        (
            "ts",
            "function f(x: number): number { if (x > 0) return 1; return 0; }",
        ),
        (
            "tsx",
            "function F(x: number) { if (x > 0) return <div />; return <span />; }",
        ),
        (
            "go",
            "package main\nfunc f(x int) int { if x > 0 { return 1 }; return 0 }",
        ),
    ];
    let repository = Repository::new();
    let from = repository.git(&["rev-parse", "HEAD"]);
    for (extension, source) in examples {
        repository.write(&format!("source.{extension}"), &format!("{source}\n"));
    }
    repository.commit();
    let (_, document) = repository.gate(&[&from, "HEAD", "--cc-limit", "0", "--mi-limit", "0"]);
    assert!(document.get("complete").is_none());
    assert_eq!(document["counts"]["analyzed_files"], examples.len());
    for (extension, _) in examples {
        assert!(
            failures(&document, "CC")
                .iter()
                .any(|value| value["file"] == format!("source.{extension}")),
            "{extension}: {document}"
        );
    }
    let existing = repository.git(&["rev-parse", "HEAD"]);
    for (extension, source) in examples {
        repository.write(&format!("source.{extension}"), &format!("\n{source}\n"));
    }
    repository.commit();
    let (_, shifted) = repository.gate(&[&existing, "HEAD", "--cc-limit", "0", "--mi-limit", "0"]);
    assert!(failures(&shifted, "CC").is_empty(), "{shifted}");
    assert!(shifted.get("complete").is_none());
}

#[test]
fn ambiguous_anonymous_scopes_are_reported_without_guessing() {
    let repository = Repository::new();
    let from = repository.git(&["rev-parse", "HEAD"]);
    repository.write("source.js", "function f(xs) {\n xs.map(x => { if (x) return 1; return 0; });\n xs.map(x => { if (x) return 2; return 0; });\n}\n");
    repository.commit();
    let (_, document) = repository.gate(&[
        &from,
        "HEAD",
        "--cc-limit",
        "100",
        "--mi-limit",
        "0",
        "--debug",
    ]);
    assert_eq!(document["status"], "incomplete", "{document}");
    assert!(document["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value["reason"].as_str().unwrap().contains("ambiguous")));
    assert!(document["violations"].as_array().unwrap().is_empty());
}

#[test]
fn file_scoped_namespaces_and_packages_are_part_of_declaration_identity() {
    for (extension, before, after) in [
        (
            "cs",
            "namespace A; class C { int F(int x) { if (x > 0) return 1; return 0; } }\n",
            "namespace B; class C { int F(int x) { if (x > 0) return 1; return 0; } }\n",
        ),
        (
            "java",
            "package a; class C { int f(int x) { if (x > 0) return 1; return 0; } }\n",
            "package b; class C { int f(int x) { if (x > 0) return 1; return 0; } }\n",
        ),
        (
            "go",
            "package a\nfunc f(x int) int { if x > 0 { return 1 }; return 0 }\n",
            "package b\nfunc f(x int) int { if x > 0 { return 1 }; return 0 }\n",
        ),
    ] {
        let repository = Repository::new();
        let path = format!("source.{extension}");
        repository.write(&path, before);
        let from = repository.commit();
        repository.write(&path, after);
        repository.commit();
        let (_, document) = repository.gate(&[&from, "HEAD", "--cc-limit", "0", "--mi-limit", "0"]);
        assert_eq!(
            failures(&document, "CC").len(),
            1,
            "{extension}: {document}"
        );
        assert_eq!(failures(&document, "CC")[0]["classification"], "new");
    }
}

#[test]
fn non_utf8_source_is_nonblocking_and_missing_git_is_setup_error() {
    let repository = Repository::new();
    let from = repository.git(&["rev-parse", "HEAD"]);
    fs::write(repository.path().join("source.c"), [0xff, 0x00]).unwrap();
    repository.commit();
    let (_, document) = repository.gate(&[&from, "HEAD"]);
    assert_eq!(document["exit_code"], 0);
    assert_eq!(document["status"], "incomplete");
    let output = Command::new(env!("CARGO_BIN_EXE_cpath"))
        .args(["gate", "HEAD"])
        .env("PATH", "")
        .current_dir(repository.path())
        .output()
        .unwrap();
    let document: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(document["status"], "error");
}

#[test]
fn missing_blob_keeps_other_confirmed_violations() {
    let repository = Repository::new();
    let from = repository.git(&["rev-parse", "HEAD"]);
    repository.write("missing.cs", "class Missing { int F() { return 4; } }");
    repository.write(
        "valid.cs",
        "class Valid { int F(int x) { if (x > 0) return 1; return 0; } }",
    );
    repository.commit();
    let oid = repository.git(&["rev-parse", "HEAD:missing.cs"]);
    fs::remove_file(
        repository
            .path()
            .join(".git/objects")
            .join(&oid[..2])
            .join(&oid[2..]),
    )
    .unwrap();
    let (_, document) = repository.gate(&[&from, "HEAD", "--cc-limit", "0", "--debug"]);
    assert_eq!(document["status"], "failed", "{document}");
    assert_eq!(document["complete"], false);
    assert_eq!(failures(&document, "CC").len(), 1);
    assert!(document["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value["file"] == "missing.cs"));
}

#[test]
fn merges_compare_explicit_endpoints_without_replacing_the_base() {
    let repository = Repository::new();
    let original = "class C { int F(int x) { return x; } }";
    repository.write("source.cs", original);
    let ancestor = repository.commit();
    repository.git(&["checkout", "-qb", "target"]);
    repository.write(
        "source.cs",
        "class C { int F(int x) { if (x > 0) return 1; return 0; } }",
    );
    let target = repository.commit();
    repository.git(&["checkout", "-qb", "feature", &ancestor]);
    repository.write("feature.txt", "feature");
    let feature = repository.commit();
    let (_, explicit) = repository.gate(&[&feature, &target, "--cc-limit", "0"]);
    assert_eq!(explicit["from"], feature);
    assert!(!failures(&explicit, "CC").is_empty());
    repository.git(&["merge", "--no-ff", "-m", "merge", "target"]);
    let (_, merged) = repository.gate(&[&feature, "HEAD", "--cc-limit", "0"]);
    assert_eq!(merged["status"], "failed", "{merged}");
    assert_eq!(failures(&merged, "CC")[0]["classification"], "existing");
    let (_, single) = repository.gate(&["HEAD", "--cc-limit", "0"]);
    assert_eq!(single, merged);
    let (_, improved) = repository.gate(&[&target, &feature]);
    assert_eq!(improved["status"], "passed", "{improved}");
}

#[cfg(unix)]
#[test]
fn unusual_paths_symlinks_and_submodules_do_not_change_git_framing() {
    let repository = Repository::new();
    let from = repository.git(&["rev-parse", "HEAD"]);
    repository.write(
        "space\nnewline\tfile.cs",
        "class C { int F(int x) { if (x > 0) return 1; return 0; } }",
    );
    std::os::unix::fs::symlink("space\nnewline\tfile.cs", repository.path().join("link.cs"))
        .unwrap();
    repository.git(&["add", "-A"]);
    repository.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("160000,{from},submodule"),
    ]);
    repository.git(&["commit", "-qm", "tree entries"]);
    let (_, document) = repository.gate(&[&from, "HEAD", "--cc-limit", "0"]);
    assert!(document.get("complete").is_none());
    assert_eq!(document["counts"]["exclusions"], 2);
    assert_eq!(
        failures(&document, "CC")[0]["file"],
        "space\nnewline\tfile.cs"
    );
}

#[test]
fn compact_and_pretty_output_are_semantically_identical_and_repeatable() {
    let repository = Repository::new();
    let from = repository.git(&["rev-parse", "HEAD"]);
    repository.write(
        "source.cs",
        "class C { int F(int x) { if (x > 0) return 1; return 0; } }",
    );
    repository.commit();
    let (first, document) = repository.gate(&[&from, "HEAD", "--cc-limit", "0"]);
    let (second, _) = repository.gate(&[&from, "HEAD", "--cc-limit", "0"]);
    let (_, pretty) = repository.gate(&[&from, "HEAD", "--cc-limit", "0", "--pretty"]);
    assert_eq!(first.stdout, second.stdout);
    assert_eq!(document, pretty);
    let (_, empty) = repository.gate(&["HEAD", "HEAD"]);
    assert_eq!(empty["status"], "passed");
    assert_eq!(empty["counts"]["changed_files"], 0);
}

#[test]
fn no_arguments_use_head_and_current_staged_or_unstaged_files() {
    for stage in [false, true] {
        let repository = Repository::new();
        repository.write("sub/source.cs", "class C { int F(int x) { return x; } }\n");
        let head = repository.commit();
        repository.write(
            "sub/source.cs",
            &format!(
                "class C {{ int F(int x) {{ {} return 0; }} }}\n",
                "if (x > 0) return 1; ".repeat(16)
            ),
        );
        if stage {
            repository.git(&["add", "sub/source.cs"]);
        }
        let index_before = fs::read(repository.path().join(".git/index")).unwrap();
        let source_before = fs::read(repository.path().join("sub/source.cs")).unwrap();
        let (_, document) = run(&repository.path().join("sub"), &[]);
        assert_eq!(document["from"], head);
        assert_eq!(document["to"], "WORKTREE");
        assert_eq!(document["status"], "failed", "{document}");
        assert_eq!(failures(&document, "CC")[0]["classification"], "existing");
        assert_eq!(
            fs::read(repository.path().join(".git/index")).unwrap(),
            index_before
        );
        assert_eq!(
            fs::read(repository.path().join("sub/source.cs")).unwrap(),
            source_before
        );
        assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
    }
}

#[test]
fn working_tree_ignores_stale_index_metadata_without_refreshing_it() {
    let repository = Repository::new();
    repository.write("broken.cs", "class {");
    repository.write("README.txt", "unchanged\n");
    repository.commit();
    for path in ["broken.cs", "README.txt"] {
        fs::File::options()
            .write(true)
            .open(repository.path().join(path))
            .unwrap()
            .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000))
            .unwrap();
    }
    let index = fs::read(repository.path().join(".git/index")).unwrap();
    let (_, document) = repository.gate(&[]);
    assert_eq!(document["status"], "passed", "{document}");
    assert_eq!(document["counts"]["changed_files"], 0);
    assert_eq!(document["counts"]["analyzed_files"], 0);
    assert!(document["diagnostics"].as_array().unwrap().is_empty());
    assert_eq!(
        fs::read(repository.path().join(".git/index")).unwrap(),
        index
    );
}

#[cfg(unix)]
#[test]
fn working_tree_content_verification_preserves_unusual_paths_and_binary_changes() {
    let repository = Repository::new();
    let path = "tab\tline\n.cs";
    repository.write(path, "class C { int F(int x) { return x; } }\n");
    repository.write("binary.dat", "\0before");
    repository.commit();
    repository.write(
        path,
        "class C { int F(int x) { if (x > 0) return 1; return 0; } }\n",
    );
    repository.write("binary.dat", "\0after");
    let (_, document) = repository.gate(&["--cc-limit", "0"]);
    assert_eq!(document["status"], "failed", "{document}");
    assert_eq!(document["counts"]["changed_files"], 2);
    assert_eq!(document["counts"]["exclusions"], 1);
    assert_eq!(failures(&document, "CC")[0]["file"], path);
}

#[test]
fn working_tree_uses_latest_content_instead_of_a_staged_snapshot() {
    let repository = Repository::new();
    let simple = "class C { int F(int x) { return x; } }\n";
    let complicated = "class C { int F(int x) { if (x > 0) return 1; return 0; } }\n";
    repository.write("source.cs", simple);
    repository.commit();
    repository.write("source.cs", complicated);
    repository.git(&["add", "source.cs"]);
    repository.write("source.cs", simple);
    let index = fs::read(repository.path().join(".git/index")).unwrap();
    let (_, document) = repository.gate(&[]);
    assert_eq!(document["status"], "passed", "{document}");
    assert_eq!(
        fs::read(repository.path().join(".git/index")).unwrap(),
        index
    );
    repository.write("source.cs", complicated);
    let (_, document) = repository.gate(&["--cc-limit", "0"]);
    assert_eq!(document["status"], "failed", "{document}");
    let (_, committed) = repository.gate(&["HEAD", "HEAD"]);
    assert_eq!(committed["status"], "passed");
    assert_eq!(committed["counts"]["changed_files"], 0);
}

#[test]
fn one_argument_compares_the_selected_commit_with_its_parent_and_ignores_local_changes() {
    let repository = Repository::new();
    repository.write("source.cs", "class C { int F(int x) { return x; } }\n");
    let from = repository.commit();
    repository.write(
        "source.cs",
        "class C { int F(int x) { if (x > 0) return 1; return 0; } }\n",
    );
    let commit = repository.commit();
    let (_, current) = repository.gate(&[]);
    assert_eq!(current["status"], "passed");
    assert_eq!(current["from"], commit);
    assert_eq!(current["to"], "WORKTREE");
    repository.write("source.cs", "class C { int F(int x) { return x; } }\n");
    repository.commit();
    repository.write("source.cs", "staged invalid source");
    repository.git(&["add", "source.cs"]);
    repository.write("source.cs", "unstaged invalid source");
    repository.write("untracked.cs", "class {");
    let index = fs::read(repository.path().join(".git/index")).unwrap();
    let (_, selected) = repository.gate(&[&commit, "--cc-limit", "0"]);
    let (_, explicit) = repository.gate(&[&from, &commit, "--cc-limit", "0"]);
    assert_eq!(selected, explicit);
    assert_eq!(selected["from"], from);
    assert_eq!(selected["to"], commit);
    assert_eq!(selected["status"], "failed", "{selected}");
    assert_eq!(
        fs::read(repository.path().join(".git/index")).unwrap(),
        index
    );
    assert_eq!(
        fs::read_to_string(repository.path().join("source.cs")).unwrap(),
        "unstaged invalid source"
    );
}

#[test]
fn one_argument_without_an_available_parent_returns_a_setup_error() {
    let repository = Repository::new();
    let root = repository.git(&["rev-parse", "HEAD"]);
    let (_, document) = repository.gate(&[&root]);
    assert_eq!(document["exit_code"], 2);
    assert_eq!(document["status"], "error");
    assert_eq!(document["diagnostics"][0]["kind"], "error");
}

#[test]
fn working_tree_includes_untracked_sources_but_honors_git_ignores() {
    let repository = Repository::new();
    repository.write(".gitignore", "ignored/\n");
    repository.commit();
    let source = "class C { int F(int x) { if (x > 0) return 1; return 0; } }\n";
    repository.write("new.cs", source);
    repository.write("ignored/hidden.cs", source);
    repository.write("README.txt", "unsupported");
    let (_, document) = repository.gate(&["--cc-limit", "0"]);
    assert_eq!(document["status"], "failed", "{document}");
    assert_eq!(document["counts"]["changed_files"], 2);
    let cc = failures(&document, "CC");
    assert_eq!(cc.len(), 1);
    assert_eq!(cc[0]["classification"], "new");
    assert_eq!(cc[0]["file"], "new.cs");
    let (_, committed) = repository.gate(&["HEAD", "HEAD", "--cc-limit", "0"]);
    assert_eq!(committed["status"], "passed");
    assert_eq!(committed["counts"]["changed_files"], 0);
}

#[test]
fn staged_deletion_restored_on_disk_keeps_its_baseline() {
    let repository = Repository::new();
    repository.write("source.cs", "class C { int F(int x) { return x; } }\n");
    repository.commit();
    repository.git(&["rm", "source.cs"]);
    repository.write(
        "source.cs",
        "class C { int F(int x) { if (x > 0) return 1; return 0; } }\n",
    );
    let index = fs::read(repository.path().join(".git/index")).unwrap();
    let (_, document) = repository.gate(&["--cc-limit", "0"]);
    assert_eq!(document["status"], "failed", "{document}");
    assert_eq!(document["counts"]["changed_files"], 1);
    assert_eq!(failures(&document, "CC")[0]["classification"], "existing");
    assert_eq!(
        fs::read(repository.path().join(".git/index")).unwrap(),
        index
    );
}

#[test]
fn working_tree_deletions_and_partial_analysis_keep_gate_exit_policy() {
    let repository = Repository::new();
    repository.write("deleted.cs", "class C { int F() { return 1; } }\n");
    repository.commit();
    fs::remove_file(repository.path().join("deleted.cs")).unwrap();
    let (_, deleted) = repository.gate(&[]);
    assert_eq!(deleted["status"], "passed", "{deleted}");
    assert_eq!(deleted["counts"]["exclusions"], 1);
    repository.write("broken.cs", "class {");
    let (_, incomplete) = repository.gate(&[]);
    assert_eq!(incomplete["status"], "incomplete", "{incomplete}");
    assert_eq!(incomplete["exit_code"], 0);
    repository.write(
        "new.cs",
        "class C { int F(int x) { if (x > 0) return 1; return 0; } }\n",
    );
    let (_, failed) = repository.gate(&["--cc-limit", "0"]);
    assert_eq!(failed["status"], "failed");
    assert!(failed.get("complete").is_none());
    assert_eq!(failed["exit_code"], 1);
}

#[cfg(unix)]
#[test]
fn working_tree_never_follows_replaced_source_symlinks() {
    let repository = Repository::new();
    repository.write("dir/source.cs", "class C { int F() { return 1; } }\n");
    repository.commit();
    let outside = tempfile::tempdir().unwrap();
    fs::write(
        outside.path().join("source.cs"),
        "class C { int F(int x) { if (x > 0) return 1; return 0; } }\n",
    )
    .unwrap();
    fs::remove_file(repository.path().join("dir/source.cs")).unwrap();
    fs::remove_dir(repository.path().join("dir")).unwrap();
    std::os::unix::fs::symlink(outside.path(), repository.path().join("dir")).unwrap();
    let (_, document) = repository.gate(&["--cc-limit", "0"]);
    assert_eq!(document["status"], "passed", "{document}");
    assert_eq!(document["counts"]["analyzed_files"], 0);
}

#[test]
fn exclusions_do_not_make_coverage_incomplete_and_tracked_vendor_code_is_checked() {
    let repository = Repository::new();
    repository.write("deleted.cs", "class C { int F() { return 1; } }");
    let from = repository.commit();
    fs::remove_file(repository.path().join("deleted.cs")).unwrap();
    repository.write("README.txt", "anything");
    repository.write(
        "vendor/generated.cs",
        "// generated source\nclass C { int F(int x) { if (x > 0) return 1; return 0; } }",
    );
    repository.commit();
    let (_, document) = repository.gate(&[&from, "HEAD", "--cc-limit", "0"]);
    assert!(document.get("complete").is_none());
    assert_eq!(document["counts"]["exclusions"], 2);
    assert_eq!(failures(&document, "CC")[0]["file"], "vendor/generated.cs");
}

#[test]
fn debug_only_adds_coverage_and_exclusion_diagnostics() {
    let repository = Repository::new();
    repository.write("broken.cs", "class {");
    repository.write("README.txt", "unsupported");
    for with_violation in [false, true] {
        if with_violation {
            repository.write(
                "valid.cs",
                "class C { int F(int x) { if (x > 0) return 1; return 0; } }\n",
            );
        }
        let (_, normal) = repository.gate(&["--cc-limit", "0"]);
        let (_, mut debug) = repository.gate(&["--debug", "--cc-limit", "0", "--pretty"]);
        assert_eq!(debug["complete"], false);
        assert_eq!(normal["exit_code"], if with_violation { 1 } else { 0 });
        assert!(normal["diagnostics"].as_array().unwrap().is_empty());
        let diagnostics = debug["diagnostics"].as_array().unwrap();
        assert!(diagnostics
            .iter()
            .any(|value| value["kind"] == "unassessed" && value["file"] == "broken.cs"));
        assert!(diagnostics
            .iter()
            .any(|value| value["kind"] == "excluded" && value["file"] == "README.txt"));
        debug["diagnostics"] = serde_json::json!([]);
        debug.as_object_mut().unwrap().remove("complete");
        assert_eq!(normal, debug);
    }
    let (_, normal) = repository.gate(&["missing-revision"]);
    let (_, mut debug) = repository.gate(&["missing-revision", "--debug"]);
    assert_eq!(debug["complete"], false);
    debug.as_object_mut().unwrap().remove("complete");
    assert_eq!(normal, debug);
    assert_eq!(normal["diagnostics"][0]["kind"], "error");
    let (_, complete) = repository.gate(&["HEAD", "HEAD", "--debug"]);
    assert_eq!(complete["complete"], true);
}

#[test]
fn invalid_arguments_and_revisions_have_gate_json_and_exit_two() {
    let repository = Repository::new();
    for arguments in [
        vec!["HEAD", "HEAD", "HEAD"],
        vec!["missing-revision"],
        vec!["HEAD", "--unknown"],
        vec!["HEAD", "--cc-limit"],
        vec!["HEAD", "--cc-limit=-1"],
        vec!["HEAD", "--mi-limit=+1"],
        vec!["HEAD", "--mi-limit=NaN"],
        vec!["HEAD", "--mi-limit=inf"],
        vec!["HEAD", "--cc-limit=1e2"],
        vec!["HEAD", "--cc-limit=1.2.3"],
        vec!["HEAD", "--cc-limit=1", "--cc-limit=2"],
        vec!["HEAD", "--pretty", "--pretty"],
        vec!["HEAD", "--debug", "--debug"],
        vec!["HEAD", "--debug=true"],
        vec!["HEAD", "--mi-limit="],
    ] {
        let (output, document) = repository.gate(&arguments);
        assert_eq!(output.status.code(), Some(2), "{arguments:?}: {document}");
        assert_eq!(document["status"], "error");
        assert_eq!(document["diagnostics"][0]["kind"], "error");
    }
    let directory = tempfile::tempdir().unwrap();
    let (_, document) = run(directory.path(), &["HEAD"]);
    assert_eq!(document["exit_code"], 2);
}
