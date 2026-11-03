// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

//! Java/Python syntax extraction; profile-specific scoring is parser independent.
mod cognitive;
mod mi;

use crate::language::{SourceAnalysis, SourceParser};
use crate::metrics::jvm_python::Language;
use crate::metrics::{CognitiveInput, MaintainabilityInput};
use crate::model::FileFacts;
use std::path::PathBuf;
use tree_sitter::{Node, Parser};

pub struct JvmPythonParser {
    parser: Parser,
    language: Language,
}

impl JvmPythonParser {
    fn create(language: Language, grammar: tree_sitter::Language) -> Result<Self, String> {
        let mut parser = Parser::new();
        parser
            .set_language(&grammar)
            .map_err(|error| format!("cannot initialize bundled grammar: {error}"))?;
        Ok(Self { parser, language })
    }

    pub fn java() -> Result<Self, String> {
        Self::create(Language::Java, tree_sitter_java::LANGUAGE.into())
    }

    pub fn python() -> Result<Self, String> {
        Self::create(Language::Python, tree_sitter_python::LANGUAGE.into())
    }
}

impl SourceParser for JvmPythonParser {
    fn parse(&mut self, path: PathBuf, source: &str) -> Result<FileFacts, String> {
        self.parse_analysis(path, source)
            .map(|analysis| analysis.file)
    }

    fn parse_analysis(&mut self, path: PathBuf, source: &str) -> Result<SourceAnalysis, String> {
        let tree = self
            .parser
            .parse(source, None)
            .ok_or("parser did not produce a syntax tree")?;
        let root = tree.root_node();
        if root.has_error() {
            let label = match self.language {
                Language::Java => "Java",
                Language::Python => "Python",
            };
            return Err(super::syntax_error(root, label));
        }
        Ok(SourceAnalysis {
            file: FileFacts {
                path,
                bytes: source.len() as u64,
                lines: source.lines().count().try_into().unwrap_or(u32::MAX),
                profiled_cognitive: Some(CognitiveInput::JvmPython(cognitive::extract(
                    root,
                    source,
                    self.language,
                ))),
                ..FileFacts::default()
            },
            maintainability: Some(crate::maintainability::MaintainabilityFacts {
                units: Vec::new(),
                profile: Some(MaintainabilityInput::JvmPython(mi::extract(
                    root,
                    source,
                    self.language,
                ))),
            }),
        })
    }
}

fn text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}

fn name(node: Node<'_>, source: &str) -> String {
    node.child_by_field_name("name").map_or_else(
        || format!("<{}>", node.kind()),
        |name| text(name, source).to_owned(),
    )
}
