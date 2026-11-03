// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT
//! Self-contained Java and Python metric regression cases.
use cpath::language::{JvmPythonParser, SourceParser};
use cpath::maintainability::{
    ApproximateMaintainability, MaintainabilityAnalyzer, MaintainabilityMetrics, UnitKind,
};
use cpath::model::ComplexityMetrics;

fn measure(python: bool, source: &str) -> (ComplexityMetrics, MaintainabilityMetrics) {
    let mut parser = if python {
        JvmPythonParser::python()
    } else {
        JvmPythonParser::java()
    }
    .unwrap();
    let analysis = parser.parse_analysis("probe".into(), source).unwrap();
    let cognitive =
        cpath::metrics::analyze_cognitive(analysis.file.profiled_cognitive.as_ref().unwrap());
    let maintainability =
        ApproximateMaintainability.analyze(analysis.maintainability.as_ref().unwrap());
    (cognitive, maintainability)
}

fn file_index(metrics: &MaintainabilityMetrics) -> f64 {
    metrics
        .units
        .iter()
        .find(|unit| unit.facts.kind == UnitKind::TopLevel)
        .unwrap()
        .index
}

fn assert_python_inputs(probe: &serde_json::Value, metrics: &MaintainabilityMetrics) {
    if probe["language"] != "python" {
        return;
    }
    let expected = &probe["inputs"];
    let actual = metrics
        .units
        .iter()
        .find(|unit| unit.facts.kind == UnitKind::TopLevel)
        .unwrap();
    for (field, count) in [
        ("N1", actual.facts.counts.operators),
        ("N2", actual.facts.counts.operands),
        ("h1", actual.facts.counts.distinct_operators),
        ("h2", actual.facts.counts.distinct_operands),
    ] {
        assert_eq!(
            count,
            expected["halstead"][field].as_u64().unwrap(),
            "{}: {field}",
            probe["name"]
        );
    }
    assert_eq!(
        actual.cyclomatic_complexity,
        expected["parameters"]["cyclomatic"].as_u64().unwrap(),
        "{}: cyclomatic",
        probe["name"]
    );
}

#[test]
fn deep_java_and_python_expressions_fit_a_small_worker_stack() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let cases = [
                (
                    false,
                    format!(
                        "class P {{ boolean f(boolean a) {{ return {}a; }} }}",
                        "a && ".repeat(10_000)
                    ),
                ),
                (
                    true,
                    format!("def f(a):\n    return {}a\n", "a and ".repeat(10_000)),
                ),
                (
                    false,
                    format!(
                        "class P {{ int f() {{ return {}1{}; }} }}",
                        "g(".repeat(5_000),
                        ")".repeat(5_000)
                    ),
                ),
                (
                    true,
                    format!("x = {}1{}\n", "f(".repeat(5_000), ")".repeat(5_000)),
                ),
            ];
            for (python, source) in cases {
                let (_, maintainability) = measure(python, &source);
                assert!(maintainability
                    .units
                    .iter()
                    .all(|unit| unit.index.is_finite()));
                assert!(!maintainability.units.is_empty() || maintainability.unscored_units > 0);
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn java_sonar_scopes_preserve_parent_ownership_and_excluded_local_methods() {
    let cases = [
        ("class Probe { void f(boolean a) { Runnable r = () -> { if(a) return; }; } }\n", 2, vec![2]),
        ("class Probe { void f(boolean a) { class Inner { void g() { if(a) return; } } } }\n", 2, vec![2, 0]),
        ("class Probe { void f(boolean a) { Runnable r = new Runnable() { public void run() { if(a) return; } }; } }\n", 2, vec![2, 0]),
        ("class Probe { static { if(true) System.out.println(1); } { if(true) System.out.println(2); } int value = true ? 1 : 2; }\n", 2, vec![1, 1]),
        ("class Probe { int f(int n) { return switch(n) { case 1 -> n > 0 ? 2 : 3; default -> 4; }; } }\n", 1, vec![1]),
        ("record Probe(int n) { Probe { if(n<0) throw new IllegalArgumentException(); } }\n", 1, vec![1]),
        ("enum Probe { A { int f(boolean a) { if(a) return 1; return 0; } }; abstract int f(boolean a); }\n", 0, vec![0, 0]),
        ("class Probe { int f(int n) { return switch(n) { case 1 -> switch(n) { case 2 -> 3; default -> 4; }; default -> 5; }; } }\n", 0, vec![0]),
        ("class Probe { void f(int n) { switch(n) { case 1: if(n>0) return; break; default: break; } } }\n", 3, vec![3]),
    ];
    for (source, total, methods) in cases {
        let (cognitive, _) = measure(false, source);
        assert_eq!(cognitive.total, total, "{source}");
        assert_eq!(
            cognitive
                .methods
                .iter()
                .map(|method| method.score)
                .collect::<Vec<_>>(),
            methods,
            "{source}"
        );
    }
}

#[test]
fn python_sonar_distinguishes_wrappers_classes_and_decorators() {
    let cases = [
        ("def outer():\n    def inner(x):\n        if x:\n            return 1\n        return 0\n    return inner\n", 1, vec![1, 1]),
        ("def outer(a):\n    x = 1\n    def inner(b):\n        if b:\n            return 1\n    if a:\n        return inner\n", 3, vec![3, 1]),
        ("def f(a):\n    class Inner:\n        def g(self):\n            if a:\n                return 1\n    return Inner\n", 1, vec![1, 1]),
        ("@decorate(True if condition else False)\ndef f(x):\n    if x:\n        return 1\n", 2, vec![2]),
        ("def f(xs):\n    return [x + 1 for x in xs if x > 0 if x < 4]\n", 0, vec![0]),
        ("def f(x):\n    match x:\n        case 1:\n            if x: return 1\n        case _:\n            return 0\n", 2, vec![2]),
    ];
    for (source, total, methods) in cases {
        let (cognitive, _) = measure(true, source);
        assert_eq!(cognitive.total, total, "{source}");
        assert_eq!(
            cognitive
                .methods
                .iter()
                .map(|method| method.score)
                .collect::<Vec<_>>(),
            methods,
            "{source}"
        );
    }
}

#[test]
fn boolean_parentheses_follow_each_pinned_teacher() {
    let (java, _) = measure(false, "class Probe { boolean f(boolean a, boolean b, boolean c, boolean d) { return a && (b && c) || d && a; } }\n");
    let (python, _) = measure(
        true,
        "def f(a,b,c,d):\n    return a and (b and c) or d and a\n",
    );
    assert_eq!(java.total, 3);
    assert_eq!(python.total, 4);
}

#[test]
fn maintainability_profiles_preserve_unrounded_published_values() {
    let cases = [
        (false, "class Probe { int f(int x) { if (x > 0) return 1; else if (x < 0) return 2; else return 3; } }\n", 84.87212339546603),
        (true, "def f(x):\n    if x > 0:\n        return 1\n    elif x < 0:\n        return 2\n    else:\n        return 3\n", 73.60511169965281),
        (true, "def f(xs):\n    return [x + 1 for x in xs if x > 0 if x < 4]\n", 83.07472846242624),
        (true, "def f(a,b,c):\n    return a < b <= c and a not in c and b is not None\n", 80.91231868811289),
        (true, "def f(x: int = 1 + 2) -> int:\n    return x + 3\n", 88.5574946685516),
        (true, "class C:\n    x: list[int] | None = None\n    y: dict[str, int | float]\n", 76.60788737769361),
        (true, "def f(x):\n    try:\n        x += 1\n    except* ValueError:\n        x += 2\n    else:\n        x += 3\n    return x\n", 70.9222281052121),
        (true, "x = 2\ns = f'''a\n{{\n{x}\n}}\nb:{x:02d}\n'''\ny = x + 1\n", 81.99083716851001),
    ];
    for (python, source, expected) in cases {
        let (_, maintainability) = measure(python, source);
        assert!(
            (file_index(&maintainability) - expected).abs() < 1e-9,
            "{source}: {} != {expected}",
            file_index(&maintainability)
        );
    }
}

#[test]
fn empty_files_follow_distinct_maintainability_profiles() {
    let (_, java) = measure(false, "");
    let (_, python) = measure(true, "");
    assert_eq!(file_index(&java), 0.0);
    assert_eq!(file_index(&python), 100.0);
}

#[test]
fn java_and_python_metrics_match_expected_values() {
    let corpus: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/jvm-python-probes/expected.json")).unwrap();
    let probes = corpus["probes"].as_array().unwrap();
    assert_eq!(probes.len(), 92);
    for probe in probes {
        let name = probe["name"].as_str().unwrap();
        let source = probe["source"].as_str().unwrap();
        let (cognitive, maintainability) = measure(probe["language"] == "python", source);
        assert_python_inputs(probe, &maintainability);
        assert_eq!(
            u64::from(cognitive.total),
            probe["cognitive"]["file_score"].as_u64().unwrap(),
            "{name}: file cognitive"
        );
        let expected_units = probe["cognitive"]["units"].as_array().unwrap();
        let method_units: Vec<_> = cognitive
            .methods
            .iter()
            .filter(|unit| {
                !matches!(
                    unit.name.as_str(),
                    "<block>" | "<static_initializer>" | "<module>"
                )
            })
            .collect();
        assert_eq!(
            method_units.len(),
            expected_units.len(),
            "{name}: cognitive unit coverage"
        );
        for (actual, expected) in method_units.iter().zip(expected_units) {
            assert_eq!(
                actual.name,
                expected["name"].as_str().unwrap(),
                "{name}: cognitive unit identity"
            );
            assert_eq!(
                u64::from(actual.score),
                expected["score"].as_u64().unwrap(),
                "{name}: {} cognitive",
                actual.name
            );
        }
        let initializer_units: Vec<_> = cognitive
            .methods
            .iter()
            .filter(|unit| matches!(unit.name.as_str(), "<block>" | "<static_initializer>"))
            .collect();
        let expected_initializers = probe["cognitive"]["initializers"]
            .as_array()
            .map_or(&[][..], |units| units.as_slice());
        assert_eq!(
            initializer_units.len(),
            expected_initializers.len(),
            "{name}: initializer coverage"
        );
        for (actual, expected) in initializer_units.iter().zip(expected_initializers) {
            assert_eq!(
                actual.name,
                expected["name"].as_str().unwrap(),
                "{name}: initializer identity"
            );
            assert_eq!(
                u64::from(actual.score),
                expected["score"].as_u64().unwrap(),
                "{name}: initializer cognitive"
            );
            assert!(
                cognitive.max_method >= actual.score,
                "{name}: initializer contributes to file ranking"
            );
            assert_eq!(
                actual.span.unwrap().start as u64,
                expected["first"]["column"].as_u64().unwrap(),
                "{name}: initializer start byte"
            );
            assert_eq!(
                actual.span.unwrap().end as u64,
                expected["end"]["column"].as_u64().unwrap(),
                "{name}: initializer end byte"
            );
        }
        let module_units: Vec<_> = cognitive
            .methods
            .iter()
            .filter(|unit| unit.name == "<module>")
            .collect();
        let expected_module = &probe["cognitive"]["module"];
        if probe["language"] == "python" {
            assert!(
                expected_module.is_object(),
                "{name}: missing official module supplement"
            );
        }
        let module_score = expected_module["score"].as_u64().unwrap_or(0);
        assert_eq!(
            module_units.len(),
            usize::from(module_score > 0),
            "{name}: module coverage"
        );
        if let Some(module) = module_units.first() {
            assert_eq!(
                u64::from(module.score),
                module_score,
                "{name}: module cognitive"
            );
            assert_eq!(
                module.span.unwrap().start as u64,
                expected_module["start_byte"].as_u64().unwrap()
            );
            assert_eq!(
                module.span.unwrap().end as u64,
                expected_module["end_byte"].as_u64().unwrap()
            );
        }
        let expected_max = expected_units
            .iter()
            .chain(expected_initializers)
            .map(|unit| unit["score"].as_u64().unwrap())
            .chain([module_score])
            .max()
            .unwrap();
        assert_eq!(
            u64::from(cognitive.max_method),
            expected_max,
            "{name}: maximum executable-unit ranking"
        );
        let expected_units = probe["maintainability"].as_array().unwrap();
        assert_eq!(
            maintainability.units.len(),
            expected_units.len(),
            "{name}: maintainability scope coverage"
        );
        for (actual, expected) in maintainability.units.iter().zip(expected_units) {
            if actual.facts.kind != UnitKind::TopLevel {
                assert_eq!(
                    actual.facts.name,
                    expected["name"].as_str().unwrap(),
                    "{name}: maintainability identity"
                );
            }
            let index = expected["index"].as_f64().unwrap();
            assert!(
                (actual.index - index).abs() < 1e-9,
                "{name}: {} MI {} != {index}",
                actual.facts.name,
                actual.index
            );
        }
    }
}

#[test]
fn java_syntax_cases_have_expected_cognitive_scores() {
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/jvm-python-coverage-probes/expected.json"
    ))
    .unwrap();
    let probes = corpus["probes"].as_array().unwrap();
    assert_eq!(probes.len(), 16);
    for probe in probes {
        let name = probe["name"].as_str().unwrap();
        let source = probe["source"].as_str().unwrap();
        let (cognitive, _) = measure(false, source);
        assert_eq!(
            u64::from(cognitive.total),
            probe["cognitive"]["file_score"].as_u64().unwrap(),
            "{name}: file cognitive"
        );
        let actual: Vec<_> = cognitive
            .methods
            .iter()
            .map(|unit| (unit.name.as_str(), u64::from(unit.score)))
            .collect();
        let expected: Vec<_> = probe["cognitive"]["units"]
            .as_array()
            .unwrap()
            .iter()
            .map(|unit| {
                (
                    unit["name"].as_str().unwrap(),
                    unit["score"].as_u64().unwrap(),
                )
            })
            .collect();
        assert_eq!(actual, expected, "{name}: method cognitive");
    }
}

#[test]
fn java_reference_mi_does_not_change_reused_cognitive_parser() {
    use cpath::metrics::jvm_python::ReferenceParse;
    use cpath::metrics::MaintainabilityInput;

    let corpus: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/jvm-python-probes/expected.json")).unwrap();
    let coverage: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/jvm-python-coverage-probes/expected.json"
    ))
    .unwrap();
    let mut parser = JvmPythonParser::java().unwrap();
    for (name, expected_parse) in [
        ("mi_qualified_empty_record", ReferenceParse::Accepted),
        ("branches", ReferenceParse::NotNeeded),
        ("mi_text_block_boundary", ReferenceParse::Accepted),
        ("qualified_record_pattern", ReferenceParse::Rejected),
        ("branches", ReferenceParse::NotNeeded),
        ("mi_text_block_boundary", ReferenceParse::Accepted),
    ] {
        let probe = corpus["probes"]
            .as_array()
            .unwrap()
            .iter()
            .chain(coverage["probes"].as_array().unwrap())
            .find(|probe| probe["name"] == name)
            .unwrap();
        let analysis = parser
            .parse_analysis("reused.java".into(), probe["source"].as_str().unwrap())
            .unwrap();
        let maintainability = analysis.maintainability.as_ref().unwrap();
        let MaintainabilityInput::JvmPython(facts) = maintainability.profile.as_ref().unwrap()
        else {
            panic!("require the Java metric profile");
        };
        assert_eq!(facts.reference_parse, expected_parse, "{name}");
        let cognitive =
            cpath::metrics::analyze_cognitive(analysis.file.profiled_cognitive.as_ref().unwrap());
        assert_eq!(
            u64::from(cognitive.total),
            probe["cognitive"]["file_score"].as_u64().unwrap(),
            "{name}: modern cognitive parser remains independent"
        );
        if let Some(expected_units) = probe["maintainability"].as_array() {
            let actual = ApproximateMaintainability.analyze(maintainability);
            assert_eq!(actual.units.len(), expected_units.len());
            for (unit, expected) in actual.units.iter().zip(expected_units) {
                assert!(
                    (unit.index - expected["index"].as_f64().unwrap()).abs() < 1e-9,
                    "{name}: pinned reference MI"
                );
            }
        }
    }
}
