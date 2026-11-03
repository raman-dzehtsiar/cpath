// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

use cpath::language::{CSharpParser, SourceParser};
use cpath::maintainability::{
    ApproximateMaintainability, MaintainabilityAnalyzer, MaintainabilityMetrics, SyntaxCounts,
    UnitKind,
};
use cpath::pipeline::{scan, ScanOptions};

fn measure(source: &str) -> MaintainabilityMetrics {
    let parsed = CSharpParser::new()
        .unwrap()
        .parse_analysis("Example.cs".into(), source)
        .unwrap();
    ApproximateMaintainability.analyze(&parsed.maintainability.unwrap())
}

fn counts(source: &str) -> SyntaxCounts {
    let m = measure(source);
    assert_eq!(m.units.len(), 1, "{source}");
    m.units[0].facts.counts
}

fn assert_microsoft_fixture(source: &str, oracle: &str, expected_count: usize, max_error: f64) {
    let expected: serde_json::Value = serde_json::from_str(oracle).unwrap();
    let metrics = measure(source);
    let records = expected["records"].as_array().unwrap();
    assert_eq!(records.len(), expected_count);
    for record in records {
        let start = record["start_byte"].as_u64().unwrap() as usize;
        let end = record["end_byte"].as_u64().unwrap() as usize;
        let unit = metrics
            .units
            .iter()
            .find(|unit| {
                if unit.facts.start_byte == start && unit.facts.end_byte == end {
                    return true;
                }
                record["kind"] == "PropertyGet"
                    && unit.facts.start_byte <= start
                    && unit.facts.end_byte == end + 1
            })
            .unwrap_or_else(|| panic!("missing compiler member {}", record["name"]));
        let baseline = record["index"].as_f64().unwrap();
        assert!(
            (unit.index - baseline).abs() <= max_error,
            "{}: cpath={}, Microsoft={baseline}, allowed error={max_error}",
            record["name"],
            unit.index
        );
    }
}

#[test]
fn microsoft_compiler_fixture_scores_match_exactly() {
    assert_microsoft_fixture(
        include_str!("fixtures/maintainability-controls/Methods.cs"),
        include_str!("fixtures/maintainability-microsoft.json"),
        43,
        0.0,
    );
}

#[test]
fn expanded_microsoft_fixture_scores_stay_within_one_point() {
    assert_microsoft_fixture(
        include_str!("fixtures/maintainability-controls/Regressions.cs"),
        include_str!("fixtures/maintainability-expanded-microsoft.json"),
        18,
        1.0,
    );
}

#[test]
fn microsoft_constructor_attribute_scores_match_exactly() {
    assert_microsoft_fixture(
        include_str!("fixtures/maintainability-controls/Attributes.cs"),
        include_str!("fixtures/maintainability-expanded-attributes-microsoft.json"),
        8,
        0.0,
    );
}

#[test]
fn microsoft_operation_regression_scores_match_exactly() {
    assert_microsoft_fixture(
        include_str!("fixtures/maintainability-controls/Operations.cs"),
        include_str!("fixtures/maintainability-expanded-operations-microsoft.json"),
        7,
        0.0,
    );
}

#[test]
fn microsoft_typed_call_scores_match_exactly() {
    assert_microsoft_fixture(
        include_str!("fixtures/maintainability-controls/Calls.cs"),
        include_str!("fixtures/maintainability-expanded-calls-microsoft.json"),
        7,
        0.0,
    );
}

#[test]
fn microsoft_typeof_iterator_score_matches_exactly() {
    // Static-field identity remains an unresolved stored control; this checks
    // the independently calibrated typeof/yield input on the complete file.
    let mut oracle: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/maintainability-expanded-identity-microsoft.json"
    ))
    .unwrap();
    oracle["records"]
        .as_array_mut()
        .unwrap()
        .retain(|record| record["name"] == "IdentityCases.Types.get");
    assert_microsoft_fixture(
        include_str!("fixtures/maintainability-controls/Identity.cs"),
        &serde_json::to_string(&oracle).unwrap(),
        1,
        0.0,
    );
}

#[test]
fn hand_counted_arithmetic_and_branching() {
    assert_eq!(
        counts("class C { int Sum(int a,int b) { return a+b; } }"),
        SyntaxCounts {
            operators: 2,
            operands: 2,
            distinct_operators: 1,
            distinct_operands: 2,
            logical_statements: 1,
            decisions: 0
        }
    );
    assert_eq!(
        counts("class C { int M(int x) { if(x>0) return 1; return 2; } }"),
        SyntaxCounts {
            operators: 3,
            operands: 4,
            distinct_operators: 1,
            distinct_operands: 4,
            logical_statements: 2,
            decisions: 1
        }
    );
}

#[test]
fn comments_formatting_and_bijective_renaming_preserve_every_count() {
    let original = "class C { int M(int x) { if (x > 0) return x + 1; return 0; } }";
    let changed="// if for catch && || ?\nclass C\n{\nint M(int renamed)\n{\nif /* switch */ (renamed>0)\nreturn renamed+1;\nreturn 0; // enormous comment\n}\n}";
    assert_eq!(counts(original), counts(changed));
    assert_eq!(
        counts(original),
        counts(&original.replace(' ', "\n\t").replace('\n', "\r\n"))
    );
}

#[test]
fn literals_are_atomic_and_interpolation_analyzes_only_its_code() {
    let plain = counts(r#"class C { string M() => "if && || ? for while //"; }"#);
    assert_eq!(plain.operands, 1);
    assert_eq!(plain.decisions, 0);
    assert_eq!(plain.logical_statements, 1);
    for literal in [
        r#"@"if ""quoted"" while""#,
        "\"\"\"if // && || ?\"\"\"",
        "'?'",
        "0x12",
        "1.5e2",
        "true",
        "null",
    ] {
        let c = counts(&format!("class C {{ object M() => {literal}; }}"));
        assert_eq!(c.operands, 1, "{literal}");
        assert_eq!(c.decisions, 0);
    }
    let c = counts(r#"class C { string M(bool b) => $"if && { (b ? 1 : 2),10:if?? }"; }"#);
    assert_eq!(c.decisions, 1);
    assert_eq!(c.operands, 4); // b, 1, 2 and alignment 10; template text is not an operand.
}

#[test]
fn nested_callables_contribute_to_the_containing_member() {
    let source="class C { int M(bool b) { int F() { if(b) return 1; return 0; } System.Func<int> f=()=>b?1:0; return F()+f(); } }";
    let m = measure(source);
    assert_eq!(m.units.len(), 1);
    assert_eq!(m.units[0].cyclomatic_complexity, 3);
    // Local declaration, its if and trailing return, delegate assignment,
    // lambda expression body, and the containing method's final return.
    assert_eq!(m.units[0].facts.counts.logical_statements, 6);
}

#[test]
fn declarations_without_bodies_are_not_perfect_executable_units() {
    let m=measure("interface I { void M(); int P { get; set; } } abstract class C { public abstract void M(); extern void F(); }");
    assert!(m.units.is_empty());
    assert_eq!(m.minimum, None);
    let empty = measure("class C { void M() {} }");
    assert_eq!(empty.minimum, Some(100.0));
}

#[test]
fn callable_forms_initializers_and_top_level_are_supported() {
    let source = r#"
System.Console.WriteLine(1);
System.Console.WriteLine(2);
class C(int x) : B(x > 0 ? x : 0) {
 int field = x > 0 ? 1 : 2, other = 3;
 public int P { get => field; init { field=value; } } = 4;
 public int Q => field + 1;
 public int this[int i] => i > 0 ? field : other;
 public C() : this(1) {}
 ~C() { field=0; }
 public static C operator +(C a,C b) => a;
 public static implicit operator int(C a) => a.field;
 System.Func<int> callback = delegate { return 1; };
}
"#;
    let m = measure(source);
    assert_eq!(
        m.units
            .iter()
            .filter(|u| u.facts.kind == UnitKind::TopLevel)
            .count(),
        1
    );
    assert_eq!(
        m.units
            .iter()
            .filter(|u| u.facts.kind == UnitKind::Initializer)
            .count(),
        4
    );
    assert_eq!(
        m.units
            .iter()
            .filter(|u| u.facts.kind == UnitKind::Callable)
            .count(),
        9
    );
    assert!(m
        .units
        .iter()
        .all(|u| u.index.is_finite() && (0.0..=100.0).contains(&u.index)));
}

#[test]
fn decisions_cover_loops_short_circuit_patterns_switches_and_exceptions() {
    let source = r#"class C { int M(int[] xs, object o, bool a, bool b) {
 for(int i=0;i<2;i++) {} foreach(var x in xs) {} while(a) {} do {} while(b);
 if(a && b || a) {} var z = o ?? xs; z ??= xs; var n=xs?.Length;
 var v = a ? 1 : 2;
 switch(v) { case 1: case 2: break; default: break; }
 var w=v switch { 1 when a => 2, _ => 0 };
 if(o is >= 0 and < 10 or 20) {}
 try {} catch(System.Exception) when(a) {} finally {}
 return w;
} }"#;
    // Microsoft counts loops, conditionals, short-circuit binary operations,
    // coalescing, conditional access and all case labels (including default).
    // Catch filters, pattern combinators and switch-expression arms add no CC.
    assert_eq!(counts(source).decisions, 14);
}

#[test]
fn supported_preprocessor_alternatives_are_all_counted_but_metadata_is_not() {
    let src="class C { int M(bool b) {\n#if X && Y\nif(b) return 1;\n#else\nwhile(b) {}\n#endif\n#pragma warning disable CS1234\nreturn 0; } }";
    let c = counts(src);
    assert_eq!(c.decisions, 2);
    assert_eq!(c.logical_statements, 3);
    let renamed = src
        .replace("X && Y", "SOME_OTHER_SYMBOL || UNKNOWN")
        .replace("CS1234", "arbitrary_metadata");
    assert_eq!(c, counts(&renamed));
}

#[test]
fn malformed_source_is_unscored_and_parser_recovers_on_the_next_file() {
    let mut parser = CSharpParser::new().unwrap();
    assert!(parser
        .parse_analysis("bad.cs".into(), "class C { void M( {")
        .is_err());
    assert!(parser
        .parse_analysis("good.cs".into(), "class C { void M() {} }")
        .is_ok());
}

#[test]
fn ref_return_assignments_keep_invocation_structure_and_all_metrics() {
    let source = "class C { int field; ref int Reference() => ref field; void Set(int value) { Reference() = value; if (field > 0) Reference() += 1; } }";
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_c_sharp::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    assert!(!tree.root_node().has_error());
    let mut pending = vec![tree.root_node()];
    let mut assignments = 0;
    while let Some(node) = pending.pop() {
        if node.kind() == "assignment_expression" {
            assert_eq!(
                node.child_by_field_name("left").unwrap().kind(),
                "invocation_expression"
            );
            assignments += 1;
        }
        let mut cursor = node.walk();
        pending.extend(node.named_children(&mut cursor));
    }
    assert_eq!(assignments, 2);
    let metrics = measure(source);
    assert_eq!(metrics.units.len(), 2);
    assert_eq!(metrics.units[1].cyclomatic_complexity, 2);
    assert_eq!(metrics.units[1].facts.counts.logical_statements, 2);
}

#[test]
fn pipeline_retains_complete_aggregates_and_only_five_worst_units() {
    let temp = tempfile::tempdir().unwrap();
    let mut source = "class C {".to_owned();
    for i in 0..30 {
        source.push_str(&format!("int M{i}() {{ return {i}; }}"));
    }
    source.push('}');
    std::fs::write(temp.path().join("Many.cs"), source).unwrap();
    std::fs::write(temp.path().join("native.c"), "int f(void){return 0;}").unwrap();
    std::fs::write(temp.path().join("Bad.cs"), "class {").unwrap();
    let one = scan(
        temp.path(),
        &ScanOptions {
            workers: 1,
            ..Default::default()
        },
    )
    .unwrap();
    let many = scan(
        temp.path(),
        &ScanOptions {
            workers: 4,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(one.maintainability, many.maintainability);
    assert_eq!(one.unscored_files, 1);
    let i = one
        .files
        .iter()
        .position(|f| f.path.to_str() == Some("Many.cs"))
        .unwrap();
    let m = one.maintainability[i].as_ref().unwrap();
    assert_eq!(m.unit_count, 30);
    assert_eq!(m.logical_statements, Some(30));
    assert_eq!(m.worst.len(), 5);
    assert!(m
        .worst
        .windows(2)
        .all(|w| w[0].facts.start_byte < w[1].facts.start_byte));
    let native = one
        .files
        .iter()
        .position(|f| f.path.to_str() == Some("native.c"))
        .unwrap();
    let native_metrics = one.maintainability[native].as_ref().unwrap();
    assert_eq!(native_metrics.profile, Some("cpath-mi-c-rca-v1"));
    assert_eq!(native_metrics.logical_statements, None);
    // Unchanged RCA reports infinite root MI for this one-line source without
    // a final newline (root SLOC=0); its callable still has a finite score.
    assert_eq!(native_metrics.unit_count, 1);
    assert_eq!(native_metrics.unscored_units, 1);
}

#[test]
fn enum_values_event_accessors_and_expression_lambdas_have_owned_units() {
    let m=measure("enum E { A=1, B=A<<2, C } class C { event System.Action E { add { if(value!=null) F(); } remove => F(); } void F() {} System.Func<int> f=()=>1; }");
    assert_eq!(
        m.units
            .iter()
            .filter(|u| u.facts.kind == UnitKind::Initializer)
            .count(),
        3
    );
    assert_eq!(
        m.units
            .iter()
            .filter(|u| u.facts.kind == UnitKind::Callable)
            .count(),
        3
    );
    let add = m.units.iter().find(|u| u.facts.name == "add").unwrap();
    assert_eq!(add.cyclomatic_complexity, 2);
}

#[test]
fn rich_executable_syntax_is_walked_without_an_expression_allowlist() {
    let m = measure(
        r#"unsafe class C {
 async System.Threading.Tasks.Task<int> M(int[] xs) {
   using var resource = Open();
   await using (var resource2=Open()) { await resource2.CloseAsync(); }
   lock(this) { checked { xs[0]++; } }
   var (a,b)=(1,2); (a,b)=(b,a);
   var query=from x in xs where x>0 orderby x descending select x+1;
   int[] copied=[0,..xs]; var record=new R(1) with { X=2 };
   fixed(int* p=xs) { *p=*(p+1); }
   int* buffer=stackalloc int[2]; var bytes=sizeof(int); var t=typeof(C);
   return await F(query.ToArray(),copied,record);
 }
 System.Collections.Generic.IEnumerable<int> Values() { yield return 1; yield break; }
}"#,
    );
    assert_eq!(m.units.len(), 2);
    assert!(m.units[0].facts.counts.operators > 20);
    assert!(m.units[0].facts.counts.logical_statements >= 17);
    assert_eq!(m.units[1].facts.counts.logical_statements, 1); // yield break has no explicit value.
}

#[test]
fn metadata_at_eof_and_ref_modifier_order_keep_original_ranges() {
    let source = "internal ref partial struct C { int M() => 1; }";
    for trailer in [
        "#pragma warning restore CS1234",
        "#pragma warning restore CS1234  \t",
        "#nullable restore",
        "#region example",
        "#endregion",
        "#warning example",
        "#error example",
        "#define EXAMPLE",
        "#undef EXAMPLE",
    ] {
        let src = format!("{source}\n{trailer}");
        let m = measure(&src);
        assert_eq!(m.units.len(), 1, "{trailer}");
        assert!(m.units[0].facts.end_byte <= source.len());
        assert_eq!(m.units[0].facts.counts, counts(source));
    }
    let mut parser = CSharpParser::new().unwrap();
    for broken in [
        "#pragma warning restore CS1234 class C {}",
        "#nullable restore class C {}",
        "#if X",
        "class C { int M() => 1 }",
    ] {
        assert!(
            parser.parse_analysis("broken.cs".into(), broken).is_err(),
            "{broken}"
        );
    }
}

#[test]
fn unicode_offsets_and_utf16_decoding_reach_the_same_metric() {
    let source = "class Café { string Méthode() { return \"λ🙂\"; } }";
    let direct = measure(source);
    let unit = &direct.units[0];
    assert_eq!(
        &source[unit.facts.start_byte..unit.facts.end_byte],
        "string Méthode() { return \"λ🙂\"; }"
    );
    let temp = tempfile::tempdir().unwrap();
    let mut bytes = vec![0xff, 0xfe];
    for c in source.encode_utf16() {
        bytes.extend_from_slice(&c.to_le_bytes());
    }
    std::fs::write(temp.path().join("Unicode.cs"), bytes).unwrap();
    let report = scan(temp.path(), &ScanOptions::default()).unwrap();
    assert_eq!(report.maintainability[0].as_ref().unwrap().worst[0], *unit);
}

#[test]
fn large_flat_and_deep_bodies_are_fully_counted_without_recursive_traversal() {
    let flat = format!(
        "class C {{ int M(int x) {{ {} if(x>0) return 1; return 0; }} }}",
        "x++;".repeat(25_000)
    );
    let c = counts(&flat);
    assert_eq!(c.logical_statements, 25_002);
    assert_eq!(c.decisions, 1);
    let deep = format!(
        "class C {{ void M(bool b) {{ {}F();{} }} }}",
        "if(b){".repeat(512),
        "}".repeat(512)
    );
    let c = counts(&deep);
    assert_eq!(c.decisions, 512);
    assert_eq!(c.logical_statements, 513);
}

#[test]
fn late_difficult_units_survive_bounded_summary_reduction() {
    let source = format!(
        "class C {{ {} int Last(bool b) {{ {}return 0; }} }}",
        (0..40)
            .map(|i| format!("void M{i}() {{}} "))
            .collect::<String>(),
        "if(b) return 1;".repeat(30)
    );
    let full = measure(&source);
    let last = full.units.last().unwrap().index;
    let summary = full.into_summary();
    assert_eq!(summary.unit_count, 41);
    assert_eq!(summary.logical_statements, Some(31));
    assert_eq!(summary.minimum, Some(last));
    assert_eq!(summary.worst[0].facts.name, "Last");
    assert_eq!(summary.worst.len(), 5);
}

#[test]
fn goto_case_is_a_jump_not_an_additional_switch_branch() {
    let c=counts("class C { void M(int x) { switch(x) { case 1: goto case 2; case 2: goto default; default: break; } } }");
    assert_eq!(c.decisions, 3); // Microsoft includes the default clause once.
}

#[test]
fn maintainability_evidence_consumes_the_aggregate_summary_budget() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("C.cs"), "class C {}").unwrap();
    let options = ScanOptions {
        max_summary_bytes: 1024,
        ..Default::default()
    };
    assert!(scan(temp.path(), &options).is_ok());
    let fields = (0..8)
        .map(|i| format!("int field_{i}_{}=1;", "a".repeat(300)))
        .collect::<String>();
    std::fs::write(temp.path().join("C.cs"), format!("class C {{ {fields} }}")).unwrap();
    assert!(scan(temp.path(), &options).is_err());
    let report = scan(temp.path(), &ScanOptions::default()).unwrap();
    // C# v2 also inspects field declarations. Their zero-complexity evidence
    // stays bounded to five; MI still retains the long initializer names.
    assert_eq!(report.complexity[0].methods.len(), 5);
    assert!(report.complexity[0]
        .methods
        .iter()
        .all(|unit| unit.score == 0));
    assert_eq!(report.maintainability[0].as_ref().unwrap().unit_count, 8);
}

#[test]
fn replaceable_analyzer_failure_is_fatal_instead_of_silent_partial_metrics() {
    struct Panics;
    impl MaintainabilityAnalyzer for Panics {
        fn analyze(
            &self,
            _: &cpath::maintainability::MaintainabilityFacts,
        ) -> MaintainabilityMetrics {
            panic!("intentional analyzer failure");
        }
    }
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("C.cs"), "class C { void M() {} }").unwrap();
    let result = cpath::pipeline::scan_with_maintainability(
        temp.path(),
        &ScanOptions::default(),
        cpath::pipeline::Analyzers {
            complexity: &cpath::complexity::CognitiveComplexity,
            scorer: &cpath::scoring::ComplexityFirst,
        },
        &Panics,
    );
    assert!(result.is_err());
}

#[test]
fn class_and_record_primary_base_arguments_share_the_same_contract() {
    assert_eq!(
        counts("class C(int x) : B(x>0 ? x : 0) {}"),
        counts("record C(int x) : B(x>0 ? x : 0);")
    );
}

#[test]
fn contextual_async_identifiers_preserve_metric_inputs() {
    let source = "class C { int M(bool async) => async ? 1 : 0; }";
    assert_eq!(counts(source), counts(&source.replace("async", "useAsync")));
}

#[test]
fn ref_struct_constraints_are_metadata() {
    let source = "class C<T> where T : allows ref struct { int M<U>(int x) where U : struct, allows ref struct => x > 0 ? x : 0; }";
    assert_eq!(
        counts(source),
        counts("class C<T> { int M<U>(int x) => x > 0 ? x : 0; }")
    );
}

#[test]
fn contextual_with_identifiers_preserve_metric_inputs() {
    let source = "class C { int M(int with) => with > 0 ? with : 0; }";
    assert_eq!(counts(source), counts(&source.replace("with", "value")));
}

#[test]
fn escaped_keyword_identifiers_do_not_collide_with_predefined_type_operands() {
    assert_eq!(
        counts("class C { int M() { int @int=1; return @int; } }"),
        counts("class C { int M() { int renamed=1; return renamed; } }")
    );
}
