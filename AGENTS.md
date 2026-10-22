# AGENTS

Read `AGENTS.local.md` when present for additional checkout-specific context.

## Product purpose

cpath is a fully local, fully offline codebase intelligence tool. Its purpose is
to highlight where human attention is needed now to avoid future development
friction and mistakes, including during autonomous agent development.

The product vision is a single binary: download it, point it at a codebase, and
get useful findings without accounts, API keys, telemetry, hosted services,
model downloads, or mandatory configuration. End users should not need to build
the target project or install its language toolchain to scan it.

Rank by cognitive complexity: control flow that is difficult to understand and
verify. Duplications and semantic clones detection, token retention, 
coupling are a future direction. We don't analyze git churn or ownership.

## Initial CLI and output contract

The first scan interface is:

```text
cpath /path/to/repo --top N
```

- N is a positive integer limiting the number of ranked files printed. Analyze
  the full selected input before ranking; --top limits output, not scan scope.
- Print at most N distinct scored files, ordered by maximum function complexity
  descending, then complexity burden descending, then normalized repository-relative
  path ascending. If fewer files can be scored, print only those files.
- Each stdout line contains exactly a numeric score, one space, and the absolute
  file path. Preserve spaces within paths. No header, table borders, rank numbers,
  colors, summary, progress, metric breakdown, or explanatory text in scan stdout.

```text
maximum_attention_score /path/to/file
```

- Empty results produce no result lines. Errors go to stderr with a nonzero exit
  status; never fabricate scores for unscored files.
- Exit 0 means complete selected-input analysis; exit 2 means invalid arguments.
  Exit 3 means partial coverage: print valid ranked rows, but report analyzed and
  unscored counts on stderr, at most 32 sorted file diagnostics, and an omitted
  count. Read, encoding, binary, per-file size, and parsing failures make files
  unscored, never zero-score. Even all-unscored input returns 3.
- Discovery/configuration, worker, and aggregate summary-budget failures are
  fatal: exit 1 without ranking rows. Output-write failures also use exit 1.
- Rank files, even when evidence is collected at method, block, type, or module
  level. Keep file aggregation explicit in the scoring component.
- Other output formats and user-facing explanations are deferred. Preserve typed
  evidence and diagnostics internally for testing and later explanations; do not
  expand the initial output contract to display them.
- The current POC uses `cpath.complexity-ranking.v1`, defined in
  `src/scoring.rs`. Print the maximum callable complexity as an integer. Burden
  is the sum of each callable's complexity above 15; that threshold is provisional.
  Duplication and coupling are absent, including extraction, token retention,
  and unused contracts; do not retain dormant implementations.
  Keep formulas versioned and tested; do not change them to flatter a benchmark.

## Mandatory architecture requirements

These are core product requirements, not optional style preferences. Apply them
from the first scanner implementation. Do not defer isolation, bounded
parallelism, or performance work until after a serial monolithic prototype.
Changes that violate these requirements need an explicit change of direction
from the user; a convenient implementation shortcut is not a reason to relax them.

### Stateless full scans

- Every invocation performs a full scan of the selected source files as they
  exist on disk. Do not reuse analysis from earlier invocations.
- No persistent scan cache, local database, saved index, background daemon, or
  incremental scan mode. Do not create hidden scan-state directories or files.
- Compact per-scan aggregation data is allowed and necessary; discard it when
  the invocation ends. Do not retain source or token collections for hypothetical
  future analyzers.
- Explicitly requested report files are output, not reusable analysis state.
- Analyze source without building it, invoking MSBuild, restoring NuGet packages,
  installing an SDK, fetching grammars or models, or depending on Git history.
- Cargo's development build cache is unrelated to this runtime requirement.

Why: the first scan is the normal experience. Results must not depend on cache
warmup, stale state, environment setup, or a previous successful run. Performance
must come from efficient analysis rather than avoiding work through persistence.

### Multithreaded pipeline from the beginning

- Design the first scan implementation as a multithreaded pipeline with explicit
  stages for discovery/selection, reading/parsing, analysis, and aggregation/reporting.
- Use bounded work queues or equivalent bounded scheduling with backpressure,
  a controlled worker budget, and clear ownership of data between stages.
- Parallelize independent file work. If a future authorized analyzer requires
  repository-wide analysis, make its cross-file dependencies, partitioning, memory
  cost, and finalization barrier explicit rather than hiding work in the pipeline.
- Avoid one thread per file, unbounded pending tasks, nested worker pools that
  oversubscribe the machine, and a global mutex around the analysis hot path.
- Keep parsers and mutable accumulators local to workers where practical. Share
  immutable facts and merge results deterministically.
- Read each selected file and parse it once where practical. Reuse decoded content
  for generated-code classification and parsing. Compute all callable metrics and
  ranking keys inside the worker, then release source, tree, and events. Retain
  only FileSummary, complete maximum/total, and the five highest-scoring callable
  locations. Compute burden before reducing evidence; never rank from samples.
- Preserve explicit resource limits: default 8 MiB per source file, 100,000
  analyzed-file summaries, 128 MiB summary bookkeeping, and 256 directory levels.
  These bound retained work, not exact RSS. Oversized files make coverage partial;
  aggregate-summary/discovery failures are fatal. Do not reintroduce a total-source
  or retained-token budget now that independent files stream through workers.
- Future cross-file algorithms must use indexed candidate discovery rather than
  unconditional all-pairs comparisons and account for repetitive worst cases.
- Bound intermediate memory, avoid unnecessary copies, and handle stage failures
  and cancellation without deadlocks or silently incomplete successful reports.
- Worker count and completion order must not change findings, scores, or ordering.

Why: full scans must remain fast on large repositories. Retrofitting concurrency
into tangled code creates synchronization problems, extra copies, and unstable
results. Bounded scheduling protects both throughput and memory consumption.

### One file-selection policy

- A dedicated selection/classification component owns all decisions about which
  files and directories to include, exclude, or classify as generated/build output.
- It accepts custom includes and excludes, respects repository ignore rules, and
  recognizes common build directories and generated source by default. For C#,
  cover conventions such as bin/obj output and generated-file markers, documenting
  heuristics and their limitations rather than assuming every naming match is exact.
- Default generated names include .autogen.cs. Vendor classification uses known
  exact directory names documented in `src/selection.rs`, not ownership guesses.
  Tests, migrations, and ambiguous external directories remain eligible.
- Includes form an allow-list and override local ignores plus build/vendor/generated
  defaults; explicit excludes win. Includes cannot override invalid content,
  metadata exclusion, symlinks, or unsupported languages. Decoding is isolated:
  UTF-8 with optional BOM and strict BOM-marked UTF-16LE/BE are supported; never
  guess legacy encodings or introduce replacement characters.
- Support early path-based filtering and content-based classification through the
  same policy. The reader supplies content; analyzers must not duplicate skip rules.
- Define and test rule precedence, path normalization, directory pruning, and how
  explicit includes interact with default exclusions and repository ignores.
  Do not prune a directory if the documented include policy permits selecting a
  descendant. Retain structured skip reasons and coverage counts internally for
  tests and future explanations; do not append them to the initial scan output.
- Keep selection independent of metric algorithms, parsing, and presentation so
  its rules can evolve and be tested without running the whole scanner.

Why: inconsistent filtering makes metrics disagree about what was analyzed and
wastes work on generated output. A single policy keeps coverage explainable and
user overrides predictable.

### Isolated, replaceable analysis components

- Keep cognitive complexity replaceable and separate from scoring. If duplication
  returns, implement it as an independent analyzer behind a deliberate contract;
  do not preload its data or retain dormant code now. Do not restore coupling.
- Define small typed contracts at these boundaries, using Rust traits or an
  equally explicit replaceable interface. Select implementations at the composition
  point; keep algorithm choices out of orchestration, the CLI, and reporters.
- Isolate filesystem access, selection, Tree-sitter/language extraction, analysis,
  ranking, and report rendering. Analysis consumes explicit inputs and
  returns facts/findings; it must not walk directories, print output, or persist state.
- Share compact domain facts and source locations. Do not expose concrete parser
  nodes or algorithm-specific internal structures throughout the pipeline. Extend
  extraction contracts deliberately when an improved algorithm needs new evidence.
- Keep score aggregation separate from raw metrics. Replacing a metric algorithm
  should require its implementation, wiring, and any necessary contract/schema
  evolution, not a rewrite of unrelated components.
- Test each component in isolation, with shared contract fixtures for replacement
  implementations and integration tests for the assembled pipeline.
- Start with one crate and clear module boundaries. These required interfaces are
  not speculative abstraction; a generic plugin framework, runtime loading system,
  or collection of tiny crates is not needed to achieve them.

Why: the first algorithms are intentionally approximate and will improve. Their
replacement must be a normal, contained change. Keeping scheduling, I/O, policy,
metrics, and presentation separate prevents algorithm experiments from becoming
whole-system rewrites.

## Current implementation and initial scope

This is an early implementation for C#, C and C++. Selection, strict decoding, the streaming
bounded pipeline, Tree-sitter extraction, complexity ranking, partial-coverage
diagnostics, and score/path output are implemented. Verify actual files and tests
before extending behavior. Current contracts are recorded in the component source
and tests; independent counting expectations are in `tests/cognitive_corpus.rs`
and `tests/c_family_corpus.rs`. `docs/initial-research-results.md` summarizes
exploratory research, not current memory or failure behavior.

- The approved project name is cpath. The first target language is C#/.NET source.
- Duplication is deferred. No clone analysis or token retention runs during scans.
- Start with one crate and the isolated component boundaries specified above.
- Keep CLI entry points thin and analysis behavior testable.
- Tree-sitter is the syntax foundation. The C# grammar is vendored from upstream
  revision 8c0abe0b84a3681d3e6852ce2e2a1eecb4f731c5 with tested local patches for
  pointer dereference, conditional comma-prefixed argument/parameter tails,
  leading list slices, header-only file-based app metadata directives, conditional
  initializer/base-list segments, catches, else tails and terminated whole RHS
  alternatives. Preserve all represented source branches, callable boundaries and
  shared code exactly once. Only directive wrappers are transparent for else-if
  nesting; actual blocks remain nesting boundaries. Pragmas are metadata.
  Conditional-else lookahead is a bounded zero-width scanner token, not source
  preprocessing. Preserve its 65,536-character work limit, ordinary successor
  statements, nearest-if else binding and scanner-state isolation. Keep scanner
  changes in the reviewable provenance patch and verify original hashes.
  Read vendor/tree-sitter-c-sharp/CPATH.md before changing it. Keep upstream MIT
  licensing, provenance, the small grammar patch, and generated artifacts together.
  Generate with Tree-sitter CLI 0.26.5, ABI 15; never hand-edit parser.c. Run the
  upstream and local grammar corpus plus cpath extraction/scoring regressions.
  Ordinary Cargo builds use checked-in C; scans require no grammar downloads.
  Some valid C# still fails, including directives splitting partial expressions
  and braces. Preserve explicit unscored diagnostics rather than silently
  rewriting source or counting an incomplete tree. Parsing does not provide type
  resolution or semantic equivalence. Check AST shape, locations, coverage and
  performance when changing grammar; error-free parsing alone is insufficient.
- The C++ grammar is vendored from tree-sitter-cpp v0.23.4 with targeted syntax
  patches. Read vendor/tree-sitter-cpp/CPATH.md before changing it.
  Preserve its MIT license, pinned C base, provenance hashes and reviewable patch.
  Generate with Tree-sitter CLI 0.24.3, ABI 14; run its corpus, reproduction
  checker and cpath AST/scoring regressions. Do not silently strip macro tokens
  or accept incomplete trees to improve the number of scored files. Preserve
  callable-declarator and file-scope-block checks: known error-free macro
  misparses must remain unscored. Explicit declaration annotations are metadata,
  not executable control flow. Do not infer arbitrary macro expansions from names.
- Failure kinds are counted for every unscored file, independently of the bounded
  CLI diagnostics. Developer diagnostic observers run in the aggregation thread
  with backpressure; errors must drain queues safely and fail the scan. Research
  reports are explicit outputs, never runtime caches. Lexical failure clues are
  hypotheses, not verified causes or proof that all remaining files are invalid.
- Describe selected files lacking trustworthy scores as **unscored**, with a
  reason. Do not call them unavailable: the file may be readable and valid source.
- Use explicit pipeline data flow and per-invocation in-memory analysis only.
- Prefer a small bounded scheduler or suitable existing parallelism primitives
  over a custom general-purpose scheduling framework.
- Introduce dependencies and language support only for concrete needs.
- Keep runtime operation offline; do not fetch grammars, models, or source.
- Use typed data and actionable errors; avoid panics on user input.
- Preserve cross-platform behavior and advisory-only output.

## Metrics and findings

Approximate metrics are acceptable when useful for decisions. Make heuristics
deterministic, explainable, and explicit about their limits.

Internal findings should identify what and where they measured, the rule or
formula, the supporting evidence, the plausible future problem, and uncertainty.
Keep that evidence separate from the initial score-and-path output.
Keep exact, structural, and semantic similarity distinguishable. Similarity is
not proof that two implementations should be merged.

The printed score is maximum callable cognitive complexity. The secondary burden
key measures concentrated complexity; raw total complexity must not rank large
collections of simple methods higher. Keep ranking versioned and documented.
Do not restore the combined score, clone runtime, or coupling implementation.
Missing analysis is unknown, not zero. Keep coverage separate from attention
priority. Logical-operator costs count syntax-tree groups by parent-operator
equality, not flattened source sequences; document that distinction and preserve
the manually audited corpus when changing extraction.

## Tests and determinism

- Specify behavior in tests before implementation whenever practical.
- Use focused unit tests and small fixture-based integration tests.
- Test meaningful negative cases as features arrive: unreadable or unsupported
  input, binaries, invalid encodings, symlinks, ignores, and parse errors.
- Keep tests offline and independent of host paths and wall-clock timestamps.
- Sort traversal, findings, groups, and ties by stable keys.
- Use repository-relative paths internally and in portable fixtures. The initial
  CLI explicitly prints absolute file paths. Avoid default report timestamps.
- Test top-N file ranking, deterministic ties, fewer-than-N and empty results,
  paths with spaces, argument errors, and stdout containing only score/path rows
  when scan output is implemented.
- Document approximation boundaries; update tests and docs when formulas change.
- Test selection independently, including conflicting includes/excludes and
  generated-code classification. Test identical results across worker counts,
  pipeline error propagation, and repeated full scans without persisted state.
- Test supported BOM encodings, malformed UTF-16, partial/all-unscored results,
  sorted bounded diagnostics, fatal budgets, and ranking before evidence truncation.
  Keep the original manually audited cognitive corpus independent of implementation
  output; new expected values require a documented hand count and review.
- Maintain reproducible end-to-end and per-stage benchmarks on representative C#
  inputs, recording input size, worker count, elapsed time, and peak memory.
  Measure complete scans without analysis caches; distinguish filesystem-cache
  effects. Use benchmarks for timing, not fragile wall-clock assertions in unit tests.
- Review changes against the mandatory architecture requirements above. Do not
  accept performance improvements that break determinism or component isolation.
- Run `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`,
  and `cargo test` before delivery when practical. Report checks that could not run.

## Licensing, documentation, and Git

- The project license is MIT.
- Include one SPDX-License-Identifier: MIT header before code in every
  source file created or edited, using native comment syntax.
- Do not add headers to generated files, lockfiles, binaries, vendor code, or
  data fixtures.
- Public documentation must distinguish product vision from working features.
- Do not promise precise risk, review-time, performance, or semantic-equivalence
  results without reproducible evidence.
- Work inside this repository, preserve user changes, and keep changes scoped.
- Keep private local material out of commits and public documentation.
- Read `.plan/` only when explicitly asked; never commit it or disclose its contents.
- Use imperative commit messages and do not change global Git settings casually.
