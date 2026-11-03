# Bundled Java grammar

This MIT-licensed subset is tree-sitter-java 0.23.5, revision
`94703d5a6bed02b98e438d7cad1136c01a60ba2c`. Preserve `LICENSE` and the original
file hashes recorded in `UPSTREAM.json`.

The maintained patch accepts type-use annotations before a varargs ellipsis,
as specified by [JLS 8.4.1](https://docs.oracle.com/javase/specs/jls/se25/html/jls-8.html#jls-8.4.1).
An explicit array-dimension conflict permits annotations on both array dimensions
and the following ellipsis. Dynamic preference preserves the existing flat
dimension tree. Existing upstream accepted forms remain available.

The patch also accepts Java 25 module imports specified by
[JLS 7.5.5](https://docs.oracle.com/javase/specs/jls/se25/html/jls-7.html#jls-7.5.5),
while preserving ordinary package and identifier uses of `module`.

The consolidated patch also supports constructor prologues before `super` or
`this`, parenthesized assignment targets, valid numeric underscores and
leading-zero floating literals, qualified record patterns, annotations and
`final` on pattern variables, annotations in multi-catch alternatives, and
literal comment text or NUL characters inside strings. Constructor prologues
follow [JLS 8.8.7](https://docs.oracle.com/javase/specs/jls/se25/html/jls-8.html#jls-8.8.7).
Source bytes are preserved, including embedded NUL characters.

All 108 unchanged upstream corpus cases, 18 additional cases and highlighting
assertions pass. The four original added sources compile with JDK 21 and retain
official Sonar cognitive-complexity labels. The pinned Mozilla teacher rejects
their syntax: no numeric MI expectations are invented or inferred from cpath.
The additional module-import probe compiles at Java 25 source/target with the
pinned Eclipse compiler and real JDK 21 platform classes; its compiler artifact
hash and official Sonar label are recorded separately.
Sixteen compiler-validated coverage probes preserve Sonar labels and explicit
Mozilla parser failures; a further text-block control retains both official
numeric labels in the main 92-probe corpus. The two constructor-prologue probes
use the same pinned Eclipse compiler and Java 25 source/target as module imports.
The complete frozen corpus must also pass after changing this grammar.

The RCA MI profile retains the unmodified 0.23.5 parser as `src/rca/parser.c`,
with its matching `src/rca/tree_sitter/parser.h` in an isolated include directory.
Both hashes are pinned separately in `UPSTREAM.json`; compilation renames only its
exported symbol. Qualified record patterns and text blocks containing `/*`
request this separate parser. If it accepts the unchanged source, MI uses its
tree; otherwise the modern tree supplies the approximate, unvalidated MI.
CC always uses the modern tree and its original ownership and ranges.
This reproduces a reference grammar quirk: the older grammar can absorb text
block argument boundaries into a comment. It does not improve Java semantics.
The reusable modern parser never changes language or state for this MI path.
Reference-parser attempts and outcomes are measured in the corpus report.

Generate with Tree-sitter CLI 0.25.10 and ABI 14. Run `cargo test --locked`
from the repository root to check application behavior. Cargo builds the committed
parser without Node, Java or a parser generator.
