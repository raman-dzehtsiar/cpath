// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

//! One native syntax traversal extracts independently versioned MI and CC facts.

use super::{SourceAnalysis, SourceParser};
use crate::maintainability::{SyntaxCounts, UnitFacts, UnitKind};
use crate::metrics::native::{
    CognitiveEvent, CognitiveFacts, CognitiveUnit, Language, LogicalOperator, MaintainabilityFacts,
    MaintainabilityUnit,
};
use crate::metrics::{CognitiveInput, MaintainabilityInput};
use crate::model::FileFacts;
use std::collections::HashSet;
use std::path::PathBuf;
use tree_sitter::{Node, Parser, Tree};

pub struct NativeParser {
    parser: Parser,     // Reused worker-local parser; no tree survives a scan call.
    language: Language, // Explicit profile, independent of filename guessing.
}

fn language_label(language: Language) -> &'static str {
    match language {
        Language::C => "C",
        Language::Cpp => "C++",
        Language::Rust => "Rust",
    }
}

impl NativeParser {
    pub fn c() -> Result<Self, String> {
        Self::new(Language::C, tree_sitter_c::LANGUAGE.into())
    }
    pub fn cpp() -> Result<Self, String> {
        Self::new(Language::Cpp, tree_sitter_cpp::LANGUAGE.into())
    }
    pub fn rust() -> Result<Self, String> {
        Self::new(Language::Rust, tree_sitter_rust::LANGUAGE.into())
    }

    fn new(language: Language, grammar: tree_sitter::Language) -> Result<Self, String> {
        let mut parser = Parser::new();
        parser.set_language(&grammar).map_err(|error| {
            format!(
                "cannot initialize bundled {} grammar: {error}",
                language_label(language)
            )
        })?;
        Ok(Self { parser, language })
    }

    fn parse_tree(&mut self, source: &str) -> Result<Tree, String> {
        if self.language != Language::Rust && decimal_initializer_fragment(source) {
            return Err(format!(
                "unsupported {} standalone decimal initializer fragment; a containing declaration is required",
                language_label(self.language)
            ));
        }
        let tree = self.parser.parse(source, None).ok_or_else(|| {
            format!(
                "{} parser did not produce a syntax tree",
                language_label(self.language)
            )
        })?;
        if self.language != Language::Cpp || !tree.root_node().has_error() {
            return Ok(tree);
        }

        let original_error = super::c_family::syntax_error(tree.root_node(), "C++");
        drop(tree); // Keep at most one full syntax tree alive during the bounded retry.
        self.parser
            .set_language(&tree_sitter_cpp_compat::LANGUAGE.into())
            .map_err(|error| format!("cannot initialize compatibility C++ grammar: {error}"))?;
        let compatible = self.parser.parse(source, None);
        self.parser
            .set_language(&tree_sitter_cpp::LANGUAGE.into())
            .map_err(|error| format!("cannot restore primary C++ grammar: {error}"))?;
        compatible
            .filter(|tree| !tree.root_node().has_error())
            .ok_or(original_error)
    }
}

fn initializer_layout(bytes: &[u8], mut offset: usize) -> Option<usize> {
    loop {
        while bytes.get(offset).is_some_and(u8::is_ascii_whitespace) {
            offset += 1;
        }
        let remaining = &bytes[offset..];
        if remaining.starts_with(b"//") {
            offset += remaining
                .iter()
                .position(|byte| matches!(byte, b'\n' | b'\r'))
                .map_or(remaining.len(), |position| position + 1);
        } else if remaining.starts_with(b"/*") {
            let end = remaining[2..].windows(2).position(|pair| pair == b"*/")?;
            offset += end + 4;
        } else {
            return Some(offset);
        }
    }
}

fn decimal_initializer_fragment(source: &str) -> bool {
    // Include-only decimal tables have no standalone declaration. Recognize
    // only the complete comma-terminated form before expensive error recovery.
    let bytes = source.as_bytes();
    let mut offset = 0;
    let mut literals = 0;
    loop {
        let Some(next) = initializer_layout(bytes, offset) else {
            return false;
        };
        offset = next;
        if offset == bytes.len() {
            return literals >= 2;
        }
        if matches!(bytes[offset], b'+' | b'-') {
            let Some(next) = initializer_layout(bytes, offset + 1) else {
                return false;
            };
            offset = next;
        }
        if !bytes
            .get(offset)
            .is_some_and(|byte| byte.is_ascii_digit() || *byte == b'.')
        {
            return false;
        }
        let start = offset;
        while bytes.get(offset).is_some_and(|byte| {
            byte.is_ascii_digit() || matches!(byte, b'.' | b'e' | b'E' | b'+' | b'-')
        }) {
            offset += 1;
        }
        if source[start..offset].parse::<f64>().is_err() {
            return false;
        }
        let Some(next) = initializer_layout(bytes, offset) else {
            return false;
        };
        if bytes.get(next) != Some(&b',') {
            return false;
        }
        offset = next + 1;
        literals += 1;
    }
}

struct LexicalScope<'source> {
    unit: MaintainabilityUnit, // Final compact counts and scope identity.
    parent: Option<usize>,     // Inclusive parent aggregation after traversal.
    operators: HashSet<&'static str>, // Grammar identities; primitive spellings share a category.
    operands: HashSet<&'source str>, // Borrowed original bytes, discarded after this parse.
    rust_boolean_anchor: RustBooleanAnchor, // RCA state belongs to each complete metric scope.
    standalone_macro_start: Option<usize>, // A verified leading uppercase type token excluded by that extension.
}

#[derive(Default)]
enum RustBooleanAnchor {
    #[default]
    Empty,
    First(LogicalOperator),
    Unary,
}

struct Frame<'tree> {
    node: Node<'tree>,
    parent: Option<Node<'tree>>,
    next_named: Option<Node<'tree>>,
    scope: Option<usize>,
    owner: Option<usize>,
    nesting: u32,
    callable_depth: u32,
    logical_ancestor: Option<LogicalOperator>,
    logical_entry: bool,
    cognitive_enabled: bool,
    if_condition: Option<bool>,
    skip_control: bool,
    type_operand: bool,
    preprocessor_condition: bool,
    declarator_argument: bool,
    file_scope: bool,
    record_context: bool,
}

fn line(node: Node<'_>) -> u32 {
    node.start_position()
        .row
        .saturating_add(1)
        .try_into()
        .unwrap_or(u32::MAX)
}
fn end_line(node: Node<'_>) -> u32 {
    node.end_position()
        .row
        .saturating_add(1)
        .try_into()
        .unwrap_or(u32::MAX)
}
fn text<'source>(node: Node<'_>, source: &'source str) -> &'source str {
    &source[node.byte_range()]
}

fn reference_forward_class<'tree>(node: Node<'tree>, source: &str) -> Option<Node<'tree>> {
    if node.kind() != "declaration" {
        return None;
    }
    let declared_type = node.child_by_field_name("type")?;
    let name = node.child_by_field_name("declarator")?;
    if name.kind() != "identifier" {
        return None;
    }
    let keyword =
        declared_type.kind() == "type_identifier" && text(declared_type, source) == "class";
    let annotated = declared_type.kind() == "class_specifier"
        && declared_type.child_by_field_name("body").is_none()
        && declared_type
            .child_by_field_name("name")
            .is_some_and(|name| text(name, source) == "JS_PUBLIC_API");
    (keyword || annotated).then_some(name)
}

fn reference_class_keyword(node: Node<'_>, parent: Option<Node<'_>>, source: &str) -> bool {
    node.kind() == "type_identifier"
        && text(node, source) == "class"
        && parent.is_some_and(|parent| {
            parent.child_by_field_name("type") == Some(node)
                && reference_forward_class(parent, source).is_some()
        })
}

fn scope_kind(
    language: Language,
    node: Node<'_>,
    parent: Option<Node<'_>>,
    source: &str,
) -> Option<UnitKind> {
    if language != Language::Rust {
        if let Some(name) = parent.and_then(|parent| reference_forward_class(parent, source)) {
            if node == name {
                return Some(UnitKind::Container);
            }
            if node.kind() == "class_specifier" {
                return None;
            }
        }
    }
    match (language, node.kind()) {
        (_, "source_file" | "translation_unit") => Some(UnitKind::TopLevel),
        (Language::Rust, "function_item" | "closure_expression") => Some(UnitKind::Callable),
        (Language::Rust, "impl_item" | "trait_item") => Some(UnitKind::Container),
        (Language::C | Language::Cpp, "function_definition") => Some(UnitKind::Callable),
        (
            Language::C | Language::Cpp,
            "struct_specifier" | "class_specifier" | "namespace_definition",
        ) => Some(UnitKind::Container),
        _ => None,
    }
}

fn callable_name(node: Node<'_>, source: &str) -> String {
    if node.kind() == "lambda_expression" {
        return "<lambda>".into();
    }
    if node.kind() == "function_definition" {
        return super::c_family::callable_name(node, source);
    }
    if let Some(name) = node
        .child_by_field_name("name")
        .or_else(|| node.child_by_field_name("type"))
    {
        return text(name, source).into();
    }
    "<anonymous>".into()
}

fn make_scope<'source>(
    node: Node<'_>,
    kind: UnitKind,
    parent: Option<usize>,
    source: &'source str,
    standalone_macros: bool,
) -> LexicalScope<'source> {
    let leading_type = node
        .child_by_field_name("type")
        .filter(|child| standalone_macros && standalone_type(*child, Some(node), source));
    let first = if leading_type.is_some() {
        node.child_by_field_name("declarator").unwrap_or(node)
    } else {
        node
    };
    // The reference grammar attaches trailing GNU attributes to the enclosing
    // declaration; newer grammars attach them to the completed type node.
    let last = if matches!(node.kind(), "struct_specifier" | "class_specifier") {
        node.child_by_field_name("body").unwrap_or(node)
    } else {
        node
    };
    let source_lines = last
        .end_position()
        .row
        .saturating_sub(first.start_position().row) as u64
        + u64::from(kind != UnitKind::TopLevel);
    LexicalScope {
        unit: MaintainabilityUnit {
            source: UnitFacts {
                name: if kind == UnitKind::TopLevel {
                    "<file>".into()
                } else if kind == UnitKind::Container && node.kind() == "identifier" {
                    text(node, source).into()
                } else {
                    callable_name(node, source)
                },
                kind,
                start_byte: first.start_byte(),
                end_byte: last.end_byte(),
                start_line: line(first),
                end_line: end_line(last),
                counts: SyntaxCounts::default(),
            },
            source_lines,
            cyclomatic: 1,
        },
        parent,
        operators: HashSet::new(),
        operands: HashSet::new(),
        rust_boolean_anchor: RustBooleanAnchor::Empty,
        standalone_macro_start: leading_type.map(|child| child.start_byte()),
    }
}

fn standalone_type(node: Node<'_>, parent: Option<Node<'_>>, source: &str) -> bool {
    if node.kind() != "type_identifier" {
        return false;
    }
    let Some(parent) = parent else {
        return false;
    };
    if parent.kind() != "function_definition"
        || parent.child_by_field_name("type") != Some(node)
        || parent.start_byte() != node.start_byte()
    {
        return false;
    }
    let Some(declarator) = parent.child_by_field_name("declarator") else {
        return false;
    };
    if declarator.start_byte() < node.end_byte() {
        return false;
    }
    let name = text(node, source).as_bytes();
    let gap = &source[node.end_byte()..declarator.start_byte()];
    name.len() > 1
        && name
            .iter()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || *byte == b'_')
        && gap.contains('\n')
        && gap.chars().all(char::is_whitespace)
}

fn annotation(node: Node<'_>, parent: Option<Node<'_>>, source: &str) -> bool {
    // The pinned reference grammar recognizes this public export qualifier.
    if node.kind() == "type_identifier"
        && text(node, source) == "JS_PUBLIC_API"
        && parent.is_some_and(|parent| {
            parent.kind() == "class_specifier" && parent.child_by_field_name("body").is_none()
        })
    {
        return true;
    }
    node.kind() == "type_identifier"
        && parent.is_some_and(|parent| {
            matches!(
                parent.kind(),
                "field_declaration" | "declaration" | "function_definition"
            )
        })
        && matches!(
            text(node, source),
            "MOZ_ALWAYS_INLINE" | "MOZ_IMPLICIT" | "MOZ_MUST_USE" | "MOZ_NEVER_INLINE"
        )
}

fn profile_primitive(
    node: Node<'_>,
    parent: Option<Node<'_>>,
    source: &str,
    type_operand: bool,
) -> bool {
    if node.kind() != "type_identifier" && !(type_operand && node.kind() == "identifier") {
        return false;
    }
    if parent.is_some_and(|parent| {
        matches!(parent.kind(), "qualified_identifier" | "requires_clause")
            || parent.kind() == "variadic_type_parameter_declaration"
            || parent.kind() == "type_parameter_declaration"
                && parent
                    .child(0)
                    .is_some_and(|keyword| keyword.kind() == "class")
            || matches!(
                parent.kind(),
                "alias_declaration"
                    | "class_specifier"
                    | "struct_specifier"
                    | "union_specifier"
                    | "enum_specifier"
                    | "template_type"
                    | "optional_type_parameter_declaration"
            ) && parent.child_by_field_name("name") == Some(node)
    }) {
        return false;
    }
    primitive_name(text(node, source))
}

fn primitive_name(name: &str) -> bool {
    if matches!(
        name,
        "int"
            | "char"
            | "void"
            | "float"
            | "double"
            | "bool"
            | "BOOL"
            | "BOOLEAN"
            | "BYTE"
            | "CHAR"
            | "DWORD"
            | "DWORDLONG"
            | "DWORD_PTR"
            | "DWORD32"
            | "DWORD64"
            | "FLOAT"
            | "HANDLE"
            | "HDC"
            | "HKL"
            | "HLOCAL"
            | "HMODULE"
            | "HRESULT"
            | "HWND"
            | "INT"
            | "INT8"
            | "INT16"
            | "INT32"
            | "INT64"
            | "INT_PTR"
            | "LANGID"
            | "LONG"
            | "LONG32"
            | "LONG64"
            | "LONGLONG"
            | "LONG_PTR"
            | "LPARAM"
            | "LPBYTE"
            | "LPCSTR"
            | "LPCWSTR"
            | "LPSTR"
            | "LPTSTR"
            | "LPWSTR"
            | "LRESULT"
            | "PBYTE"
            | "PCWSTR"
            | "PUCHAR"
            | "PULONG"
            | "PVOID"
            | "SHORT"
            | "SIZE_T"
            | "SSIZE_T"
            | "UCHAR"
            | "UINT"
            | "UINT8"
            | "UINT16"
            | "UINT32"
            | "UINT64"
            | "UINT_PTR"
            | "ULONG"
            | "ULONG32"
            | "ULONG64"
            | "ULONGLONG"
            | "ULONG_PTR"
            | "USHORT"
            | "VOID"
            | "WCHAR"
            | "WORD"
            | "WPARAM"
            | "charptr_t"
            | "intmax_t"
            | "intptr_t"
            | "max_align_t"
            | "ptrdiff_t"
            | "sig_atomic_t"
            | "size_t"
            | "ssize_t"
            | "uintmax_t"
            | "uintptr_t"
            | "wchar_t"
    ) {
        return true;
    }
    let Some(integer) = name.strip_suffix("_t") else {
        return false;
    };
    let integer = integer.strip_prefix('u').unwrap_or(integer);
    let Some(width) = integer
        .strip_prefix("int")
        .or_else(|| integer.strip_prefix("char"))
    else {
        return false;
    };
    let width = width
        .strip_prefix("_fast")
        .or_else(|| width.strip_prefix("_least"))
        .unwrap_or(width);
    matches!(width, "8" | "16" | "32" | "64")
}

fn parenthesized_primitive(node: Node<'_>, source: &str) -> bool {
    if node.kind() != "parenthesized_expression" {
        return false;
    }
    let mut cursor = node.walk();
    let mut children = node
        .named_children(&mut cursor)
        .filter(|child| child.kind() != "comment");
    let Some(child) = children.next() else {
        return false;
    };
    child.kind() == "identifier" && primitive_name(text(child, source)) && children.next().is_none()
}

fn c_reference_keyword_operand(
    node: Node<'_>,
    parent: Option<Node<'_>>,
    preprocessor_condition: bool,
    declarator_argument: bool,
    source: &str,
) -> bool {
    if preprocessor_condition || declarator_argument || node.kind() != "identifier" {
        return false;
    }
    if parent.is_some_and(|parent| {
        parent.child_by_field_name("declarator") == Some(node)
            || matches!(parent.kind(), "parenthesized_declarator" | "preproc_params")
            || matches!(parent.kind(), "preproc_def" | "preproc_function_def")
                && parent.child_by_field_name("name") == Some(node)
    }) {
        return false;
    }
    // RCA uses its C++ grammar for C inputs too. Preserve declarator names,
    // while matching reserved type tokens in accepted shared-header syntax.
    // The native C++ grammar already supplies those roles directly.
    matches!(
        text(node, source),
        "int"
            | "char"
            | "void"
            | "float"
            | "double"
            | "bool"
            | "wchar_t"
            | "char8_t"
            | "char16_t"
            | "char32_t"
    )
}

fn single_identifier_argument(node: Node<'_>) -> bool {
    let mut cursor = node.walk();
    let mut arguments = node
        .named_children(&mut cursor)
        .filter(|child| !is_comment(*child));
    arguments
        .next()
        .is_some_and(|argument| argument.kind() == "identifier")
        && arguments.next().is_none()
}

fn primitive_operand_context(
    node: Node<'_>,
    child: Node<'_>,
    parent: Option<Node<'_>>,
    inherited: bool,
    source: &str,
) -> bool {
    match node.kind() {
        "sizeof_expression" => {
            if child.kind() != "identifier" {
                return true;
            }
            let mut cursor = node.walk();
            let is_pack = node
                .children(&mut cursor)
                .any(|child| child.kind() == "...");
            !is_pack
        }
        "template_argument_list" => true,
        "parenthesized_expression" | "argument_list" => inherited,
        "call_expression" => {
            node.child_by_field_name("function") == Some(child)
                || node.child_by_field_name("arguments") == Some(child)
                    && (function_pointer_declaration(node, parent)
                        || node
                            .child_by_field_name("function")
                            .is_some_and(|callee| parenthesized_primitive(callee, source))
                            && parent.is_some_and(|parent| {
                                parent.kind() == "call_expression"
                                    && parent.child_by_field_name("function") == Some(node)
                            }))
        }
        "binary_expression" => {
            node.child_by_field_name("left") == Some(child)
                && parenthesized_primitive(child, source)
                && node
                    .child_by_field_name("operator")
                    .is_some_and(|operator| matches!(operator.kind(), "*" | "&" | "+" | "-"))
        }
        _ => false,
    }
}

fn function_pointer_declaration(node: Node<'_>, parent: Option<Node<'_>>) -> bool {
    if !parent.is_some_and(|parent| parent.kind() == "expression_statement") {
        return false;
    }
    let Some(function) = node.child_by_field_name("function") else {
        return false;
    };
    if function.kind() != "call_expression"
        || !function
            .child_by_field_name("function")
            .is_some_and(|name| name.kind() == "identifier")
    {
        return false;
    }
    let Some(arguments) = function.child_by_field_name("arguments") else {
        return false;
    };
    let mut cursor = arguments.walk();
    let mut children = arguments
        .named_children(&mut cursor)
        .filter(|child| child.kind() != "comment");
    let Some(pointer) = children.next() else {
        return false;
    };
    // C can parse a typedef-based declaration as TYPE(*name)(parameters).
    // Preserve its parameter type roles without reclassifying ordinary values.
    pointer.kind() == "pointer_expression"
        && pointer
            .child(0)
            .is_some_and(|operator| operator.kind() == "*")
        && children.next().is_none()
}

fn cast_literal_sign<'tree>(node: Node<'tree>, source: &str) -> Option<Node<'tree>> {
    if node.kind() != "binary_expression"
        || !trailing_primitive(node.child_by_field_name("left")?, source)
    {
        return None;
    }
    let operator = node.child_by_field_name("operator")?;
    let value = node.child_by_field_name("right")?;
    (matches!(operator.kind(), "+" | "-")
        && value.kind() == "number_literal"
        && operator.end_byte() == value.start_byte())
    .then_some(operator)
}

fn trailing_primitive(mut node: Node<'_>, source: &str) -> bool {
    loop {
        let field = match node.kind() {
            "binary_expression" => "right",
            "pointer_expression" | "unary_expression" => "argument",
            _ => return parenthesized_primitive(node, source),
        };
        let Some(child) = node.child_by_field_name(field) else {
            return false;
        };
        node = child;
    }
}

fn rust_operator(node: Node<'_>, parent: Option<Node<'_>>) -> bool {
    let kind = node.kind();
    if matches!(kind, "/" | "||") {
        return parent.is_some_and(|parent| parent.kind() == "binary_expression");
    }
    if kind == "!" {
        return !parent.is_some_and(|parent| parent.kind() == "inner_doc_comment_marker");
    }
    matches!(
        kind,
        "!=" | "%"
            | "%="
            | "&"
            | "&&"
            | "&="
            | "("
            | "*"
            | "*="
            | "+"
            | "+="
            | ","
            | "-"
            | "-="
            | "->"
            | "."
            | ".."
            | "..="
            | "/="
            | ";"
            | "<"
            | "<<"
            | "<<="
            | "<="
            | "="
            | "=="
            | "=>"
            | ">"
            | ">="
            | ">>"
            | ">>="
            | "?"
            | "["
            | "^"
            | "^="
            | "async"
            | "await"
            | "continue"
            | "fn"
            | "for"
            | "if"
            | "let"
            | "loop"
            | "match"
            | "move"
            | "mutable_specifier"
            | "primitive_type"
            | "return"
            | "unsafe"
            | "while"
            | "{"
            | "|"
            | "|="
    )
}

fn c_operator(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "!" | "!="
            | "%"
            | "%="
            | "&"
            | "&&"
            | "&="
            | "("
            | "*"
            | "*="
            | "+"
            | "++"
            | "+="
            | ","
            | "-"
            | "--"
            | "->"
            | "."
            | "/"
            | "/="
            | ":"
            | "::"
            | ";"
            | "<"
            | "<<"
            | "<<="
            | "<="
            | "="
            | "=="
            | ">"
            | ">="
            | ">>"
            | ">>="
            | "?"
            | "["
            | "^"
            | "^="
            | "break"
            | "case"
            | "catch"
            | "continue"
            | "default"
            | "delete"
            | "do"
            | "else"
            | "for"
            | "goto"
            | "if"
            | "new"
            | "primitive_type"
            | "return"
            | "sizeof"
            | "switch"
            | "throw"
            | "try"
            | "__try"
            | "type_specifier"
            | "while"
            | "{"
            | "|"
            | "|="
            | "||"
            | "~"
    )
}

fn operand(
    language: Language,
    node: Node<'_>,
    parent: Option<Node<'_>>,
    source: &str,
    type_operand: bool,
) -> bool {
    if language != Language::Rust
        && (profile_primitive(node, parent, source, type_operand)
            || reference_class_keyword(node, parent, source)
            || annotation(node, parent, source))
    {
        return false;
    }
    match language {
        Language::Rust => matches!(
            node.kind(),
            "identifier"
                | "string_literal"
                | "raw_string_literal"
                | "integer_literal"
                | "float_literal"
                | "boolean_literal"
                | "self"
                | "char_literal"
                | "_"
        ),
        _ => {
            matches!(
                node.kind(),
                "identifier"
                    | "type_identifier"
                    | "field_identifier"
                    | "raw_string_literal"
                    | "string_literal"
                    | "number_literal"
                    | "true"
                    | "false"
                    | "null"
                    | "..."
            ) || node.kind() == "namespace_identifier"
                && parent.is_some_and(|parent| parent.kind() == "namespace_definition")
        }
    }
}

fn decision(language: Language, node: Node<'_>) -> bool {
    if language == Language::Rust {
        return matches!(
            node.kind(),
            "if" | "for" | "while" | "loop" | "match_arm" | "try_expression" | "&&" | "||"
        );
    }
    matches!(
        node.kind(),
        "if" | "for" | "while" | "case" | "catch" | "conditional_expression" | "&&" | "||"
    )
}

fn add_lexical(
    scope: &mut LexicalScope<'_>,
    language: Language,
    node: Node<'_>,
    parent: Option<Node<'_>>,
    source: &str,
    type_operand: bool,
) {
    if language != Language::Rust
        && (annotation(node, parent, source)
            || scope.standalone_macro_start == Some(node.start_byte())
            || parent.and_then(|parent| cast_literal_sign(parent, source)) == Some(node))
    {
        return;
    }
    let operator = if language == Language::Rust {
        rust_operator(node, parent).then_some(node.kind())
    } else if profile_primitive(node, parent, source, type_operand) {
        Some("primitive_type")
    } else {
        c_operator(node).then_some(node.kind())
    };
    if let Some(operator) = operator {
        scope.operators.insert(operator);
        scope.unit.source.counts.operators += 1;
    }
    if decision(language, node) {
        scope.unit.cyclomatic += 1;
    }
}

fn logical_operator(node: Node<'_>) -> Option<&'static str> {
    if node.kind() != "binary_expression" {
        return None;
    }
    node.child_by_field_name("operator")
        .map(|operator| operator.kind())
        .filter(|operator| matches!(*operator, "&&" | "||" | "and" | "or"))
}

fn rust_boolean_increment(scope: &mut LexicalScope<'_>, node: Node<'_>) -> u32 {
    if node.kind() == "unary_expression" {
        scope.rust_boolean_anchor = RustBooleanAnchor::Unary;
    }
    let Some(operator) = logical_kind(node) else {
        return 0;
    };
    match scope.rust_boolean_anchor {
        RustBooleanAnchor::Empty => {
            scope.rust_boolean_anchor = RustBooleanAnchor::First(operator);
            1
        }
        RustBooleanAnchor::First(anchor) => u32::from(anchor != operator),
        RustBooleanAnchor::Unary => 1,
    }
}

fn logical_kind(node: Node<'_>) -> Option<LogicalOperator> {
    match logical_operator(node)? {
        "&&" | "and" => Some(LogicalOperator::And),
        "||" | "or" => Some(LogicalOperator::Or),
        _ => None,
    }
}

fn is_else_if(node: Node<'_>, parent: Option<Node<'_>>) -> bool {
    parent.is_some_and(|parent| {
        parent.kind() == "else_clause"
            || parent.kind() == "if_statement"
                && parent.child_by_field_name("alternative") == Some(node)
    })
}

fn is_direct_if_body(node: Node<'_>, child: Node<'_>) -> bool {
    match node.kind() {
        "if_statement" => {
            node.child_by_field_name("consequence") == Some(child)
                || node.child_by_field_name("alternative") == Some(child)
        }
        "else_clause" => child.is_named() && child.kind() != "comment",
        _ => false,
    }
}

fn is_comment(node: Node<'_>) -> bool {
    matches!(node.kind(), "comment" | "line_comment" | "block_comment")
}

fn is_condition_declaration(node: Node<'_>) -> bool {
    if node.kind() != "declaration" {
        return false;
    }
    let mut declarator = node.child_by_field_name("declarator");
    while let Some(current) = declarator {
        // Relational/logical conditions can resemble a reference to a
        // template-id. A condition variable cannot have a template-id name;
        // retain those expression events instead of skipping an initializer.
        if current.kind() == "template_function" {
            return false;
        }
        declarator = current.child_by_field_name("declarator").or_else(|| {
            matches!(
                current.kind(),
                "reference_declarator" | "parenthesized_declarator"
            )
            .then(|| current.named_child(0))
            .flatten()
        });
    }
    true
}

fn else_body_is_if(node: Node<'_>) -> bool {
    let mut cursor = node.walk();
    let is_if = node
        .named_children(&mut cursor)
        .find(|child| !is_comment(*child))
        .is_some_and(|child| matches!(child.kind(), "if_statement" | "if_expression"));
    is_if
}

fn control(
    language: Language,
    node: Node<'_>,
    parent: Option<Node<'_>>,
    next_named: Option<Node<'_>>,
) -> Option<(bool, bool)> {
    let kind = node.kind();
    if matches!(kind, "if_statement" | "if_expression") {
        return Some((!is_else_if(node, parent), true));
    }
    if kind == "else" {
        if next_named.is_some_and(|next| matches!(next.kind(), "if_statement" | "if_expression")) {
            return None;
        }
        return Some((false, false));
    }
    if language == Language::Rust {
        if matches!(
            kind,
            "for_expression" | "while_expression" | "match_expression"
        ) {
            return Some((true, true));
        }
        if matches!(kind, "break_expression" | "continue_expression")
            && node
                .named_child(0)
                .is_some_and(|child| child.kind() == "label")
        {
            return Some((false, false));
        }
        return None;
    }
    // Clang visits GNU's omitted-middle conditional as BinaryConditionalOperator,
    // which contributes neither a decision nor a nesting level.
    if kind == "conditional_expression" && node.child_by_field_name("consequence").is_none() {
        return None;
    }
    if matches!(
        kind,
        "for_statement"
            | "for_range_loop"
            | "while_statement"
            | "do_statement"
            | "switch_statement"
            | "catch_clause"
            | "conditional_expression"
    ) {
        return Some((true, true));
    }
    if kind == "goto_statement" {
        return Some((false, false));
    }
    None
}

fn record_event(facts: &mut CognitiveFacts, owner: Option<usize>, event: CognitiveEvent) {
    match owner {
        Some(owner) => facts.units[owner].events.push(event),
        None => facts.global_events.push(event),
    }
}

fn finalize_scopes(mut scopes: Vec<LexicalScope<'_>>, language: Language) -> MaintainabilityFacts {
    let mut units = Vec::with_capacity(scopes.len());
    while let Some(mut scope) = scopes.pop() {
        scope.unit.source.counts.distinct_operators = scope.operators.len() as u64;
        scope.unit.source.counts.distinct_operands = scope.operands.len() as u64;
        scope.unit.source.counts.decisions = scope.unit.cyclomatic.saturating_sub(1);
        if let Some(parent) = scope.parent {
            let parent = &mut scopes[parent];
            parent.unit.cyclomatic += scope.unit.cyclomatic;
            parent.unit.source.counts.operators += scope.unit.source.counts.operators;
            parent.unit.source.counts.operands += scope.unit.source.counts.operands;
            if parent.operators.len() < scope.operators.len() {
                std::mem::swap(&mut parent.operators, &mut scope.operators);
            }
            if parent.operands.len() < scope.operands.len() {
                std::mem::swap(&mut parent.operands, &mut scope.operands);
            }
            parent.operators.extend(scope.operators.drain());
            parent.operands.extend(scope.operands.drain());
        }
        units.push(scope.unit);
    }
    units.reverse();
    MaintainabilityFacts { language, units }
}

impl SourceParser for NativeParser {
    fn parse(&mut self, path: PathBuf, source: &str) -> Result<FileFacts, String> {
        self.parse_analysis(path, source)
            .map(|analysis| analysis.file)
    }

    fn parse_analysis(&mut self, path: PathBuf, source: &str) -> Result<SourceAnalysis, String> {
        let tree = self.parse_tree(source)?;
        let root = tree.root_node();
        if root.has_error() {
            return Err(super::c_family::syntax_error(
                root,
                language_label(self.language),
            ));
        }
        let mut scopes: Vec<LexicalScope<'_>> = Vec::new();
        let mut cognitive = CognitiveFacts {
            language: self.language,
            source_length: source.len(),
            source_lines: source.lines().count().try_into().unwrap_or(u32::MAX),
            units: vec![],
            global_events: vec![],
        };
        let mut stack = vec![Frame {
            node: root,
            parent: None,
            next_named: None,
            scope: None,
            owner: None,
            nesting: 0,
            callable_depth: 0,
            logical_ancestor: None,
            logical_entry: true,
            cognitive_enabled: true,
            if_condition: None,
            skip_control: false,
            type_operand: false,
            preprocessor_condition: false,
            declarator_argument: false,
            file_scope: false,
            record_context: false,
        }];
        let mut children = Vec::new();
        while let Some(Frame {
            node,
            parent,
            next_named,
            mut scope,
            mut owner,
            mut nesting,
            mut callable_depth,
            mut logical_ancestor,
            mut logical_entry,
            mut cognitive_enabled,
            if_condition,
            skip_control,
            mut type_operand,
            preprocessor_condition,
            declarator_argument,
            file_scope,
            record_context,
        }) = stack.pop()
        {
            let kind = node.kind();
            if self.language != Language::Rust
                && kind == "attribute"
                && node.child_by_field_name("prefix").is_none()
                && node
                    .child_by_field_name("name")
                    .is_some_and(|name| &source[name.byte_range()] == "assume")
            {
                // The pinned Clang18 profile ignores standard assume payloads.
                // Keep their original syntax available to the separate MI pass.
                cognitive_enabled = false;
            }
            type_operand |= self.language != Language::Rust
                && parenthesized_primitive(node, source)
                || self.language == Language::C
                    && c_reference_keyword_operand(
                        node,
                        parent,
                        preprocessor_condition,
                        declarator_argument,
                        source,
                    );
            if self.language != Language::Rust {
                if kind == "compound_statement" && file_scope {
                    return Err(format!(
                        "unsupported or ambiguous {} file-scope block at line {}",
                        language_label(self.language),
                        line(node)
                    ));
                }
                if kind == "function_definition" && !super::c_family::has_function_declarator(node)
                {
                    return Err(format!(
                        "unsupported or ambiguous {} callable declaration at line {}",
                        language_label(self.language),
                        line(node)
                    ));
                }
            }
            if let Some(kind) = scope_kind(self.language, node, parent, source) {
                let next = scopes.len();
                let standalone_macros =
                    parent.is_some_and(|parent| parent.kind() == "translation_unit");
                scopes.push(make_scope(node, kind, scope, source, standalone_macros));
                scope = Some(next);
            }
            let callable = matches!(
                kind,
                "function_definition"
                    | "function_item"
                    | "closure_expression"
                    | "lambda_expression"
            );
            if callable {
                let parent = if cognitive_enabled { owner } else { None };
                let parent_logical = logical_ancestor;
                let inherits_parent_logical = logical_entry && cognitive_enabled;
                let parent_nesting = if self.language == Language::Rust {
                    if kind == "closure_expression" {
                        nesting.saturating_add(1)
                    } else {
                        // Named functions reset control nesting; closures retain it.
                        callable_depth
                    }
                } else if parent.is_some() {
                    nesting.saturating_add(u32::from(kind == "lambda_expression" && !skip_control))
                } else {
                    0
                };
                nesting = if self.language == Language::Rust {
                    parent_nesting
                } else {
                    0
                };
                callable_depth = callable_depth.saturating_add(1);
                owner = Some(cognitive.units.len());
                cognitive.units.push(CognitiveUnit {
                    name: callable_name(node, source),
                    start_byte: node.start_byte(),
                    end_byte: node.end_byte(),
                    start_line: line(node),
                    end_line: end_line(node),
                    events: vec![],
                    parent,
                    parent_nesting,
                    parent_logical,
                    inherits_parent_logical,
                    logical_roots: [0; 2],
                });
                logical_ancestor = None;
                logical_entry = true;
                cognitive_enabled = true;
            }
            if let Some(scope) = scope {
                add_lexical(
                    &mut scopes[scope],
                    self.language,
                    node,
                    parent,
                    source,
                    type_operand,
                );
                if scopes[scope].standalone_macro_start != Some(node.start_byte())
                    && operand(self.language, node, parent, source, type_operand)
                {
                    let start =
                        if self.language != Language::Rust && node.kind() == "number_literal" {
                            parent
                                .and_then(|parent| cast_literal_sign(parent, source))
                                .map_or(node.start_byte(), |sign| sign.start_byte())
                        } else {
                            node.start_byte()
                        };
                    scopes[scope]
                        .operands
                        .insert(&source[start..node.end_byte()]);
                    scopes[scope].unit.source.counts.operands += 1;
                }
            }
            let mut child_nesting = nesting;
            if self.language != Language::Rust
                && owner.is_none()
                && kind == "conditional_expression"
                && node.child_by_field_name("consequence").is_none()
            {
                // Non-callable control flow still has no certified Clang score.
                cognitive.global_events.push(CognitiveEvent {
                    nesting,
                    structural: true,
                });
            }
            if let Some((structural, nests)) = control(self.language, node, parent, next_named)
                .filter(|_| cognitive_enabled && !skip_control)
            {
                if self.language == Language::Rust && structural {
                    if let Some(scope) = scope {
                        scopes[scope].rust_boolean_anchor = RustBooleanAnchor::Empty;
                    }
                }
                record_event(
                    &mut cognitive,
                    owner,
                    CognitiveEvent {
                        nesting,
                        structural,
                    },
                );
                child_nesting += u32::from(nests);
            }
            if cognitive_enabled {
                let increments = if self.language == Language::Rust {
                    scope.map_or(0, |scope| rust_boolean_increment(&mut scopes[scope], node))
                } else if let Some(operator) = logical_kind(node) {
                    if logical_ancestor.is_none() && logical_entry {
                        if let Some(owner) = owner {
                            cognitive.units[owner].logical_roots[operator.index()] += 1;
                        }
                    }
                    u32::from(logical_ancestor != Some(operator))
                } else {
                    0
                };
                for _ in 0..increments {
                    record_event(
                        &mut cognitive,
                        owner,
                        CognitiveEvent {
                            nesting,
                            structural: false,
                        },
                    );
                }
            }
            if kind == "else_clause" && !else_body_is_if(node) {
                child_nesting = nesting.saturating_add(1);
            }
            let record = matches!(kind, "class_specifier" | "struct_specifier");
            let statement_expression = kind == "statement_expression"
                || kind == "parenthesized_expression"
                    && node
                        .named_children(&mut node.walk())
                        .any(|child| child.kind() == "compound_statement");
            if self.language != Language::Rust
                && owner.is_some()
                && (record && !record_context || statement_expression && !skip_control)
            {
                child_nesting = child_nesting.saturating_add(1);
            }
            let child_record_context = !callable && (record || record_context);
            let child_logical_ancestor = if kind == "call_expression" {
                None
            } else {
                logical_kind(node).or(logical_ancestor)
            };
            let child_logical_entry = logical_entry && kind != "call_expression";
            let child_file_scope = kind == "translation_unit"
                || file_scope
                    && matches!(
                        kind,
                        "preproc_if"
                            | "preproc_ifdef"
                            | "preproc_else"
                            | "preproc_elif"
                            | "preproc_elifdef"
                    );
            let mut next_named = None;
            let mut cursor = node.walk();
            children.clear();
            children.extend(node.children(&mut cursor));
            let preserve_argument =
                declarator_argument && kind == "argument_list" && single_identifier_argument(node);
            for child in children.iter().copied().rev() {
                let child_scope = if matches!(kind, "struct_specifier" | "class_specifier")
                    && node
                        .child_by_field_name("body")
                        .is_some_and(|body| child.start_byte() >= body.end_byte())
                {
                    scope.and_then(|index| scopes[index].parent)
                } else {
                    scope
                };
                let alternative = matches!(kind, "if_statement" | "if_expression")
                    && (child.kind() == "else_clause"
                        || node.child_by_field_name("alternative") == Some(child));
                let child_if_condition = (self.language != Language::Rust
                    && kind == "if_statement"
                    && node.child_by_field_name("condition") == Some(child))
                .then(|| is_else_if(node, parent));
                let condition_value = if kind == "condition_clause" {
                    node.child_by_field_name("value") == Some(child)
                } else {
                    kind == "parenthesized_expression" && child.is_named()
                };
                let omitted_declaration =
                    if_condition.is_some() && condition_value && is_condition_declaration(child);
                // Clang18 traverses direct if/else bodies through its base
                // visitor. Their root control is bypassed, except nested ifs
                // handled by a separate visitor. Expression statements have
                // no corresponding Clang AST wrapper.
                let direct_body = self.language != Language::Rust
                    && (is_direct_if_body(node, child)
                        || skip_control && kind == "expression_statement")
                    && child.kind() != "if_statement";
                let child_preprocessor_condition = if matches!(
                    kind,
                    "preproc_if" | "preproc_elif" | "preproc_ifdef" | "preproc_elifdef"
                ) {
                    node.child_by_field_name("condition") == Some(child)
                        || node.child_by_field_name("name") == Some(child)
                } else {
                    preprocessor_condition
                };
                stack.push(Frame {
                    node: child,
                    parent: Some(node),
                    next_named,
                    scope: child_scope,
                    owner,
                    nesting: if alternative || child_if_condition == Some(false) {
                        nesting
                    } else {
                        child_nesting
                    },
                    callable_depth,
                    logical_ancestor: child_logical_ancestor,
                    logical_entry: child_logical_entry,
                    cognitive_enabled: cognitive_enabled && !omitted_declaration,
                    if_condition: child_if_condition,
                    skip_control: direct_body || if_condition == Some(true) && condition_value,
                    type_operand: self.language != Language::Rust
                        && primitive_operand_context(node, child, parent, type_operand, source),
                    preprocessor_condition: child_preprocessor_condition,
                    declarator_argument: self.language == Language::C
                        && (preserve_argument
                            || kind == "call_expression"
                                && node.child_by_field_name("arguments") == Some(child)
                                && parent
                                    .is_some_and(|parent| parent.kind() == "expression_statement")),
                    file_scope: child_file_scope,
                    record_context: child_record_context,
                });
                if child.is_named() && !is_comment(child) {
                    next_named = Some(child);
                }
            }
        }
        let file = FileFacts {
            path,
            bytes: source.len() as u64,
            lines: source.lines().count().try_into().unwrap_or(u32::MAX),
            profiled_cognitive: Some(CognitiveInput::Native(cognitive)),
            ..Default::default()
        };
        let maintainability = crate::maintainability::MaintainabilityFacts {
            units: vec![],
            profile: Some(MaintainabilityInput::Native(finalize_scopes(
                scopes,
                self.language,
            ))),
        };
        Ok(SourceAnalysis {
            file,
            maintainability: Some(maintainability),
        })
    }
}
