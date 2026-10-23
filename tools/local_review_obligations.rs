// SPDX-License-Identifier: MIT

//! Research-only, narrowly structural low-complexity attention candidates.
//! These observations are not defects, type resolution, or feasible-path proofs.

use std::collections::BTreeMap;

use serde_json::{json, Value};
use tree_sitter::Node;

pub const RULES: [&str; 6] = ["R", "X", "F", "E", "W", "D"];
const TARGET_LIMIT: usize = 128;
const ANCESTOR_LIMIT: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evidence {
    pub line: u32,
    pub related_lines: Vec<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MethodInteractions {
    pub counts: [u32; 6],
    pub evidence: [Vec<Evidence>; 6],
    /// Fixed reason codes only; no source names are retained.
    pub abstentions: BTreeMap<&'static str, u32>,
}

impl MethodInteractions {
    pub fn has_findings(&self) -> bool {
        self.counts.iter().any(|count| *count != 0)
    }

    pub fn incomplete(&self) -> bool {
        self.abstentions.contains_key("target_budget")
            || self.abstentions.contains_key("ancestor_budget")
            || self.abstentions.contains_key("read_ancestor_budget")
            || self.abstentions.contains_key("parameter_budget")
            || self
                .abstentions
                .contains_key("unsupported_parameter_shadowing")
    }

    pub fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.abstentions.len() * 128
            + self
                .evidence
                .iter()
                .map(|items| {
                    items.capacity() * std::mem::size_of::<Evidence>()
                        + items
                            .iter()
                            .map(|item| item.related_lines.capacity() * 4)
                            .sum::<usize>()
                })
                .sum::<usize>()
    }

    pub fn json(&self) -> Value {
        let counts: BTreeMap<_, _> = RULES.into_iter().zip(self.counts).collect();
        let evidence: BTreeMap<_, _> = RULES
            .into_iter()
            .zip(self.evidence.iter().map(|items| {
                items
                    .iter()
                    .map(|item| json!({"line":item.line,"related_lines":item.related_lines}))
                    .collect::<Vec<_>>()
            }))
            .collect();
        json!({"counts":counts,"evidence":evidence,"abstentions":self.abstentions,"incomplete":self.incomplete()})
    }

    fn hit(&mut self, rule: usize, node: Node<'_>, related_lines: Vec<u32>) {
        self.counts[rule] = self.counts[rule].saturating_add(1);
        if self.evidence[rule].len() < 3 {
            self.evidence[rule].push(Evidence {
                line: line(node),
                related_lines,
            });
        }
    }

    fn abstain(&mut self, reason: &'static str) {
        let count = self.abstentions.entry(reason).or_default();
        *count = count.saturating_add(1);
    }
}

struct Write {
    end: usize,
    line: u32,
    suspension: Option<(usize, u32)>,
}

#[derive(Default)]
struct Method {
    facts: MethodInteractions,
    // At most TARGET_LIMIT keys per callable, released by finish().
    targets: BTreeMap<(usize, String), Write>,
    w_capped: bool,
    ancestry_capped: bool,
    parameters: BTreeMap<String, u32>,
    d_disabled: bool,
}

#[derive(Default)]
pub struct Collector {
    methods: Vec<Method>,
}

impl Collector {
    pub fn observe(&mut self, node: Node<'_>, owner: Option<usize>, source: &str) {
        let Some(index) = owner else { return };
        self.methods
            .resize_with(self.methods.len().max(index + 1), Method::default);
        let method = &mut self.methods[index];
        observe_parameters(method, node, source);
        match node.kind() {
            "local_declaration_statement" if child_kind(node, "using") => {
                method.facts.abstain("unsupported_using_declaration");
            }
            "catch_clause"
                if node.child_by_field_name("body").is_some_and(|body| {
                    named_children(body).all(|child| child.kind() == "comment")
                }) =>
            {
                method.facts.hit(3, node, vec![]);
            }
            "return_statement" | "throw_statement" | "throw_expression" => {
                inspect_ancestors(method, node, source);
            }
            "invocation_expression" if !method.d_disabled => {
                inspect_parameter_invocation(method, node, source);
            }
            _ => {}
        }
        if method.w_capped {
            return;
        }
        if node.kind() == "assignment_expression" {
            if let Some(target) = node
                .child_by_field_name("left")
                .and_then(|target| direct_member(target, source))
            {
                if let Some(block) = direct_expression_block(node) {
                    let key = (block.start_byte(), target.to_owned());
                    if method.targets.len() >= TARGET_LIMIT && !method.targets.contains_key(&key) {
                        method.w_capped = true;
                        method.targets.clear();
                        method.facts.counts[4] = 0;
                        method.facts.evidence[4].clear();
                        method.facts.abstain("target_budget");
                    } else {
                        method.targets.insert(
                            key,
                            Write {
                                end: node.end_byte(),
                                line: line(node),
                                suspension: None,
                            },
                        );
                    }
                }
            }
        } else if node.kind() == "await_expression" {
            if let Some(block) = direct_expression_block(node) {
                for ((owner_block, _), write) in &mut method.targets {
                    if *owner_block == block.start_byte() && write.end < node.start_byte() {
                        write.suspension = Some((node.end_byte(), line(node)));
                    }
                }
            } else {
                method.facts.abstain("unsupported_await_shape");
            }
        } else if let Some(target) = direct_member(node, source) {
            match read_block(node, source) {
                Ok(Some((block, out_argument))) => {
                    if out_argument {
                        // An out argument is a write-only use. It also kills the
                        // earlier value relation for subsequent direct reads.
                        method
                            .targets
                            .remove(&(block.start_byte(), target.to_owned()));
                        return;
                    }
                    if let Some(write) =
                        method.targets.get(&(block.start_byte(), target.to_owned()))
                    {
                        if let Some((end, await_line)) = write.suspension {
                            if end < node.start_byte() {
                                method.facts.hit(4, node, vec![write.line, await_line]);
                            }
                        }
                    }
                }
                Err(()) => {
                    method.w_capped = true;
                    method.targets.clear();
                    method.facts.counts[4] = 0;
                    method.facts.evidence[4].clear();
                    method.facts.abstain("read_ancestor_budget");
                }
                Ok(None) => {}
            }
        }
    }

    pub fn finish(mut self, method_count: usize) -> Vec<MethodInteractions> {
        self.methods.resize_with(method_count, Method::default);
        self.methods
            .into_iter()
            .map(|method| method.facts)
            .collect()
    }
}

fn inspect_ancestors(method: &mut Method, node: Node<'_>, source: &str) {
    if method.ancestry_capped {
        return;
    }
    let expression = named_children(node).find(|child| child.kind() != "comment");
    let identifier = expression
        .filter(|value| value.kind() == "identifier")
        .map(|value| name(value, source));
    let is_return = node.kind() == "return_statement";
    let mut parent = node.parent();
    let mut nearest_catch = false;
    let mut resource_found = false;
    let mut finally_found = false;
    for _ in 0..ANCESTOR_LIMIT {
        let Some(ancestor) = parent else { return };
        if callable(ancestor) {
            return;
        }
        if is_return && !resource_found && ancestor.kind() == "using_statement" {
            if let Some(identifier) = identifier {
                if let Some(declaration) =
                    named_children(ancestor).find(|child| child.kind() == "variable_declaration")
                {
                    let variables: Vec<_> = named_children(declaration)
                        .filter(|child| child.kind() == "variable_declarator")
                        .take(2)
                        .collect();
                    if variables.len() == 1 {
                        if variables[0]
                            .child_by_field_name("name")
                            .is_some_and(|binding| name(binding, source) == identifier)
                        {
                            method.facts.hit(0, node, vec![line(variables[0])]);
                            resource_found = true;
                        }
                    } else {
                        method.facts.abstain("unsupported_using_binding");
                    }
                }
            }
        }
        if !is_return && !nearest_catch && ancestor.kind() == "catch_clause" {
            nearest_catch = true;
            if let Some(identifier) = identifier {
                if let Some(binding) = named_children(ancestor)
                    .find(|child| child.kind() == "catch_declaration")
                    .and_then(|declaration| declaration.child_by_field_name("name"))
                {
                    if name(binding, source) == identifier {
                        method.facts.hit(1, node, vec![line(binding)]);
                    }
                }
            }
        }
        if !is_return && !finally_found && ancestor.kind() == "finally_clause" {
            method.facts.hit(2, node, vec![line(ancestor)]);
            finally_found = true;
        }
        parent = ancestor.parent();
    }
    invalidate_ancestors(method);
}

fn invalidate_ancestors(method: &mut Method) {
    // All ancestor-based rules become unknown for this callable, never zero evidence.
    method.ancestry_capped = true;
    for rule in [0, 1, 2, 5] {
        method.facts.counts[rule] = 0;
        method.facts.evidence[rule].clear();
    }
    method.facts.abstain("ancestor_budget");
}

fn observe_parameters(method: &mut Method, node: Node<'_>, source: &str) {
    if method.d_disabled {
        return;
    }
    let parameter = match node.kind() {
        "parameter" => node.child_by_field_name("name"),
        "implicit_parameter" => Some(node),
        _ => None,
    };
    if let Some(parameter) = parameter {
        let key = name(parameter, source);
        if method.parameters.len() >= TARGET_LIMIT && !method.parameters.contains_key(key) {
            method.d_disabled = true;
            method.parameters.clear();
            method.facts.counts[5] = 0;
            method.facts.evidence[5].clear();
            method.facts.abstain("parameter_budget");
        } else {
            method.parameters.insert(key.to_owned(), line(parameter));
        }
        return;
    }
    let declaration = match node.kind() {
        "variable_declarator"
        | "catch_declaration"
        | "declaration_pattern"
        | "recursive_pattern"
        | "var_pattern"
        | "declaration_expression"
        | "from_clause" => node.child_by_field_name("name"),
        "let_clause" | "join_clause" | "join_into_clause" => {
            named_children(node).find(|child| child.kind() == "identifier")
        }
        "foreach_statement" => node
            .child_by_field_name("left")
            .filter(|child| child.kind() == "identifier"),
        _ => None,
    };
    let tuple_collision = matches!(
        node.kind(),
        "tuple_pattern" | "parenthesized_variable_designation"
    ) && named_children(node).any(|child| {
        child.kind() == "identifier" && method.parameters.contains_key(name(child, source))
    });
    if tuple_collision
        || declaration.is_some_and(|binding| method.parameters.contains_key(name(binding, source)))
    {
        // Conservatively discard this rule for the whole callable. Parsing alone
        // does not validate C# scope legality, so a text collision is not a binding.
        method.d_disabled = true;
        method.parameters.clear();
        method.facts.counts[5] = 0;
        method.facts.evidence[5].clear();
        method.facts.abstain("unsupported_parameter_shadowing");
    }
}

fn inspect_parameter_invocation(method: &mut Method, node: Node<'_>, source: &str) {
    if method.ancestry_capped {
        return;
    }
    let Some(function) = node
        .child_by_field_name("function")
        .filter(|function| function.kind() == "identifier")
    else {
        return;
    };
    if &source[function.byte_range()] == "nameof" {
        return;
    }
    let Some(parameter_line) = method.parameters.get(name(function, source)).copied() else {
        return;
    };
    let mut parent = node.parent();
    for _ in 0..ANCESTOR_LIMIT {
        let Some(ancestor) = parent else { return };
        if callable(ancestor) {
            return;
        }
        if ancestor.kind() == "invocation_expression"
            && ancestor
                .child_by_field_name("function")
                .is_some_and(|function| &source[function.byte_range()] == "nameof")
        {
            return;
        }
        if ancestor.kind() == "lock_statement" {
            let body = named_children(ancestor)
                .filter(|child| child.kind() != "comment")
                .last();
            if body.is_some_and(|body| {
                body.start_byte() <= node.start_byte() && body.end_byte() >= node.end_byte()
            }) {
                method
                    .facts
                    .hit(5, node, vec![parameter_line, line(ancestor)]);
                return;
            }
        }
        parent = ancestor.parent();
    }
    invalidate_ancestors(method);
}

fn named_children(node: Node<'_>) -> impl Iterator<Item = Node<'_>> {
    (0..node.named_child_count()).filter_map(move |index| node.named_child(index))
}

fn child_kind(node: Node<'_>, kind: &str) -> bool {
    (0..node.child_count()).any(|index| node.child(index).is_some_and(|child| child.kind() == kind))
}

fn name<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    source[node.byte_range()]
        .strip_prefix('@')
        .unwrap_or(&source[node.byte_range()])
}

fn direct_member<'a>(node: Node<'_>, source: &'a str) -> Option<&'a str> {
    if node.kind() != "member_access_expression" {
        return None;
    }
    let receiver = node.child_by_field_name("expression")?;
    let member = node.child_by_field_name("name")?;
    (receiver.kind() == "this" && member.kind() == "identifier").then(|| name(member, source))
}

fn direct_expression_block(node: Node<'_>) -> Option<Node<'_>> {
    let statement = node.parent()?;
    let block = statement.parent()?;
    (statement.kind() == "expression_statement" && block.kind() == "block").then_some(block)
}

/// Separate an unsupported shape from exhaustion of the bounded ancestor walk.
fn read_block<'a>(node: Node<'a>, source: &str) -> Result<Option<(Node<'a>, bool)>, ()> {
    let mut current = node;
    let mut out_argument = false;
    for _ in 0..ANCESTOR_LIMIT {
        let Some(parent) = current.parent() else {
            return Ok(None);
        };
        if parent.kind() == "argument" && child_kind(parent, "out") {
            out_argument = true;
        }
        if parent.kind() == "assignment_expression"
            && parent.child_by_field_name("left") == Some(node)
        {
            return Ok(None);
        }
        if parent.kind() == "invocation_expression" {
            if parent.child_by_field_name("function") == Some(node) {
                return Ok(None);
            }
            if parent
                .child_by_field_name("function")
                .is_some_and(|function| &source[function.byte_range()] == "nameof")
            {
                return Ok(None);
            }
        }
        if matches!(parent.kind(), "expression_statement" | "return_statement") {
            return Ok(parent
                .parent()
                .filter(|block| block.kind() == "block")
                .map(|block| (block, out_argument)));
        }
        if matches!(
            parent.kind(),
            "conditional_expression" | "binary_expression" | "assignment_expression"
        ) || callable(parent)
            || parent.kind() == "block"
        {
            return Ok(None);
        }
        current = parent;
    }
    Err(())
}

fn callable(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "method_declaration"
            | "constructor_declaration"
            | "destructor_declaration"
            | "operator_declaration"
            | "conversion_operator_declaration"
            | "accessor_declaration"
            | "local_function_statement"
            | "lambda_expression"
            | "anonymous_method_expression"
            | "property_declaration"
            | "indexer_declaration"
    )
}

fn line(node: Node<'_>) -> u32 {
    node.start_position().row as u32 + 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpath::language::CSharpParser;

    fn extract(source: &str) -> Vec<MethodInteractions> {
        let mut collector = Collector::default();
        let facts = CSharpParser::new()
            .unwrap()
            .parse_observed("Example.cs".into(), source, |node, owner, _| {
                collector.observe(node, owner, source)
            })
            .unwrap();
        collector.finish(facts.methods.len())
    }

    #[test]
    fn resource_return_uses_enclosing_declaration_and_callable_boundary() {
        let values = extract("class C { object M() { using (var r = Get()) { return r; } } object N() { using (var r = Get()) {} return r; } object L() { using (var r = Get()) { return () => r; } } object D() { using var r = Get(); return r; } }");
        assert_eq!(
            values
                .iter()
                .map(|value| value.counts[0])
                .collect::<Vec<_>>(),
            vec![1, 0, 0, 0, 0]
        );
        assert_eq!(values[4].abstentions["unsupported_using_declaration"], 1);
        let renamed =
            extract("class C { object M() { using (var @renamed = Get()) { return renamed; } } }");
        assert_eq!(renamed[0].counts[0], 1);
    }

    #[test]
    fn catch_identity_empty_body_and_finally_are_independent() {
        let values = extract("class C { void M() { try {} catch (Exception ex) { throw ex; } finally { throw new Exception(); } } void N() { try {} catch (Exception ex) { throw; } } void E() { try {} catch { /* deliberate suppression */ } } }");
        assert_eq!(values[0].counts, [0, 1, 1, 0, 0, 0]);
        assert_eq!(values[1].counts, [0; 6]);
        assert_eq!(values[2].counts, [0, 0, 0, 1, 0, 0]);
        let values = extract("class C { void M() { try {} catch (Exception ex) { Action a = () => { throw ex; }; } finally { Action b = () => { throw new Exception(); }; } } }");
        assert!(values.iter().all(|value| value.counts == [0; 6]));
    }

    #[test]
    fn suspension_requires_completed_write_and_same_straight_line_block() {
        let positive =
            extract("class C { async Task M() { this.x = 1; await Work(); Use(this.x); } }");
        assert_eq!(positive[0].counts[4], 1);
        for source in [
            "class C { async Task M() { this.x = await Work(); Use(this.x); } }",
            "class C { async Task M() { this.x = 1; await Work(); this.x = 2; } }",
            "class C { async Task M() { this.x = 1; await Work(); Use(this.y); } }",
            "class C { async Task M(bool b) { if(b) { this.x = 1; } else { await Work(); } Use(this.x); } }",
            "class C { async Task M() { this.x = 1; await Work(); Action a = () => Use(this.x); } }",
            "class C { async Task M() { this.x = 1; await Work(); this.x(); Use(nameof(this.x)); } }",
        ] {
            assert!(extract(source).iter().all(|value| value.counts[4] == 0), "{source}");
        }
    }

    #[test]
    fn evidence_is_bounded_but_all_hits_are_counted() {
        let value = &extract("class C { async Task M() { this.x = 1; await Work(); Use(this.x); Use(this.x); Use(this.x); Use(this.x); } }")[0];
        assert_eq!(value.counts[4], 4);
        assert_eq!(value.evidence[4].len(), 3);
        assert!(value.has_findings());
        assert!(!value.incomplete());
        assert!(value.retained_bytes() > std::mem::size_of::<MethodInteractions>());
        assert_eq!(value.json()["counts"]["W"], 4);
    }

    #[test]
    fn out_argument_is_write_only_and_kills_the_earlier_value_relation() {
        for source in [
            "class C { int x; async Task M() { this.x = 1; await Work(); Set(out this.x); } }",
            "class C { int x; async Task M() { this.x = 1; await Work(); Set(out this.x); Use(this.x); } }",
        ] {
            assert_eq!(extract(source)[0].counts[4], 0, "{source}");
        }
        let value = extract(
            "class C { int x; async Task M() { this.x = 1; await Work(); Use(ref this.x); } }",
        );
        assert_eq!(value[0].counts[4], 1);
    }

    #[test]
    fn target_budget_invalidates_rule_instead_of_retaining_partial_zero() {
        let mut source = String::from("class C { async Task M() {");
        for index in 0..=TARGET_LIMIT {
            source.push_str(&format!("this.x{index} = 1;"));
        }
        source.push_str("await Work(); Use(this.x0); } }");
        let value = &extract(&source)[0];
        assert_eq!(value.counts[4], 0);
        assert!(value.incomplete());
        assert_eq!(value.abstentions["target_budget"], 1);
    }

    #[test]
    fn read_ancestor_budget_invalidates_prior_findings_instead_of_silent_miss() {
        let source = format!(
            "class C {{ async Task M() {{ this.x = 1; await Work(); Use(this.x); Use({}this.x{}); Use(this.x); }} }}",
            "(".repeat(ANCESTOR_LIMIT),
            ")".repeat(ANCESTOR_LIMIT),
        );
        let value = &extract(&source)[0];
        assert!(value.incomplete());
        assert_eq!(value.abstentions["read_ancestor_budget"], 1);
        assert_eq!(value.counts[4], 0);
        assert!(value.evidence[4].is_empty());
        assert_eq!(value.json()["incomplete"], true);
    }

    #[test]
    fn parameter_callback_requires_its_own_lock_body_and_parameter_binding() {
        let values = extract("class C { void M(Action callback) { lock(gate) { callback(); } } void N(Action callback) { callback(); lock(gate) { Helper(); } } object G(Func<object> callback) { lock(callback()) { return gate; } } void L(Action callback) { lock(gate) { Action nested = () => callback(); } } void I(Action callback) { lock(gate) { callback.Invoke(); callback?.Invoke(); Use(nameof(callback)); } } }");
        assert_eq!(
            values
                .iter()
                .map(|value| value.counts[5])
                .collect::<Vec<_>>(),
            vec![1, 0, 0, 0, 0, 0]
        );
        let renamed = extract("class C { void M(Action @renamed) { lock(gate) renamed(); } }");
        assert_eq!(renamed[0].counts[5], 1);
        let lambda = extract("class C { void M() { Action<Action> nested = callback => { lock(gate) callback(); }; } }");
        assert_eq!(lambda[0].counts[5], 0);
        assert_eq!(lambda[1].counts[5], 1);
    }

    #[test]
    fn parameter_shadowing_and_parameter_or_ancestor_caps_are_explicit_unknowns() {
        // Intentionally semantically invalid C# verifies conservative abstention:
        // the syntax parser does not validate declaration-space legality.
        let shadowed = extract("class C { void M(Action callback) { lock(gate) callback(); { Action callback = Get(); callback(); } } }");
        assert!(shadowed[0].incomplete());
        assert_eq!(
            shadowed[0].abstentions["unsupported_parameter_shadowing"],
            1
        );
        assert_eq!(shadowed[0].counts[5], 0);
        let parameters = (0..=TARGET_LIMIT)
            .map(|index| format!("Action p{index}"))
            .collect::<Vec<_>>()
            .join(",");
        let capped = extract(&format!(
            "class C {{ void M({parameters}) {{ lock(gate) p0(); }} }}"
        ));
        assert!(capped[0].incomplete());
        assert_eq!(capped[0].abstentions["parameter_budget"], 1);
        assert_eq!(capped[0].counts[5], 0);
        let deep = extract(&format!(
            "class C {{ void M(Action callback) {{ lock(gate) {{ {} callback(); {} }} }} }}",
            "{".repeat(ANCESTOR_LIMIT),
            "}".repeat(ANCESTOR_LIMIT)
        ));
        assert!(deep[0].incomplete());
        assert_eq!(deep[0].abstentions["ancestor_budget"], 1);
        assert_eq!(deep[0].counts[5], 0);
    }

    #[test]
    fn ancestor_budget_marks_unknown_and_independent_manual_controls_match() {
        let source = format!(
            "class C {{ object M() {{ using (var r = Get()) {{ {} return r; {} }} }} }}",
            "{".repeat(ANCESTOR_LIMIT),
            "}".repeat(ANCESTOR_LIMIT)
        );
        let value = &extract(&source)[0];
        assert!(value.incomplete());
        assert_eq!(value.abstentions["ancestor_budget"], 1);
        assert_eq!(value.counts[0], 0);

        let expected: Value = serde_json::from_str(include_str!(
            "../tests/fixtures/local_review_obligations/expectations.json"
        ))
        .unwrap();
        let mut checked = 0;
        let mut lambdas = 0;
        for (path, source) in [
            (
                "await_regions.cs",
                include_str!("../tests/fixtures/local_review_obligations/await_regions.cs"),
            ),
            (
                "using_escape.cs",
                include_str!("../tests/fixtures/local_review_obligations/using_escape.cs"),
            ),
            (
                "catch_regions.cs",
                include_str!("../tests/fixtures/local_review_obligations/catch_regions.cs"),
            ),
            (
                "finally_regions.cs",
                include_str!("../tests/fixtures/local_review_obligations/finally_regions.cs"),
            ),
            (
                "lock_callbacks.cs",
                include_str!("../tests/fixtures/local_review_obligations/lock_callbacks.cs"),
            ),
        ] {
            let mut collector = Collector::default();
            let facts = CSharpParser::new()
                .unwrap()
                .parse_observed(path.into(), source, |node, owner, _| {
                    collector.observe(node, owner, source)
                })
                .unwrap();
            let signals = collector.finish(facts.methods.len());
            let complexity = cpath::complexity::ComplexityAnalyzer::analyze(
                &cpath::complexity::CognitiveComplexity,
                &facts,
            );
            for ((method, metric), signals) in
                facts.methods.iter().zip(complexity.methods).zip(signals)
            {
                if let Some(row) = expected["cases"][path].get(&method.name) {
                    let mut actual = vec![metric.score];
                    actual.extend(signals.counts);
                    assert_eq!(json!(actual), *row, "{path}:{}", method.name);
                    checked += 1;
                } else if method.name.starts_with("<lambda_expression@") {
                    assert_eq!(metric.score, 0);
                    assert_eq!(signals.counts, [0; 6]);
                    lambdas += 1;
                }
            }
        }
        assert_eq!(checked, 45);
        assert_eq!(lambdas, 6);
    }
}
