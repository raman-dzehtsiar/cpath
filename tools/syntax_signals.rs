// SPDX-License-Identifier: MIT

//! Research-only C# syntax signals. These are syntactic proxies, not resolved
//! dataflow, ownership, concurrency hazards, or evidence of a defect. No source,
//! trees, target names, or tokens survive extraction.

use std::collections::BTreeMap;
use std::path::PathBuf;

use cpath::language::CSharpParser;
use cpath::model::{ComplexityEvent, ControlKind, FileFacts};
use serde_json::{json, Value};
use tree_sitter::Node;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MethodSignals {
    pub baseline: u32,
    pub event_count: u32,
    pub nesting_sum: u32,
    /// Maximum sum of event weights on one syntax-tree ancestor chain.
    /// This is not a feasible execution path; alternatives remain represented.
    pub peak_ancestry: u32,
    pub peak_nesting: u32,
    pub span_lines: u32,
    pub body_span_lines: u32,
    pub parameter_count: u32,
    pub statement_count: u32,
    pub awaits: u32,
    pub locks: u32,
    pub usings: u32,
    pub finallys: u32,
    pub yields: u32,
    pub throws: u32,
    pub catches: u32,
    pub returns: u32,
    /// Assignment expressions and ++/--; declaration initializers are excluded.
    pub writes: u32,
    /// Simple identifier/member-chain spellings only, without name resolution.
    pub distinct_write_targets: u32,
    pub repeated_writes: u32,
    /// Direct this.member targets only; fields and properties are not resolved.
    pub explicit_this_writes: u32,
    pub explicit_this_repeats: u32,
    /// Exact lexical receiver names; even qualified names can be shadowed.
    pub interlocked_calls: u32,
    pub volatile_calls: u32,
    pub monitor_calls: u32,
    /// First three distinct source lines per signal, for manual evidence audits.
    pub locations: BTreeMap<&'static str, Vec<u32>>,
}

impl MethodSignals {
    pub fn json(&self) -> Value {
        json!({
            "baseline": self.baseline, "event_count": self.event_count,
            "nesting_sum": self.nesting_sum, "peak_ancestry": self.peak_ancestry,
            "peak_nesting": self.peak_nesting, "span_lines": self.span_lines,
            "body_span_lines": self.body_span_lines,
            "parameter_count": self.parameter_count, "statement_count": self.statement_count,
            "awaits": self.awaits, "locks": self.locks, "usings": self.usings,
            "finallys": self.finallys, "yields": self.yields, "throws": self.throws,
            "catches": self.catches, "returns": self.returns, "writes": self.writes,
            "distinct_write_targets": self.distinct_write_targets,
            "repeated_writes": self.repeated_writes,
            "explicit_this_writes": self.explicit_this_writes,
            "explicit_this_repeats": self.explicit_this_repeats,
            "interlocked_calls": self.interlocked_calls, "volatile_calls": self.volatile_calls,
            "monitor_calls": self.monitor_calls, "locations": self.locations,
        })
    }

    fn location(&mut self, signal: &'static str, node: Node<'_>) {
        let lines = self.locations.entry(signal).or_default();
        let line = node.start_position().row as u32 + 1;
        if lines.len() < 3 && lines.last() != Some(&line) {
            lines.push(line);
        }
    }
}

#[derive(Default)]
struct Accumulator {
    signals: MethodSignals,
    // Worker-local bounded by this file's syntax; discarded before returning.
    ancestors: Vec<(usize, usize, u32)>,
    targets: BTreeMap<String, u32>,
    this_targets: BTreeMap<String, u32>,
}

pub fn extract(
    parser: &mut CSharpParser,
    path: PathBuf,
    source: &str,
) -> Result<(FileFacts, Vec<MethodSignals>), String> {
    extract_observed(parser, path, source, |_, _| {})
}

/// Forward the existing traversal to an additional research-only observer.
pub fn extract_observed(
    parser: &mut CSharpParser,
    path: PathBuf,
    source: &str,
    mut observer: impl FnMut(Node<'_>, Option<usize>),
) -> Result<(FileFacts, Vec<MethodSignals>), String> {
    let mut methods: Vec<Accumulator> = Vec::new();
    let facts = parser.parse_observed(path, source, |node, method, event| {
        observer(node, method);
        let Some(index) = method else { return };
        if methods.len() <= index {
            methods.resize_with(index + 1, Accumulator::default);
            if let Some(body) = node
                .child_by_field_name("body")
                .or_else(|| node.child_by_field_name("value"))
            {
                methods[index].signals.body_span_lines =
                    (body.end_position().row - body.start_position().row + 1) as u32;
            }
        }
        observe(&mut methods[index], node, event, source);
    })?;
    let signals = methods
        .into_iter()
        .zip(&facts.methods)
        .map(|(mut method, facts)| {
            method.signals.span_lines = facts.end_line - facts.start_line + 1;
            method.signals.distinct_write_targets = method.targets.len() as u32;
            method.signals.repeated_writes = repeated(&method.targets);
            method.signals.explicit_this_repeats = repeated(&method.this_targets);
            method.signals
        })
        .collect();
    Ok((facts, signals))
}

fn repeated(targets: &BTreeMap<String, u32>) -> u32 {
    targets.values().fold(0u32, |sum, count| {
        sum.saturating_add(count.saturating_sub(1))
    })
}

fn observe(
    method: &mut Accumulator,
    node: Node<'_>,
    event: Option<&ComplexityEvent>,
    source: &str,
) {
    let signals = &mut method.signals;
    if let Some(event) = event {
        let nesting = match event.kind {
            ControlKind::If
            | ControlKind::Loop
            | ControlKind::Switch
            | ControlKind::Catch
            | ControlKind::Ternary => event.nesting,
            ControlKind::Else | ControlKind::BooleanSequence | ControlKind::Jump => 0,
        };
        let weight = 1u32.saturating_add(nesting);
        signals.baseline = signals.baseline.saturating_add(weight);
        signals.event_count = signals.event_count.saturating_add(1);
        signals.nesting_sum = signals.nesting_sum.saturating_add(nesting);
        signals.peak_nesting = signals.peak_nesting.max(event.nesting);
        while method
            .ancestors
            .last()
            .is_some_and(|(start, end, _)| *start > node.start_byte() || *end < node.end_byte())
        {
            method.ancestors.pop();
        }
        let path_score = method
            .ancestors
            .last()
            .map_or(0, |(_, _, sum)| *sum)
            .saturating_add(weight);
        if path_score > signals.peak_ancestry {
            signals.peak_ancestry = path_score;
            signals
                .locations
                .insert("peak_ancestry", vec![node.start_position().row as u32 + 1]);
        }
        method
            .ancestors
            .push((node.start_byte(), node.end_byte(), path_score));
    }
    let kind = node.kind();
    if kind.ends_with("_statement")
        && !matches!(kind, "global_statement" | "local_function_statement")
    {
        signals.statement_count = signals.statement_count.saturating_add(1);
    }
    let signal = match kind {
        "parameter" | "implicit_parameter" => {
            signals.parameter_count += 1;
            Some("parameters")
        }
        "await_expression" => {
            signals.awaits += 1;
            Some("await")
        }
        "lock_statement" => {
            signals.locks += 1;
            Some("lock")
        }
        "using_statement" => {
            signals.usings += 1;
            Some("using")
        }
        "local_declaration_statement" if has_child(node, "using") => {
            signals.usings += 1;
            Some("using")
        }
        "finally_clause" => {
            signals.finallys += 1;
            Some("finally")
        }
        "yield_statement" => {
            signals.yields += 1;
            Some("yield")
        }
        "throw_statement" | "throw_expression" => {
            signals.throws += 1;
            Some("throw")
        }
        "catch_clause" => {
            signals.catches += 1;
            Some("catch")
        }
        "return_statement" => {
            signals.returns += 1;
            Some("return")
        }
        _ => None,
    };
    if let Some(signal) = signal {
        signals.location(signal, node);
    }
    let target = if kind == "assignment_expression" {
        node.child_by_field_name("left")
    } else if matches!(kind, "prefix_unary_expression" | "postfix_unary_expression")
        && (has_child(node, "++") || has_child(node, "--"))
    {
        node.named_child(0)
    } else {
        None
    };
    if let Some(target) = target {
        signals.writes += 1;
        signals.location("write", node);
        if let Some(key) = target_key(target, source) {
            *method.targets.entry(key.clone()).or_default() += 1;
            if target.kind() == "member_access_expression"
                && target
                    .child_by_field_name("expression")
                    .is_some_and(|receiver| text(receiver, source) == "this")
            {
                signals.explicit_this_writes += 1;
                signals.location("explicit_this_write", node);
                *method.this_targets.entry(key).or_default() += 1;
            }
        }
    }
    if kind == "invocation_expression" {
        if let Some(function) = node.child_by_field_name("function") {
            if let Some(receiver) = function.child_by_field_name("expression") {
                let signal = match text(receiver, source) {
                    "Interlocked" | "System.Threading.Interlocked" => {
                        signals.interlocked_calls += 1;
                        Some("interlocked_name")
                    }
                    "Volatile" | "System.Threading.Volatile" => {
                        signals.volatile_calls += 1;
                        Some("volatile_name")
                    }
                    "Monitor" | "System.Threading.Monitor" => {
                        signals.monitor_calls += 1;
                        Some("monitor_name")
                    }
                    _ => None,
                };
                if let Some(signal) = signal {
                    signals.location(signal, node);
                }
            }
        }
    }
}

fn has_child(node: Node<'_>, kind: &str) -> bool {
    (0..node.child_count()).any(|index| node.child(index).is_some_and(|child| child.kind() == kind))
}

fn text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}

/// Canonicalize only simple name chains. Calls, indexers, deconstruction, and
/// dereferences are deliberately not assigned an identity. Lexically identical
/// names may denote different bindings; these counts are explicitly unresolved.
fn target_key(mut node: Node<'_>, source: &str) -> Option<String> {
    let mut names = Vec::new();
    while node.kind() == "member_access_expression" {
        let name = node.child_by_field_name("name")?;
        if name.kind() != "identifier" {
            return None;
        }
        names.push(text(name, source));
        node = node.child_by_field_name("expression")?;
    }
    if !matches!(node.kind(), "identifier" | "this" | "base") {
        return None;
    }
    names.push(text(node, source));
    names.reverse();
    Some(names.join("."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpath::complexity::{CognitiveComplexity, ComplexityAnalyzer};

    fn signals(source: &str) -> Vec<MethodSignals> {
        let (facts, signals) = extract(
            &mut CSharpParser::new().unwrap(),
            "Example.cs".into(),
            source,
        )
        .unwrap();
        let baseline = CognitiveComplexity.analyze(&facts);
        assert_eq!(signals.len(), facts.methods.len());
        for (signals, method) in signals.iter().zip(baseline.methods) {
            assert_eq!(signals.baseline, method.score);
            assert!(signals.peak_ancestry <= signals.baseline);
        }
        signals
    }

    fn lifecycle(signal: &MethodSignals) -> u32 {
        [
            signal.awaits,
            signal.locks,
            signal.usings,
            signal.finallys,
            signal.yields,
            signal.throws,
        ]
        .into_iter()
        .filter(|count| *count > 0)
        .count() as u32
    }

    #[test]
    fn exact_ancestry_separates_repeated_regions_from_a_deep_chain() {
        let deep = signals(include_str!(
            "../tests/fixtures/syntax_signals/deep_chain.cs"
        ));
        let repeated = signals(include_str!(
            "../tests/fixtures/syntax_signals/repeated_nested.cs"
        ));
        assert_eq!((deep[0].baseline, deep[0].peak_ancestry), (10, 10));
        assert_eq!((repeated[0].baseline, repeated[0].peak_ancestry), (45, 3));
        // Syntax ancestry is not path feasibility: an else-if chain is still
        // nested in the AST, even though its individual weights have no premium.
        let ladder = signals("class C { void M(bool a) { if(a) {} else if(a) {} else if(a) {} } }");
        assert_eq!((ladder[0].baseline, ladder[0].peak_ancestry), (3, 3));
    }

    #[test]
    fn member_target_repetition_is_rename_invariant() {
        for source in [
            include_str!("../tests/fixtures/syntax_signals/member_updates.cs"),
            include_str!("../tests/fixtures/syntax_signals/member_updates_rename.cs"),
        ] {
            let value = &signals(source)[0];
            assert_eq!((value.writes, value.explicit_this_writes), (3, 3));
            assert_eq!((value.repeated_writes, value.explicit_this_repeats), (2, 2));
        }
    }

    #[test]
    fn source_spelling_in_comments_strings_and_names_cannot_create_lifecycle_syntax() {
        for source in [
            include_str!("../tests/fixtures/syntax_signals/spoof_base.cs"),
            include_str!("../tests/fixtures/syntax_signals/spoof_comment.cs"),
            include_str!("../tests/fixtures/syntax_signals/spoof_string.cs"),
            include_str!("../tests/fixtures/syntax_signals/spoof_rename.cs"),
        ] {
            let value = &signals(source)[0];
            assert_eq!(
                (
                    lifecycle(value),
                    value.explicit_this_repeats,
                    value.repeated_writes
                ),
                (0, 0, 1)
            );
        }
    }

    #[test]
    fn unresolved_spelling_repetition_cannot_distinguish_separate_bindings() {
        for source in [
            include_str!("../tests/fixtures/syntax_signals/sibling_bindings.cs"),
            include_str!("../tests/fixtures/syntax_signals/shared_binding.cs"),
        ] {
            assert_eq!(signals(source)[0].repeated_writes, 1);
        }
        let renamed = signals(include_str!(
            "../tests/fixtures/syntax_signals/sibling_bindings_rename.cs"
        ));
        assert_eq!(renamed[0].repeated_writes, 0);
        assert_eq!(renamed[0].explicit_this_repeats, 0);
        assert_eq!(renamed[0].peak_ancestry, 0);
        assert_eq!(lifecycle(&renamed[0]), 0);
        let fields = signals(include_str!(
            "../tests/fixtures/syntax_signals/shadowed_field.cs"
        ));
        assert_eq!(
            (fields[0].explicit_this_repeats, fields[0].repeated_writes),
            (1, 1)
        );
        assert_eq!(
            (fields[1].explicit_this_repeats, fields[1].repeated_writes),
            (0, 1)
        );
    }

    #[test]
    fn nested_callables_do_not_pollute_parent_mutation_or_event_stacks() {
        let values = signals(include_str!(
            "../tests/fixtures/syntax_signals/nested_lambda.cs"
        ));
        assert_eq!(values[0].repeated_writes, 0);
        assert_eq!(values[1].repeated_writes, 1);
        let values = signals("class C { void M(bool a) { if(a) { System.Action local = () => { if(a) { if(a) {} } }; if(a) {} } } }");
        assert_eq!((values[0].baseline, values[0].peak_ancestry), (3, 3));
        assert_eq!((values[1].baseline, values[1].peak_ancestry), (3, 3));
    }

    #[test]
    fn lifecycle_counts_typed_constructs_and_does_not_prove_missing_cleanup_safe() {
        let value = &signals(include_str!(
            "../tests/fixtures/syntax_signals/typed_obligations.cs"
        ))[0];
        assert_eq!(lifecycle(value), 5);
        assert_eq!(
            (
                value.awaits,
                value.locks,
                value.usings,
                value.finallys,
                value.throws,
                value.catches
            ),
            (1, 1, 1, 1, 1, 1)
        );
        let missing = &signals(include_str!(
            "../tests/fixtures/syntax_signals/cleanup_missing.cs"
        ))[0];
        let protected = &signals(include_str!(
            "../tests/fixtures/syntax_signals/cleanup_finally.cs"
        ))[0];
        assert_eq!((missing.baseline, protected.baseline), (1, 1));
        assert_eq!((lifecycle(missing), lifecycle(protected)), (0, 1));
        let value = &signals("class C { async System.Threading.Tasks.Task M() { await using var resource = Get(); using var other = Get(); yield return 1; yield break; } }")[0];
        assert_eq!((value.usings, value.yields), (2, 2));
    }

    #[test]
    fn qualified_names_remain_spoofable_and_are_separate_from_typed_signals() {
        let spoof = &signals(include_str!(
            "../tests/fixtures/syntax_signals/qualified_api_spoof.cs"
        ))[0];
        let ordinary = &signals(include_str!(
            "../tests/fixtures/syntax_signals/qualified_api_ordinary.cs"
        ))[0];
        assert_eq!(lifecycle(spoof), 0);
        assert_eq!(spoof.monitor_calls, 2);
        assert_eq!(ordinary.monitor_calls, 0);
        let value = &signals("class C { void M() { System.Threading.Interlocked.Increment(ref x); System.Threading.Volatile.Read(ref x); Something.Monitor.Enter(); } }")[0];
        assert_eq!(
            (
                value.interlocked_calls,
                value.volatile_calls,
                value.monitor_calls
            ),
            (1, 1, 0)
        );
    }

    #[test]
    fn assignments_and_updates_exclude_initializers_and_unsupported_target_identity() {
        let value = &signals("class C { void M() { int local = 1; local++; ++local; local += 2; this . x = 1; this.x--; items[0] = 1; items[0]++; Get().x = 1; } }")[0];
        assert_eq!(value.writes, 8);
        assert_eq!(value.distinct_write_targets, 2);
        assert_eq!(value.repeated_writes, 3);
        assert_eq!(
            (value.explicit_this_writes, value.explicit_this_repeats),
            (2, 1)
        );
    }

    #[test]
    fn parameters_and_statements_are_owned_by_their_callable_and_locations_are_bounded() {
        let values = signals("class C { void M(int x, int y) { x++;\nx++;\nx++;\nx++;\nSystem.Action<int> a = z => { z++; }; } }");
        assert_eq!(values[0].parameter_count, 2);
        assert_eq!(values[1].parameter_count, 1);
        assert_eq!(values[0].statement_count, 5);
        assert_eq!(values[1].statement_count, 1);
        assert_eq!(values[0].locations["write"], vec![1, 2, 3]);
        assert_eq!(values[0].json()["parameter_count"], 2);
    }
}
