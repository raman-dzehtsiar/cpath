// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

//! JavaScript, TypeScript and Go syntax facts. Scoring lives in metrics/web_go.

mod cognitive;
mod mi;

use crate::language::{SourceAnalysis, SourceParser};
use crate::metrics::web_go::Language;
use crate::metrics::{CognitiveInput, MaintainabilityInput};
use crate::model::FileFacts;
use std::path::PathBuf;
use tree_sitter::{Node, Parser};

pub struct WebGoParser {
    parser: Parser,
    compatibility_parser: Option<Parser>,
    language: Language,
}

impl WebGoParser {
    fn create_parser(grammar: tree_sitter::Language) -> Result<Parser, String> {
        let mut parser = Parser::new();
        parser
            .set_language(&grammar)
            .map_err(|error| format!("cannot initialize bundled grammar: {error}"))?;
        Ok(parser)
    }

    fn create(
        language: Language,
        grammar: tree_sitter::Language,
        compatibility: Option<tree_sitter::Language>,
    ) -> Result<Self, String> {
        Ok(Self {
            parser: Self::create_parser(grammar)?,
            compatibility_parser: compatibility.map(Self::create_parser).transpose()?,
            language,
        })
    }

    pub fn javascript() -> Result<Self, String> {
        Self::create(
            Language::JavaScript,
            tree_sitter_javascript::LANGUAGE.into(),
            None,
        )
    }

    pub fn typescript() -> Result<Self, String> {
        Self::create(
            Language::TypeScript,
            tree_sitter_typescript_cpath::LANGUAGE_TYPESCRIPT.into(),
            Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
        )
    }

    pub fn tsx() -> Result<Self, String> {
        Self::create(
            Language::TypeScript,
            tree_sitter_typescript_cpath::LANGUAGE_TSX.into(),
            Some(tree_sitter_typescript::LANGUAGE_TSX.into()),
        )
    }

    pub fn go() -> Result<Self, String> {
        Self::create(Language::Go, tree_sitter_go::LANGUAGE.into(), None)
    }

    fn compatible_maintainability(
        &mut self,
        source: &str,
    ) -> Result<crate::metrics::web_go::MaintainabilityFacts, String> {
        let parser = self
            .compatibility_parser
            .as_mut()
            .ok_or("corrected syntax requires its bundled RCA compatibility grammar")?;
        let tree = parser
            .parse(source, None)
            .ok_or("RCA compatibility parser did not produce a syntax tree")?;
        if tree.root_node().has_error() {
            return Ok(crate::metrics::web_go::MaintainabilityFacts {
                language: self.language,
                units: Vec::new(),
                used_compatibility_grammar: true,
                unscored_roots: 1,
            });
        }
        let mut facts = mi::extract(
            tree.root_node(),
            source,
            self.language,
            &std::collections::HashMap::new(),
            false,
        )
        .ok_or("the pinned RCA grammar unexpectedly requested compatibility")?;
        facts.used_compatibility_grammar = true;
        Ok(facts)
    }
}

impl SourceParser for WebGoParser {
    fn parse(&mut self, path: PathBuf, source: &str) -> Result<FileFacts, String> {
        self.parse_analysis(path, source).map(|result| result.file)
    }

    fn parse_analysis(&mut self, path: PathBuf, source: &str) -> Result<SourceAnalysis, String> {
        let tree = self
            .parser
            .parse(source, None)
            .ok_or("parser did not produce a syntax tree")?;
        let root = tree.root_node();
        if root.has_error() {
            return Err(format!(
                "{:?} source contains unsupported or malformed syntax",
                self.language
            ));
        }
        let bindings = if self.language == Language::Go {
            mi::go_bindings(root, source)
        } else {
            std::collections::HashMap::new()
        };
        let cognitive = cognitive::extract(root, source, self.language, &bindings);
        let maintainability = mi::extract(
            root,
            source,
            self.language,
            &bindings,
            self.language == Language::TypeScript,
        );
        // Cognitive facts own no parser nodes. Release the corrected tree before
        // a rare compatibility parse rather than retaining two complete trees.
        drop(tree);
        let maintainability = match maintainability {
            Some(facts) => facts,
            None => self.compatible_maintainability(source)?,
        };
        Ok(SourceAnalysis {
            file: FileFacts {
                path,
                bytes: source.len() as u64,
                lines: source.lines().count().try_into().unwrap_or(u32::MAX),
                profiled_cognitive: Some(CognitiveInput::WebGo(cognitive)),
                ..FileFacts::default()
            },
            maintainability: Some(crate::maintainability::MaintainabilityFacts {
                units: Vec::new(),
                profile: Some(MaintainabilityInput::WebGo(maintainability)),
            }),
        })
    }
}

fn callable(node: Node<'_>, language: Language) -> bool {
    if language == Language::Go {
        matches!(node.kind(), "function_declaration" | "method_declaration")
    } else {
        matches!(
            node.kind(),
            "function_declaration"
                | "function_expression"
                | "generator_function"
                | "generator_function_declaration"
                | "arrow_function"
                | "method_definition"
        )
    }
}

fn text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}

fn name(node: Node<'_>, parent: Option<Node<'_>>, source: &str) -> String {
    if let Some(name) = node.child_by_field_name("name") {
        return text(name, source).to_owned();
    }
    if let Some(parent) = parent {
        if let Some(name) = parent
            .child_by_field_name("name")
            .or_else(|| parent.child_by_field_name("key"))
        {
            return text(name, source).to_owned();
        }
    }
    format!("<{}@{}>", node.kind(), node.start_position().row + 1)
}
