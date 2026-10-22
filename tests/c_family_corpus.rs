// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

//! Hand-counted source fixtures. Expectations do not come from parser output.
use cpath::complexity::{CognitiveComplexity, ComplexityAnalyzer};
use cpath::language::{BundledParser, CFamilyParser, SourceParser};

fn verify(
    parser: &mut dyn SourceParser,
    path: &str,
    source: &str,
    expected: &[(&str, u32, u32, u32)],
) {
    let facts = parser.parse(path.into(), source).unwrap();
    let metrics = CognitiveComplexity.analyze(&facts);
    let actual: Vec<_> = metrics
        .methods
        .iter()
        .map(|method| {
            (
                method.name.as_str(),
                method.start_line,
                method.end_line,
                method.score,
            )
        })
        .collect();
    assert_eq!(actual, expected);
    assert_eq!(metrics.total, expected.iter().map(|e| e.3).sum());
    assert_eq!(
        metrics.max_method,
        expected.iter().map(|e| e.3).max().unwrap_or(0)
    );
}

#[test]
fn c_control_flow_and_preprocessor_branches_have_hand_counted_costs() {
    verify(
        &mut CFamilyParser::c().unwrap(),
        "control_flow.c",
        include_str!("fixtures/cognitive/control_flow.c"),
        &[
            // if 1, && 1, while 2, inner if 3, else-if 1, for 2,
            // inner if 3, else 1, braced if 2.
            ("branches", 1, 8, 16),
            // switch 1, if 2, goto 1, ternary 2, inner ternary 3.
            ("choices", 10, 17, 9),
            // for 1, while 2, do 1.
            ("loops", 19, 23, 4),
            // || 1 and its two && subgroups 1 each.
            ("groups", 25, 27, 3),
            // Both source branches: if 1 + while 1. Directives and macro
            // replacement bodies are not expanded or counted.
            ("configured", 30, 39, 2),
        ],
    );
}

#[test]
fn cpp_callables_reset_nesting_and_preserve_names_and_locations() {
    verify(
        &mut CFamilyParser::cpp().unwrap(),
        "callable_scopes.cpp",
        include_str!("fixtures/cognitive/callable_scopes.cpp"),
        &[
            ("choose", 2, 2, 1),
            // Constructor initializer ternary 1 + body if 1.
            ("Counter", 6, 6, 2),
            ("~Counter", 7, 7, 1),
            ("operator()", 8, 8, 1),
            ("operator bool", 9, 9, 1),
            ("run", 10, 15, 1),
            // Independent lambda: while 1 + if 2.
            ("<lambda@12>", 12, 12, 3),
            ("read", 13, 13, 1),
            // try-if 1; catch 1 + if 2 + one and/&& group 1;
            // second catch 1 + ternary 2.
            ("guarded", 18, 23, 8),
            ("range", 25, 28, 3),
            ("literals", 30, 34, 0),
        ],
    );
}

#[test]
fn declarator_wrappers_keep_function_names_and_prototypes_are_not_callables() {
    verify(&mut CFamilyParser::cpp().unwrap(), "declarators.cpp",
        "int prototype(int n);\nint *pointer(int n) { if (n) return 0; return 0; }\nint &reference(int &n) { return n; }\nint (*factory(int n))(int) { if (n) return 0; return 0; }\nint Space::C::method(int n) { return n ? 1 : 0; }",
        &[("pointer", 2, 2, 1), ("reference", 3, 3, 0), ("factory", 4, 4, 1), ("Space::C::method", 5, 5, 1)]);
}

#[test]
fn shared_headers_accept_c_specific_and_cpp_specific_syntax_without_includes() {
    let mut parser = BundledParser::new().unwrap();
    verify(
        &mut parser,
        "c_only.h",
        "int old(n)\nint n;\n{ if (n) return 1; return 0; }",
        &[("old", 1, 3, 1)],
    );
    verify(
        &mut parser,
        "cpp_only.h",
        "namespace N { template<class T> T f(T n) { return n ? n : T{}; } }",
        &[("f", 1, 1, 1)],
    );
    verify(
        &mut parser,
        "inline.C",
        "auto f() { return [] { if (true) return 1; return 0; }; }",
        &[("f", 1, 1, 0), ("<lambda@1>", 1, 1, 1)],
    );
}

#[test]
fn malformed_syntax_is_unscored_and_parser_recovers_for_next_file() {
    let mut parser = BundledParser::new().unwrap();
    for path in ["broken.c", "broken.cpp", "broken.h"] {
        let error = parser.parse(path.into(), "int broken( {\n").unwrap_err();
        assert!(error.contains("syntax at line"), "{path}: {error}");
        verify(
            &mut parser,
            path,
            "int ok(void) { return 0; }",
            &[("ok", 1, 1, 0)],
        );
    }
    assert!(parser.parse("unsupported.rs".into(), "").is_err());
}

#[test]
fn missing_nodes_also_reject_the_entire_file() {
    for mut parser in [CFamilyParser::c().unwrap(), CFamilyParser::cpp().unwrap()] {
        assert!(parser
            .parse("missing.c".into(), "int f() { if (1) return 1 }")
            .is_err());
    }
}

#[test]
fn cpp_templates_constexpr_function_try_blocks_and_generic_lambdas() {
    verify(&mut CFamilyParser::cpp().unwrap(), "modern.cpp",
        "template<class T> int classify(T n) { if constexpr (sizeof(T) > 4) { if (n) return 1; } else if (n) return 2; return 0; }\nstruct C { C(int n) try { if (n) throw n; } catch (...) { if (n) throw; } };\nauto task = []<class T>(T n) { return n ? 1 : 0; };",
        &[("classify", 1, 1, 4), ("C", 2, 2, 4), ("<lambda@3>", 3, 3, 1)]);
}

#[test]
fn native_boolean_groups_use_tree_parent_equality_and_ignore_bitwise_operators() {
    for mut parser in [CFamilyParser::c().unwrap(), CFamilyParser::cpp().unwrap()] {
        verify(&mut parser, "groups.c",
            "int f(int a, int b, int c, int d) { return a && (b || c) && d; }\nint bits(int a, int b) { return (a & b) | (a ^ b); }",
            &[("f", 1, 1, 2), ("bits", 2, 2, 0)]);
    }
    verify(
        &mut CFamilyParser::cpp().unwrap(),
        "aliases.cpp",
        "bool f(bool a, bool b, bool c) { return a or b || c; }",
        &[("f", 1, 1, 1)],
    );
}

#[test]
fn directives_keep_original_event_lines_and_do_not_resolve_includes() {
    for mut parser in [CFamilyParser::c().unwrap(), CFamilyParser::cpp().unwrap()] {
        let facts = parser.parse("directives.h".into(),
            "#include <not-installed.h>\nint f(int n) {\n#if A && B\nif (n) return 1;\n#else\nwhile (n) --n;\n#endif\nreturn 0;\n}").unwrap();
        assert_eq!(
            facts.methods[0]
                .events
                .iter()
                .map(|e| e.line)
                .collect::<Vec<_>>(),
            [4, 6]
        );
        // Valid after preprocessing, but this partial expression is not a
        // complete tree in the bundled grammars. Do not rewrite it to fit.
        assert!(parser
            .parse(
                "split.c".into(),
                "int f(int n) { return n\n#if A\n+ 1\n#endif\n; }"
            )
            .is_err());
    }
}
