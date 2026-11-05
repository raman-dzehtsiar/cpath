// Copyright (c) 2026 Raman Dzehtsiar
// SPDX-License-Identifier: MIT

//! Physical lines containing code, counted independently of CC/MI extraction.

use crate::model::SourceLanguage;
use std::ops::Range;
use tree_sitter::{Language, Node, Parser};

fn grammars(language: SourceLanguage) -> Vec<Language> {
    match language {
        SourceLanguage::CSharp => vec![tree_sitter_c_sharp::LANGUAGE.into()],
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

fn is_comment(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "comment" | "line_comment" | "block_comment" | "hash_bang_line" | "shebang"
    )
}

fn comment_ranges(root: Node<'_>) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut cursor = root.walk();
    loop {
        let node = cursor.node();
        if is_comment(node) {
            ranges.push(node.byte_range());
        } else if cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return ranges;
            }
        }
    }
}

fn count_code_lines(source: &str, comments: &[Range<usize>]) -> u32 {
    let mut comments = comments.iter().peekable();
    let mut lines = 0_u32;
    let mut contains_code = false;
    for (offset, character) in source.char_indices() {
        if character == '\n' {
            lines = lines.saturating_add(u32::from(contains_code));
            contains_code = false;
            continue;
        }
        while comments.peek().is_some_and(|range| offset >= range.end) {
            comments.next();
        }
        if comments.peek().is_some_and(|range| range.contains(&offset)) {
            continue;
        }
        if !character.is_whitespace() {
            contains_code = true;
        }
    }
    lines.saturating_add(u32::from(contains_code))
}

pub(crate) fn count(source: &str, language: SourceLanguage) -> Result<u32, String> {
    let mut parser = Parser::new();
    for grammar in grammars(language) {
        parser
            .set_language(&grammar)
            .map_err(|error| format!("Failed to initialize LOC parser: {error}"))?;
        let Some(tree) = parser.parse(source, None) else {
            continue;
        };
        if tree.root_node().has_error() {
            continue;
        }
        return Ok(count_code_lines(source, &comment_ranges(tree.root_node())));
    }
    Err("cannot calculate LOC: source could not be parsed for comment boundaries".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_code_and_ignores_comment_only_lines_in_every_supported_language() {
        for (language, source, expected) in [
            (
                SourceLanguage::C,
                "// comment\n\nint f(void) { /* inline */\n return 1; // tail\n}\n",
                3,
            ),
            (
                SourceLanguage::Cpp,
                "/* comment\ncontinued */\nclass C {\n int f() { return 1; }\n};",
                3,
            ),
            (
                SourceLanguage::CHeader,
                "// comment\nnamespace N {\n class C {};\n}\n",
                3,
            ),
            (
                SourceLanguage::CSharp,
                "// comment\nclass C {\n string s = \"// code\";\n}\n",
                3,
            ),
            (
                SourceLanguage::Rust,
                "/* outer\n /* nested */\n*/\nfn f() {\n let s = r#\"/* code */\"#;\n}\n",
                3,
            ),
            (
                SourceLanguage::Java,
                "// comment\nclass C {\n String s = \"// code\";\n}\n",
                3,
            ),
            (
                SourceLanguage::Python,
                "# comment\n\ndef f():\n    s = '# code'\n    return s # tail\n",
                3,
            ),
            (
                SourceLanguage::JavaScript,
                "#!/usr/bin/env node\n// comment\nconst s = '// code';\n",
                1,
            ),
            (
                SourceLanguage::TypeScript,
                "// comment\nconst s: string = '/* code */';\n",
                1,
            ),
            (
                SourceLanguage::Tsx,
                "const element = (\n <div>\n {/* comment */}\n text\n </div>\n);\n",
                6,
            ),
            (
                SourceLanguage::Go,
                "// comment\npackage main\n\nfunc f() string {\n return `// code`\n}\n",
                4,
            ),
        ] {
            assert_eq!(count(source, language).unwrap(), expected, "{language:?}");
            assert_eq!(
                count(&source.replace('\n', "\r\n"), language).unwrap(),
                expected,
                "{language:?} CRLF"
            );
        }
    }

    #[test]
    fn handles_empty_files_inline_comments_multiline_strings_and_missing_final_newlines() {
        for (source, expected) in [
            ("", 0),
            (" \t\r\n\n", 0),
            ("/* comment\n continued */\n// final", 0),
            ("/* comment */ int a; /* comment */ int b;", 1),
            ("int a; /* comment\n continued\n */ int b;\n", 2),
            ("const char *s = \"// café\";", 1),
            (
                "const char *s = R\"(first\n// string contents\n\nlast)\";\n",
                3,
            ),
        ] {
            assert_eq!(
                count(source, SourceLanguage::Cpp).unwrap(),
                expected,
                "{source:?}"
            );
        }
        assert!(count("class {", SourceLanguage::CSharp).is_err());
    }
}
