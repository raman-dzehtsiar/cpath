# Score-blinded validation protocol v1

This protocol was written before receiving review ratings. It evaluates the
current ranking without changing its formula. This is an exploratory assessment
by coding agents, not a human-participant study or a defect-prediction benchmark.

## Selection and blinding

Use fresh full scans of the five previously pinned repositories: OpenRA,
PowerShell, Jellyfin, FluentValidation and Roslyn. Sample only scored files;
unavailable files remain a separate coverage limitation. For each repository,
select two files from each of these rank ranges:

- Top: first 20 files.
- Middle: zero-based indices floor(0.40N) through floor(0.60N), end exclusive.
- Low: zero-based indices floor(0.80N) through N, end exclusive.

Within each range sort SHA-256 of the fixed seed, repository, range and path;
take the first two. Seed: `cpath-blind-v1-20261020`. No size, production/test or
score-value exclusions; no substitutions after viewing a selected file. This
produces 30 files, equally split by repository and range. Ties in scores can
cross rank ranges; retain and disclose them. This deliberately stratified sample
does not represent repository-wide prevalence and cannot estimate recall.

`tools/blind_validation.py` generates a sealed score/rank key and shuffled review
packets. Three fresh-context agents receive 10 primary files each; two per packet
are selected deterministically for a second reviewer, giving six overlap files
and 36 assessments. Reviewers receive source paths but no scores, rank ranges,
score-selected method locations or previous assessment history. They must not
run cpath, read ranking reports or communicate ratings before submitting JSON.
They may inspect source and local dependencies/tests for context. Source content
and familiar project names cannot be blinded. The coordinator knows the key but
does not assign or alter the ratings. Blinding is procedural, not a filesystem
access-control boundary. Reviews are frozen before comparison with scores.

## Review question and rubric

For a meaningful behavior-changing edit in this file, how much deliberate human
review does its existing implementation warrant? Assess observed interactions,
state invariants, ordering, data transformations, hidden assumptions and clarity.
Do not count branches or reproduce the metric. Do not infer importance solely
from file length, component name or hypothetical failures. Consider the hardest
substantive behavior in the file, not the average method. Repetitive tests may be
mechanical or may encode subtle interactions: explain which and why.

Record separate ordinal ratings for **review difficulty** (understanding and
verifying local behavior) and **attention priority** (need for deliberate human
attention before a behavior change). Attention priority is the primary endpoint:

- 0: routine; declarative or trivial behavior, no observed interacting invariant.
- 1: focused local review; behavior is straightforward with localized assumptions.
- 2: careful review; interacting state, ordering or domain rules require tracing.
- 3: intensive review; multiple interacting invariants or nonlocal effects need
  substantial reasoning. Cite the actual source evidence.

These are advisory judgments, not defect labels, urgency estimates, review-time
measurements or permission for autonomous changes. Report confidence, file role,
at least one concrete source-line anchor, hardest behavior, counterevidence and
inspection scope. Partial inspection must be explicit; do not assert full-file
understanding from a narrow excerpt. Scores 2–3 count as a positive attention
judgment for the predefined binary summaries below.

## Analysis fixed before ratings

Use primary ratings for all headline results; secondary ratings only measure
agreement, without adjudication after score disclosure. Report:

- Attention-positive fraction and mean ordinal rating by range, with raw counts.
- Top-versus-low pairwise concordance within each repository: all four pairs per
  repository, 20 total; ties count one-half. Repeat for review difficulty.
- Spearman association of printed score and each primary rating, with tied ranks.
- Exact and within-one-point agreement on the six overlap files; list differences.
- Every sampled top file rated 0–1 (ranking disagreement/possible over-prioritizing)
  and middle/low file rated 2–3 (possible omitted attention priority), with evidence.

Use the term false positive only as shorthand for a disagreement with this
rubric, not proof that the algorithm is wrong or a file is safe. Do not treat the
small, clustered sample as an accuracy percentage for all repositories. Record
coverage, source hashes, repository pins, review artifacts and sampling method so
humans can reproduce or challenge judgments. Do not tune the score on this sample.
