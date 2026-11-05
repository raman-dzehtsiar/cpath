# Changelog

## 0.1.0

First release: offline code metrics and quality gates, with no setup or build required.

- Support for C, C++, C#, Go, Java, JavaScript, Python, Rust, TypeScript, and TSX
  on Linux, Windows, and macOS.
- Repository scans reporting cognitive complexity (CC), maintainability index
  (MI), and lines of code (LOC) for each supported source file.
- Quality gates for uncommitted changes, individual commits, or commit ranges,
  with configurable CC and MI limits and findings that identify affected functions.
- Metric selection and file filters to focus scans on relevant code.
- JSON output for automation, with optional readable formatting.
