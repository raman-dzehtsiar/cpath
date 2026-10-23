// SPDX-License-Identifier: MIT

//! Research-only cognitive formula ablations, using the normal bounded pipeline.
//! Formulas were fixed before the 2026-10-22 matched-source review ratings.
use std::io::Write;
use std::path::PathBuf;

use cpath::complexity::{CognitiveComplexity, ComplexityAnalyzer};
use cpath::model::{ComplexityMetrics, ControlKind, FileFacts, MethodComplexity};
use cpath::pipeline::{scan_with, Analyzers};
use cpath::scoring::ComplexityFirst;
use serde_json::json;

#[derive(Clone, Copy, Debug)]
enum Variant {
    Baseline,
    Events,
    Breadth,
}

impl Variant {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "baseline" => Ok(Self::Baseline),
            "events" => Ok(Self::Events),
            "breadth" => Ok(Self::Breadth),
            _ => Err("variant must be baseline, events, or breadth".into()),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Baseline => "baseline",
            Self::Events => "events",
            Self::Breadth => "breadth",
        }
    }

    fn formula(self) -> &'static str {
        match self {
            Self::Baseline => "cpath.complexity-ranking.v1",
            Self::Events => "cpath.research.event-count.v1",
            Self::Breadth => "cpath.research.breadth-compressed.v1",
        }
    }

    fn score(self, events: u32, nesting: u32) -> u32 {
        match self {
            Self::Baseline => events.saturating_add(nesting),
            Self::Events => events,
            Self::Breadth => nesting.saturating_add(ceil_sqrt(events)),
        }
    }
}

/// Exact integer arithmetic: the largest possible answer is 65,536.
fn ceil_sqrt(value: u32) -> u32 {
    let (mut low, mut high) = (0u32, 65_536u32);
    while low < high {
        let middle = low + (high - low) / 2;
        if u64::from(middle) * u64::from(middle) < u64::from(value) {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    low
}

impl ComplexityAnalyzer for Variant {
    fn analyze(&self, file: &FileFacts) -> ComplexityMetrics {
        if matches!(self, Self::Baseline) {
            return CognitiveComplexity.analyze(file);
        }
        let methods: Vec<_> = file
            .methods
            .iter()
            .map(|method| {
                let (events, nesting) =
                    method
                        .events
                        .iter()
                        .fold((0u32, 0u32), |(events, nesting), event| {
                            let premium = match event.kind {
                                ControlKind::If
                                | ControlKind::Loop
                                | ControlKind::Switch
                                | ControlKind::Catch
                                | ControlKind::Ternary => event.nesting,
                                ControlKind::Else
                                | ControlKind::BooleanSequence
                                | ControlKind::Jump => 0,
                            };
                            (events.saturating_add(1), nesting.saturating_add(premium))
                        });
                MethodComplexity {
                    name: method.name.clone(),
                    start_line: method.start_line,
                    end_line: method.end_line,
                    score: self.score(events, nesting),
                }
            })
            .collect();
        ComplexityMetrics {
            max_method: methods.iter().map(|method| method.score).max().unwrap_or(0),
            total: methods
                .iter()
                .fold(0u32, |sum, method| sum.saturating_add(method.score)),
            methods,
        }
    }
}

fn main() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let root = PathBuf::from(args.next().ok_or(
        "usage: cognitive_ranking_experiment REPOSITORY --variant baseline|events|breadth [--threads N] [--include GLOB | --exclude GLOB]...",
    )?);
    let mut variant = None;
    let mut scan_args = vec![root.clone().into_os_string(), "--top".into(), "10".into()];
    while let Some(arg) = args.next() {
        if arg == "--variant" {
            if variant.is_some() {
                return Err("--variant must appear exactly once".into());
            }
            let value = args.next().ok_or("--variant requires a value")?;
            variant = Some(Variant::parse(value.to_str().ok_or("invalid variant")?)?);
        } else {
            scan_args.push(arg);
        }
    }
    let variant = variant.ok_or("--variant is required")?;
    let cpath::cli::Command::Scan { options, .. } = cpath::cli::parse(scan_args)? else {
        return Err("expected scan arguments".into());
    };
    let report = scan_with(
        &root,
        &options,
        Analyzers {
            complexity: &variant,
            scorer: &ComplexityFirst,
        },
    )?;
    let rows: Vec<_> = report
        .ranked
        .iter()
        .map(|rank| {
            let file = &report.files[rank.file_index];
            let complexity = &report.complexity[rank.file_index];
            let mut methods: Vec<_> = complexity.methods.iter().collect();
            methods.sort_by(|a, b| b.score.cmp(&a.score).then(a.start_line.cmp(&b.start_line)));
            json!({
                "path": file.path, "score": rank.score.maximum,
                "bytes": file.bytes, "lines": file.lines,
                "complexity": complexity.max_method,
                "complexity_total": complexity.total,
                "complexity_burden": rank.score.burden,
                "methods": methods.iter().take(5).map(|method| json!({
                    "name": method.name, "start": method.start_line,
                    "end": method.end_line, "complexity": method.score,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    let output = json!({
        "research_only": true,
        "variant": variant.name(), "formula": variant.formula(),
        "burden_threshold": cpath::scoring::BURDEN_THRESHOLD,
        "includes": options.includes, "excludes": options.excludes,
        "workers": options.workers, "files": report.files.len(),
        "bytes": report.files.iter().map(|file| file.bytes).sum::<u64>(),
        "lines": report.files.iter().map(|file| u64::from(file.lines)).sum::<u64>(),
        "unscored_files": report.unscored_files,
        "failure_counts": report.failure_counts.iter().map(|(kind, count)|
            (format!("{kind:?}"), *count)
        ).collect::<std::collections::BTreeMap<_, _>>(),
        "diagnostics": report.diagnostics.iter().map(|diagnostic| json!({
            "path": diagnostic.path, "message": diagnostic.message,
            "kind": format!("{:?}", diagnostic.kind),
        })).collect::<Vec<_>>(),
        "path_selection": format!("{:?}", report.selection),
        "content_skips": format!("{:?}", report.content_skips),
        "seconds": {"file_pipeline": report.times.files.as_secs_f64(),
            "ranking": report.times.ranking.as_secs_f64(),
            "total": report.times.total.as_secs_f64()},
        "ranking": rows,
    });
    let mut bytes = serde_json::to_vec_pretty(&output).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    std::io::stdout()
        .lock()
        .write_all(&bytes)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpath::language::{CSharpParser, SourceParser};
    use cpath::model::{ComplexityEvent, MethodFacts};

    fn scores(source: &str) -> [u32; 3] {
        let file = CSharpParser::new()
            .unwrap()
            .parse("Example.cs".into(), source)
            .unwrap();
        [Variant::Baseline, Variant::Events, Variant::Breadth]
            .map(|variant| variant.analyze(&file).max_method)
    }

    #[test]
    fn hand_counted_flat_and_nested_control_flow() {
        assert_eq!(scores("class C { void M() {} }"), [0, 0, 0]);
        // Four flat ifs: E=4, N=0; sqrt(4)=2.
        assert_eq!(
            scores("class C { void M(bool a) { if(a){} if(a){} if(a){} if(a){} } }"),
            [4, 4, 2]
        );
        // Four nested ifs: E=4, N=0+1+2+3=6.
        assert_eq!(
            scores("class C { void M(bool a) { if(a){ if(a){ if(a){ if(a){} } } } } }"),
            [10, 4, 8]
        );
    }

    #[test]
    fn repeated_nested_blocks_keep_linear_nesting_premium() {
        // Four separate two-if blocks: E=8, N=4, ceil(sqrt(8))=3.
        // Breadth compression still cannot recognize independent repetition.
        let block = "if(a){ if(a){} }";
        let source = format!("class C {{ void M(bool a) {{ {} }} }}", block.repeat(4));
        assert_eq!(scores(&source), [12, 8, 7]);
        // At sixteen copies, compression again exceeds the four-deep chain:
        // E=32, N=16, ceil(sqrt(32))=6. This is a retained adverse example.
        let source = format!("class C {{ void M(bool a) {{ {} }} }}", block.repeat(16));
        assert_eq!(scores(&source), [48, 32, 22]);
    }

    #[test]
    fn transform_each_callable_before_selecting_the_file_maximum() {
        // Flat has E=16,N=0 -> 16/16/4. Nested has E=4,N=6 -> 10/4/8.
        // The hardest callable changes, so transforming the baseline file
        // maximum (or just its retained winning method) would be incorrect.
        let source = format!(
            "class C {{ void Flat(bool a) {{ {} }} void Nested(bool a) {{ if(a){{ if(a){{ if(a){{ if(a){{}} }} }} }} }} }}",
            "if(a){}".repeat(16)
        );
        assert_eq!(scores(&source), [16, 16, 8]);
    }

    #[test]
    fn syntax_variants_cannot_infer_sibling_data_dependencies() {
        // Both have E=2,N=0. One updates separate inputs; the other reuses the
        // first update in its second condition. Equal scores document missing
        // dataflow evidence, not an assertion of equal review difficulty.
        let separate = "class C { int M(int a,int b) { if(a>0)a--; if(b>0)b--; return a+b; } }";
        let dependent = "class C { int M(int a,int b) { if(a>0)a-=b; if(a<0)a+=b; return a; } }";
        assert_eq!(scores(separate), [2, 2, 2]);
        assert_eq!(scores(dependent), [2, 2, 2]);
    }

    #[test]
    fn auxiliary_events_never_gain_a_nesting_premium() {
        // Outer if 1; inner if 2; && sequence 1; else 1; goto 1.
        assert_eq!(
            scores(
                "class C { void M(bool a) { start: if(a){ if(a && a){} else { goto start; } } } }"
            ),
            [6, 5, 4]
        );
    }

    #[test]
    fn integer_square_root_boundaries_are_exact() {
        for root in 0u32..=65_535 {
            let square = root * root;
            assert_eq!(ceil_sqrt(square), root);
            if root > 1 {
                assert_eq!(ceil_sqrt(square - 1), root);
            }
            assert_eq!(ceil_sqrt(square + 1), root + 1);
        }
        assert_eq!(ceil_sqrt(u32::MAX), 65_536);
    }

    #[test]
    fn score_is_monotonic_and_saturates_without_wrapping() {
        for variant in [Variant::Baseline, Variant::Events, Variant::Breadth] {
            for events in 0..100 {
                for nesting in 0..100 {
                    assert!(variant.score(events + 1, nesting) >= variant.score(events, nesting));
                    assert!(variant.score(events, nesting + 1) >= variant.score(events, nesting));
                }
            }
            assert_eq!(variant.score(u32::MAX, u32::MAX), u32::MAX);
        }
        let file = FileFacts {
            methods: vec![MethodFacts {
                name: "M".into(),
                start_line: 1,
                end_line: 1,
                events: vec![
                    ComplexityEvent {
                        kind: ControlKind::If,
                        nesting: u32::MAX,
                        line: 1,
                    };
                    2
                ],
            }],
            ..FileFacts::default()
        };
        assert_eq!(Variant::Baseline.analyze(&file).max_method, u32::MAX);
        assert_eq!(Variant::Events.analyze(&file).max_method, 2);
        assert_eq!(Variant::Breadth.analyze(&file).max_method, u32::MAX);
    }
}
