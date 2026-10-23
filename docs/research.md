# Research

We are looking for the best way to define an **attention score**: a metric that
helps developers and AI agents prioritize review and improvement efforts to keep
codebases maintainable.
The current preview uses cognitive complexity alone; a combined
score and architectural-drift detection remain research goals.

## Why start with cognitive complexity?

Our [initial research results](initial-research-results.md) give us practical
reasons to start here. Cognitive-complexity ranking surfaced plausible review
targets involving shared state, ordering, and interacting rules. In the first
FluentValidation experiment, it brought stateful validation logic ahead of simple
factory methods and repetitive tests favored by the earlier combined score.

In a score-blinded assessment, coding agents judged all ten sampled top-ranked
files to need careful or intensive review. This supports further investigation,
but file size showed a similar association with their judgments, and human
maintainers have not yet validated the ranking.

See the [latest attention-ranking research](attention-ranking-results.md) for
comparisons with additional signals and low-complexity review cases.

Scientific studies also suggest cognitive complexity can help identify code that
is difficult to understand, while showing limits to its predictive value:

- Muñoz Barón, Wyrich, and Wagner (2020),
  [An Empirical Validation of Cognitive Complexity as a Measure of Source Code Understandability](https://arxiv.org/abs/2007.12520):
  associations with comprehension time and perceived understandability.
- Esposito et al. (2024 preprint),
  [Early Career Developers' Perceptions of Code Understandability](https://arxiv.org/abs/2303.07722v2):
  modest predictive value for perceived understandability, but not problem severity.
- Lavazza et al. (2023),
  [An Empirical Evaluation of the “Cognitive Complexity” Measure as a Predictor of Code Understandability](https://doi.org/10.1016/j.jss.2022.111561):
  no substantial improvement over traditional measures.

## Starting points

Our next step is a focused maintainer study before developing a broader attention score.

- Evaluate cpath's current cognitive-complexity ranking as the initial approach.
- Keep the research focused on C# projects to keep the scope manageable.
- Ask those projects' maintainers which files need attention first and why,
  before showing them cpath's scores. Compare their assessments with cpath's ranking.

## Future steps

- Define what counts as a useful review finding and build a benchmark from
  maintenance cases and maintainer assessments.
- Compare the current ranking with simple alternatives, such as file size,
  on repositories not used to develop the formula.
- Evaluate additional signals and include those that improve the ranking.
- Test whether the resulting ranking helps developers find more consequential
  problems within a fixed review time, including in agent-assisted workflows.
- Publish the formula, evaluation results, and limitations.

## Contribute

The most useful contributions are:

- **Maintenance cases:** source before a problem, the intended task, and evidence
  of mistakes, rework, or the eventual fix.
- **Comparison cases:** routine successful changes, complex code that is easy
  to maintain, and simple code that causes trouble.
- **Maintainer participation:** assess file priorities and take part in review
  or change tasks, including tasks involving agent-generated code.
- **Research support:** run local team trials or independently reproduce the
  published evaluations.

Share only material you are permitted to contribute. To participate or discuss
private research, contact [contact@cpath.app](mailto:contact@cpath.app).
