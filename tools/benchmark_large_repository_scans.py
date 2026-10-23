# SPDX-License-Identifier: MIT
"""Benchmark full large-repository C# scans without persistent analysis state.

Usage: python3 tools/benchmark_large_repository_scans.py STUDY_ROOT NEW_OUTPUT_DIRECTORY
Requires final raw/{roslyn,aspnetcore}.json --lowc reports, {repo}-pin.json,
release examples inspect/syntax_signal_experiment, and /usr/bin/time. Runs 24 serial
fresh processes. Writes small timing files, incremental runs.jsonl and runtime.json;
captures scanner stdout in memory. Never deletes files or clears filesystem caches.
"""

import hashlib
import json
import os
import shutil
import statistics
import subprocess
import sys
from collections import Counter
from pathlib import Path

from large_repository_review import REPOSITORIES, SHARED_EXCLUDES, require

MIN_FREE_BYTES = 750 * 1024 * 1024
TOOLS = ("inspect", "syntax_signal_experiment")
BUILD_COMMANDS = frozenset(("cargo", "rustc", "cc1", "cc1plus", "gcc", "g++", "clang", "clang++", "ld", "lld"))


def file_sha(path):
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def digest(value):
    # Avoid a second complete serialized copy of large research reports.
    result = hashlib.sha256()
    for part in json.JSONEncoder(sort_keys=True, separators=(",", ":"), allow_nan=False).iterencode(value):
        result.update(part.encode())
    return result.hexdigest()


def semantic(report):
    return {key: value for key, value in report.items() if key not in ("seconds", "workers")}


def comparable(report, research):
    rows = report["files"] if research else report["ranking"]
    result = {row["path"]: ((row["baseline_max"], row["baseline_total"], row["scores"]["C"]["burden"])
                            if research else (row["complexity"], row["complexity_total"], row["complexity_burden"]))
              for row in rows}
    require(len(result) == len(rows), "Duplicate scored paths")
    count = report["analyzed_files"] if research else report["files"]
    require(count == len(result), "Reported analyzed count mismatch")
    if research:
        require(all(row["scores"]["C"]["maximum"] == row["baseline_max"] for row in rows),
                "Research baseline formula mismatch")
    return result


def coverage(report):
    return dict(unscored_files=report["unscored_files"], failure_counts=report["failure_counts"])


def expected_diagnostics(failures, research):
    # Product PathBuf compares components; research normalized String compares
    # the full path. Truncation can change membership, not just display order.
    key = (lambda row: row["path"]) if research else (lambda row: row["path"].split("/"))
    return [{"path": row["path"], "kind": row["kind"].capitalize()}
            for row in sorted(failures, key=key)[:32]]


def diagnostics(report):
    return [{key: row[key] for key in ("path", "kind")} for row in report["diagnostics"]]


def command(binary, source, workers, research):
    result = [str(binary), str(source), str(workers)]
    if research:
        result.append("--lowc")
    for pattern in SHARED_EXCLUDES:
        result.extend(("--exclude", pattern))
    return result


def clean_pin(source, expected):
    actual = subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"], text=True).strip()
    require(actual == expected, f"Pinned revision changed: {source}")
    status = subprocess.check_output(["git", "-C", str(source), "status", "--porcelain"], text=True)
    require(not status, f"Target source worktree is not clean: {source}")
    return dict(commit=actual, clean=True)


def preflight(output, sources, binaries, expected_binaries):
    free = {str(path): shutil.disk_usage(path).free for path in (output, *sources)}
    require(all(value >= MIN_FREE_BYTES for value in free.values()),
            f"Less than 750 MiB free before starting a new scan: {free}")
    processes = subprocess.check_output(["ps", "-eo", "comm="], text=True).splitlines()
    builds = sorted({name.strip() for name in processes} & BUILD_COMMANDS)
    require(not builds, f"Build processes would contaminate timing; wait before rerunning: {builds}")
    require({name: file_sha(path) for name, path in binaries.items()} == expected_binaries,
            "Scanner binary changed during benchmark")
    return free


def grouped(rows):
    groups = []
    for repository in REPOSITORIES:
        for tool in TOOLS:
            for workers in (1, 2):
                selected = [r for r in rows if (r["repository"], r["tool"], r["workers"]) == (repository, tool, workers)]
                require(len(selected) == 3, "Incomplete benchmark repetitions")
                item = dict(repository=repository, tool=tool, workers=workers, repetitions=len(selected))
                for metric in ("pipeline_seconds", "wall_seconds", "user_seconds", "system_seconds", "peak_rss_kib"):
                    values = [r[metric] for r in selected]
                    item[f"median_{metric}"] = statistics.median(values)
                    item[f"min_{metric}"] = min(values)
                    item[f"max_{metric}"] = max(values)
                groups.append(item)
    return groups


def main(root, output):
    output.mkdir(parents=True, exist_ok=False)
    project = Path(__file__).resolve().parents[1]
    binaries = {name: project / "target/release/examples" / name for name in TOOLS}
    inputs, reference, populations = {}, {}, {}
    for repository in REPOSITORIES:
        pins_path = root / f"{repository}-pin.json"
        pins = json.loads(pins_path.read_text())
        source = Path(pins["source_root"]).resolve()
        require(pins["repository"] == repository and source == (root / "repos" / repository).resolve(), "Pin root mismatch")
        status = clean_pin(source, pins["commit"])
        raw_path = root / "raw" / f"{repository}.json"
        report = json.loads(raw_path.read_bytes())
        require(report["schema"] == "cpath.research.signals.v1"
                and report["formula_family"] == "cpath.research.phase2-formulas.v1", "Frozen report schema mismatch")
        require(report.get("lowc_contract") == "cpath.research.lowc-obligations.v1"
                and report.get("evidence_formula") is None and report.get("evidence_selection"), "Frozen report is not --lowc")
        require(report["includes"] == [] and tuple(report["excludes"]) == SHARED_EXCLUDES, "Frozen scope mismatch")
        require(Path(report["root"]).resolve() == source, "Frozen report root mismatch")
        scores = comparable(report, True)
        failure_path = root / "raw" / f"{repository}-failures.jsonl"
        failures = [json.loads(line) for line in failure_path.read_text().splitlines()]
        failures = [row for row in failures if row["path"].endswith(".cs")]
        require(len({row["path"] for row in failures}) == len(failures), "Duplicate failure census path")
        require(coverage(report) == dict(unscored_files=len(failures),
                failure_counts=dict(Counter(row["kind"].capitalize() for row in failures))), "Failure census mismatch")
        require(set(scores).isdisjoint(row["path"] for row in failures), "Scored and unscored overlap")
        for name in TOOLS:
            reference[(repository, name, "diagnostics")] = expected_diagnostics(failures, name == "syntax_signal_experiment")
        require(diagnostics(report) == reference[(repository, "syntax_signal_experiment", "diagnostics")],
                "Frozen research diagnostics disagree with full census")
        reference[(repository, "parity")] = (digest(scores), digest(coverage(report)))
        reference[(repository, "syntax_signal_experiment")] = digest(semantic(report))
        populations[repository] = dict(commit=pins["commit"], source_root=str(source),
            analyzed_files=len(scores), analyzed_bytes=sum(row["bytes"] for row in report["files"]),
            analyzed_lines=sum(row["lines"] for row in report["files"]),
            unscored_files=report["unscored_files"], failure_counts=report["failure_counts"],
            selection=report["selection"], content_skips=report["content_skips"], initial_checkout=status)
        inputs[repository] = dict(source=source, commit=pins["commit"], pin_sha256=file_sha(pins_path),
                                  raw_sha256=file_sha(raw_path), failure_census_sha256=file_sha(failure_path),
                                  frozen_semantic_sha256=reference[(repository, "syntax_signal_experiment")])
        del report, scores
    source_paths = [project / "Cargo.toml", project / "Cargo.lock", project / "build.rs",
                    *sorted((project / "src").rglob("*.rs")), *sorted((project / "tools").glob("*.rs")),
                    Path(__file__).resolve(), project / "tools/large_repository_review.py", project / "tools/combined_signal_review.py"]
    source_paths = [path for path in source_paths if path.exists()]
    provenance = dict(binary_sha256={name: file_sha(path) for name, path in binaries.items()},
        source_sha256={str(path.relative_to(project)): file_sha(path) for path in source_paths},
        rustc=subprocess.check_output(["rustc", "--version"], text=True).strip(),
        host_cpu_count=os.cpu_count(), minimum_free_bytes=MIN_FREE_BYTES,
        inputs={repository: {k: str(v) if isinstance(v, Path) else v for k, v in values.items()}
                for repository, values in inputs.items()}, includes=[], excludes=SHARED_EXCLUDES,
        research_mode="--lowc", process_stdout="captured in memory, not retained as per-run report files")
    with (output / "provenance.json").open("x") as handle:
        json.dump(provenance, handle, indent=2)
        handle.write("\n")
    rows = []
    with (output / "runs.jsonl").open("x") as journal:
        for round_index in range(3):
            repositories = REPOSITORIES if round_index % 2 == 0 else REPOSITORIES[::-1]
            for repository in repositories:
                source = inputs[repository]["source"]
                clean_pin(source, inputs[repository]["commit"])
                require(file_sha(root / "raw" / f"{repository}.json") == inputs[repository]["raw_sha256"], "Frozen raw report changed")
                for workers in ((1, 2) if round_index % 2 == 0 else (2, 1)):
                    names = TOOLS if (round_index + workers) % 2 == 0 else TOOLS[::-1]
                    for name in names:
                        free = preflight(output, [v["source"] for v in inputs.values()], binaries, provenance["binary_sha256"])
                        run_id = f"{repository}-{name}-w{workers}-r{round_index + 1}"
                        timing = output / f"{run_id}.time"
                        args = command(binaries[name], source, workers, name == "syntax_signal_experiment")
                        invocation = ["/usr/bin/time", "-f", "%e %U %S %M", "-o", str(timing), *args]
                        print(f"starting {run_id}", flush=True)
                        proc = subprocess.run(invocation, capture_output=True, check=False)
                        if proc.stderr:
                            with (output / f"{run_id}.stderr").open("xb") as handle:
                                handle.write(proc.stderr)
                        if proc.returncode != 0 or proc.stderr:
                            journal.write(json.dumps(dict(run_id=run_id, command=invocation, returncode=proc.returncode,
                                stderr_bytes=len(proc.stderr), status="process_failed")) + "\n")
                            journal.flush()
                            raise RuntimeError(f"Scanner failed or wrote diagnostics: {run_id}; see preserved timing/stderr")
                        stdout_bytes = len(proc.stdout)
                        report = json.loads(proc.stdout)
                        del proc
                        wall, user, system, rss = timing.read_text().split()
                        score_hash = digest(comparable(report, name == "syntax_signal_experiment"))
                        coverage_hash = digest(coverage(report))
                        semantic_hash = digest(semantic(report))
                        reference.setdefault((repository, name), semantic_hash)
                        checks = dict(baseline_and_coverage=(score_hash, coverage_hash) == reference[(repository, "parity")],
                                      diagnostics_match_census=diagnostics(report) == reference[(repository, name, "diagnostics")],
                                      semantic_determinism=semantic_hash == reference[(repository, name)])
                        row = dict(run_id=run_id, repository=repository, tool=name, workers=workers, round=round_index + 1,
                                   command=invocation, scan_arguments=args, wall_seconds=float(wall), user_seconds=float(user),
                                   system_seconds=float(system), peak_rss_kib=int(rss),
                                   pipeline_seconds=report["seconds"]["total"], stdout_bytes=stdout_bytes,
                                   semantic_sha256=semantic_hash, baseline_sha256=score_hash, coverage_sha256=coverage_hash,
                                   free_bytes_before=free, checks=checks)
                        journal.write(json.dumps(row) + "\n")
                        journal.flush()
                        require(all(checks.values()), f"Parity or complete semantic determinism failure: {run_id}")
                        rows.append(row)
                        del report
                        print(f"passed {run_id}: wall={wall}s peak_rss={rss}KiB", flush=True)
    require({name: file_sha(path) for name, path in binaries.items()} == provenance["binary_sha256"], "Binary changed")
    require({str(path.relative_to(project)): file_sha(path) for path in source_paths} == provenance["source_sha256"],
            "Benchmark/scanner source changed during measurements")
    for repository, values in inputs.items():
        require(file_sha(root / "raw" / f"{repository}-failures.jsonl") == values["failure_census_sha256"],
                "Failure census changed")
        populations[repository]["final_checkout"] = clean_pin(values["source"], values["commit"])
    result = dict(provenance=provenance, populations=populations, groups=grouped(rows), runs=rows,
                  all_checks_passed=True, limitations=[
                      "Serial fresh-process full scans; filesystem caches are warm/uncontrolled and never cleared.",
                      "Shared host, three repetitions; no timing significance or isolated-hardware claim.",
                      "Pipeline totals include discovery, read, parse, analysis and aggregation, not JSON serialization.",
                      "Process wall time includes different JSON output volumes; observer parsing/hashing occurs between timed processes.",
                      "Research includes all twelve formulas plus low-C extraction and evidence, not a minimal candidate implementation.",
                      "Bounded diagnostics are verified against one complete census using each tool's documented path comparator; cross-tool truncated membership can differ.",
                      "Scope excludes three native suffixes; separate full-default product scans establish whole-repository coverage."])
    with (output / "runtime.json").open("x") as handle:
        json.dump(result, handle, indent=2)
        handle.write("\n")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    main(Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve())
