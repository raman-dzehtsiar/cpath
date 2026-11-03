// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

//! Independent syntax accounting for the explicitly named RCA and maintidx profiles.
use super::{callable, name, text};
use crate::maintainability::{SyntaxCounts, UnitFacts, UnitKind};
use crate::metrics::web_go::{Language, MaintainabilityFacts, MaintainabilityUnit};
use std::collections::{BTreeSet, HashMap, HashSet};
use tree_sitter::Node;

#[derive(Default)]
struct Tokens {
    operators: HashMap<String, u64>,
    grammar_operators: HashMap<u16, u64>,
    operands: HashMap<String, u64>,
    operator_occurrences: u64,
    operand_occurrences: u64,
    decisions: u64,
}

fn merge_counts<Key: Eq + std::hash::Hash>(
    target: &mut HashMap<Key, u64>,
    mut source: HashMap<Key, u64>,
) {
    if target.len() < source.len() {
        std::mem::swap(target, &mut source);
    }
    for (key, count) in source {
        *target.entry(key).or_default() += count;
    }
}

struct LineDirective {
    offset: usize,
    physical_line: i64,
    logical_line: i64,
}

fn line_directive(node: Node<'_>, source: &str) -> Option<LineDirective> {
    let comment = text(node, source);
    let (target, offset, physical_line) = if let Some(target) = comment.strip_prefix("//line ") {
        if node.start_position().column != 0 || node.end_byte() >= source.len() {
            return None;
        }
        (
            target.strip_suffix('\r').unwrap_or(target),
            node.end_byte() + 1,
            node.end_position().row as i64 + 2,
        )
    } else {
        let target = comment.strip_prefix("/*line ")?.strip_suffix("*/")?;
        (target, node.end_byte(), node.end_position().row as i64 + 1)
    };
    let (prefix, last_number) = target.rsplit_once(':')?;
    let last_number = last_number.parse::<i64>().ok()?;
    if !(1..=1 << 30).contains(&last_number) {
        return None;
    }
    let logical_line = prefix
        .rsplit_once(':')
        .and_then(|(_, value)| value.parse::<i64>().ok())
        .unwrap_or(last_number);
    if !(1..=1 << 30).contains(&logical_line) {
        return None;
    }
    Some(LineDirective {
        offset,
        physical_line,
        logical_line,
    })
}

fn adjusted_line(offset: usize, physical_line: u32, directives: &[LineDirective]) -> i64 {
    let index = directives.partition_point(|directive| directive.offset <= offset);
    if index == 0 {
        return i64::from(physical_line);
    }
    let directive = &directives[index - 1];
    i64::from(physical_line) + directive.logical_line - directive.physical_line
}

impl Tokens {
    fn add_text(counts: &mut HashMap<String, u64>, value: &str, amount: u64) {
        if let Some(count) = counts.get_mut(value) {
            *count += amount;
            return;
        }
        counts.insert(value.to_owned(), amount);
    }
    fn operator(&mut self, value: &str) {
        Self::add_text(&mut self.operators, value, 1);
        self.operator_occurrences += 1;
    }
    fn operand(&mut self, value: &str) {
        Self::add_text(&mut self.operands, value, 1);
        self.operand_occurrences += 1;
    }
    fn grammar_operator(&mut self, kind: u16) {
        *self.grammar_operators.entry(kind).or_default() += 1;
        self.operator_occurrences += 1;
    }
    fn merge(&mut self, child: Self) {
        merge_counts(&mut self.operators, child.operators);
        merge_counts(&mut self.grammar_operators, child.grammar_operators);
        merge_counts(&mut self.operands, child.operands);
        self.operator_occurrences += child.operator_occurrences;
        self.operand_occurrences += child.operand_occurrences;
        self.decisions += child.decisions + 1;
    }
    fn counts(&self) -> SyntaxCounts {
        SyntaxCounts {
            operators: self.operator_occurrences,
            operands: self.operand_occurrences,
            distinct_operators: (self.operators.len() + self.grammar_operators.len()) as u64,
            distinct_operands: self.operands.len() as u64,
            decisions: self.decisions,
            logical_statements: 0,
        }
    }
}

fn web_operator(kind: &str) -> bool {
    matches!(
        kind,
        "export"
            | "import"
            | "extends"
            | "."
            | "from"
            | "("
            | ","
            | "as"
            | "*"
            | ">>"
            | ">>>"
            | ":"
            | "return"
            | "delete"
            | "throw"
            | "break"
            | "continue"
            | "if"
            | "else"
            | "switch"
            | "case"
            | "default"
            | "async"
            | "for"
            | "in"
            | "of"
            | "while"
            | "try"
            | "catch"
            | "finally"
            | "with"
            | "="
            | "@"
            | "&&"
            | "||"
            | "+"
            | "-"
            | "--"
            | "++"
            | "/"
            | "%"
            | "**"
            | "|"
            | "&"
            | "<<"
            | "~"
            | "<"
            | "<="
            | "=="
            | "!="
            | ">="
            | ">"
            | "+="
            | "!"
            | "!=="
            | "==="
            | "-="
            | "*="
            | "/="
            | "%="
            | "**="
            | ">>="
            | ">>>="
            | "<<="
            | "&="
            | "^"
            | "^="
            | "|="
            | "yield"
            | "["
            | "{"
            | "await"
            | "?"
            | "??"
            | "new"
            | "let"
            | "var"
            | "const"
            | "function"
            | "function_expression"
            | ";"
    )
}

fn web_operand(kind: &str) -> bool {
    matches!(
        kind,
        "identifier"
            | "member_expression"
            | "nested_identifier"
            | "property_identifier"
            | "string"
            | "number"
            | "true"
            | "false"
            | "null"
            | "void"
            | "this"
            | "super"
            | "undefined"
            | "set"
            | "get"
            | "typeof"
            | "instanceof"
    )
}

fn web_tokens(tokens: &mut Tokens, node: Node<'_>, source: &str) {
    let kind = node.kind();
    if web_operator(kind) {
        tokens.grammar_operator(node.kind_id());
    } else if web_operand(kind) && (!matches!(kind, "number" | "string") || node.is_named()) {
        tokens.operand(text(node, source));
    }
    if matches!(
        kind,
        "if" | "for" | "while" | "case" | "catch" | "ternary_expression" | "&&" | "||"
    ) {
        tokens.decisions += 1;
    }
}

#[derive(Clone, Copy)]
struct Binding {
    start: usize,
    key_start: usize,
    end: usize,
    declaration_start: usize,
    declaration_end: usize,
    function: bool,
}

pub(super) type GoBindings = HashMap<String, NameBindings>;

pub(super) struct NameBindings {
    ordinary: BindingIndex,
    literal_keys: Option<BindingIndex>,
    labels: HashSet<usize>,
    unresolved: HashSet<usize>,
}

struct BindingIndex {
    changes: Vec<(usize, Option<bool>)>,
}

struct BindingEvent {
    offset: usize,
    binding: usize,
    enters: bool,
}

impl BindingIndex {
    fn new(bindings: &[Binding], name_bytes: usize, literal_keys: bool) -> Self {
        let mut events = Vec::new();
        for (index, binding) in bindings.iter().enumerate() {
            let start = if literal_keys {
                binding.key_start
            } else {
                binding.start
            };
            let mut add = |start, end| {
                events.push(BindingEvent {
                    offset: start,
                    binding: index,
                    enters: true,
                });
                events.push(BindingEvent {
                    offset: end,
                    binding: index,
                    enters: false,
                });
            };
            if let Some(last_start) = binding
                .end
                .checked_sub(name_bytes)
                .filter(|last| *last >= start)
            {
                add(start, last_start + 1);
            }
            if binding.declaration_end - binding.declaration_start == name_bytes {
                add(binding.declaration_start, binding.declaration_start + 1);
            }
        }
        events.sort_by_key(|event| event.offset);
        let mut active = BTreeSet::new();
        let mut counts = vec![0_u8; bindings.len()];
        let mut changes = Vec::new();
        let mut previous = None;
        let mut index = 0;
        while index < events.len() {
            let offset = events[index].offset;
            while index < events.len() && events[index].offset == offset {
                let event = &events[index];
                let binding = &bindings[event.binding];
                // Preserve the original shortest-scope choice and first-binding tie break.
                let priority = (binding.end - binding.start, event.binding);
                if event.enters {
                    counts[event.binding] += 1;
                    active.insert(priority);
                } else {
                    counts[event.binding] -= 1;
                    if counts[event.binding] == 0 {
                        active.remove(&priority);
                    }
                }
                index += 1;
            }
            let value = active.first().map(|(_, index)| bindings[*index].function);
            if value != previous {
                changes.push((offset, value));
                previous = value;
            }
        }
        Self { changes }
    }

    fn get(&self, offset: usize) -> Option<bool> {
        let index = self.changes.partition_point(|(start, _)| *start <= offset);
        index.checked_sub(1).and_then(|index| self.changes[index].1)
    }
}

fn function_scope(kind: &str) -> bool {
    matches!(
        kind,
        "source_file" | "function_declaration" | "method_declaration" | "func_literal"
    )
}

fn lexical_scope(kind: &str) -> bool {
    function_scope(kind)
        || matches!(
            kind,
            "block"
                | "for_statement"
                | "if_statement"
                | "expression_switch_statement"
                | "type_switch_statement"
                | "communication_case"
                | "expression_case"
                | "type_case"
        )
}

fn identifier_kind(kind: &str) -> bool {
    matches!(
        kind,
        "identifier"
            | "type_identifier"
            | "field_identifier"
            | "package_identifier"
            | "label_name"
            | "nil"
            | "true"
            | "false"
            | "iota"
    )
}

fn unresolved_declaration_left(
    root: Node<'_>,
    source: &str,
    visited: &mut HashSet<usize>,
    unresolved: &mut HashMap<String, HashSet<usize>>,
) {
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if !visited.insert(node.id()) {
            continue;
        }
        if identifier_kind(node.kind()) {
            unresolved
                .entry(text(node, source).to_owned())
                .or_default()
                .insert(node.start_byte());
        }
        let mut cursor = node.walk();
        pending.extend(node.named_children(&mut cursor));
    }
}

pub(super) fn go_bindings(root: Node<'_>, source: &str) -> GoBindings {
    let mut bindings: HashMap<String, Vec<Binding>> = HashMap::new();
    let mut declared_labels = HashSet::new();
    let mut label_references = Vec::new();
    let mut labels: HashMap<String, HashSet<usize>> = HashMap::new();
    let mut unresolved = HashMap::new();
    let mut unresolved_nodes = HashSet::new();
    let mut stack = vec![(root, root, root, root.id())];
    while let Some((node, scope, parameter_scope, function_owner)) = stack.pop() {
        let kind = node.kind();
        if kind == "label_name" {
            let name = text(node, source).to_owned();
            bindings.entry(name.clone()).or_default();
            if node
                .parent()
                .is_some_and(|parent| parent.kind() == "labeled_statement")
            {
                labels
                    .entry(name.clone())
                    .or_default()
                    .insert(node.start_byte());
                if name != "_" {
                    declared_labels.insert((function_owner, name));
                }
            } else {
                label_references.push((function_owner, name, node.start_byte()));
            }
        }
        let parameter = matches!(
            kind,
            "parameter_declaration"
                | "variadic_parameter_declaration"
                | "type_parameter_declaration"
        );
        let declaration = parameter
            || matches!(
                kind,
                "var_spec" | "const_spec" | "type_spec" | "type_alias" | "function_declaration"
            );
        let short = matches!(
            kind,
            "short_var_declaration"
                | "range_clause"
                | "receive_statement"
                | "type_switch_statement"
        ) && {
            let mut cursor = node.walk();
            let declares = node.children(&mut cursor).any(|child| child.kind() == ":=");
            declares
        };
        if declaration || short {
            let scope = if parameter {
                parameter_scope
            } else if kind == "type_switch_statement" {
                node
            } else {
                scope
            };
            let global = scope.kind() == "source_file";
            let mut names = Vec::new();
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if declaration && node.child_by_field_name("name") == Some(child) {
                    names.push(child);
                }
            }
            if parameter || matches!(kind, "var_spec" | "const_spec") {
                let mut cursor = node.walk();
                if cursor.goto_first_child() {
                    loop {
                        if cursor.field_name() == Some("name") && !names.contains(&cursor.node()) {
                            names.push(cursor.node());
                        }
                        if !cursor.goto_next_sibling() {
                            break;
                        }
                    }
                }
            }
            if short {
                let field = if kind == "type_switch_statement" {
                    "alias"
                } else {
                    "left"
                };
                if let Some(left) = node.child_by_field_name(field) {
                    if left.kind() == "identifier" {
                        names.push(left);
                    } else {
                        let mut cursor = left.walk();
                        for child in left.named_children(&mut cursor) {
                            if child.kind() == "identifier" {
                                names.push(child);
                            } else if child.kind() != "comment" && kind != "range_clause" {
                                // Go's parser does not resolve a non-identifier declaration LHS.
                                unresolved_declaration_left(
                                    child,
                                    source,
                                    &mut unresolved_nodes,
                                    &mut unresolved,
                                );
                            }
                        }
                    }
                }
            }
            for identifier in names {
                let identifier_name = text(identifier, source);
                if kind == "function_declaration" && identifier_name == "init" {
                    continue;
                }
                let start = if identifier_name == "_" {
                    identifier.start_byte()
                } else if parameter && kind != "type_parameter_declaration" {
                    scope
                        .child_by_field_name("body")
                        .map_or(scope.end_byte(), |body| body.start_byte())
                } else if global || parameter {
                    scope.start_byte()
                } else if matches!(kind, "type_spec" | "type_alias") {
                    identifier.start_byte()
                } else if kind == "type_switch_statement" {
                    node.child_by_field_name("value")
                        .map_or(node.end_byte(), |value| value.end_byte())
                } else {
                    node.end_byte()
                };
                bindings
                    .entry(identifier_name.to_owned())
                    .or_default()
                    .push(Binding {
                        start,
                        key_start: if global && identifier_name != "_" {
                            if matches!(kind, "type_spec" | "type_alias") {
                                identifier.start_byte()
                            } else {
                                node.end_byte()
                            }
                        } else {
                            start
                        },
                        end: if identifier_name == "_" {
                            identifier.end_byte()
                        } else {
                            scope.end_byte()
                        },
                        declaration_start: identifier.start_byte(),
                        declaration_end: identifier.end_byte(),
                        function: kind == "function_declaration",
                    });
            }
        }
        let mut cursor = node.walk();
        let scope = if lexical_scope(kind) { node } else { scope };
        let parameter_scope = if function_scope(kind)
            || matches!(
                kind,
                "function_type" | "method_elem" | "type_spec" | "type_alias"
            ) {
            node
        } else {
            parameter_scope
        };
        let function_owner = if function_scope(kind) {
            node.id()
        } else {
            function_owner
        };
        stack.extend(
            node.named_children(&mut cursor)
                .map(|child| (child, scope, parameter_scope, function_owner)),
        );
    }
    for (owner, name, offset) in label_references {
        if declared_labels.contains(&(owner, name.clone())) {
            labels.entry(name).or_default().insert(offset);
        }
    }
    for name in unresolved.keys() {
        bindings.entry(name.clone()).or_default();
    }
    bindings
        .into_iter()
        .map(|(name, bindings)| {
            let index = NameBindings {
                ordinary: BindingIndex::new(&bindings, name.len(), false),
                literal_keys: bindings
                    .iter()
                    .any(|binding| binding.start != binding.key_start)
                    .then(|| BindingIndex::new(&bindings, name.len(), true)),
                labels: labels.remove(&name).unwrap_or_default(),
                unresolved: unresolved.remove(&name).unwrap_or_default(),
            };
            (name, index)
        })
        .collect()
}

pub(super) fn identifier_binding(
    node: Node<'_>,
    source: &str,
    bindings: &GoBindings,
) -> Option<bool> {
    let index = bindings.get(text(node, source))?;
    if index.unresolved.contains(&node.start_byte()) {
        return None;
    }
    index.ordinary.get(node.start_byte())
}

fn token_binding(
    node: Node<'_>,
    parent: Option<Node<'_>>,
    source: &str,
    bindings: &GoBindings,
) -> Option<bool> {
    let index = bindings.get(text(node, source))?;
    if index.unresolved.contains(&node.start_byte()) {
        return None;
    }
    let direct_key = parent.is_some_and(|parent| {
        parent.kind() == "literal_element"
            && parent.parent().is_some_and(|owner| {
                owner.kind() == "keyed_element" && owner.child_by_field_name("key") == Some(parent)
            })
    });
    // Go resolves plain literal keys immediately and does not forward-resolve them.
    let scope = if direct_key {
        index.literal_keys.as_ref().unwrap_or(&index.ordinary)
    } else {
        &index.ordinary
    };
    scope.get(node.start_byte())
}

fn go_tokens(
    tokens: &mut Tokens,
    node: Node<'_>,
    parent: Option<Node<'_>>,
    source: &str,
    bindings: &GoBindings,
) {
    let kind = node.kind();
    if matches!(kind, "field_identifier" | "label_name")
        && bindings
            .get(text(node, source))
            .is_some_and(|index| index.unresolved.contains(&node.start_byte()))
    {
        tokens.operator(text(node, source));
        return;
    }
    match kind {
        "identifier" | "type_identifier" | "nil" | "true" | "false" | "iota" => {
            if parent.is_some_and(|parent| {
                parent.kind() == "function_declaration"
                    && parent.child_by_field_name("name") == Some(node)
            }) {
                if text(node, source) == "init" {
                    tokens.operator("init");
                }
                return;
            }
            if kind == "type_identifier"
                && parent.is_some_and(|parent| parent.kind() == "qualified_type")
            {
                tokens.operator(text(node, source));
                return;
            }
            match token_binding(node, parent, source, bindings) {
                Some(true) => (),
                Some(false) => tokens.operand(text(node, source)),
                None => tokens.operator(text(node, source)),
            }
        }
        "field_identifier" if parent.is_some_and(|parent| parent.kind() == "field_declaration") => {
            tokens.operand(text(node, source));
        }
        "field_identifier" if parent.is_some_and(|parent| parent.kind() == "method_elem") => {}
        "package_identifier" if identifier_binding(node, source, bindings) == Some(false) => {
            tokens.operand(text(node, source));
        }
        "label_name"
            if bindings
                .get(text(node, source))
                .is_some_and(|index| index.labels.contains(&node.start_byte())) =>
        {
            tokens.operand(text(node, source));
        }
        "field_identifier" | "package_identifier" | "label_name" => {
            tokens.operator(text(node, source))
        }
        "int_literal"
        | "float_literal"
        | "imaginary_literal"
        | "rune_literal"
        | "raw_string_literal"
        | "interpreted_string_literal" => tokens.operand(text(node, source)),
        "function_declaration" => {
            if let Some(name) = node.child_by_field_name("name") {
                tokens.operator(text(name, source));
            }
            tokens.operator("func");
            tokens.operator("()");
        }
        "method_declaration" => {
            tokens.operator("()");
            tokens.operator("func");
            tokens.operator("()");
        }
        "func_literal" | "function_type" => {
            tokens.operator("func");
            tokens.operator("()");
        }
        "var_declaration" | "const_declaration" | "type_declaration" => {
            tokens.operand(match kind {
                "var_declaration" => "var",
                "const_declaration" => "const",
                _ => "type",
            });
            let mut cursor = node.walk();
            if node.children(&mut cursor).any(|child| child.kind() == "(") {
                tokens.operator("()");
            }
        }
        "block" | "composite_literal" => tokens.operator("{}"),
        "parenthesized_expression"
        | "parenthesized_type"
        | "var_spec_list"
        | "type_assertion_expression"
        | "call_expression"
        | "type_conversion_expression"
        | "method_elem" => tokens.operator("()"),
        "literal_value" if parent.is_some_and(|parent| parent.kind() != "composite_literal") => {
            tokens.operator("{}")
        }
        "index_expression" => tokens.operator("{}"),
        "type_instantiation_expression" => {
            // A value index can share this syntax shape with chained type arguments.
            // Go's IndexExpr counts a bracket pair; IndexListExpr does not.
            let mut arguments = None;
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                match child.kind() {
                    "[" => arguments = Some(0),
                    "]" => {
                        if arguments == Some(1) {
                            tokens.operator("{}");
                        }
                        arguments = None;
                    }
                    "comment" => (),
                    _ if child.is_named() => {
                        if let Some(count) = &mut arguments {
                            *count += 1;
                        }
                    }
                    _ => (),
                }
            }
        }
        "slice_expression" => tokens.operator("[]"),
        "pointer_type" => tokens.operator("*"),
        "field_declaration" => {
            let mut cursor = node.walk();
            if node.children(&mut cursor).any(|child| child.kind() == "*") {
                tokens.operator("*");
            }
        }
        "negated_type" => tokens.operator("~"),
        "type_elem" | "type_constraint" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "|" {
                    tokens.operator("|");
                }
            }
        }
        "variadic_argument" | "variadic_parameter_declaration" | "implicit_length_array_type" => {
            tokens.operator("...")
        }
        "binary_expression"
        | "unary_expression"
        | "assignment_statement"
        | "short_var_declaration" => {
            if let Some(operator) = node.child_by_field_name("operator") {
                tokens.operator(text(operator, source));
            } else if kind == "short_var_declaration" {
                tokens.operator(":=");
            }
        }
        "inc_statement" => tokens.operator("++"),
        "dec_statement" => tokens.operator("--"),
        "send_statement" => tokens.operator("<-"),
        "channel_type" => {
            tokens.operator("chan");
            let mut cursor = node.walk();
            if node.children(&mut cursor).any(|child| child.kind() == "<-") {
                tokens.operator("<-");
            }
        }
        "keyed_element" => tokens.operator(":"),
        "return_statement" => tokens.operator("return"),
        "go_statement" => tokens.operator("go"),
        "defer_statement" => tokens.operator("defer"),
        "break_statement" => tokens.operand("break"),
        "continue_statement" => tokens.operand("continue"),
        "goto_statement" => tokens.operand("goto"),
        "fallthrough_statement" => tokens.operand("fallthrough"),
        "if_statement" => {
            tokens.operator("if");
            if node.child_by_field_name("alternative").is_some() {
                tokens.operator("else");
            }
            tokens.decisions += 1;
        }
        "expression_switch_statement" => {
            tokens.operator("switch");
            tokens.operator("{}");
        }
        "type_switch_statement" => {
            tokens.operator("{}");
            tokens.operator("()");
            if node.child_by_field_name("alias").is_some() {
                tokens.operator(":=");
            }
        }
        "select_statement" => {
            tokens.operator("select");
            tokens.operator("{}");
        }
        "for_statement" => {
            tokens.operator("for");
            tokens.decisions += 1;
        }
        "range_clause" => {
            tokens.operator("range");
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if matches!(child.kind(), ":=" | "=") {
                    tokens.operator(child.kind());
                }
            }
        }
        "receive_statement" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if matches!(child.kind(), ":=" | "=") {
                    tokens.operator(child.kind());
                }
            }
        }
        "expression_case" | "type_case" => {
            tokens.operator(":");
            tokens.decisions += 1;
        }
        "communication_case" => tokens.decisions += 1,
        "default_case" if parent.is_some_and(|parent| parent.kind() != "select_statement") => {
            tokens.operator("default");
            tokens.operator(":");
        }
        _ => (),
    }
    if let Some(arguments) = node.child_by_field_name("type_arguments") {
        let mut cursor = arguments.walk();
        if arguments
            .named_children(&mut cursor)
            .filter(|child| child.kind() != "comment")
            .count()
            == 1
        {
            tokens.operator("{}");
        }
    }
    if kind == "binary_expression"
        && node
            .child_by_field_name("operator")
            .is_some_and(|operator| matches!(text(operator, source), "&&" | "||"))
    {
        tokens.decisions += 1;
    }
}

fn needs_original_typescript(node: Node<'_>, parent: Option<Node<'_>>) -> bool {
    match node.kind() {
        "type_parameters" => parent.is_some_and(|parent| parent.kind() == "function_type"),
        "arrow_function" if node.child_by_field_name("return_type").is_some() => parent
            .is_some_and(|parent| {
                parent.kind() == "ternary_expression"
                    && parent.child_by_field_name("consequence") == Some(node)
            }),
        _ => false,
    }
}

pub(super) fn extract(
    root: Node<'_>,
    source: &str,
    language: Language,
    bindings: &GoBindings,
    check_typescript_compatibility: bool,
) -> Option<MaintainabilityFacts> {
    let go = language == Language::Go;
    let mut units = Vec::new();
    let mut tokens = Vec::<Tokens>::new();
    let mut parents = Vec::new();
    let mut directives = Vec::new();
    let mut stack = vec![(root, None, None)];
    while let Some((node, parent, mut scope)) = stack.pop() {
        let kind = node.kind();
        if check_typescript_compatibility && needs_original_typescript(node, parent) {
            // The official RCA profile retains its original syntax ownership.
            // Stop before accumulating more token maps; the caller reuses the
            // published grammar only when one of these productions is present.
            return None;
        }
        if kind == "comment" {
            if go {
                if let Some(directive) = line_directive(node, source) {
                    directives.push(directive);
                }
            }
            continue;
        }
        let function = callable(node, language);
        let container = !go
            && node.is_named()
            && matches!(
                node.kind(),
                "class" | "class_declaration" | "interface_declaration"
            );
        let program = !go && node == root;
        if function || container || program {
            let index = units.len();
            parents.push(scope);
            scope = Some(index);
            tokens.push(Tokens::default());
            let lines = node.end_position().row - node.start_position().row + usize::from(!program);
            units.push(MaintainabilityUnit {
                formula_lines: lines as u64,
                cyclomatic: 1,
                facts: UnitFacts {
                    name: if program {
                        "<file>".into()
                    } else {
                        name(node, parent, source)
                    },
                    kind: if program {
                        UnitKind::TopLevel
                    } else if container {
                        UnitKind::Container
                    } else {
                        UnitKind::Callable
                    },
                    start_byte: node.start_byte(),
                    end_byte: node.end_byte(),
                    start_line: node.start_position().row as u32 + 1,
                    end_line: node.end_position().row as u32 + 1,
                    counts: SyntaxCounts::default(),
                },
            });
            if program && node.child_count() == 0 {
                units[index].facts.start_line = 0;
                units[index].facts.end_line = 0;
            }
        }
        if let Some(index) = scope {
            if go {
                go_tokens(&mut tokens[index], node, parent, source, bindings);
            } else {
                web_tokens(&mut tokens[index], node, source);
            }
        }
        let mut cursor = node.walk();
        let children: Vec<_> = node.children(&mut cursor).collect();
        for child in children.into_iter().rev() {
            stack.push((child, Some(node), scope));
        }
    }
    for index in (0..units.len()).rev() {
        units[index].facts.counts = tokens[index].counts();
        units[index].cyclomatic = tokens[index].decisions + 1;
        if let Some(parent) = parents[index] {
            // Counts are frozen above; move the completed maps instead of retaining
            // a copy at every ancestor of a deeply nested callable.
            let completed = std::mem::take(&mut tokens[index]);
            tokens[parent].merge(completed);
        }
    }
    if go && !directives.is_empty() {
        for unit in &mut units {
            let start = adjusted_line(unit.facts.start_byte, unit.facts.start_line, &directives);
            let end = adjusted_line(unit.facts.end_byte, unit.facts.end_line, &directives);
            unit.formula_lines = (end - start + 1).max(0) as u64;
        }
    }
    Some(MaintainabilityFacts {
        language,
        units,
        used_compatibility_grammar: false,
        unscored_roots: 0,
    })
}
