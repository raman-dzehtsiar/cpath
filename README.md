# Find the code that needs human attention
[![Quality gate status](https://sonarcloud.io/api/project_badges/measure?project=raman-dzehtsiar-cpath&metric=alert_status)](https://sonarcloud.io/summary/new_code?id=raman-dzehtsiar-cpath)
[![Coverage](https://sonarcloud.io/api/project_badges/measure?project=raman-dzehtsiar-cpath&metric=coverage)](https://sonarcloud.io/summary/new_code?id=raman-dzehtsiar-cpath)

**cpath** is an experimental preview of a local codebase intelligence tool for
identifying code that may need human attention. Its goal is to help prioritize
human review as coding agents and autonomous workflows produce more code.
It is an early implementation, not a production-ready release.

The project is built around four commitments:

- **Always free**
- **Fully offline**
- **Your code stays yours**
- **Single binary, zero setup**

Point the binary at a local codebase and get a ranked list of files to review.
No accounts, API keys, telemetry, hosted services, model downloads, or mandatory
configuration. Scanning requires no language SDK, package restore, or
target-project build. 

## Project goal

We are researching and defining an **attention score**: a combined metric intended
to help teams decide where human review is critical in the era of coding agents
and autonomous workflows. The aim is to make the best use of human time by helping
people prioritize the code that needs their judgment most.

For now, scoring focuses on **cognitive complexity**. As our research progresses, we plan to 
combine more signals and account for different codebase conditions. Our ultimate goal is to help 
detect unwanted architectural drift and identify the code that demands the most maintenance
effort. These are research goals, not capabilities established by the current
preview.

More details can be found on the [Research Page](docs/research.md).

If you would like to contribute to the project by participating in research or
have private questions or feedback, email [contact@cpath.app](mailto:contact@cpath.app).

## Language support

- **C#:** supported in preview.
- **C and C++:** experimental support.

Support for more languages is planned.

## Usage

Scan a codebase and show the 10 highest-ranked files:

```text
cpath /path/to/repo --top 10
```

Example output (illustrative):

```text
42 /path/to/repo/src/OrderProcessor.cs
27 /path/to/repo/src/PaymentService.cs
18 /path/to/repo/src/ReportBuilder.cs
```

Each line shows the highest cognitive complexity of any function in the file and
the file path. Higher scores suggest candidates for review; they do not establish
how urgently a file needs attention.

There are no headers, progress messages, summaries, or explanations in scan stdout.
Coverage warnings and errors go to stderr:

| Exit status | Meaning |
| --- | --- |
| `0` | Complete scan of selected input, including an empty selection. |
| `1` | Fatal scan failure; no ranking is printed. Output-write failures also use `1`. |
| `2` | Invalid command-line arguments. |
| `3` | Partial scan: stdout ranks analyzed files only; stderr reports incomplete coverage. |

Selected files that are unreadable, oversized, unparseable, binary, or in an
unsupported encoding are unscored, never assigned zero scores. Other files can still be ranked.
Partial scans report analyzed/unscored counts, up to 32 deterministically sorted
file diagnostics, and an omitted-diagnostic count. Check the exit status before treating
the output as complete coverage. Discovery, configuration, worker, and retained-summary
budget failures prevent a usable scan result.

## Development

This is an early implementation, not a production release. Building requires Rust
and a native C compiler:

```text
cargo build --release
target/release/cpath /path/to/repo --top 10
cargo fmt --check
cargo clippy --locked --offline --all-targets --all-features -- -D warnings
cargo test --locked --offline
```

## License

cpath’s original code is licensed under the MIT license. See [LICENSE.txt](LICENSE.txt).
Third-party components retain their respective copyrights and licenses.

