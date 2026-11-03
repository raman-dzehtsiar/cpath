# Maintained TypeScript and TSX grammar

This package preserves tree-sitter-typescript 0.23.2 source at commit
`f975a621f4e7f532fe322e13c4f79495e0a7b2e7` and the inherited JavaScript
grammar from npm 0.23.1. `UPSTREAM.json` records their source hashes and
package integrity; the upstream licenses are retained.

The maintained patch corrects typed arrow return annotations in conditional
expressions and generic function types in call type arguments. An external
lookahead recognizes a generic function-type opening while preserving the
original atomic shift token. This avoids the quadratic ambiguity caused by
splitting every `<<`. Public node
kinds remain stable. Internal corrected productions have a `_cpath_` prefix.
Extraction recognizes their public structural contexts without a source-text
heuristic; aliases do not reliably expose the private grammar names.

This grammar supplies cognitive-complexity ownership. The separate published
0.23.2 dependency remains the independent RCA MI compatibility grammar. The C
exports are renamed at compilation to allow both grammars in the same binary.
The custom Rust build/binding files contain no parser or metric logic.

Regenerate with Tree-sitter CLI 0.25.10 and ABI 14. Run `cargo test --locked`
from the repository root to check application behavior against the committed
parser.
