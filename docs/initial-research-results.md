# Initial exploratory research results

These observations come from evaluations on 20–21 October 2026. Cognitive
complexity surfaced plausible review candidates, but the evaluations did not
establish that it prioritizes human attention better than file size. All review
assessments were made by coding agents, not human maintainers. No review-time
savings, defect prediction, or reduction in maintenance effort was demonstrated.

## Early observations

An initial combined-score experiment on FluentValidation elevated a simple
factory facade because of its many dependencies and repetitive tests because of
duplication. Ranking by cognitive complexity instead brought stateful validation
logic to the top. This motivated a simpler starting point; it did not establish
that duplication or dependencies are generally unhelpful signals.

Further source inspections in Jellyfin, PowerShell, OpenRA, Roslyn, and LLVM found
plausible review targets involving shared state, ordering, and interacting rules.
These assessments were not blinded to the ranking and did not demonstrate bugs.

## Score-blinded assessment

A protocol was fixed before three fresh-context coding agents assessed 30 files
from five C# repositories: FluentValidation, Jellyfin, OpenRA, PowerShell, and
Roslyn. Each repository contributed two files from its first 20 ranks, two from
the middle 40–60%, and two from the bottom 20%. Reviewers saw source and paths,
but were instructed not to inspect scores or ranks. Six files received a second
review, giving 36 assessments; the results below use the primary ratings.

Reviewers judged how much deliberate human review a file would need before a
meaningful behavior change. Ratings ranged from routine (0) to intensive (3);
ratings of 2 or 3 meant careful or intensive review.

| Sample range | Files judged to need careful or intensive review | Mean rating |
| --- | ---: | ---: |
| Top | 10 / 10 | 2.8 |
| Middle | 3 / 10 | 1.0 |
| Low | 0 / 10 | 0.5 |

The score–rating correlation was **0.875**. However, an exploratory comparison
added after review found a **0.893** correlation between source-line count and
the same ratings. The small difference does not establish that file size is
better; this experiment demonstrated no advantage for cognitive complexity.

The sample deliberately emphasized ranking extremes, and 11 files scored zero.
Only scored files were eligible. Reviewers shared the same configured model,
and two large files received partial inspection. Review difficulty and attention
priority received identical ratings, so the experiment did not distinguish them.
These results are not repository-wide accuracy or recall estimates.

## Counterexamples worth investigating

- **Simple control flow can hide review needs.** Three middle-range files scored
  only 1–5 but warranted careful review: PowerShell's results-comparison tool and
  Roslyn's document-cloning and process-telemetry code. Their concerns involved
  data-processing conventions, shared ownership, and lifecycle ordering.
- **Repetition can dominate a score.** LLVM's highest-ranked file was a large
  repetitive test matrix. Its counted complexity was real, but source inspection
  did not justify prioritizing it above compiler production logic.
- **Purpose changes priority.** Generated or adapted serializers, migration code,
  and largely independent dispatch cases need context before recommending action.
- **Incomplete coverage limits conclusions.** Important files were unscored in
  several repositories, particularly macro-heavy C/C++ projects. A ranking of
  scored files cannot establish priorities across the entire codebase.

## What these results support

Keep cognitive complexity as a provisional starting point. Next, compare it with
file size on similarly sized files, include low-complexity lifecycle and ownership
cases, and involve human maintainers in concrete review or change tasks. Evaluate
whether ranking improves useful findings within a fixed review time. These
exploratory samples should not be reused as independent validation of a formula
tuned to them. See the [research overview](research.md) for the broader direction.

## Evidence

- [Original score-blinded protocol](evidence/blind-validation-protocol.md) and
  [complete assessments and results](evidence/blind-validation-results.json).
- [Early C# repository observations](evidence/large-repository-results.json).
- [LLVM observations and counterexamples](evidence/cpp-large-repository-validation-results.json).

The evidence retains its original contents, repository revisions, and study
limitations. Historical technical measurements within it are not current product
benchmarks.
