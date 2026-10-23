# Attention ranking research results

We are researching an **attention score** to help developers and AI agents
prioritize review and improvement efforts to keep codebases maintainable.
The current preview uses cognitive complexity alone; a combined
score and architectural-drift detection remain research goals.

**Current conclusion:** keep `cpath.complexity-ranking.v1`. Experiments on
20–22 October 2026 did not establish a better combined formula. The next question
is whether local state and lifetime explanations help people make correct changes,
including in code with little control flow.

All review ratings came from coding agents, not human maintainers. These studies
measure agreement with static judgments, not defects, urgency, maintenance cost,
or review-time savings. Large scan counts do not validate whole-repository top-N
priorities.

## What was tested

The production score is the maximum callable cognitive complexity in a file.
Ties use the sum of callable complexity above 15, then repository-relative path.
The burden threshold is provisional. Experiments combine features within each
callable before taking the file maximum; retained locations do not determine scores.

| Signal | Definition | Intended distinction |
| --- | --- | --- |
| C: current complexity | Event count E plus structural nesting contribution N | Breadth and nesting |
| B: compressed breadth | N + ceil(sqrt(E)) | Reduce the weight of many flat decisions |
| K: concentration | P + ceil(sqrt(C − P)), where P is the greatest event weight along an AST ancestor chain | Reduce repetition outside that chain |
| H: lifecycle syntax | Number of categories present among await expressions, lock, using, finally, yield and throw | Presence of several kinds of obligation |
| KH | K + H | Concentration with lifecycle syntax |

An AST ancestor chain is not a feasible execution path. Syntax categories do not
prove interactions or incorrect behavior. Additional candidates used repeated
assignment targets, API spellings and statement counts. Name-sensitive variants
failed renaming controls; explicit `this.member` updates were too sparse to
establish usefulness. Twelve formulas were evaluated in the combined-signal study; their complete
definitions remain in the research tools.

## Results that determine the direction

Concordance gives 1 for agreement, 0 for reversal and 0.5 when either the judgment
or predictor ties. Values describe selected pairs, not population accuracy.

| Study | Sample | Essential result |
| --- | --- | --- |
| Initial exploration | 30 files from five C# repositories | All ten sampled top-ranked files received careful/intensive-review ratings, but file size was similarly associated with ratings. |
| Size-matched follow-up | 12 pairs from Serilog, CsvHelper and Dapper | C concordance 0.750 versus lines 0.500. C, event count and B ordered every pair identically; the study could not distinguish them. |
| Combined-signal study | 44 fresh files from Polly, MediatR and LiteDB; 18 neutral and four challenge pairs | Development winner B tied C on neutral pairs (0.6667 each) and lost on challenges (0.2500 versus 0.7500). KH's neutral 0.7222 was exploratory. |
| Large-repository validation | 40 fresh files from Roslyn and ASP.NET Core; 16 neutral and four challenge pairs | Preselected KH failed: neutral C 0.78125, KH 0.71875, lines 0.59375. C plus a syntax-size term tied C. |

The first matched study favored baseline-score disagreements and had reviewer
imbalance. B then won three deliberately selected formula-reversal comparisons,
but that small result did not survive fresh validation. Repeated nested blocks
also defeated B's intended compression. K addressed that structural example,
without establishing better review priorities.

In the large-repository validation, C had nine wins, zero losses and seven ties; KH had seven wins,
zero losses and nine ties. Its shortfall was reduced discrimination, not opposite
judgments. KH did better on reversal-enriched challenges (0.625 versus 0.375),
with opposite trends across repositories. Challenges cannot rescue a failed
neutral test.

Samples are small, tie-heavy and use coarse path roles. Reviewers shared a model,
and attention and difficulty ratings usually coincided. Roslyn also appeared in
the initial exploration: fresh files do not make an entirely unseen project.
No candidate has earned replacement of C.

## Low complexity does not mean low attention

Reviewed examples with whole-file C at most 3 involved reference acquisition and
release, concurrent initialization, waiter registration/completion and cancellation
handoff. These require preserving behavior despite having few branches. They are
review obligations, not demonstrated defects.

Six narrow probes investigated returned using-bound resources, explicit throws
of a catch binding, throws in finally, empty catches, a member write/await/read
sequence, and parameter callbacks under locks. In six matched pairs, flagged
regions won zero, lost four and tied two. Correct cleanup, intentional exception
handling and deliberate synchronization explain several benign matches.

These probes do not support an automatic score bonus. Two had no eligible hits;
restricted syntax coverage does not establish that the obligations are absent.
Controls also contained important obligations missed by every probe. The sample
was enriched from bounded retained locations and cannot estimate recall or defect
precision.

## Coverage and full-scan feasibility

Unscored files remain unknown, never zero. Important omissions included Dapper's
central `SqlMapper.cs`. In fmt, scored files represented only 3.2% of selected
bytes, leaving library implementation bodies outside the ranking. File-count
coverage alone can be misleading. Macros can hide executable behavior even in
successfully parsed files; these experiments did not add macro expansion.

The large-input C# comparison used pinned, unbuilt source and default selection
with native extensions excluded:

| Repository | Scored / selected C# files | Scored fraction | Two-worker inspector median |
| --- | ---: | ---: | ---: |
| Roslyn | 16,658 / 16,945 | 98.31% | 35.98 s |
| ASP.NET Core | 10,516 / 10,652 | 98.72% | 13.42 s |

All 24 repeated scans passed recorded score, coverage and within-tool determinism
checks: two repositories, two developer executables, one/two workers and three
repetitions. Every invocation analyzed the full selected input without a scan
cache or target build. Existing resource limits were unchanged.

Timings include developer JSON output on a shared two-CPU host with warm or
uncontrolled filesystem caches. They are historical observations, not cold-scan
guarantees or production CLI benchmarks. Exporters allocate differently, so their
memory figures do not isolate analyzer costs.

## Next experiment

Investigate **bounded local relationships as explanations**, keeping production
ranking unchanged until usefulness is demonstrated:

1. Define a small shared representation of bindings, reads/writes, branches,
   calls, cleanup and suspension boundaries, with locations and explicit unknowns.
   Language adapters must preserve evaluation and lifetime semantics; Tree-sitter
   alone does not provide type or effect resolution.
2. Start with callable-local dependency slicing and a small abstract state domain
   to track definitions, overwrites and branch merges. Bound nodes, edges and
   propagation steps; keep state worker-local and discard it after analysis.
3. Freeze concrete behavior-changing tasks, intended invariants and benign controls
   before evaluation. Use fresh cases and maintainer judgments to test whether
   explanations help preserve behavior within a fixed review budget.

Unsupported aliasing, unknown calls and exhausted limits must produce uncertainty.
More graph nodes or unknowns must not automatically raise attention scores.
Symbolic refinement and same-file function summaries are later possibilities,
conditional on simpler analysis proving useful. Cross-file coupling, mandatory
target builds, model downloads and persistent analysis state remain outside this
experiment.

## Evidence and methodology

Essential historical data is retained as machine-readable evidence:

- [Size-matched results](evidence/size-matched-review/results.json),
  [formula disagreements](evidence/size-matched-review/formula-disagreement-results.json)
  and [independent repeat ratings](evidence/size-matched-review/formula-disagreement-repeat-ratings.json).
- [Combined-signal results](evidence/combined-signal-review/results.json),
  [development selection](evidence/combined-signal-review/formula-selection.json)
  and [source controls](evidence/combined-signal-review/syntax-controls.json).
- [Large-repository results](evidence/large-repository-review/results.json),
  [low-complexity results](evidence/low-complexity-review/results.json)
  and [repeated scan measurements](evidence/large-repository-review/scan-benchmarks.json).
- [Evidence manifest](evidence/research-evidence-manifest.json): retained file hashes
  and source revisions. [Initial research](initial-research-results.md) remains as
  the earlier historical record.

Protocols and candidate choices preceded holdout ratings. Reviewers inspected
source without scores, pair identities or sampling strata. Later phases assigned
both pair members to the same reviewer and included overlap reviews. Neutral
files were selected independently of scores and paired by length within repository
and path role; reversal challenges were evaluated separately. Previously reviewed
files became development material, not new holdouts.

The large-repository review tool selects neutral files of 40–800 lines, up to eight per role
per repository, and four disjoint challenge pairs overall. KH must strictly beat
C and the size-augmented control CS on repository-macro neutral concordance,
without reversal in repository or role subgroups, and pass operational checks.
Passing would justify further validation, not automatic product promotion.
Low-complexity pairs match repository, path role, exact C, syntax family and a
span ratio at most two; eligibility is C <= 5 and span <= 40 lines.

This consolidation preserves selected ratings, scores and measurements, replacing
local paths with placeholders such as `<repo root>` and `<research root>`.
The evidence manifest hashes the sanitized files and retains their original hashes
where content changed. Hashes embedded in historical results still identify the
original observations. Duplicate reports, source snapshots and execution logs are
omitted; this is not the complete original archive. Original large raw reports and
source checkouts were absent from the supplied environment. Frozen ratings are
observations, not reproducible scanner outputs.

## Research tools

These developer-only tools do not change product scores or stdout:

| Tool | Purpose |
| --- | --- |
| `cognitive_ranking_experiment` | Compare C, event-only and breadth-compressed formulas through the existing pipeline |
| `syntax_signal_experiment` | Export twelve C# formulas and optional low-complexity evidence |
| `size_matched_review.py`, `combined_signal_review.py` | Evaluate size-matched and combined-signal comparisons |
| `large_repository_review.py`, `low_complexity_review.py` | Prepare blinded packets and evaluate file/region comparisons |
| `benchmark_syntax_signals.py`, `benchmark_large_repository_scans.py` | Repeated full-scan comparisons with parity checks; require Linux/WSL and `/usr/bin/time` |

Build and check with dependencies already available:

```sh
cargo fmt --check
cargo clippy --locked --offline --all-targets --all-features -- -D warnings
cargo test --locked --offline --all-targets
python3 -m unittest discover -s tools -p 'test_*.py'
cargo build --release --locked --offline --bin cpath --examples
target/release/examples/cognitive_ranking_experiment SOURCE --variant breadth --threads 2
target/release/examples/syntax_signal_experiment SOURCE 2 --lowc
```

The signal exporter is C#-only; other selected languages are explicitly unscored.
For C# comparisons on mixed repositories, add these exclusions:

```sh
--exclude '**/*.c' --exclude '**/*.h' --exclude '**/*.cpp'
```

Sampling scripts document their input layout and refuse to replace frozen outputs.
New large-repository studies fingerprint this consolidated document; historical results
retain their original protocol hashes. Full historical replay also requires the
omitted raw reports, packets and original protocols. Do not substitute newly
collected ratings for historical observations.

## Contribute

The most useful contributions are concrete maintenance tasks, intended behavior,
routine successful changes, low-complexity mistakes, and maintainer participation
in blinded review/change tasks. Share only material you are permitted to contribute.
For research participation or private feedback, contact
[contact@cpath.app](mailto:contact@cpath.app).
