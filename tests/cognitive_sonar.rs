// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT
//! Hand-audited C# v2 counts, separate from the retained v1 corpus.
use cpath::complexity::{CognitiveComplexity, ComplexityAnalyzer};
use cpath::language::{CSharpParser, SourceParser};

fn measure(source: &str) -> cpath::model::ComplexityMetrics {
    let facts = CSharpParser::new()
        .unwrap()
        .parse("audit.cs".into(), source)
        .unwrap();
    CognitiveComplexity.analyze(&facts)
}

#[test]
fn nested_scopes_recursion_patterns_and_goto_have_independent_hand_counts() {
    let metrics = measure(
        r#"
class C {
 int Recursive(int x) { if (x > 0) return Recursive(x - 1); return 0; }
 int Local(bool x) { int Nested(bool y) { if (y) return 1; return 0; } if(x) return Nested(x); return 0; }
 int StaticLocal(bool x) { static int Nested(bool y) { if (y) return 1; return 0; } if(x) return Nested(x); return 0; }
 bool Boolean(bool a, bool b, bool c, bool d) => (a && b) && (c || d);
 bool Pattern(object value) => value is int and > 0 or string;
 int Lambda(bool x) { System.Func<int> f = () => { if(x) return 1; return 0; }; return f(); }
 int Anonymous(bool x) { System.Func<int> f = delegate { if(x) return 1; return 0; }; return f(); }
 int Jump(bool x) { if(x) goto done; return 0; done: return 1; }
 int Value => true ? 1 : 2;
}
"#,
    );
    // Recursion: if 1 + one recursion increment. Local: nested if 2 + outer
    // if 1. Static local: excluded from parent, independently starts at nesting
    // one. Lambda: its if costs 2; anonymous delegates add no nesting. Jump:
    // if 1 + nested goto 2. Pattern and/or each add one.
    let scores: Vec<_> = metrics.methods.iter().map(|method| method.score).collect();
    assert_eq!(scores, [2, 3, 2, 1, 2, 2, 2, 2, 2, 1, 1, 3, 1]);
    assert_eq!(metrics.max_method, 3);
    // The file walk counts each body once, skips static local bodies, and
    // includes nested non-static bodies; it is not the sum of overlapping units.
    assert_eq!(metrics.total, 17);
}

#[test]
fn a_field_initializer_and_its_lambda_are_one_body_in_the_file_total() {
    let metrics = measure("class C { System.Func<int> callback = () => true ? 1 : 0; }");
    assert_eq!(
        metrics
            .methods
            .iter()
            .map(|method| method.score)
            .collect::<Vec<_>>(),
        [2, 2]
    );
    assert_eq!((metrics.max_method, metrics.total), (2, 2));
}

#[test]
fn recursion_requires_an_unqualified_name_and_matching_argument_count() {
    let metrics = measure(
        r#"class C {
 int M(int x) { M(x); M(x); return 0; }
 int Qualified(int x) { this.Qualified(x); return 0; }
 int Different(int x) { Different(x, x); return 0; }
 int Generic<T>(int x) { Generic<T>(x); return 0; }
 int @Escaped(int x) { Escaped(x); return 0; }
 int F\u006fo(int x) { Foo(x); return 0; }
}"#,
    );
    assert_eq!(
        metrics
            .methods
            .iter()
            .map(|method| method.score)
            .collect::<Vec<_>>(),
        [1, 0, 0, 0, 1, 1]
    );
}

#[test]
fn params_and_varargs_still_count_as_one_declared_parameter() {
    let metrics = measure("class C { int Params(params int[] values) => Params(values); int Empty(params int[] values) => Empty(); int Args(__arglist) => Args(__arglist(1)); }");
    assert_eq!(
        metrics
            .methods
            .iter()
            .map(|method| method.score)
            .collect::<Vec<_>>(),
        [1, 0, 1]
    );
}

#[test]
fn sonar_right_grouping_and_top_level_file_aggregation_are_explicit() {
    // The pinned analyzer ignores a same-operator right child. That ignored
    // child does not mark its own right child, so this shape costs two.
    let metrics =
        measure("class C { bool M(bool a,bool b,bool c,bool d) => a && (b && (c && d)); }");
    assert_eq!(metrics.max_method, 2);
    // Sonar's file metric visits global statements twice. A global callable
    // is measured once; a static local is skipped in that containing walk.
    let metrics = measure("if (true) System.Console.WriteLine(1); static int Local(bool x) { if(x) return 1; return 0; }");
    assert_eq!(
        metrics
            .methods
            .iter()
            .map(|method| method.score)
            .collect::<Vec<_>>(),
        [1, 2]
    );
    assert_eq!(metrics.total, 2);
}

#[test]
fn deconstructing_foreach_matches_the_pinned_analyzers_distinct_syntax_case() {
    let metrics = measure("class C { void M() { foreach(var (a,b) in xs) { if(a) N(); } } void Plain() { foreach(var a in xs) { if(a) N(); } } void Generic() { foreach(List<T> item in xs) { if(item) N(); } } }");
    // A generic type plus variable can have a relational-expression tree in
    // this grammar. It is still an ordinary foreach, not a deconstruction.
    assert_eq!(
        metrics
            .methods
            .iter()
            .map(|method| method.score)
            .collect::<Vec<_>>(),
        [1, 3, 3]
    );
}

#[test]
fn repeated_pattern_operators_form_sequences_including_inside_not_patterns() {
    let metrics = measure("class C { bool M(int x) => x is not (1 or 2 or 3 or 4); bool N(int x) => x is >0 and <10 and not 5; bool NotChain(int x) => x is not 1 and not 2 and not 3; }");
    assert_eq!(
        metrics
            .methods
            .iter()
            .map(|method| method.score)
            .collect::<Vec<_>>(),
        [1, 1, 1]
    );
}

#[test]
fn member_access_and_pattern_operands_do_not_split_short_circuit_sequences() {
    let metrics = measure("class C { bool M() => a?.X == b?.X && a?.Y == b?.Y && a?.Z == b?.Z; bool N() => a is not null && b is not null && c is not null; }");
    assert_eq!(
        metrics
            .methods
            .iter()
            .map(|method| method.score)
            .collect::<Vec<_>>(),
        [1, 1]
    );
}

#[test]
fn boolean_normalization_preserves_precedence_parentheses_and_opaque_contexts() {
    let metrics = measure("class C { bool A(bool a,bool b,bool c,bool d) => a || b && c || d; bool B(bool a,bool b,bool c,bool d,bool e) => ((a && b) == (c && d)) && e; bool D(bool a,bool b,bool c,bool d) => a && (b && (c && d)); bool E(bool a,bool b,bool c) => a && F(b && c); bool F(bool a) => a; }");
    assert_eq!(
        metrics
            .methods
            .iter()
            .map(|method| method.score)
            .collect::<Vec<_>>(),
        [2, 3, 2, 2, 0]
    );
}

#[test]
fn logical_regions_preserve_separate_patterns_and_parenthesized_operands() {
    let metrics = measure("class C { bool A() => x?.F() == true || y?.F() == true || z?.F() == true; bool B() => (x is not null && x.P) && (y is not null && y.P); bool D() => x is >= 1 and <= 2 || y is >= 3 and <= 4 || z is >= 5 and <= 6; bool E() => x is A or B || y is C or D; }");
    // One Boolean sequence; one parenthesized Boolean sequence; three distinct
    // pattern sequences plus one Boolean sequence; two patterns plus Boolean.
    assert_eq!(
        metrics
            .methods
            .iter()
            .map(|method| method.score)
            .collect::<Vec<_>>(),
        [1, 1, 4, 3]
    );
}

#[test]
fn indexed_null_conditional_operands_keep_the_boolean_sequence() {
    let metrics = measure("class C { bool M() => a is not null && b is not null && c.P && d?.Items[name] is not null && e.Add(name); }");
    assert_eq!(metrics.max_method, 1);
}
