# Bundled Python grammar

This maintained subset is MIT-licensed tree-sitter-python 0.25.0, revision
`293fdc02038ee2bf0e2e206711b69c90ac0d413f`. Preserve `LICENSE` and the upstream
hashes recorded in `UPSTREAM.json`.

A distinct format-colon token takes precedence over `:=` only where an f-string
format delimiter is valid. Thus `f'{x:=a if b else c}'` contains literal formatting
text, while nested replacement fields and parenthesized assignment expressions
remain executable syntax. The original bytes, ranges and ordinary colon tokens
are unchanged. This also preserves assignment expressions inside indexing.

The grammar also accepts type-parameter defaults, starred expression lists and
multiline attribute access whose continuation has less indentation. A scanner
lookahead marker prevents a recovery dedent while an attribute name is required;
it never creates a token or changes source positions.

Annotation index arguments also accept slices, and ordinary subscripts accept
starred arguments. Type-parameter declarations retain their separate rule;
existing public type-argument node names and source ranges are preserved.

The patch passes all 117 unchanged upstream corpus cases and ten additional
cases. The added sources are accepted by the pinned CPython 3.13.18 parser.
Their expected trees preserve four real conditional expressions and three
assignment expressions; formatting text creates none. Upstream highlighting
and tag assertions also pass. The additional coverage cases preserve the
original type, expression and attribute subtrees used by the metric extractors.

Generate with Tree-sitter CLI 0.25.10 and ABI 15:

```sh
tree-sitter generate --abi 15
tree-sitter test
```

Run `cargo test --locked` from the repository root to check application behavior.
Cargo builds the checked-in parser; consumers do not need Python, Node or the
grammar generator.
