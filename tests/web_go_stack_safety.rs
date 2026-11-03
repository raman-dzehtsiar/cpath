// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT
use cpath::language::{SourceParser, WebGoParser};

#[test]
fn typescript_shift_chains_remain_scoreable_with_generic_type_arguments() {
    let (sender, receiver) = std::sync::mpsc::channel();
    let worker = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || {
            for mut parser in [
                WebGoParser::typescript().unwrap(),
                WebGoParser::tsx().unwrap(),
            ] {
                for mixed in [false, true] {
                    let mut source = format!("const value = {}value;", "value << ".repeat(10_000));
                    if mixed {
                        source.push_str("\nfactory<<T>(value:T)=>T>(identity);");
                    }
                    let analysis = parser
                        .parse_analysis("shift-control".into(), &source)
                        .unwrap();
                    assert!(analysis.file.profiled_cognitive.is_some());
                    let Some(cpath::metrics::MaintainabilityInput::WebGo(facts)) =
                        analysis.maintainability.unwrap().profile
                    else {
                        panic!("missing MI facts")
                    };
                    assert_eq!(facts.used_compatibility_grammar, mixed);
                    assert_eq!(facts.unscored_roots, 0);
                }
            }
            sender.send(()).unwrap();
        })
        .unwrap();
    receiver
        .recv_timeout(std::time::Duration::from_secs(20))
        .expect("valid shifts must not enter the former quadratic grammar path");
    worker.join().expect("shift worker must not panic or abort");
}

#[test]
fn deep_valid_web_go_syntax_is_safe_on_worker_stacks() {
    let workers: Vec<_> = (0..4)
        .map(|language| {
            std::thread::Builder::new()
                .stack_size(256 * 1024)
                .spawn(move || {
                    let mut parser = match language {
                        0 => WebGoParser::javascript(),
                        1 => WebGoParser::typescript(),
                        2 => WebGoParser::tsx(),
                        _ => WebGoParser::go(),
                    }
                    .unwrap();
                    for (index, expression) in [
                        format!("{}x", "x&&".repeat(20_000)),
                        format!("{}x{}", "(".repeat(10_000), ")".repeat(10_000)),
                        format!("{}x&&x{}", "(".repeat(10_000), ")".repeat(10_000)),
                        format!("{}x", "x+".repeat(20_000)),
                    ].into_iter().enumerate() {
                        let source = if language == 3 {
                            let datatype = if index == 3 { "int" } else { "bool" };
                            format!("package p\nfunc f(x {datatype}) {datatype} {{return {expression}}}\n")
                        } else {
                            format!("function f(x){{return {expression};}}\n")
                        };
                        assert!(source.len() < 8 * 1024 * 1024);
                        let analysis = parser
                            .parse_analysis("deep-source".into(), &source)
                            .expect("valid deep syntax remains scoreable");
                        assert!(analysis.file.profiled_cognitive.is_some());
                        assert!(analysis.maintainability.is_some());
                    }
                    if language != 3 {
                        let source = format!("const f={}true;", "()=>".repeat(10_000));
                        let analysis = parser
                            .parse_analysis("nested-callables".into(), &source)
                            .expect("deep arrow-function boundaries remain scoreable");
                        assert!(analysis.file.profiled_cognitive.is_some());
                        assert!(analysis.maintainability.is_some());
                        let mut source = String::from("const f=");
                        for index in 0..5_000 {
                            source.push_str(&format!("value{index}=>"));
                        }
                        source.push_str("true;");
                        parser.parse_analysis("distinct-nested-operands".into(), &source).expect("nested MI scopes move token maps instead of copying them");
                        let source = format!("function f(x){{{}x;{}}}", "if(x){".repeat(5_000), "}".repeat(5_000));
                        parser.parse_analysis("nested-branches".into(), &source).expect("deep branches use carried parent context");
                    } else {
                        let source = format!("package p\nfunc f(){{{}{}}}", "{var x int;_=x;".repeat(5_000), "}".repeat(5_000));
                        parser.parse_analysis("nested-bindings".into(), &source).expect("deep lexical scopes use carried context");
                        let mut source = String::from("package p\n");
                        for index in 0..5_000 {
                            source.push_str(&format!("func f{index}(x int)int{{return x}}\n"));
                        }
                        parser.parse_analysis("repeated-bindings".into(), &source).expect("repeated parameter names use indexed scope lookup");
                    }
                })
                .unwrap()
        })
        .collect();
    for worker in workers {
        worker
            .join()
            .expect("analysis worker must not panic or abort");
    }
}
