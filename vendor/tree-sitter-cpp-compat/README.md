# C++ compatibility grammar

This MIT-licensed grammar derives from the pinned and patched sibling
`tree-sitter-cpp` grammar. Its only rule change removes the abstract member-pointer
alternative that introduced ambiguity in some existing qualified template types.
The sibling `UPSTREAM.json`, `cpath.patch`, `LICENSE` and `CPATH.md` provide its
upstream provenance. The unchanged upstream external scanner is shared by inclusion
with distinct exported symbols.

cpath first parses with the primary grammar. Only a primary syntax error permits
one compatibility parse of the same original bytes. The failed tree is dropped
before the second parse, and the worker-local parser is restored to the primary
language afterward. A compatibility tree containing errors is rejected. No
benchmark names, source hashes, text rewrites or numeric labels affect selection.

The primary grammar retains member-pointer support. Sources combining a primary
ambiguity with syntax unsupported by the compatibility grammar can still remain
unscored. This fallback does not claim complete C++ language support.

Generate with Tree-sitter CLI 0.24.3 and ABI 14. Run `cargo test --locked`
from the repository root to check parser compatibility and reuse.
