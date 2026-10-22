// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

//! Explicit annotations are syntax. Unknown macros are not annotations by shape.
use cpath::complexity::{CognitiveComplexity, ComplexityAnalyzer};
use cpath::language::{BundledParser, CFamilyParser, SourceParser};

#[test]
fn microsoft_annotation_arguments_preserve_callables_and_control_flow() {
    let source = concat!(
        "__declspec(deprecated(\"use replacement\") noinline) int choose(int n) {\n",
        "  if (n) { while (n > 1) --n; }\n",
        "  return n ? n : 1;\n",
        "}\n",
        "struct __declspec(align(16)) Aligned {\n",
        "  __declspec(property(get = read, put = write)) int value;\n",
        "  int read() { return value; }\n",
        "  void write(int n) { if (n) value = n; }\n",
        "};\n",
        "_declspec(noinline) int legacy(int n) { return n ? 1 : 0; }\n",
        "__declspec() int empty(int n) { if (n) return n; return 0; }\n",
    );
    let facts = CFamilyParser::cpp()
        .unwrap()
        .parse("annotations.cpp".into(), source)
        .unwrap();
    let metrics = CognitiveComplexity.analyze(&facts);
    let methods: Vec<_> = metrics
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
    // choose: if 1, nested while 2, ternary 1. The other decisions are 1 each.
    assert_eq!(
        methods,
        [
            ("choose", 1, 4, 4),
            ("read", 7, 7, 0),
            ("write", 8, 8, 1),
            ("legacy", 10, 10, 1),
            ("empty", 11, 11, 1)
        ]
    );
    assert_eq!(metrics.total, 7);
    assert_eq!(metrics.max_method, 4);
}

#[test]
fn malformed_annotation_and_unknown_macro_prefixes_remain_unscored() {
    let mut parser = CFamilyParser::cpp().unwrap();
    for source in [
        "__declspec(align(16) int f() { return 0; }",
        "__declspec(deprecated(\"message\") int f() { return 0; }",
        "__declspec(noinline; dllexport) int f() { return 0; }",
        "__declspec(if (flag) {}) int f() { return 0; }",
        "EXPORTED int f() { if (flag) return 1; return 0; }",
        "class EXPORTED Widget { int f() { if (flag) return 1; return 0; } };",
        "#define EXPORTED __declspec(dllexport)\nEXPORTED int f() { return 0; }",
        "#define EXPORTED if (flag)\nEXPORTED int f() { return 0; }",
        "#define DECLARE_BODY(name) int name() { if (flag) return 1; return 0; }\nDECLARE_BODY(f)\n",
    ] {
        assert!(parser.parse("invalid.cpp".into(), source).is_err(), "unexpectedly accepted: {source}");
    }
}

#[test]
fn explicit_annotation_arguments_remain_structured_syntax_nodes() {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_cpp::LANGUAGE.into())
        .unwrap();
    let source = "struct __declspec(uuid(\"a-b\") align(16)) Item { __declspec(property(get = read, put = write)) int value; };";
    let tree = parser.parse(source, None).unwrap();
    assert!(!tree.root_node().has_error());
    let shape = tree.root_node().to_sexp();
    assert_eq!(shape.matches("ms_declspec_modifier").count(), 2, "{shape}");
    assert_eq!(shape.matches("argument_list").count(), 3, "{shape}");
    assert_eq!(shape.matches("assignment_expression").count(), 2, "{shape}");
    assert!(!shape.contains("function_definition"), "{shape}");
}

#[test]
fn function_shape_guard_keeps_legitimate_declarator_wrappers() {
    let source = concat!(
        "struct T { int value; T() {} ~T() {} operator int&() { return value; } operator int*() { return &value; } };\n",
        "T::operator int&() { return value; }\n",
        "int (*factory(int n))(int) { if (n) return 0; return 0; }\n",
        "int (&reference())(int) { return target; }\n",
        "auto trailing(int n) -> int { return n ? 1 : 0; }\n",
        "int (parenthesized)(int n) { if (n) return 1; return 0; }\n",
        "struct T empty{}; struct T initialized{1};\n",
        "UNKNOWN_TYPE typed() { return {}; }\n",
    );
    let facts = CFamilyParser::cpp()
        .unwrap()
        .parse("wrappers.cpp".into(), source)
        .unwrap();
    let metrics = CognitiveComplexity.analyze(&facts);
    assert_eq!(metrics.methods.len(), 10);
    assert_eq!(metrics.total, 3);
    assert_eq!(metrics.max_method, 1);
    for (name, line) in [
        ("factory", 3),
        ("reference", 4),
        ("trailing", 5),
        ("parenthesized", 6),
        ("typed", 8),
    ] {
        assert!(
            metrics
                .methods
                .iter()
                .any(|method| method.name == name && method.start_line == line),
            "missing {name}: {:?}",
            metrics.methods
        );
    }
}

#[test]
fn error_free_class_macro_misparse_does_not_create_a_fabricated_callable() {
    let mut syntax = tree_sitter::Parser::new();
    syntax
        .set_language(&tree_sitter_cpp::LANGUAGE.into())
        .unwrap();
    for source in [
        "class EXPORTED Widget { int f() { if (flag) return 1; return 0; } };",
        // A function-shaped parameter type elsewhere in the tree does not prove
        // that Widget has a function declarator of its own.
        "class EXPORTED Widget { int (*callback)(int); };",
    ] {
        let tree = syntax.parse(source, None).unwrap();
        assert!(
            !tree.root_node().has_error(),
            "this guards an error-free ambiguity: {source}"
        );
        let error = CFamilyParser::cpp()
            .unwrap()
            .parse("macro.cpp".into(), source)
            .unwrap_err();
        assert!(
            error.contains("ambiguous C++ callable declaration at line 1, column 1"),
            "{error}"
        );
        assert!(error.contains("unresolved macro"), "{error}");
    }
}

#[test]
fn declaration_modifier_metadata_does_not_create_complexity_or_callables() {
    let source = "int f(int n) { __declspec(align((n && flag) ? 16 : 32)) int value; __declspec(custom([] { if (flag) return 1; return 0; })) int other; if (n) return 1; return 0; }";
    // Attribute semantics are not type-checked. Even when an extension argument
    // has expression-shaped syntax, it is metadata rather than a callable body.
    let facts = CFamilyParser::cpp()
        .unwrap()
        .parse("metadata.cpp".into(), source)
        .unwrap();
    let metrics = CognitiveComplexity.analyze(&facts);
    assert_eq!(metrics.methods.len(), 1);
    assert_eq!(metrics.methods[0].name, "f");
    assert_eq!(metrics.methods[0].score, 1);
}

#[test]
fn annotated_class_header_uses_cpp_guard_after_c_rejection() {
    let source = "class EXPORTED Widget { int f() { if (flag) return 1; return 0; } };";
    assert!(CFamilyParser::c()
        .unwrap()
        .parse("macro.h".into(), source)
        .is_err());
    let error = BundledParser::new()
        .unwrap()
        .parse("macro.h".into(), source)
        .unwrap_err();
    assert!(
        error.contains("ambiguous C++ callable declaration"),
        "{error}"
    );
}

#[test]
fn struct_annotation_ambiguity_is_rejected_by_shared_header_fallback() {
    let source = "struct EXPORTED Widget { int (*callback)(int); };";
    let error = BundledParser::new()
        .unwrap()
        .parse("macro.h".into(), source)
        .unwrap_err();
    assert!(error.contains("ambiguous"), "{error}");
}

#[test]
fn declaration_only_cpp_headers_fall_back_without_fabricated_callables() {
    for source in [
        "class Plain { public: Plain(); void declared(); private: int value; };",
        "namespace Containers { void declared(); namespace Inner { int other(); } }",
    ] {
        let error = CFamilyParser::c()
            .unwrap()
            .parse("declarations.h".into(), source)
            .unwrap_err();
        assert!(
            error.contains("ambiguous C callable declaration"),
            "{error}"
        );
        let facts = BundledParser::new()
            .unwrap()
            .parse("declarations.h".into(), source)
            .unwrap();
        assert!(facts.methods.is_empty(), "{:?}", facts.methods);
    }
}

#[test]
fn c_function_declarator_guard_keeps_old_style_and_pointer_returns() {
    let source = "int old(n) int n; { if (n) return 1; return 0; }\nint *pointer(int n) { if (n) return 0; return 0; }\nint (*factory(int n))(int) { if (n) return 0; return 0; }\n";
    let facts = CFamilyParser::c()
        .unwrap()
        .parse("legacy.c".into(), source)
        .unwrap();
    let metrics = CognitiveComplexity.analyze(&facts);
    assert_eq!(metrics.methods.len(), 3, "{:?}", metrics.methods);
    assert_eq!(metrics.total, 3);
    assert_eq!(metrics.max_method, 1);
}

#[test]
fn omitted_old_style_pointer_function_is_unscored_instead_of_zero() {
    // Valid legacy C, but the bundled C grammar sees a declaration followed by
    // an error-free file-scope block and would otherwise silently omit its body.
    for source in [
        "int *pointer(n) int n; { if (n) return 0; return 0; }\n",
        "#if PLATFORM\nint *pointer(n) int n; { if (n) return 0; return 0; }\n#endif\n",
    ] {
        let error = CFamilyParser::c()
            .unwrap()
            .parse("legacy.c".into(), source)
            .unwrap_err();
        assert!(error.contains("ambiguous C file-scope block"), "{error}");
    }
}

#[test]
fn block_guard_keeps_namespaces_linkage_and_gnu_statement_expressions() {
    let source = "namespace N { int f(int n) { if (n) return 1; return 0; } }\nextern \"C\" { int g(int n) { if (n) return 1; return 0; } }\nint value = ({ int n = 0; n; });\n";
    let facts = CFamilyParser::cpp()
        .unwrap()
        .parse("blocks.cpp".into(), source)
        .unwrap();
    let metrics = CognitiveComplexity.analyze(&facts);
    assert_eq!(metrics.methods.len(), 2);
    assert_eq!(metrics.total, 2);
}

#[test]
fn direct_lambda_captures_keep_initializer_decisions_and_body_scope() {
    let source = concat!(
        "int outer(int n) {\n",
        "  auto f = [x{n ? 1 : 2}, y(n ? 3 : 4), &ref(n)]() {\n",
        "    if (x && y) return ref;\n",
        "    return 0;\n",
        "  };\n",
        "  return f();\n",
        "}\n",
    );
    let facts = CFamilyParser::cpp()
        .unwrap()
        .parse("captures.cpp".into(), source)
        .unwrap();
    let metrics = CognitiveComplexity.analyze(&facts);
    // Existing callable convention assigns capture syntax to the lambda:
    // two initializer ternaries + body if + Boolean group = 4.
    assert_eq!(metrics.methods.len(), 2);
    assert_eq!(
        (metrics.methods[0].name.as_str(), metrics.methods[0].score),
        ("outer", 0)
    );
    assert_eq!(
        (
            metrics.methods[1].name.as_str(),
            metrics.methods[1].start_line,
            metrics.methods[1].end_line,
            metrics.methods[1].score
        ),
        ("<lambda@2>", 2, 5, 4)
    );
    let mut syntax = tree_sitter::Parser::new();
    syntax
        .set_language(&tree_sitter_cpp::LANGUAGE.into())
        .unwrap();
    let tree = syntax.parse(source, None).unwrap();
    let shape = tree.root_node().to_sexp();
    assert_eq!(
        shape.matches("lambda_capture_initializer").count(),
        3,
        "{shape}"
    );
    assert_eq!(
        shape.matches("conditional_expression").count(),
        2,
        "{shape}"
    );
    assert_eq!(shape.matches("initializer_list").count(), 1, "{shape}");
}

#[test]
fn scoped_attribute_feature_tests_do_not_change_callable_scores() {
    let source = "#if __has_cpp_attribute(gnu::fallthrough) && defined(PLATFORM)\nint a(int n) { if (n) return 1; return 0; }\n#elif __has_cpp_attribute(clang::fallthrough)\nint b(int n) { return n ? 1 : 0; }\n#endif\n";
    let facts = CFamilyParser::cpp()
        .unwrap()
        .parse("features.cpp".into(), source)
        .unwrap();
    let metrics = CognitiveComplexity.analyze(&facts);
    assert_eq!(metrics.methods.len(), 2);
    assert_eq!(metrics.total, 2);
    assert_eq!(metrics.methods[0].start_line, 2);
    assert_eq!(metrics.methods[1].start_line, 4);
    for malformed in [
        "#if __has_cpp_attribute(gnu::)\nint a();\n#endif\n",
        "auto f = [x{1] { return 0; };",
        "auto f = [x(1] { return 0; };",
        "auto f = [x{1} y{2}] { return 0; };",
    ] {
        assert!(
            CFamilyParser::cpp()
                .unwrap()
                .parse("malformed.cpp".into(), malformed)
                .is_err(),
            "{malformed}"
        );
    }
}

#[test]
fn direct_capture_ambiguity_keeps_subscripts_and_ordinary_calls() {
    let source = "int f(int n) { int a[2]; return a[index(n ? 0 : 1)]; }\nauto g = [x = pick(flag ? 1 : 2)] { return x; };\n";
    let facts = CFamilyParser::cpp()
        .unwrap()
        .parse("expressions.cpp".into(), source)
        .unwrap();
    let metrics = CognitiveComplexity.analyze(&facts);
    assert_eq!(metrics.methods.len(), 2);
    assert_eq!(metrics.methods[0].name, "f");
    assert_eq!(metrics.methods[0].score, 1);
    assert_eq!(metrics.methods[1].name, "<lambda@2>");
    assert_eq!(metrics.methods[1].score, 1);
}

#[test]
fn ordinary_declarations_do_not_hide_functions_or_fabricate_instantiations() {
    let source = concat!(
        "struct Item {};\n",
        "int choose(Item value = {}, int n = 0) { if (n) return 1; return 0; }\n",
        "enum : unsigned { first = 1, second = 2 };\n",
        "template<class T> struct Box { int run(int n) { return n ? 1 : 0; } };\n",
        "template class Box<int>;\n",
        "extern template struct Box<long>;\n",
        "template<class T> T identity(T x) { return x; }\n",
        "template int identity<int>(int);\n",
    );
    let facts = CFamilyParser::cpp()
        .unwrap()
        .parse("declarations.cpp".into(), source)
        .unwrap();
    let metrics = CognitiveComplexity.analyze(&facts);
    let methods: Vec<_> = metrics
        .methods
        .iter()
        .map(|m| (m.name.as_str(), m.start_line, m.end_line, m.score))
        .collect();
    assert_eq!(
        methods,
        [("choose", 2, 2, 1), ("run", 4, 4, 1), ("identity", 7, 7, 0)]
    );
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_cpp::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    assert!(!tree.root_node().has_error());
    let shape = tree.root_node().to_sexp();
    assert_eq!(
        shape.matches("template_instantiation").count(),
        3,
        "{shape}"
    );
    assert!(
        shape.contains("default_value: (initializer_list)"),
        "{shape}"
    );
    for malformed in [
        "void f(Item value = {);",
        "enum : { value = 1 };",
        "template class Box<int>; extra broken syntax",
        "extern template;",
    ] {
        assert!(
            CFamilyParser::cpp()
                .unwrap()
                .parse("broken.cpp".into(), malformed)
                .is_err(),
            "{malformed}"
        );
    }
}

#[test]
fn pack_capture_initializers_keep_the_lambda_boundary() {
    let source = "template<class... T> auto pack_capture(T... args) { return [...xs{args}] { return (xs + ...); }; }";
    let facts = CFamilyParser::cpp()
        .unwrap()
        .parse("pack.cpp".into(), source)
        .unwrap();
    let metrics = CognitiveComplexity.analyze(&facts);
    assert_eq!(metrics.methods.len(), 2);
    assert_eq!(metrics.methods[0].name, "pack_capture");
    assert_eq!(metrics.methods[1].name, "<lambda@1>");
    assert_eq!(metrics.total, 0);
}

#[test]
fn attributed_declarator_wrappers_keep_their_function_shape() {
    let source = concat!(
        "[[nodiscard]] int standard(int n) { return n ? 1 : 0; }\n",
        "__attribute__((noinline)) int gnu(int n) { if (n) return 1; return 0; }\n",
        "int *pointer(int n) __attribute__((noinline)) { if (n) return 0; return 0; }\n",
        "int &reference(int &n) __attribute__((noinline)) { if (n) return n; return n; }\n",
        "int wrapped(int n) [[gnu::noinline]] { if (n) return 1; return 0; }\n",
    );
    let facts = CFamilyParser::cpp()
        .unwrap()
        .parse("attributes.cpp".into(), source)
        .unwrap();
    let metrics = CognitiveComplexity.analyze(&facts);
    assert_eq!(metrics.methods.len(), 5);
    assert_eq!(metrics.total, 5);
    assert_eq!(metrics.max_method, 1);
}
