// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

use cpath::complexity::{CognitiveComplexity, ComplexityAnalyzer};
use cpath::language::{CFamilyParser, CSharpParser, SourceParser};

fn tree(source: &str, language: tree_sitter::Language) -> tree_sitter::Tree {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&language).unwrap();
    let tree = parser.parse(source, None).unwrap();
    assert!(
        !tree.root_node().has_error(),
        "{}",
        tree.root_node().to_sexp()
    );
    tree
}

#[test]
fn pointer_array_new_preserves_bounds_placement_and_callable_evidence() {
    let source = "void allocate(bool b, void *p) {\n  if (b) {\n    auto a = new (p) int **[b ? 1 : 2][3];\n    auto c = new int * const[2];\n  }\n}\nint unchanged(int *p, int n) { return *p * n; }\n";
    let parsed = tree(source, tree_sitter_cpp::LANGUAGE.into());
    let shape = parsed.root_node().to_sexp();
    assert_eq!(shape.matches("(new_expression").count(), 2);
    assert_eq!(shape.matches("(abstract_pointer_declarator").count(), 3);
    assert_eq!(shape.matches("(new_declarator").count(), 3);
    assert!(shape.contains("placement: (argument_list"));
    assert!(shape.contains("length: (conditional_expression"));
    let facts = CFamilyParser::cpp()
        .unwrap()
        .parse("new.cpp".into(), source)
        .unwrap();
    let metrics = CognitiveComplexity.analyze(&facts);
    let methods: Vec<_> = metrics
        .methods
        .iter()
        .map(|m| (m.name.as_str(), m.start_line, m.end_line, m.score))
        .collect();
    // if = 1; conditional bound nested in its body = 2. Dereference and
    // multiplication are not decisions. Allocations create no callable scopes.
    assert_eq!(methods, [("allocate", 1, 6, 3), ("unchanged", 7, 7, 0)]);
}

#[test]
fn malformed_pointer_allocations_remain_unscored() {
    let mut parser = CFamilyParser::cpp().unwrap();
    for source in [
        "void f() { auto p = new int *[true ? 1 : ]; }",
        "void f() { auto p = new int **[2; }",
    ] {
        assert!(parser.parse("bad.cpp".into(), source).is_err());
    }
}

#[test]
fn bare_list_slices_do_not_become_range_expressions_or_missing_postfix_operators() {
    let source = "class C {\n  bool M(int[] a) {\n    if (a is [.., var last] && last > 0) return true;\n    return a is [1, ..] || a is [..];\n  }\n  int[] N(int[] a) => [.. a, 1];\n  int[] R(int[] a) => a[1..];\n  bool P(int[] a) => a is [.. var rest, 1];\n}\n";
    let parsed = tree(source, tree_sitter_c_sharp::LANGUAGE.into());
    let shape = parsed.root_node().to_sexp();
    assert_eq!(shape.matches("(slice_pattern").count(), 4);
    assert_eq!(shape.matches("(range_expression").count(), 1);
    assert_eq!(shape.matches("(spread_element").count(), 1);
    let facts = CSharpParser::new()
        .unwrap()
        .parse("slices.cs".into(), source)
        .unwrap();
    let metrics = CognitiveComplexity.analyze(&facts);
    // if 1 + && group 1 + || group 1; patterns/ranges/spreads add no cost.
    assert_eq!(metrics.max_method, 3);
    assert_eq!(metrics.total, 3);
    assert_eq!(metrics.methods.len(), 4);
    assert_eq!(
        (metrics.methods[0].start_line, metrics.methods[0].end_line),
        (2, 5)
    );
}

#[test]
fn file_based_directives_are_header_metadata_and_preserve_code_locations() {
    let source = "#!/usr/bin/env dotnet\n#:sdk Microsoft.NET.Sdk\n#:property TargetFramework=net10.0\n#:package Example@1.0\n#:project ../Example/Example.csproj\n#:include ../shared/*.cs\n// References are never resolved, even when absent on disk.\nint F(bool b) { if (b) return 1; return 0; }\n";
    let parsed = tree(source, tree_sitter_c_sharp::LANGUAGE.into());
    assert_eq!(
        parsed
            .root_node()
            .to_sexp()
            .matches("(file_based_directive")
            .count(),
        5
    );
    let facts = CSharpParser::new()
        .unwrap()
        .parse("app.cs".into(), source)
        .unwrap();
    let metrics = CognitiveComplexity.analyze(&facts);
    let f = metrics.methods.iter().find(|m| m.name == "F").unwrap();
    assert_eq!((f.start_line, f.end_line, f.score), (8, 8, 1));
    assert_eq!(metrics.total, 1);
    for invalid in [
        "class C {}\n#:project X\n",
        "#:package\nclass C {}",
        "#:unknown X\nclass C {}",
        "class C {\n#:project X\n}",
    ] {
        assert!(CSharpParser::new()
            .unwrap()
            .parse("bad.cs".into(), invalid)
            .is_err());
    }
}
