# cpath's bundled C# parser

This directory is a maintained subset of
[tree-sitter/tree-sitter-c-sharp](https://github.com/tree-sitter/tree-sitter-c-sharp)
at revision `8c0abe0b84a3681d3e6852ce2e2a1eecb4f731c5`, with small cpath patches.
The upstream code remains **MIT licensed**; preserve [LICENSE](LICENSE).
`UPSTREAM.json` records the revision and SHA-256 of each original vendored file.
The Rust binding, queries, upstream tests and build script are unchanged.
Other language bindings and upstream CI/package tooling are not needed here.

## Local changes

`cpath.patch` contains the reviewable `grammar.js` and `src/scanner.c` deltas
against the pinned base.
The additional `test/corpus/cpath-*.txt` cases are kept separately from upstream
tests. Generated `src/parser.c`, `src/grammar.json`, `src/node-types.json` and
`src/tree_sitter/*.h` come from the pinned generator and patched grammar.
The generator refreshes `array.h` relative to the upstream snapshot as well.
Never hand-edit generated output.

1. **Pointer dereference:** permit an expression operand, including parenthesized
   arithmetic and casts. A targeted expression/assignment conflict handles
   dereference on the left of an assignment. Keep address-of restricted to an
   lvalue so Boolean `&&` is not reinterpreted as two address-of operations.
2. **Conditional list tails:** permit comma-prefixed arguments and parameters
   inside `#if`/`#elif`/`#else`, including nesting and empty branches, after an
   unconditional first item. Preserve all branch nodes and original positions.
   Commas remain required. This is a bounded syntax extension, not evaluation
   of build symbols or enumeration of configurations.
3. **Leading list slices:** give bare slices precedence over range-expression recovery
   paths in pattern contexts, and prefer list-pattern trees over collection/element-binding recovery
   interpretations in pattern contexts, including following Boolean operators.
   Range expressions and collection spreads remain separate syntax nodes.
4. **File-based app directives:** recognize `#:sdk`, `#:property`, `#:package`,
   `#:project` and `#:include` before code as opaque metadata. No referenced
   inputs, packages or build properties are resolved or evaluated.
5. **Conditional fragments with complete local structure:** preserve conditional
   initializer segments, comma-terminated base-list prefixes, catch lists and
   else tails. Prefix initializer/base elements keep their required commas;
   only a terminal initializer segment can omit the final comma. A conditional
   else-if prefix can precede a shared final else; a branch containing a terminal
   else cannot. All supported branches remain visible in one original-source tree.
6. **Branch-local expression terminators:** declarations, assignments and arrow
   bodies can end in `#if` alternatives containing complete expressions with their
   own semicolons. Each branch must have an expression and semicolon, and a final
   `#else` is required. This does not make ordinary semicolons optional or accept
   arbitrary expression fragments. Property/indexer arrow and assignment node
   shapes remain compatible with callable and complexity extraction.
7. **Implementation-defined pragma metadata:** identifier-led pragma commands and
   warning subcommands such as `warning suppress` accept opaque, line-bounded
   arguments. Recognized warning/checksum structures retain their original shapes.
   No pragma payload is interpreted as executable source or evaluated.

The conditional-else extension uses a stateless, zero-width external scanner marker
before the existing right-associative if production. It distinguishes an else tail
from ordinary conditional statements after a completed if without changing nearest-
if dangling-else binding. The marker scans at most **65,536 Unicode code points**,
including leading whitespace, uses fixed-size storage, skips comments/directive
headers, and accepts only an exact `else` token or an entirely empty directive
group. Grammar productions still validate every branch and consume the nonempty
conditional group; the marker never rewrites or removes source. On budget exhaustion
it declines the extension: ordinary successor statements remain parseable, while a
conditional else beyond the limit remains explicitly unscored. LF/CRLF are supported;
CR-only directives remain outside this grammar's existing line-ending support.
The keyword boundary conservatively rejects non-ASCII code points, including Unicode
whitespace immediately following `else`, to avoid misreading Unicode identifiers.

These changes do not provide type checking, preprocessing or complete C# support.
Directives splitting operators, expression suffixes, braces, or other unsupported
partial constructs can still fail. cpath continues to report unscored files and partial coverage. The
extractor visits all supported branch bodies and skips directive conditions;
scores therefore describe source alternatives, not one selected build.

Conditional else wrappers are transparent for direct else-if detection; real
blocks remain nesting boundaries. A shared else keyword counts once when any
alternative has a non-if body, and is omitted when all alternatives are direct
else-if clauses. Branch-local callables retain independent scopes. These rules
are tested with hand-counted mixed alternatives and original source positions.

## Integration

The root Cargo manifest uses this directory as a path dependency. Ordinary
`cargo build` compiles the checked-in C parser with the upstream Rust build script.
It needs no Node.js or Tree-sitter CLI. The parser is statically linked into cpath;
end users need no compiler, SDK, grammar download, or external parser library.
Normal Rust dependency acquisition at development/build time is unchanged.

## Regenerate and verify

Maintainers need Git, Node.js and **Tree-sitter CLI 0.26.5**, with **ABI 15**. Obtain the
CLI through its official release or package; it is development tooling only.
From this directory, using the actual path to that executable:

```sh
tree-sitter --version                  # must report 0.26.5
tree-sitter generate --abi 15
tree-sitter test
```

From the cpath root:

```sh
python3 tools/check_csharp_grammar.py /absolute/path/to/tree-sitter
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

The Python check verifies untouched upstream files against `UPSTREAM.json`,
reverse-applies `cpath.patch` in a temporary directory to verify the original
grammar and scanner hashes, regenerates there and compares parser C, metadata and headers
byte for byte. It does not update vendored output. Upstream corpus tests check
syntax trees; cpath's Rust tests additionally check operator shapes, all-branch
complexity, callable boundaries, source lines and rejection of missing separators.

When updating upstream, review its changes, replace the pristine-file manifest,
reapply or retire each patch, regenerate, run both suites and repeat full-scan
coverage/performance measurements. Do not accept a parse merely because errors
disappear: inspect tree structure and unchanged-file scores too.
