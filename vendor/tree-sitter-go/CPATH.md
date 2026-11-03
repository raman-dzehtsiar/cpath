# Bundled Go grammar

This maintained subset comes from MIT-licensed tree-sitter-go 0.25.0,
revision `1547678a9da59885853f5f5cc8a99cc203fa2e2c`.
Preserve `LICENSE`. `UPSTREAM.json` records original source hashes.

Grouped `var` and `const` declarations accept an omitted final semicolon before
the closing parenthesis, as permitted by Go's semicolon rules. Separators between
specifications remain required. The original declaration and expression nodes,
source bytes and positions remain unchanged. A real Go compiler accepted the
added corpus example. One existing malformed-string
recovery snapshot changes shape while retaining its `ERROR` node and unchanged
input.

A negative dynamic precedence for composite literals resolves an ambiguity in
valid selector conditions followed by standalone blocks. For example,
`if !value.Ready { report() }` must retain its own body when another block follows;
the qualified selector and body must not become a composite literal condition.
The new example was accepted by the Go compiler. All 71 corpus cases pass,
including the unchanged valid upstream composite-literal examples.
`cpath.patch` records the maintained grammar and recovery changes.

Allocation arguments also accept expressions, including modern `new(value)`
and calls to a locally shadowed `make`. Method declarations accept type
parameters. The maintained ambiguity between a type parameter declaration and
a simple type keeps both interpretations available to the parser. Authored
allocation and generic-method examples compile with Go 1.27.3, the compiler
used for the expanded corpus. Their original sources and independent numeric
labels are included in the offline metric probes.

Generate using **Tree-sitter CLI 0.25.10, ABI 15**:

```sh
tree-sitter generate --abi 15
tree-sitter test
```

Run `cargo test --locked` from the repository root to check application behavior.

End users do not need Node, Go or the generator: Cargo compiles the checked-in C
parser. This correction provides syntax extraction, not Go type checking.
