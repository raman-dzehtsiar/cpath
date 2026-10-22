# Copyright (c) 2026 Raman Dzehtsiar
# SPDX-License-Identifier: MIT
"""Measure full cpath CLI scans on Linux/WSL; does not build or modify the target.

Usage: python3 tools/benchmark.py BINARY REPOSITORY OUTPUT_DIRECTORY
Requires /usr/bin/time for peak RSS. Run after building the release binary.
All measurements and stdout samples are written to the requested directory.
CPATH_BENCHMARK_ROUNDS defaults to 7; CPATH_BENCHMARK_WORKERS defaults to 1,4,8.
Exit 3 is accepted only with the documented incomplete-scan warning. Rankings,
diagnostics, and exit status must be identical for every measured invocation.
Completed measurements are checkpointed to runs.json before Git verification.
Git metadata checks time out after 30 seconds; failures remain explicit in the
final report instead of discarding measurements or claiming a clean worktree.
"""

import hashlib
import json
import os
import platform
import statistics
import subprocess
import sys
import time
from pathlib import Path


def configuration():
    rounds_text = os.environ.get("CPATH_BENCHMARK_ROUNDS", "7")
    workers_text = os.environ.get("CPATH_BENCHMARK_WORKERS", "1,4,8")
    try:
        rounds = int(rounds_text)
        workers = tuple(int(value.strip()) for value in workers_text.split(","))
    except ValueError as error:
        raise SystemExit("benchmark rounds and comma-separated workers must be integers") from error
    if rounds < 1:
        raise SystemExit("CPATH_BENCHMARK_ROUNDS must be positive")
    if not workers or any(value < 1 or value > 64 for value in workers):
        raise SystemExit("CPATH_BENCHMARK_WORKERS must contain worker counts from 1 to 64")
    if len(set(workers)) != len(workers):
        raise SystemExit("CPATH_BENCHMARK_WORKERS must not repeat worker counts")
    return rounds, workers


def accepted_result(result):
    if result.returncode == 0:
        return not result.stderr
    return result.returncode == 3 and result.stderr.startswith(
        b"cpath: warning: incomplete scan:"
    )


def git_metadata(repository):
    sha = None
    status = None
    error = None
    operation = "rev-parse HEAD"
    environment = dict(os.environ, GIT_OPTIONAL_LOCKS="0")
    try:
        sha = subprocess.check_output(
            ["git", "-C", str(repository), "rev-parse", "HEAD"],
            text=True, stderr=subprocess.PIPE, timeout=30, env=environment,
        ).strip()
        operation = "status --porcelain"
        status = subprocess.check_output(
            ["git", "-C", str(repository), "status", "--porcelain"],
            text=True, stderr=subprocess.PIPE, timeout=30, env=environment,
        )
    except subprocess.TimeoutExpired:
        error = f"git {operation} timed out after 30 seconds; target verification incomplete"
    except subprocess.CalledProcessError as failure:
        detail = (failure.stderr or "").strip().replace("\n", " ")[:300]
        error = f"git {operation} failed with exit status {failure.returncode}: {detail}"
    except OSError as failure:
        error = f"git {operation} unavailable: {failure}"
    return dict(commit=sha, target_worktree_status=status, target_verification_error=error)


def main():
    if len(sys.argv) != 4:
        raise SystemExit(__doc__)
    binary, repository, output = [Path(arg).resolve() for arg in sys.argv[1:]]
    if not binary.is_file() or not repository.is_dir():
        raise SystemExit("binary and repository must exist")
    if not Path("/usr/bin/time").is_file():
        raise SystemExit("/usr/bin/time is required (run this script on Linux/WSL)")
    rounds, worker_counts = configuration()
    binary_digest = hashlib.sha256(binary.read_bytes()).hexdigest()
    output.mkdir(parents=True, exist_ok=True)
    baseline = None
    runs = []
    # Alternate worker counts to reduce systematic ordering bias. Every run is
    # a new process, with no cpath cache. OS filesystem caching is uncontrolled.
    for repetition in range(rounds):
        for workers in worker_counts:
            timing = output / f"time-{workers}-{repetition}.txt"
            command = [str(binary), str(repository), "--top", "10", "--threads", str(workers)]
            start = time.perf_counter()
            result = subprocess.run(
                # GNU time otherwise adds a nonzero-status sentence to its
                # timing file for the supported partial-scan exit status 3.
                ["/usr/bin/time", "--quiet", "-f", "%e %U %S %M", "-o", str(timing), *command],
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False,
            )
            elapsed = time.perf_counter() - start
            (output / f"stdout-{workers}-{repetition}.txt").write_bytes(result.stdout)
            (output / f"stderr-{workers}-{repetition}.txt").write_bytes(result.stderr)
            if not accepted_result(result):
                raise SystemExit(f"scan failed ({result.returncode}): {result.stderr.decode(errors='replace')}")
            digest = hashlib.sha256(result.stdout).hexdigest()
            stderr_digest = hashlib.sha256(result.stderr).hexdigest()
            observed = (result.returncode, digest, stderr_digest)
            if baseline is None:
                baseline = observed
                (output / "top10.txt").write_bytes(result.stdout)
                (output / "stderr.txt").write_bytes(result.stderr)
            if observed != baseline:
                raise SystemExit("stdout, stderr, or exit status changed between worker counts or repeated full scans")
            wall, user, system, rss = timing.read_text().strip().split()
            runs.append(dict(repetition=repetition, workers=workers,
                             elapsed_seconds=elapsed, time_wall_seconds=float(wall),
                             user_seconds=float(user), system_seconds=float(system),
                             max_rss_kib=int(rss), stdout_sha256=digest,
                             stderr_sha256=stderr_digest, exit_status=result.returncode,
                             stderr=result.stderr.decode(errors="replace")))
    summary = {}
    for workers in worker_counts:
        selected = [run for run in runs if run["workers"] == workers]
        values = [run["elapsed_seconds"] for run in selected]
        summary[str(workers)] = dict(median_seconds=statistics.median(values),
                                    min_seconds=min(values), max_seconds=max(values),
                                    max_rss_kib=max(run["max_rss_kib"] for run in selected))
    report = dict(repository=str(repository),
                  binary=str(binary), binary_sha256=binary_digest,
                  platform=platform.platform(), logical_cpus=os.cpu_count(),
                  python_version=platform.python_version(), rounds=rounds,
                  worker_counts=worker_counts, scan_exit_status=baseline[0],
                  scan_stderr=(output / "stderr.txt").read_text(encoding="utf-8", errors="replace"),
                  filesystem_cache="uncontrolled; repeated warm OS-cache scans, no cpath cache",
                  runs=runs, summary=summary)
    # Slow or interrupted Git checks must not lose completed scan measurements.
    (output / "runs.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    if hashlib.sha256(binary.read_bytes()).hexdigest() != binary_digest:
        raise SystemExit("benchmark binary changed while measurements were running")
    report.update(git_metadata(repository))
    (output / "benchmark.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    if report["target_verification_error"]:
        print(f"benchmark: warning: {report['target_verification_error']}", file=sys.stderr)
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
