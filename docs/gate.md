# CI quality gate

Run inside a Git working tree, including from a subdirectory:

```console
cpath gate [<from> [<to>]] [--cc-limit VALUE] [--mi-limit VALUE] [--pretty]
```

| Command | Comparison |
| --- | --- |
| `cpath gate` | `HEAD` against uncommitted changes |
| `cpath gate <from>` | The specified commit against uncommitted changes |
| `cpath gate <from> <to>` | Two specified commits; uncommitted changes are ignored |

Working-tree mode includes staged and unstaged changes plus non-ignored untracked
files. It reads current file contents, including edits made after staging. Deleted
files introduce no violations. It does not modify HEAD, the index, or source files.
Use two explicit commits in CI when you want committed changes only.

Committed source is read directly from Git. For working-tree comparisons, JSON
reports `"to": "WORKTREE"`; otherwise `to` contains the resolved target commit ID.
The baseline must resolve to a commit, so a repository without any commits returns
a setup error. Read/parse failures retain the existing nonblocking partial-analysis policy.

Gate JSON uses `cpath_version` to track output format changes, with no separate schema version.

## Rules

For each matched existing scope, the CC maximum is `max(previous CC, CC limit)`
and the MI minimum is `min(previous MI, MI limit)`. Healthy scopes may change
within the limits. Scopes already outside the limits may stay unchanged or improve,
but cannot worsen. Improvements in other scopes do not offset failures.
The two metrics are evaluated independently.

New scopes must have CC at most 25 for C/C++ and headers, or 15 for other
supported languages, and MI at least 20. These defaults use
[Sonar's CC thresholds](https://community.sonarsource.com/t/webinar-refactoring-with-cognitive-complexity/45331/2)
and the green boundary in
[Microsoft's MI groups](https://learn.microsoft.com/en-us/visualstudio/code-quality/code-metrics-maintainability-index-range-and-meaning?view=visualstudio).

