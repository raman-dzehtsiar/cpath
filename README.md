# Fast, offline code metrics and quality gates for multiple languages
[![Quality gate status](https://sonarcloud.io/api/project_badges/measure?project=raman-dzehtsiar-cpath&metric=alert_status)](https://sonarcloud.io/summary/new_code?id=raman-dzehtsiar-cpath)
[![Coverage](https://sonarcloud.io/api/project_badges/measure?project=raman-dzehtsiar-cpath&metric=coverage)](https://sonarcloud.io/summary/new_code?id=raman-dzehtsiar-cpath)

cpath makes code metrics accessible without requiring anything but your source code.
It is fast, free, and fully offline. It requires no setup, you use a single binary executable.
Point the binary at local source code to measure it. Scanning requires no mandatory configuration, 
language SDK or build procedure.

#### Supported languages
- C
- C++
- C#
- Go
- Java
- JavaScript
- Python
- Rust
- TypeScript and TSX

#### Supported platforms
- Linux
- Windows
- macOS

#### Supported metrics
- CC - cognitive complexity
- MI - maintainability index
- LOC - nonblank lines containing code, excluding comment-only lines

## CI gate

Check changes without checking out another revision:

```console
cpath gate [<commit> | <from> <to>] --pretty
```

With no commits, compare `HEAD` with uncommitted changes. With one commit, compare
its first parent with that commit. With two commits, compare those snapshots.
Both committed comparisons ignore uncommitted changes. Working-tree comparisons
include staged, unstaged, and non-ignored untracked files, using their current
contents on disk.

Example output (abbreviated):

```json
{
    "status": "failed",
    "violations": [
        {
            "file": "src/parser.cpp",
            "line": 142,
            "scope": "parseExpression",
            "classification": "existing",
            "metric": "CC",
            "expected": { "operator": "<=", "value": 28 },
            "actual": 31,
            "previous": 28
        }
    ],
    "exit_code": 1
}
```

The gate checks individual functions and methods. Existing scopes may change
within the CC/MI limits; scopes already outside the limits must not worsen.
Possible return codes:

- 0 - no confirmed metric violations, possibly with incomplete analysis
- 1 - one or more metric violations were found
- 2 - command-line arguments were invalid or repository setup failed

See [gate behavior, output, and CI usage](docs/gate.md).

## Full repository scan

Calculate all metrics for the current repository and print formatted JSON:

```console
cpath . --metrics=all --pretty
```

Example output:

```json
{
    "cpath_version": "0.1.0",
    "repository_root": "/path/to/my-project",
    "scanned_files": [
        {
            "path": "src/main.rs",
            "CC": 3,
            "MI": 82,
            "LOC": 120
        }
    ],
    "exit_code": 0
}
```

Possible return codes:
- 0 - scan completed
- 1 - a fatal error prevented the scan
- 2 - command-line arguments were invalid
- 3 - the scan completed with one or more unscored files 

Scans report CC, MI, and LOC by default. Use `--metrics=LOC` or a comma-separated
list such as `--metrics=CC,MI` to select metrics. CC and LOC are unsigned integers;
MI is rounded to the nearest unsigned integer, with half values rounded up.
An unavailable MI score remains `null`. Rounding does not change the underlying
metric calculations. Quality gates evaluate only CC and MI.

LOC counts physical lines containing code. Blank and comment-only lines are
excluded; lines containing both code and comments count once. Comment markers
inside strings are code. Nonblank lines inside multiline strings count, including
Python docstrings. A final line without a newline also counts.

## Limitations
Initially, cpath was created as an assisting tool for a research project studying how code metrics affect
different properties of AI coding agents. The current design, functional scope, and CLI interface are defined
by those needs. However, cpath can also be useful for many other practical applications.

#### Approximation
Values reported by cpath may differ slightly from values reported by baseline language-specific tools.
In most cases, the difference is close to zero and rarely reaches 1 point. For most supported
languages and metrics, cpath matches baseline tools 100% of the time, with no differences; many of the
remaining comparisons have 99.9% accuracy. You can find more details in [the benchmark notes](docs/benchmark.md).

#### Conditional compilation in C and C++
Although most C and C++ syntax is covered, some rare, highly specific conditional-compilation
statements are not supported. If your code contains such statements, those files will be
skipped explicitly with a clear reason in the report, as in the usage example.


## Preferred communication channels
To ask a question, request a change or a new feature, propose an idea, or report a bug,
please open a GitHub issue in this project. For anything related to the initial research
mentioned above, please send an email to [contact@cpath.app](mailto:contact@cpath.app).

## License
cpath’s original code is licensed under the MIT license. See [LICENSE.txt](LICENSE.txt).
Third-party components retain their respective copyrights and licenses.

