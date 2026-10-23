# SPDX-License-Identifier: MIT
"""Repeat full production/research scans with one and two workers.

Usage: python3 tools/benchmark_syntax_signals.py STUDY_ROOT NEW_OUTPUT_DIRECTORY
Requires release examples inspect and syntax_signal_experiment and /usr/bin/time.
Writes explicit experiment reports, never scanner state. Does not clear caches.
"""

import hashlib
import json
import statistics
import subprocess
import sys
from pathlib import Path


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True).encode()).hexdigest()


def comparable(report, research):
    if research:
        return {r["path"]: (r["baseline_max"], r["baseline_total"], r["scores"]["C"]["burden"])
                for r in report["files"]}
    return {r["path"]: (r["complexity"], r["complexity_total"], r["complexity_burden"])
            for r in report["ranking"]}


def main(root, output):
    output.mkdir(parents=True, exist_ok=False)
    repository = Path(__file__).resolve().parents[1]
    manifest = json.loads((root / "scan-input-manifest.json").read_text())
    provenance = dict(
        input_manifest_sha256=hashlib.sha256((root / "scan-input-manifest.json").read_bytes()).hexdigest(),
        binary_sha256={name: hashlib.sha256((repository / "target/release/examples" / name).read_bytes()).hexdigest()
                       for name in ("inspect", "syntax_signal_experiment")},
        source_sha256={name: hashlib.sha256((repository / name).read_bytes()).hexdigest()
                       for name in ("Cargo.lock", "src/language.rs", "tools/syntax_signal_experiment.rs",
                                    "tools/syntax_signals.rs", "tools/benchmark_syntax_signals.py")},
        rustc=subprocess.check_output(["rustc", "--version"], text=True).strip())
    rows, reference, populations = [], {}, {}
    # Serial execution avoids self-contention; alternate worker/tool order.
    for round_index in range(3):
        for repo, inputs in manifest.items():
            source = Path(inputs["source_root"])
            pinned = subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"], text=True).strip()
            assert pinned == inputs["commit"]
            assert not subprocess.check_output(["git", "-C", str(source), "status", "--porcelain"], text=True)
            for workers in ((1, 2) if round_index % 2 == 0 else (2, 1)):
                names = ("inspect", "syntax_signal_experiment")
                if (round_index + workers) % 2:
                    names = names[::-1]
                for name in names:
                    run_id = f"{repo}-{name}-w{workers}-r{round_index + 1}"
                    timing = output / f"{run_id}.time"
                    proc = subprocess.run(["/usr/bin/time", "-f", "%e %U %S %M", "-o", str(timing),
                                           str(repository / "target/release/examples" / name),
                                           str(source), str(workers)], capture_output=True, check=True)
                    assert not proc.stderr, proc.stderr.decode()
                    report = json.loads(proc.stdout)
                    wall, user, system, rss = timing.read_text().split()
                    scores = comparable(report, name == "syntax_signal_experiment")
                    score_hash = digest(scores)
                    coverage = (report["unscored_files"], report["failure_counts"])
                    key = (repo, "parity")
                    reference.setdefault(key, (score_hash, coverage))
                    assert reference[key] == (score_hash, coverage), run_id
                    semantic = {k: v for k, v in report.items() if k not in ("seconds", "workers")}
                    semantic_hash = digest(semantic)
                    key = (repo, name)
                    reference.setdefault(key, semantic_hash)
                    assert reference[key] == semantic_hash, run_id
                    populations.setdefault(repo, dict(commit=pinned, analyzed_files=len(scores),
                        analyzed_bytes=sum(r["bytes"] for r in (report["files"] if name == "syntax_signal_experiment" else report["ranking"])),
                        unscored_files=coverage[0], failure_counts=coverage[1]))
                    row = dict(repository=repo, tool=name, workers=workers, round=round_index + 1,
                               wall_seconds=float(wall), user_seconds=float(user), system_seconds=float(system),
                               peak_rss_kib=int(rss), pipeline_seconds=report["seconds"]["total"],
                               stdout_bytes=len(proc.stdout), semantic_sha256=semantic_hash,
                               baseline_sha256=score_hash)
                    rows.append(row)
                    with (output / "runs.jsonl").open("a") as handle:
                        handle.write(json.dumps(row) + "\n")
            print(f"round {round_index + 1}: {repo} parity and determinism passed", flush=True)
    groups = []
    for repo in manifest:
        for name in ("inspect", "syntax_signal_experiment"):
            for workers in (1, 2):
                group = [r for r in rows if (r["repository"], r["tool"], r["workers"]) == (repo, name, workers)]
                groups.append(dict(repository=repo, tool=name, workers=workers, repetitions=len(group),
                    median_pipeline_seconds=statistics.median(r["pipeline_seconds"] for r in group),
                    median_wall_seconds=statistics.median(r["wall_seconds"] for r in group),
                    min_pipeline_seconds=min(r["pipeline_seconds"] for r in group),
                    max_pipeline_seconds=max(r["pipeline_seconds"] for r in group),
                    max_peak_rss_kib=max(r["peak_rss_kib"] for r in group)))
    result = dict(provenance=provenance, populations=populations, groups=groups, runs=rows,
                  limitations=["Warm filesystem cache; full scans with no analysis reuse.",
                               "Shared two-CPU host; three repetitions, no timing significance claim.",
                               "Pipeline totals include discovery, read, parse, analysis and aggregation.",
                               "Process wall time includes differing JSON serialization/output volumes.",
                               "Research computes all twelve formulas and additional evidence; not a minimal implementation."])
    with (output / "runtime.json").open("x") as handle:
        json.dump(result, handle, indent=2)
        handle.write("\n")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    main(Path(sys.argv[1]), Path(sys.argv[2]))
