# cpath's bundled C++ parser

This is a maintained subset of MIT-licensed
[tree-sitter-cpp](https://github.com/tree-sitter/tree-sitter-cpp) v0.23.4,
revision `f41e1a044c8a84ea9fa8577fdd2eab92ec96de02`.
Preserve its LICENSE. `UPSTREAM.json` records original file hashes and the
separately licensed C grammar base, v0.23.1, matching upstream's package lock.
The runtime C parser remains the independent tree-sitter-c 0.24.2 dependency.

## Maintained changes

`cpath.patch` contains the grammar and query-path changes. Grammar generation
uses the checked-in `c-base` source and highlighting query, without npm installs.
Pointer operators are allowed before array bounds in `new` expressions, including
placement allocation, multiple pointer levels, qualifiers, and array dimensions.
The tree retains `new_expression`, pointer declarators, placement arguments and
bound expressions; ternary decisions in bounds remain visible to the analyzer.
This deliberately does not reuse general abstract declarators, which would also
admit reference and function types in this position.

Microsoft `__declspec` and its documented `_declspec` synonym accept modifier
arguments, whitespace-separated modifier sequences, and the empty sequence.
Arguments remain syntax subtrees, including property assignments and string or
numeric values. This fixes explicit compiler-extension syntax without treating
unknown identifiers as annotations. It does not validate each modifier's semantic
requirements or expand macros which may produce them. See Microsoft's
[declaration modifier grammar](https://learn.microsoft.com/en-us/cpp/cpp/declspec).
The extractor does not count declaration-modifier metadata as executable
control flow or callable bodies, even if an argument has expression-like syntax.

Direct lambda init-captures preserve braced and parenthesized initializer
subtrees, including reference and pack forms. Scoped attribute tokens such as
`gnu::fallthrough` are accepted inside preprocessor argument lists; their
conditions remain excluded from cognitive complexity just like other directive
conditions. Braced default parameter values, anonymous enum underlying types,
and class/struct/union explicit instantiations (including `extern template`) are
also supported. Instantiation declarations do not fabricate callable bodies.

No source rewriting, macro expansion, include resolution or type checking is
introduced. Macro annotations and directives splitting syntax can still leave
files unscored. The patch makes no claim of complete C++ compatibility.
The C/C++ extractor additionally rejects error-free `function_definition` trees
without an actual function declarator in their declarator chain. Upstream can
misparse `class API Name { ... }` as a parameterless function named `Name`;
accepting that tree fabricates callable evidence. This check does not resolve
what `API` means and does not reject valid unknown return-type identifiers.
It also rejects orphan compound blocks directly at translation-unit scope,
including through conditional-directive wrappers. The bundled C grammar can
otherwise silently omit some valid old-style pointer-return functions by
misreading their signature as a declaration and their body as an unrelated block.
Those forms remain explicitly unscored; GNU statement expressions, namespace
declaration lists and linkage blocks are not rejected by this check.

## Reproduction

Maintainers need Node.js, Git and **Tree-sitter CLI 0.24.3, ABI 14**, matching this
upstream release. End users need none of these. Cargo compiles the checked-in C
parser into the binary; no grammar is downloaded during a scan.

From this directory:

```sh
tree-sitter generate --abi 14
tree-sitter test
```

From the repository root:

```sh
python3 tools/check_cpp_grammar.py /absolute/path/to/tree-sitter
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

The checker verifies unchanged upstream and C-base hashes, reverse-applies the
patch to reconstruct the original grammar/configuration, and regenerates in a
temporary directory to compare parser C, metadata and headers byte for byte.
Never edit generated files by hand. Additional corpus cases live in
the `test/corpus/cpath-*.txt` files; Rust regressions independently check hand-counted
complexity, callable boundaries, locations and malformed-input rejection.

For future patches, rerun real repositories and compare every previously scored
file's metrics and retained callable evidence. A larger number of accepted files
alone is not evidence that a grammar patch is correct.
