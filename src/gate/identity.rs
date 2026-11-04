// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

//! Gate-only declaration metadata. This pass never extracts metric inputs.

use crate::model::SourceLanguage;
use tree_sitter::{Language, Node, Parser, Tree};

pub(super) struct Identities {
    tree: Tree, // Independent syntax tree used solely to locate declarations.
}

fn grammars(language: SourceLanguage) -> Vec<Language> {
    match language {
        SourceLanguage::C => vec![tree_sitter_c::LANGUAGE.into()],
        SourceLanguage::Cpp => vec![
            tree_sitter_cpp::LANGUAGE.into(),
            tree_sitter_cpp_compat::LANGUAGE.into(),
        ],
        SourceLanguage::CHeader => vec![
            tree_sitter_c::LANGUAGE.into(),
            tree_sitter_cpp::LANGUAGE.into(),
            tree_sitter_cpp_compat::LANGUAGE.into(),
        ],
        SourceLanguage::CSharp => vec![tree_sitter_c_sharp::LANGUAGE.into()],
        SourceLanguage::Rust => vec![tree_sitter_rust::LANGUAGE.into()],
        SourceLanguage::Java => vec![tree_sitter_java::LANGUAGE.into()],
        SourceLanguage::Python => vec![tree_sitter_python::LANGUAGE.into()],
        SourceLanguage::JavaScript => vec![tree_sitter_javascript::LANGUAGE.into()],
        SourceLanguage::TypeScript => vec![
            tree_sitter_typescript_cpath::LANGUAGE_TYPESCRIPT.into(),
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        ],
        SourceLanguage::Tsx => vec![
            tree_sitter_typescript_cpath::LANGUAGE_TSX.into(),
            tree_sitter_typescript::LANGUAGE_TSX.into(),
        ],
        SourceLanguage::Go => vec![tree_sitter_go::LANGUAGE.into()],
    }
}

impl Identities {
    pub fn parse(
        parser: &mut Parser,
        language: SourceLanguage,
        source: &str,
    ) -> Result<Self, String> {
        for grammar in grammars(language) {
            parser
                .set_language(&grammar)
                .map_err(|error| error.to_string())?;
            if let Some(tree) = parser.parse(source, None) {
                if !tree.root_node().has_error() {
                    return Ok(Self { tree });
                }
            }
        }
        Err(
            "gate identity metadata could not be parsed; metric calculations were not changed"
                .into(),
        )
    }

    pub fn key(&self, source: &str, start: usize, end: usize, name: &str) -> Option<String> {
        if matches!(name, "<file>" | "<module>" | "<top-level>") {
            return Some(name.to_owned());
        }
        if end > source.len() {
            return None;
        }
        let node = self.exact_node(start, end)?;
        let root = self.tree.root_node();
        let mut chain = vec![header(node, source)?];
        let mut ancestor = node.parent();
        while let Some(parent) = ancestor {
            if is_declaration(parent.kind()) || is_binding(parent.kind()) {
                chain.push(header(parent, source)?);
            }
            ancestor = parent.parent();
        }
        chain.reverse();
        let mut cursor = root.walk();
        for declaration in root.named_children(&mut cursor) {
            if matches!(
                declaration.kind(),
                "file_scoped_namespace_declaration" | "package_declaration" | "package_clause"
            ) {
                // These declarations can be siblings rather than AST ancestors.
                chain.insert(0, header(declaration, source)?);
            }
        }
        serde_json::to_string(&chain).ok()
    }

    pub fn is_callable(&self, start: usize, end: usize) -> Option<bool> {
        let node = self.exact_node(start, end)?;
        Some(matches!(
            node.kind(),
            "function_definition"
                | "function_declaration"
                | "function_item"
                | "function_expression"
                | "generator_function"
                | "generator_function_declaration"
                | "arrow_function"
                | "method_definition"
                | "method_declaration"
                | "constructor_declaration"
                | "destructor_declaration"
                | "operator_declaration"
                | "conversion_operator_declaration"
                | "local_function_statement"
                | "lambda"
                | "lambda_expression"
                | "anonymous_method_expression"
                | "closure_expression"
                | "func_literal"
                | "property_declaration"
                | "indexer_declaration"
                | "accessor_declaration"
        ))
    }

    fn exact_node(&self, start: usize, end: usize) -> Option<Node<'_>> {
        if start >= end {
            return None;
        }
        let mut node = self
            .tree
            .root_node()
            .descendant_for_byte_range(start, end)?;
        // Exact spans prevent silently joining a score to an enclosing declaration.
        while node.start_byte() != start || node.end_byte() != end {
            node = node.parent()?;
            if node.start_byte() < start || node.end_byte() > end {
                return None;
            }
        }
        Some(node)
    }
}

fn is_binding(kind: &str) -> bool {
    matches!(
        kind,
        "variable_declarator" | "let_declaration" | "assignment" | "assignment_expression" | "pair"
    )
}

fn is_declaration(kind: &str) -> bool {
    matches!(
        kind,
        "function_definition"
            | "function_declaration"
            | "function_item"
            | "function_expression"
            | "generator_function"
            | "generator_function_declaration"
            | "arrow_function"
            | "method_definition"
            | "method_declaration"
            | "constructor_declaration"
            | "destructor_declaration"
            | "operator_declaration"
            | "conversion_operator_declaration"
            | "local_function_statement"
            | "lambda"
            | "lambda_expression"
            | "closure_expression"
            | "func_literal"
            | "class"
            | "class_definition"
            | "class_declaration"
            | "class_specifier"
            | "struct_specifier"
            | "struct_declaration"
            | "record_declaration"
            | "interface_declaration"
            | "namespace_definition"
            | "namespace_declaration"
            | "file_scoped_namespace_declaration"
            | "impl_item"
            | "trait_item"
            | "mod_item"
            | "property_declaration"
            | "indexer_declaration"
            | "accessor_declaration"
            | "enum_declaration"
            | "object_creation_expression"
    )
}

fn header(node: Node<'_>, source: &str) -> Option<String> {
    let body = node.child_by_field_name("body");
    let value = node
        .child_by_field_name("value")
        .or_else(|| node.child_by_field_name("right"));
    let mut tokens = vec![node.kind().to_owned()];
    let mut pending = vec![node];
    while let Some(current) = pending.pop() {
        if current.is_extra() || Some(current) == body || Some(current) == value {
            continue;
        }
        if current != node
            && matches!(
                current.kind(),
                "arrow_expression_clause"
                    | "constructor_initializer"
                    | "accessor_list"
                    | "declaration_list"
                    | "field_declaration_list"
                    | "class_body"
                    | "interface_body"
            )
        {
            continue;
        }
        if current.child_count() == 0 {
            tokens.push(source.get(current.byte_range())?.to_owned());
            continue;
        }
        let mut cursor = current.walk();
        let children: Vec<_> = current.children(&mut cursor).collect();
        if is_binding(current.kind()) {
            // Some grammars (notably C#) do not name the initializer field.
            // Exclude its expression even when this binding is inside a field scope.
            let end = children
                .iter()
                .position(|child| matches!(child.kind(), "=" | ":="))
                .unwrap_or(children.len());
            pending.extend(children[..end].iter().copied().rev());
            continue;
        }
        pending.extend(children.into_iter().rev());
    }
    serde_json::to_string(&tokens).ok()
}
