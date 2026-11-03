// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

//! Syntax approximation of Microsoft.CodeAnalysis.Metrics 5.6 member inputs.
//! Symbol identity and operation kinds are inferred locally, without compilation.

use crate::maintainability::{MaintainabilityFacts, SyntaxCounts, UnitFacts, UnitKind};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

fn text<'s>(node: Node<'_>, source: &'s str) -> &'s str {
    &source[node.byte_range()]
}

fn children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

fn body(node: Node<'_>) -> Option<Node<'_>> {
    node.child_by_field_name("body").or_else(|| {
        children(node)
            .into_iter()
            .find(|child| matches!(child.kind(), "arrow_expression_clause" | "block"))
    })
}

fn literal(kind: &str) -> bool {
    matches!(
        kind,
        "integer_literal"
            | "real_literal"
            | "boolean_literal"
            | "null_literal"
            | "character_literal"
            | "string_literal"
            | "verbatim_string_literal"
            | "raw_string_literal"
    )
}

fn literal_key(node: Node<'_>, source: &str) -> String {
    let value = text(node, source);
    if node.kind() == "integer_literal" {
        let digits = value.replace('_', "");
        let digits = digits.trim_end_matches(['u', 'U', 'l', 'L']);
        let parsed = if let Some(hex) = digits
            .strip_prefix("0x")
            .or_else(|| digits.strip_prefix("0X"))
        {
            i128::from_str_radix(hex, 16)
        } else if let Some(binary) = digits
            .strip_prefix("0b")
            .or_else(|| digits.strip_prefix("0B"))
        {
            i128::from_str_radix(binary, 2)
        } else {
            digits.parse::<i128>()
        };
        if let Ok(number) = parsed {
            return format!("constant:int:{number}");
        }
    }
    format!("constant:{}:{value}", node.kind())
}

#[derive(Default)]
struct Symbols<'s> {
    members: HashSet<&'s str>,
    void_methods: HashSet<&'s str>,
    value_methods: HashSet<&'s str>,
    constants: HashMap<&'s str, String>,
    types: HashSet<&'s str>,
    value_members: HashSet<(&'s str, &'s str)>,
    void_members: HashSet<(&'s str, &'s str)>,
}

fn enclosing_type(mut node: Node<'_>) -> Option<Node<'_>> {
    loop {
        if matches!(
            node.kind(),
            "class_declaration"
                | "struct_declaration"
                | "record_declaration"
                | "interface_declaration"
        ) {
            return node.child_by_field_name("name");
        }
        node = node.parent()?;
    }
}

fn simple_type(value: &str) -> &str {
    value
        .trim_end_matches('?')
        .split('<')
        .next()
        .unwrap_or(value)
        .rsplit(['.', ':'])
        .next()
        .unwrap_or(value)
}

fn symbols<'s>(root: Node<'_>, source: &'s str) -> Symbols<'s> {
    let mut symbols = Symbols::default();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if matches!(
            node.kind(),
            "class_declaration"
                | "struct_declaration"
                | "interface_declaration"
                | "enum_declaration"
                | "record_declaration"
        ) {
            if let Some(name) = node.child_by_field_name("name") {
                symbols.types.insert(text(name, source));
            }
            if node.kind() == "record_declaration" {
                for parameter in children(node)
                    .into_iter()
                    .filter(|child| child.kind() == "parameter_list")
                    .flat_map(children)
                {
                    if let Some(name) = parameter.child_by_field_name("name") {
                        // Positional records synthesize instance properties.
                        symbols.members.insert(text(name, source));
                    }
                }
            }
        }
        if node.kind() == "method_declaration" {
            if let Some(name) = node.child_by_field_name("name") {
                let owner = enclosing_type(node).map(|owner| text(owner, source));
                if node
                    .child_by_field_name("returns")
                    .is_some_and(|kind| text(kind, source) == "void")
                {
                    symbols.void_methods.insert(text(name, source));
                    if let Some(owner) = owner {
                        symbols.void_members.insert((owner, text(name, source)));
                    }
                } else {
                    symbols.value_methods.insert(text(name, source));
                    if let Some(owner) = owner {
                        symbols.value_members.insert((owner, text(name, source)));
                    }
                }
            }
            continue;
        }
        if matches!(
            node.kind(),
            "property_declaration"
                | "event_declaration"
                | "variable_declarator"
                | "enum_member_declaration"
        ) {
            if let Some(name) = node.child_by_field_name("name") {
                symbols.members.insert(text(name, source));
                let declaration = node.parent().and_then(|parent| parent.parent());
                if declaration.is_some_and(|parent| {
                    children(parent)
                        .iter()
                        .any(|child| child.kind() == "modifier" && text(*child, source) == "const")
                }) {
                    if let Some(value) = children(node)
                        .into_iter()
                        .last()
                        .filter(|value| literal(value.kind()))
                    {
                        symbols
                            .constants
                            .insert(text(name, source), literal_key(value, source));
                    }
                }
            }
        }
        if !super::is_callable(node) {
            pending.extend(children(node));
        }
    }
    symbols
}

struct Unit<'s> {
    facts: UnitFacts,
    binary: HashSet<&'s str>,
    unary: HashSet<&'s str>,
    cases: HashSet<&'static str>,
    operands: HashSet<String>,
    locals: HashSet<&'s str>,
    resolve_constants: bool,
    resolve_attribute_members: bool,
    out_locals: HashSet<&'s str>,
    owner_type: Option<&'s str>,
    local_types: HashMap<&'s str, &'s str>,
}

impl<'s> Unit<'s> {
    fn new(node: Node<'_>, kind: UnitKind, source: &'s str, symbols: &Symbols<'s>) -> Self {
        let name = node
            .child_by_field_name("name")
            .or_else(|| node.child_by_field_name("operator"))
            .map(|name| text(name, source).to_owned())
            .unwrap_or_else(|| {
                format!(
                    "<{}@{}:{}>",
                    node.kind(),
                    node.start_position().row + 1,
                    node.start_position().column + 1
                )
            });
        let mut unit = Self {
            facts: UnitFacts {
                name,
                kind,
                start_byte: node.start_byte(),
                end_byte: node.end_byte(),
                start_line: node.start_position().row as u32 + 1,
                end_line: node.end_position().row as u32 + 1,
                counts: SyntaxCounts::default(),
            },
            binary: HashSet::new(),
            unary: HashSet::new(),
            cases: HashSet::new(),
            operands: HashSet::new(),
            locals: HashSet::new(),
            resolve_constants: body(node).is_some_and(|body| resolved_constant_body(body, source)),
            // New attribute-member calibration covers primary constructors
            // and constructors without user-written statements. Other bodies
            // retain the v2 approximation until their remaining inputs resolve.
            resolve_attribute_members: node.kind() == "parameter_list"
                || node.kind() == "constructor_declaration"
                    && body(node).is_some_and(|body| {
                        body.kind() == "block"
                            && children(body).iter().all(|child| child.kind() == "comment")
                    }),
            out_locals: HashSet::new(),
            owner_type: enclosing_type(node).map(|owner| text(owner, source)),
            local_types: HashMap::new(),
        };
        let mut pending = vec![node];
        while let Some(child) = pending.pop() {
            if child.kind() == "argument" {
                let mut cursor = child.walk();
                if child
                    .children(&mut cursor)
                    .any(|token| token.kind() == "out")
                {
                    for value in children(child).into_iter().filter(|value| {
                        value.kind() == "identifier"
                            && Some(*value) != child.child_by_field_name("name")
                    }) {
                        unit.out_locals
                            .insert(text(value, source).trim_start_matches('@'));
                    }
                }
            }
            if matches!(
                child.kind(),
                "parameter"
                    | "variable_declarator"
                    | "declaration_pattern"
                    | "foreach_statement"
                    | "catch_declaration"
            ) {
                if let Some(name) = child
                    .child_by_field_name("name")
                    .or_else(|| child.child_by_field_name("left"))
                {
                    unit.locals
                        .insert(text(name, source).trim_start_matches('@'));
                    let declared = child.child_by_field_name("type").or_else(|| {
                        child
                            .parent()
                            .filter(|parent| parent.kind() == "variable_declaration")
                            .and_then(|parent| parent.child_by_field_name("type"))
                    });
                    let kind = declared
                        .filter(|kind| text(*kind, source) != "var")
                        .or_else(|| {
                            children(child)
                                .into_iter()
                                .find(|value| value.kind() == "object_creation_expression")
                                .and_then(|value| value.child_by_field_name("type"))
                        });
                    if let Some(kind) = kind {
                        let kind = simple_type(text(kind, source));
                        if symbols.types.contains(kind) {
                            unit.local_types
                                .insert(text(name, source).trim_start_matches('@'), kind);
                        }
                    }
                }
            }
            if child.kind() == "implicit_parameter" {
                unit.locals.insert(text(child, source));
            }
            pending.extend(children(child));
        }
        if node.kind() == "accessor_declaration" {
            unit.locals.insert("value");
        }
        unit
    }

    fn operand(&mut self, value: String) {
        self.facts.counts.operands += 1;
        self.operands.insert(value);
    }

    fn operator(&mut self) {
        self.facts.counts.operators += 1;
    }

    fn finish(mut self) -> UnitFacts {
        // Released Microsoft 5.6.0 starts from binary kinds, not all operation kinds.
        self.facts.counts.distinct_operators = (self.binary.len()
            + self.binary.len().saturating_sub(1)
            + self.unary.len().saturating_sub(1)
            + self.cases.len().saturating_sub(1))
            as u64;
        self.facts.counts.distinct_operands = self.operands.len() as u64;
        self.facts
    }
}

/// Restrict new constant-expression inference to bodies whose complete input
/// vocabulary is locally known. Mixed bodies retain the approximate convention;
/// folding one operand amid unresolved calls can otherwise amplify their drift.
fn resolved_constant_body(root: Node<'_>, source: &str) -> bool {
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if literal(node.kind()) || node.kind() == "comment" {
            continue;
        }
        match node.kind() {
            "arrow_expression_clause"
            | "parenthesized_expression"
            | "throw_expression"
            | "argument_list" => {
                pending.extend(children(node));
            }
            "argument" => pending.extend(
                children(node)
                    .into_iter()
                    .filter(|child| Some(*child) != node.child_by_field_name("name")),
            ),
            "object_creation_expression" | "implicit_object_creation_expression" => {
                if let Some(arguments) = node.child_by_field_name("arguments") {
                    pending.push(arguments);
                }
                if children(node)
                    .into_iter()
                    .any(|child| child.kind() == "initializer_expression")
                {
                    return false;
                }
            }
            "invocation_expression"
                if node
                    .child_by_field_name("function")
                    .is_some_and(|function| text(function, source) == "nameof") => {}
            "member_access_expression" if method_name(node, source) == "Length" => {
                let Some(receiver) = node.child_by_field_name("expression") else {
                    return false;
                };
                if receiver.kind() != "string_literal" && receiver.kind() != "invocation_expression"
                {
                    return false;
                }
                pending.push(receiver);
            }
            "interpolated_string_expression"
                if constant_value(node, source, &Symbols::default()).is_some() => {}
            _ => return false,
        }
    }
    true
}

fn is_type(kind: &str) -> bool {
    matches!(
        kind,
        "predefined_type"
            | "type_argument_list"
            | "type_parameter_list"
            | "type_parameter_constraints_clause"
            | "nullable_type"
            | "pointer_type"
            | "function_pointer_type"
            | "qualified_name"
            | "alias_qualified_name"
            | "modifier"
    )
}

fn executable(node: Node<'_>) -> bool {
    if empty_delegate_declaration(node) {
        return false;
    }
    let mut pending = vec![node];
    while let Some(child) = pending.pop() {
        if child.kind() == "return_statement" && child.child_count() > 2 {
            return true;
        }
        if literal(child.kind())
            || matches!(
                child.kind(),
                "identifier"
                    | "this"
                    | "base"
                    | "typeof_expression"
                    | "sizeof_expression"
                    | "break_statement"
                    | "continue_statement"
                    | "goto_statement"
            )
        {
            return true;
        }
        if child.kind() != "comment" {
            pending.extend(children(child));
        }
    }
    false
}

fn empty_delegate_declaration(node: Node<'_>) -> bool {
    if node.kind() != "local_declaration_statement" {
        return false;
    }
    let Some(declaration) = children(node)
        .into_iter()
        .find(|child| child.kind() == "variable_declaration")
    else {
        return false;
    };
    let variables: Vec<_> = children(declaration)
        .into_iter()
        .filter(|child| child.kind() == "variable_declarator")
        .collect();
    !variables.is_empty()
        && variables.into_iter().all(|variable| {
            let values: Vec<_> = children(variable)
                .into_iter()
                .filter(|child| {
                    Some(*child) != variable.child_by_field_name("name")
                        && child.kind() != "comment"
                })
                .collect();
            values.len() == 1
                && matches!(
                    values[0].kind(),
                    "lambda_expression" | "anonymous_method_expression"
                )
                && body(values[0]).is_some_and(|body| body.kind() == "block" && !executable(body))
        })
}

fn count_line(mut node: Node<'_>) -> bool {
    let original = node;
    while let Some(parent) = node.parent() {
        if super::is_conditional_directive(parent) || parent.kind() == "parenthesized_expression" {
            node = parent;
            continue;
        }
        if parent.kind() == "block"
            && (original.kind().ends_with("_statement") || original.kind() == "block")
        {
            return executable(original);
        }
        return matches!(parent.kind(), "if_statement" | "conditional_expression")
            && matches!(original.kind(), "if_statement" | "conditional_expression")
            && executable(original);
    }
    false
}

fn uninitialized_out_declaration(node: Node<'_>, source: &str, out_locals: &HashSet<&str>) -> bool {
    if node.kind() != "local_declaration_statement" || out_locals.is_empty() {
        return false;
    }
    let Some(declaration) = children(node)
        .into_iter()
        .find(|child| child.kind() == "variable_declaration")
    else {
        return false;
    };
    let variables: Vec<_> = children(declaration)
        .into_iter()
        .filter(|child| child.kind() == "variable_declarator")
        .collect();
    !variables.is_empty()
        && variables.into_iter().all(|variable| {
            let name = variable.child_by_field_name("name");
            name.is_some_and(|name| out_locals.contains(text(name, source).trim_start_matches('@')))
                && children(variable)
                    .iter()
                    .all(|child| Some(*child) == name || child.kind() == "comment")
        })
}

fn operator_text<'s>(node: Node<'_>, source: &'s str) -> &'s str {
    if let Some(operator) = node.child_by_field_name("operator") {
        return text(operator, source);
    }
    let mut cursor = node.walk();
    let result = node
        .children(&mut cursor)
        .find(|child| !child.is_named())
        .map(|child| text(child, source))
        .unwrap_or("");
    result
}

fn method_name<'s>(mut node: Node<'_>, source: &'s str) -> &'s str {
    while let Some(name) = node
        .child_by_field_name("name")
        .or_else(|| node.child_by_field_name("function"))
    {
        node = name;
    }
    if node.kind() == "generic_name" {
        return node
            .named_child(0)
            .map(|name| text(name, source))
            .unwrap_or("");
    }
    text(node, source)
}

fn looks_static(node: Node<'_>, source: &str, unit: &Unit<'_>, symbols: &Symbols<'_>) -> bool {
    if node.kind() == "member_access_expression" {
        // A property reached through a type is still an evaluated receiver:
        // DateTime.UtcNow.AddSeconds() must count UtcNow, while System.Math.Abs()
        // must not count namespace/type names as operands.
        let name = method_name(node, source);
        return symbols.types.contains(name)
            || matches!(
                name,
                "Math"
                    | "MathF"
                    | "DateTime"
                    | "DateTimeOffset"
                    | "TimeSpan"
                    | "Type"
                    | "String"
                    | "Convert"
                    | "Enumerable"
                    | "Task"
                    | "Guid"
                    | "Console"
                    | "Environment"
            );
    }
    if is_type(node.kind()) {
        return true;
    }
    let value = text(node, source);
    if unit.locals.contains(value) || symbols.members.contains(value) {
        return false;
    }
    matches!(node.kind(), "identifier" | "generic_name")
        && value.chars().next().is_some_and(char::is_uppercase)
}

fn constant_value(node: Node<'_>, source: &str, symbols: &Symbols<'_>) -> Option<String> {
    if literal(node.kind()) {
        return Some(literal_key(node, source));
    }
    if node.kind() == "identifier" {
        return symbols.constants.get(text(node, source)).cloned();
    }
    if node.kind() == "interpolated_string_expression" {
        let interpolations: Vec<_> = children(node)
            .into_iter()
            .filter(|child| child.kind() == "interpolation")
            .collect();
        let constant = interpolations.iter().all(|interpolation| {
            let expressions: Vec<_> = children(*interpolation)
                .into_iter()
                .filter(|child| child.kind() != "interpolation_brace")
                .collect();
            expressions.len() == 1
                && matches!(
                    expressions[0].kind(),
                    "string_literal" | "identifier" | "invocation_expression"
                )
                && constant_value(expressions[0], source, symbols)
                    .is_some_and(|key| key.starts_with("constant:string_literal:"))
        });
        if constant {
            return Some(format!("constant:interpolated:{}", text(node, source)));
        }
    }
    if node.kind() == "invocation_expression" {
        let function = node.child_by_field_name("function")?;
        if text(function, source) == "nameof" {
            let arguments = node.child_by_field_name("arguments")?;
            let argument = arguments.named_child(0)?.named_child(0)?;
            let name = method_name(argument, source).trim_start_matches('@');
            return Some(format!("constant:string_literal:\"{name}\""));
        }
    }
    if node.kind() != "binary_expression" {
        return None;
    }
    let left = node.child_by_field_name("left")?;
    let right = node.child_by_field_name("right")?;
    if left.kind() != "integer_literal" || right.kind() != "integer_literal" {
        return None;
    }
    let left: i128 = literal_key(left, source)
        .strip_prefix("constant:int:")?
        .parse()
        .ok()?;
    let right: i128 = literal_key(right, source)
        .strip_prefix("constant:int:")?
        .parse()
        .ok()?;
    let value = match operator_text(node, source) {
        "+" => left.checked_add(right),
        "-" => left.checked_sub(right),
        "*" => left.checked_mul(right),
        "/" => left.checked_div(right),
        "%" => left.checked_rem(right),
        _ => None,
    }?;
    Some(format!("constant:int:{value}"))
}

fn identity_select(node: Node<'_>) -> bool {
    node.named_child(0)
        .is_some_and(|expression| expression.kind() == "identifier")
        && node
            .prev_named_sibling()
            .is_some_and(|previous| previous.kind() == "where_clause")
}

fn visit_invocation<'t, 's>(
    unit: &mut Unit<'s>,
    node: Node<'t>,
    source: &'s str,
    symbols: &Symbols<'s>,
    pending: &mut Vec<Node<'t>>,
) {
    unit.operator();
    if let Some(function) = node.child_by_field_name("function") {
        let conditional = function.kind() == "conditional_access_expression";
        let target = if conditional {
            children(function)
                .into_iter()
                .find(|child| child.kind() == "member_binding_expression")
                .unwrap_or(function)
        } else {
            function
        };
        let name = method_name(target, source);
        let parent = node.parent();
        let void_arrow = parent
            .filter(|parent| parent.kind() == "arrow_expression_clause")
            .and_then(|parent| parent.parent())
            .and_then(|parent| parent.child_by_field_name("returns"))
            .is_some_and(|returns| text(returns, source) == "void");
        let discarded =
            parent.is_some_and(|parent| parent.kind() == "expression_statement") || void_arrow;
        let known_void =
            symbols.void_methods.contains(name) && !symbols.value_methods.contains(name);
        // Resolve explicit receivers only. Unqualified/inherited overload
        // lookup retains the established approximation.
        let receiver_type = function
            .child_by_field_name("expression")
            .and_then(|receiver| {
                let value = text(receiver, source).trim_start_matches('@');
                if receiver.kind() == "this" {
                    unit.owner_type
                } else if symbols.types.contains(value) {
                    Some(value)
                } else {
                    unit.local_types.get(value).copied()
                }
            });
        let known_value = receiver_type.is_some_and(|owner| {
            symbols.value_members.contains(&(owner, name))
                && !symbols.void_members.contains(&(owner, name))
        });
        if !known_void && (!discarded || known_value) {
            unit.operand(if function.kind() == "invocation_expression" {
                "method:<delegate-invoke>".into()
            } else {
                format!("method:{name}")
            });
        }
        if function.kind() == "identifier"
            && (unit.locals.contains(name) || symbols.members.contains(name))
        {
            pending.push(function);
        }
        if let Some(receiver) = function.child_by_field_name("expression") {
            if !looks_static(receiver, source, unit, symbols) {
                pending.push(receiver);
            }
        }
        if conditional {
            unit.operator();
            unit.facts.counts.decisions += 1;
            if let Some(receiver) = function.child_by_field_name("condition") {
                pending.push(receiver);
            }
        }
        if matches!(
            function.kind(),
            "invocation_expression"
                | "parenthesized_expression"
                | "cast_expression"
                | "element_access_expression"
        ) {
            pending.push(function);
        }
    }
    if let Some(arguments) = node.child_by_field_name("arguments") {
        pending.push(arguments);
    }
}

fn visit_variable<'t>(
    unit: &mut Unit<'_>,
    node: Node<'t>,
    source: &str,
    pending: &mut Vec<Node<'t>>,
) {
    let name = node.child_by_field_name("name");
    let values: Vec<_> = children(node)
        .into_iter()
        .filter(|child| Some(*child) != name && child.kind() != "comment")
        .collect();
    if values.is_empty() {
        return;
    }
    unit.operator();
    if let Some(name) = name {
        unit.operand(format!(
            "symbol:{}",
            text(name, source).trim_start_matches('@')
        ));
    }
    pending.extend(values);
}

fn visit_parameter<'t>(
    unit: &mut Unit<'_>,
    node: Node<'t>,
    source: &str,
    pending: &mut Vec<Node<'t>>,
) {
    let mut after_equals = false;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "=" {
            after_equals = true;
            continue;
        }
        if after_equals && child.is_named() {
            unit.facts.counts.logical_statements += 2;
            if child.kind() == "member_access_expression" {
                // A legal member-valued parameter default is a constant (e.g.
                // an enum value), rather than a runtime property reference.
                unit.operand(format!("constant:{}", text(child, source)));
            } else {
                pending.push(child);
            }
        }
    }
}

fn count_operation<'s>(unit: &mut Unit<'s>, node: Node<'_>, source: &'s str) {
    match node.kind() {
        "binary_expression" => {
            let operator = operator_text(node, source);
            unit.operator();
            if operator != "??" {
                unit.binary.insert(operator);
            }
            if matches!(operator, "&&" | "||" | "??") {
                unit.facts.counts.decisions += 1;
            }
        }
        "assignment_expression" => {
            let operator = operator_text(node, source);
            if operator != "??=" {
                unit.operator();
            }
            if !matches!(operator, "=" | "??=") {
                unit.binary.insert(operator.trim_end_matches('='));
            }
        }
        "prefix_unary_expression" | "postfix_unary_expression" => {
            let operator = operator_text(node, source);
            if node.kind() == "postfix_unary_expression" && operator == "!" {
                return;
            }
            unit.operator();
            if !matches!(operator, "++" | "--") {
                unit.unary.insert(operator);
            }
        }
        "return_statement" | "yield_statement" => {
            if node.child_count() > 2 {
                unit.operator();
            }
        }
        "if_statement"
        | "conditional_expression"
        | "while_statement"
        | "do_statement"
        | "for_statement"
        | "foreach_statement" => {
            unit.facts.counts.decisions += 1;
        }
        "conditional_access_expression" => {
            unit.operator();
            unit.facts.counts.decisions += 1;
        }
        "select_clause" => {
            // Identity select is elided by the compiler when it follows a where.
            if !identity_select(node) {
                unit.facts.counts.logical_statements += 1;
            }
        }
        "where_clause" | "let_clause" | "ordering" => {
            unit.facts.counts.logical_statements += 1;
        }
        "group_clause" => {
            unit.facts.counts.logical_statements += 2;
        }
        "from_clause" => {
            if node
                .parent()
                .is_some_and(|parent| parent.named_child(0) != Some(node))
            {
                unit.facts.counts.logical_statements += 1;
            }
        }
        "switch_section" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if matches!(child.kind(), "case" | "default") {
                    unit.operator();
                    unit.facts.counts.decisions += 1;
                    unit.cases.insert(if child.kind() == "default" {
                        "default"
                    } else {
                        "case"
                    });
                }
            }
        }
        "element_access_expression"
        | "element_binding_expression"
        | "await_expression"
        | "throw_statement"
        | "throw_expression"
        | "lock_statement"
        | "using_statement"
        | "tuple_expression"
        | "array_creation_expression"
        | "implicit_array_creation_expression"
        | "initializer_expression"
        | "typeof_expression"
        | "sizeof_expression"
        | "is_pattern_expression" => unit.operator(),
        _ => {}
    }
}

fn visit_declaration<'t, 's>(
    unit: &mut Unit<'s>,
    node: Node<'t>,
    source: &'s str,
    pending: &mut Vec<Node<'t>>,
) -> bool {
    match node.kind() {
        "parameter_list" => {
            // The grammar's params-array production is transparent: its type,
            // name and attributes are siblings, not a named parameter node.
            // Attributes on that transparent production still belong to the
            // parameter, not to its containing method.
            pending.extend(
                children(node)
                    .into_iter()
                    .filter(|child| child.kind() != "attribute_list"),
            );
            true
        }
        "parameter" => {
            visit_parameter(unit, node, source, pending);
            true
        }
        "attribute" => {
            unit.facts.counts.logical_statements += 1;
            let arguments = children(node)
                .into_iter()
                .find(|child| child.kind() == "attribute_argument_list");
            let Some(arguments) = arguments else {
                return true;
            };
            if children(arguments).is_empty() {
                return true;
            }
            pending.push(arguments);
            true
        }
        "variable_declaration" => {
            pending.extend(
                children(node)
                    .into_iter()
                    .filter(|child| child.kind() == "variable_declarator"),
            );
            true
        }
        "array_type" => {
            pending.extend(
                children(node)
                    .into_iter()
                    .filter(|child| child.kind() == "array_rank_specifier"),
            );
            true
        }
        "variable_declarator" => {
            visit_variable(unit, node, source, pending);
            true
        }
        "local_function_statement" | "lambda_expression" | "anonymous_method_expression" => {
            if let Some(body) = body(node) {
                if body.kind() != "block" {
                    unit.facts.counts.logical_statements += 1;
                }
                pending.push(body);
            }
            true
        }
        _ => false,
    }
}

fn visit_reference<'t, 's>(
    unit: &mut Unit<'s>,
    node: Node<'t>,
    source: &'s str,
    symbols: &Symbols<'s>,
    pending: &mut Vec<Node<'t>>,
) -> bool {
    match node.kind() {
        "identifier" | "implicit_parameter" => {
            let value = text(node, source).trim_start_matches('@');
            if !unit.locals.contains(value) && symbols.members.contains(value) {
                unit.operator();
            }
            unit.operand(format!("symbol:{value}"));
            true
        }
        "member_access_expression" | "member_binding_expression" => {
            unit.operator();
            unit.operand(format!("member:{}", method_name(node, source)));
            if let Some(receiver) = node.child_by_field_name("expression") {
                if !looks_static(receiver, source, unit, symbols) {
                    pending.push(receiver);
                }
            }
            true
        }
        "invocation_expression" => {
            visit_invocation(unit, node, source, symbols, pending);
            true
        }
        "object_creation_expression" | "implicit_object_creation_expression" => {
            unit.operator();
            let type_name = node
                .child_by_field_name("type")
                .map(|kind| text(kind, source))
                .unwrap_or("<inferred>");
            unit.operand(format!("constructor:{type_name}"));
            pending.extend(children(node).into_iter().filter(|child| {
                matches!(child.kind(), "argument_list" | "initializer_expression")
            }));
            true
        }
        "cast_expression" => {
            unit.operator();
            if let Some(value) = node.child_by_field_name("value") {
                pending.push(value);
            }
            true
        }
        "is_expression" | "as_expression" => {
            unit.operator();
            if let Some(value) = node.child_by_field_name("left") {
                pending.push(value);
            }
            true
        }
        "typeof_expression" | "sizeof_expression" => {
            unit.operator();
            true
        }
        _ => false,
    }
}

fn visit<'t, 's>(
    unit: &mut Unit<'s>,
    node: Node<'t>,
    source: &'s str,
    symbols: &Symbols<'s>,
    pending: &mut Vec<Node<'t>>,
) {
    let kind = node.kind();
    if count_line(node) && !uninitialized_out_declaration(node, source, &unit.out_locals) {
        unit.facts.counts.logical_statements += 1;
    }
    if kind == "comment" || is_type(kind) {
        return;
    }
    if matches!(
        kind,
        "string_literal" | "verbatim_string_literal" | "raw_string_literal"
    ) && text(node, source).ends_with("u8")
    {
        // Released Microsoft metrics do not classify UTF-8 string operations
        // as literal operands. Concatenation operators are still visited.
        return;
    }
    if kind.starts_with("preproc_") {
        if super::is_conditional_directive(node) {
            pending.extend(
                children(node)
                    .into_iter()
                    .filter(|child| Some(*child) != node.child_by_field_name("condition")),
            );
        }
        return;
    }
    let constant = constant_value(node, source, symbols).filter(|_| {
        unit.resolve_constants
            || !matches!(
                kind,
                "invocation_expression" | "interpolated_string_expression"
            )
    });
    if let Some(value) = constant {
        unit.operand(value);
        if kind == "binary_expression" {
            pending.extend(children(node));
        } else if kind == "interpolated_string_expression" {
            pending.extend(
                children(node)
                    .into_iter()
                    .filter(|child| child.kind() == "interpolation"),
            );
        } else if kind == "invocation_expression" {
            if let Some(argument) = node
                .child_by_field_name("arguments")
                .and_then(|arguments| arguments.named_child(0))
                .and_then(|argument| argument.named_child(0))
            {
                let name = method_name(argument, source);
                // Nameof itself is a constant, but Roslyn still exposes bound
                // field/parameter references beneath it. Type and method-group
                // names have no runtime operand operation.
                if !looks_static(argument, source, unit, symbols)
                    && !symbols.value_methods.contains(name)
                    && !symbols.void_methods.contains(name)
                {
                    pending.push(argument);
                }
            }
        }
        return;
    }
    if visit_declaration(unit, node, source, pending) {
        return;
    }
    if visit_reference(unit, node, source, symbols, pending) {
        return;
    }
    match kind {
        "type_pattern" => {
            return;
        }
        "constant_pattern" => {
            if let Some(value) = node.named_child(0) {
                if value.kind() == "identifier" && looks_static(value, source, unit, symbols) {
                    return;
                }
            }
        }
        "select_clause" if identity_select(node) => {
            return;
        }
        "from_clause" => {
            if node
                .parent()
                .is_some_and(|parent| parent.named_child(0) != Some(node))
            {
                unit.facts.counts.logical_statements += 1;
            }
            pending.extend(children(node).into_iter().filter(|child| {
                Some(*child) != node.child_by_field_name("name")
                    && Some(*child) != node.child_by_field_name("type")
            }));
            return;
        }
        "constructor_initializer" => {
            unit.operator();
        }
        "catch_declaration" | "interpolation_format_clause" | "interpolation_brace" => {
            return;
        }
        "interpolated_string_expression" => {
            unit.operator();
            pending.extend(
                children(node)
                    .into_iter()
                    .filter(|child| child.kind() == "interpolation"),
            );
            return;
        }
        "argument" | "attribute_argument" => {
            if kind == "attribute_argument" && unit.resolve_attribute_members {
                let mut cursor = node.walk();
                let assignment = node.children(&mut cursor).any(|child| child.kind() == "=");
                if assignment {
                    if let Some(name) = node.child_by_field_name("name") {
                        // Named attribute properties expose an assignment and
                        // member reference; a named constructor argument ':' does not.
                        unit.operator();
                        unit.operator();
                        unit.operand(format!("member:{}", text(name, source)));
                    }
                }
            }
            pending.extend(
                children(node)
                    .into_iter()
                    .filter(|child| Some(*child) != node.child_by_field_name("name")),
            );
            return;
        }
        _ => count_operation(unit, node, source),
    }
    pending.extend(children(node));
}

fn measure_unit(node: Node<'_>, kind: UnitKind, source: &str, symbols: &Symbols<'_>) -> UnitFacts {
    let mut unit = Unit::new(node, kind, source, symbols);
    let mut pending = Vec::new();
    if kind == UnitKind::Callable {
        if let Some(body) = body(node) {
            if body.kind() != "block" {
                unit.facts.counts.logical_statements += 1;
            }
            pending.push(body);
        }
        let expression_property =
            matches!(node.kind(), "property_declaration" | "indexer_declaration");
        pending.extend(children(node).into_iter().filter(|child| {
            if child.kind() == "attribute_list" {
                return !expression_property;
            }
            matches!(child.kind(), "parameter_list" | "constructor_initializer")
        }));
    } else {
        unit.facts.counts.logical_statements = 2;
        pending.extend(
            children(node)
                .into_iter()
                .filter(|child| Some(*child) != node.child_by_field_name("name")),
        );
    }
    while let Some(child) = pending.pop() {
        visit(&mut unit, child, source, symbols, &mut pending);
    }
    unit.finish()
}

fn primary_constructor(node: Node<'_>, source: &str, symbols: &Symbols<'_>) -> Option<UnitFacts> {
    if !matches!(
        node.kind(),
        "class_declaration" | "struct_declaration" | "record_declaration"
    ) {
        return None;
    }
    let parameters = children(node)
        .into_iter()
        .find(|child| child.kind() == "parameter_list")?;
    let mut unit = Unit::new(parameters, UnitKind::Callable, source, symbols);
    unit.facts.start_byte = node.start_byte();
    unit.facts.end_byte = node.end_byte();
    unit.facts.start_line = node.start_position().row as u32 + 1;
    unit.facts.end_line = node.end_position().row as u32 + 1;
    unit.facts.name = node
        .child_by_field_name("name")
        .map(|name| text(name, source).to_owned())
        .unwrap_or_default();
    // Without an explicit base invocation, Microsoft reaches defaults from
    // both the declaration operation and parameter symbols. An explicit base
    // invocation replaces the declaration operation, leaving one default visit.
    let mut pending = vec![parameters];
    let attributes: Vec<_> = children(node)
        .into_iter()
        .filter(|child| child.kind() == "attribute_list")
        .filter(|list| {
            children(*list).into_iter().any(|child| {
                child.kind() == "attribute_target_specifier"
                    && child
                        .child(0)
                        .is_some_and(|target| target.kind() == "method")
            })
        })
        .collect();
    pending.extend(attributes.iter().copied());
    let mut explicit_base = false;
    let mut base_nodes: Vec<_> = children(node)
        .into_iter()
        .filter(|child| child.kind() == "base_list")
        .collect();
    while let Some(base_node) = base_nodes.pop() {
        if base_node.kind() == "argument_list" {
            explicit_base = true;
            unit.operator();
            pending.push(base_node);
            continue;
        }
        base_nodes.extend(children(base_node));
    }
    if !explicit_base {
        pending.push(parameters);
        pending.extend(attributes);
    }
    while let Some(child) = pending.pop() {
        visit(&mut unit, child, source, symbols, &mut pending);
    }
    Some(unit.finish())
}

pub(super) fn extract(root: Node<'_>, source: &str) -> MaintainabilityFacts {
    let symbols = symbols(root, source);
    let mut units = Vec::new();
    let mut pending = vec![root];
    let mut top_level: Option<Unit<'_>> = None;
    while let Some(node) = pending.pop() {
        if let Some(constructor) = primary_constructor(node, source, &symbols) {
            units.push(constructor);
        }
        if super::is_callable(node) {
            if body(node).is_some() {
                units.push(measure_unit(node, UnitKind::Callable, source, &symbols));
            }
            continue;
        }
        if matches!(
            node.kind(),
            "variable_declarator" | "enum_member_declaration"
        ) {
            if children(node)
                .iter()
                .any(|child| Some(*child) != node.child_by_field_name("name"))
            {
                units.push(measure_unit(node, UnitKind::Initializer, source, &symbols));
            }
            continue;
        }
        if node.kind() == "global_statement" {
            let unit = top_level
                .get_or_insert_with(|| Unit::new(node, UnitKind::TopLevel, source, &symbols));
            unit.facts.end_byte = node.end_byte();
            unit.facts.end_line = node.end_position().row as u32 + 1;
            unit.facts.counts.logical_statements += 1;
            let mut statements = children(node);
            while let Some(child) = statements.pop() {
                visit(unit, child, source, &symbols, &mut statements);
            }
            continue;
        }
        if node.kind() == "property_declaration" {
            if let Some(value) = node
                .child_by_field_name("value")
                .filter(|value| value.kind() != "arrow_expression_clause")
            {
                let mut unit = Unit::new(node, UnitKind::Initializer, source, &symbols);
                unit.facts.counts.logical_statements = 2;
                let mut statements = vec![value];
                while let Some(child) = statements.pop() {
                    visit(&mut unit, child, source, &symbols, &mut statements);
                }
                units.push(unit.finish());
            }
        }
        let primary_base = node.kind() == "primary_constructor_base_type"
            || node.kind() == "argument_list"
                && node
                    .parent()
                    .is_some_and(|parent| parent.kind() == "base_list");
        if primary_base {
            continue;
        }
        pending.extend(children(node).into_iter().rev());
    }
    if let Some(top_level) = top_level {
        let top_level = top_level.finish();
        let position = units.partition_point(|unit| unit.start_byte < top_level.start_byte);
        units.insert(position, top_level);
    }
    MaintainabilityFacts {
        units,
        profile: None,
    }
}
