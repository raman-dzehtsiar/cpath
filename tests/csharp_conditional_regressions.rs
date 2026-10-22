// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

use cpath::complexity::{CognitiveComplexity, ComplexityAnalyzer};
use cpath::language::{CSharpParser, SourceParser};

fn parsed(source: &str) -> tree_sitter::Tree {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_c_sharp::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    assert!(
        !tree.root_node().has_error(),
        "{}",
        tree.root_node().to_sexp()
    );
    tree
}

#[test]
fn conditional_lists_keep_branches_and_shared_successors_once() {
    let source = include_str!("fixtures/conditional/lists.cs");
    let tree = parsed(source);
    let shape = tree.root_node().to_sexp();
    assert_eq!(shape.matches("(conditional_expression").count(), 5);
    assert_eq!(shape.matches("(catch_clause").count(), 2);
    assert_eq!(shape.matches("(finally_clause").count(), 1);
    assert_eq!(shape.matches("(base_list").count(), 1);
    let facts = CSharpParser::new()
        .unwrap()
        .parse("lists.cs".into(), source)
        .unwrap();
    let metrics = CognitiveComplexity.analyze(&facts);
    assert_eq!(metrics.methods.len(), 1);
    let method = &metrics.methods[0];
    // Five ternaries + runtime && = 6; try-body if = 1; two catches = 2;
    // nested if and loop = 2 each; catch-filter || = 1; finally if = 1.
    // The #if/#elif Boolean conditions are metadata, not runtime decisions.
    assert_eq!(
        (
            method.name.as_str(),
            method.start_line,
            method.end_line,
            method.score
        ),
        ("M", 11, 36, 15)
    );
}

#[test]
fn malformed_conditional_lists_remain_unscored() {
    let mut parser = CSharpParser::new().unwrap();
    for source in [
        "class C { void M() { var a = new [] {\n#if A\n1 2,\n#endif\n3 }; } }",
        "class C { void M() { var a = new [] {\n#if A\n1\n#endif\n2 }; } }",
        "class C :\n#if A\nIA\n#endif\nIB { }",
        "class C { void M() { try {}\n#if A\ncatch (E) { if (true) }\n#endif\n} }",
        "class C { void M() { var a = new [] {\n#if A\n1,\n#else\n2,\n#else\n3,\n#endif\n4 }; } }",
        "class C { void M() { var a = new [] {\n#if A\n1,\n}; } }",
    ] {
        assert!(
            parser.parse("bad.cs".into(), source).is_err(),
            "unexpectedly accepted {source}"
        );
    }
}

#[test]
fn conditional_else_preserves_else_if_cost_and_shared_code_once() {
    let source = include_str!("fixtures/conditional/else.cs");
    parsed(source);
    let facts = CSharpParser::new()
        .unwrap()
        .parse("else.cs".into(), source)
        .unwrap();
    let metrics = CognitiveComplexity.analyze(&facts);
    // First chain: if + && + nested while = 4; two conditional else-if
    // clauses each cost 1 + nested decision 2; shared final else costs 1 + 2.
    // Second chain: if 1, conditional else + nested loop 3, else-if 1.
    // Last chain: if 1 + shared else 1 (while alternative) + direct else-if 1
    // + nested while 2. Only preprocessor wrappers are transparent.
    assert_eq!(metrics.total, 23);
    assert_eq!(metrics.methods.len(), 1);
    assert_eq!(
        (metrics.methods[0].start_line, metrics.methods[0].end_line),
        (2, 25)
    );
}

#[test]
fn terminal_else_cannot_be_followed_by_another_shared_else() {
    let mut parser = CSharpParser::new().unwrap();
    for source in [
        "class C { void M() { if (true) {}\n#if A\nelse {}\n#endif\nelse {} } }",
        "class C { void M() { if (true) {}\n#if A\nelse if () {}\n#endif\n} }",
        "class C { void M() {\n#if A\nelse {}\n#endif\n} }",
    ] {
        assert!(
            parser.parse("bad.cs".into(), source).is_err(),
            "unexpectedly accepted {source}"
        );
    }
}

#[test]
fn pragma_metadata_stops_at_newlines_and_preserves_runtime_decisions() {
    let source = "class C { void M(bool a, bool b) {\r\n#pragma warning suppress 56503\r\nif (a && b) {}\r\n#pragma custom if (a || b) {\r\nwhile (b) {}\r\n#pragma warning disable CS0123\r\nif (b) {}\r\n} }\r\n";
    let tree = parsed(source);
    assert_eq!(
        tree.root_node()
            .to_sexp()
            .matches("(preproc_pragma")
            .count(),
        5
    ); // includes opaque argument nodes
    let facts = CSharpParser::new()
        .unwrap()
        .parse("pragma.cs".into(), source)
        .unwrap();
    assert_eq!(CognitiveComplexity.analyze(&facts).total, 4);
    let bad = "class C { void M() {\n#pragma warning suppress 56503\nif () {}\n} }";
    assert!(CSharpParser::new()
        .unwrap()
        .parse("bad.cs".into(), bad)
        .is_err());
}

#[test]
fn transparent_directives_do_not_flatten_actual_else_blocks() {
    let cases = [
        // Three direct else-if alternatives: outer if 1 + 3 clauses + && 1.
        (
            "if (a) {} else\n#if A\nif (b) {}\n#else\n#if B\nif (a && b) {}\n#else\nif (a) {}\n#endif\n#endif\n",
            5,
        ),
        // Mixed if/block/return: outer if 1; shared else 1; direct if 1;
        // block-nested if 2. Return has no decision cost.
        (
            "if (a) {} else\n#if A\nif (b) {}\n#elif B\n{ if (b) {} }\n#else\nreturn;\n#endif\n",
            5,
        ),
        // A real block is never transparent: if 1 + else 1 + nested if 2.
        ("if (a) {} else {\n#if A\nif (b) {}\n#endif\n}\n", 4),
    ];
    for (body, expected) in cases {
        let source = format!("class C {{ void M(bool a, bool b) {{ {body} }} }}");
        parsed(&source);
        let facts = CSharpParser::new()
            .unwrap()
            .parse("else.cs".into(), &source)
            .unwrap();
        assert_eq!(
            CognitiveComplexity.analyze(&facts).total,
            expected,
            "{body}"
        );
    }
}

#[test]
fn terminated_rhs_alternatives_keep_all_expressions_and_callable_boundaries() {
    let source = include_str!("fixtures/conditional/terminated.cs");
    let tree = parsed(source);
    let shape = tree.root_node().to_sexp();
    assert_eq!(shape.matches("(conditional_expression").count(), 5);
    assert_eq!(shape.matches("(lambda_expression").count(), 2);
    let facts = CSharpParser::new()
        .unwrap()
        .parse("terminated.cs".into(), source)
        .unwrap();
    let metrics = CognitiveComplexity.analyze(&facts);
    let methods: Vec<_> = metrics
        .methods
        .iter()
        .map(|m| (m.name.as_str(), m.start_line, m.end_line, m.score))
        .collect();
    // Property: two source ternaries = 2. M: if 1, three nested ternaries
    // at 2 each, runtime && 1 = 8. Each lambda starts a new callable and
    // contributes one decision independently of M's enclosing if.
    assert_eq!(
        methods,
        [
            ("P", 8, 15, 2),
            ("M", 16, 41, 8),
            ("<lambda_expression@36>", 36, 36, 1),
            ("<lambda_expression@38>", 38, 38, 1)
        ]
    );
}

#[test]
fn terminated_alternatives_require_expressions_semicolons_and_final_else() {
    let mut parser = CSharpParser::new().unwrap();
    for body in [
        "int x =\n#if A\n1;\n#endif\n",
        "int x =\n#if A\n1;\n#else\n#endif\n",
        "int x =\n#if A\n1\n#else\n2;\n#endif\n",
        "int x =\n#if A\n1; F();\n#else\n2;\n#endif\n",
        "int x =\n#if A\n1;\n#else\n2;\n#endif\nif () {}",
    ] {
        let source = format!("class C {{ void M() {{ {body} }} }}");
        assert!(
            parser.parse("bad.cs".into(), &source).is_err(),
            "unexpectedly accepted {source}"
        );
    }
}
#[test]
fn conditional_indexer_and_branch_local_functions_preserve_callable_scopes() {
    let source = "class C {\nint this[int i] =>\n#if A\ni > 0 ? 1 : 2;\n#else\ni > 1 ? 3 : 4;\n#endif\nvoid M() {\nSystem.Action a =\n#if A\n() => { if (true) { void Local() { while (true) {} } } };\n#else\n() => { if (true) {} };\n#endif\n}\n}\n";
    parsed(source);
    let facts = CSharpParser::new()
        .unwrap()
        .parse("indexer.cs".into(), source)
        .unwrap();
    let metrics = CognitiveComplexity.analyze(&facts);
    let methods: Vec<_> = metrics
        .methods
        .iter()
        .map(|m| (m.name.as_str(), m.start_line, m.end_line, m.score))
        .collect();
    // Indexer: both ternaries 1 each. M has no direct decisions. Each lambda
    // and Local starts a fresh callable even inside conditional RHS syntax.
    assert_eq!(
        methods,
        [
            ("<indexer_declaration@2>", 2, 7, 2),
            ("M", 8, 15, 0),
            ("<lambda_expression@11>", 11, 11, 1),
            ("Local", 11, 11, 1),
            ("<lambda_expression@13>", 13, 13, 1)
        ]
    );
}

#[test]
fn long_conditional_else_chain_counts_each_source_decision_once() {
    let mut source = String::from("class C { void M(bool a) { if (a) {}\n");
    for _ in 0..512 {
        source.push_str("#if A\nelse if (a) {}\n#endif\n");
    }
    source.push_str("else {} } }\n");
    let facts = CSharpParser::new()
        .unwrap()
        .parse("chain.cs".into(), &source)
        .unwrap();
    // One shared if, 512 distinct branch-local else-if clauses, one final else.
    assert_eq!(facts.methods.len(), 1);
    assert_eq!(facts.methods[0].events.len(), 514);
    assert_eq!(CognitiveComplexity.analyze(&facts).total, 514);
}

#[test]
fn ordinary_conditionals_following_if_or_try_remain_separate_statements() {
    let source = "class C { void M(bool a, bool b) {\nif (a) { if (b) {} }\n#if DEBUG\nF();\nif (b) {}\n#endif\ntry { if (a) {} } catch (E) {}\n#if DEBUG\nif (b) {}\n#endif\nif (a) if (b) {} else {}\n} }";
    let tree = parsed(source);
    let shape = tree.root_node().to_sexp();
    assert!(!shape.contains("preproc_if_in_else"));
    // Outer if1 + nested if2 + conditional successor if1 + try-body if1
    // + catch1 + conditional successor if1 + dangling if1/nested if2/else1.
    let facts = CSharpParser::new()
        .unwrap()
        .parse("successors.cs".into(), source)
        .unwrap();
    assert_eq!(CognitiveComplexity.analyze(&facts).total, 11);
    let mut stack = vec![tree.root_node()];
    let mut dangling_else_owner = None;
    while let Some(node) = stack.pop() {
        if node.kind() == "if_statement"
            && node.start_position().row == 10
            && node.child_by_field_name("alternative").is_some()
        {
            dangling_else_owner = node
                .child_by_field_name("condition")
                .map(|n| source[n.byte_range()].to_owned());
        }
        for index in 0..node.child_count() {
            stack.push(node.child(index).unwrap());
        }
    }
    assert_eq!(dangling_else_owner.as_deref(), Some("b"));
}

#[test]
fn conditional_else_lookahead_preserves_comments_empty_groups_and_word_boundaries() {
    let mut parser = CSharpParser::new().unwrap();
    for prefix in [
        "#if A\n/* else_if is a comment */\n",
        "#if A\r\n// else fake\r\n/* multiline\r\ncomment */\r\n",
        "#if A\n#else\n",
        "#if A\n#if B\n#endif\n",
    ] {
        let source = format!(
            "class C {{ void M(bool a) {{ if (a) {{}}\n{prefix}else {{ if (a) {{}} }}\n#endif\n}} }}"
        );
        parsed(&source);
        assert_eq!(
            CognitiveComplexity
                .analyze(&parser.parse("else.cs".into(), &source).unwrap())
                .total,
            4
        );
    }
    for name in ["elsewhere", "else_name", "elseπ", "else\\u0061"] {
        let source =
            format!("class C {{ void M(bool a) {{ if (a) {{}}\n#if A\n{name}();\n#endif\n}} }}");
        parsed(&source);
        assert_eq!(
            CognitiveComplexity
                .analyze(&parser.parse("names.cs".into(), &source).unwrap())
                .total,
            1
        );
    }
    for suffix in [
        "#if A",
        "#if A\n/* unterminated",
        "#if A\nelse",
        "#if A\nelse {}",
    ] {
        let source = format!("class C {{ void M(bool a) {{ if (a) {{}}\n{suffix}");
        assert!(parser.parse("unfinished.cs".into(), &source).is_err());
    }
}

#[test]
fn conditional_else_lookahead_limit_fails_closed_without_hiding_source() {
    let comment = "x".repeat(70_000);
    let mut parser = CSharpParser::new().unwrap();
    let ordinary = format!(
        "class C {{ void M(bool a) {{ if (a) {{}}\n#if A\n/*{comment}*/ F();\n#endif\n}} }}"
    );
    // The ordinary conditional still parses normally when suffix lookahead stops.
    assert_eq!(
        CognitiveComplexity
            .analyze(&parser.parse("ordinary.cs".into(), &ordinary).unwrap())
            .total,
        1
    );
    let suffix = format!(
        "class C {{ void M(bool a) {{ if (a) {{}}\n#if A\n/*{comment}*/ else {{ if (a) {{}} }}\n#endif\n}} }}"
    );
    // A valid source outside the bounded extension is explicitly unscored.
    assert!(parser.parse("limited.cs".into(), &suffix).is_err());
}
