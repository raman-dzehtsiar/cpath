# Copyright (c) 2026 Raman Dzehtsiar
# SPDX-License-Identifier: MIT
"""Prepare a reproducible score-blinded source review sample.

python tools/blind_validation.py RANKINGS_DIRECTORY REPOSITORIES_DIRECTORY OUTPUT_DIRECTORY
Inputs are complete JSON reports from the developer inspect example. Repository
directories must be named NAME-cpath-poc. Outputs are evaluation artifacts only;
the scanner never reads them. Do not give sample-key.json to blind reviewers.
"""

import hashlib
import json
import sys
from pathlib import Path

SEED = "cpath-blind-v1-20261020"
REPOSITORIES = ("openra", "powershell", "jellyfin", "fluentvalidation", "roslyn")


def key(value):
    return hashlib.sha256((SEED + "|" + value).encode()).hexdigest()


def write(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8", newline="\n")


def prepare(rankings, repositories, output):
    output.mkdir(parents=True, exist_ok=False)
    samples = []
    populations = {}
    for name in REPOSITORIES:
        report = json.loads((rankings / f"{name}.json").read_text(encoding="utf-8"))
        rows = report["ranking"]
        n = len(rows)
        assert n >= 100, "This protocol needs at least 100 scored files per repository"
        bands = {"top": (0, min(20, n)), "middle": (n * 40 // 100, n * 60 // 100),
                 "low": (n * 80 // 100, n)}
        populations[name] = {"analyzed": n, "unscored": (report["unscored_files"] if "unscored_files" in report else report["unavailable_files"]),
                             "bands_zero_based_half_open": bands}
        for band, (start, end) in bands.items():
            choices = sorted(range(start, end), key=lambda i: key(f"{name}|{band}|{rows[i]['path']}"))[:2]
            for index in choices:
                row = rows[index]
                path = repositories / f"{name}-cpath-poc" / row["path"]
                identity = name + "|" + row["path"]
                samples.append({"id": "F" + key(identity)[:8], "repository": name,
                                "path": row["path"], "stratum": band, "rank": index + 1,
                                "score": row["score"], "burden": row["complexity_burden"],
                                "source_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                                "source_path": str(path.resolve())})
    assert len({s["id"] for s in samples}) == 30
    samples.sort(key=lambda s: key("shuffle|" + s["id"]))
    packets = {name: [] for name in ("a", "b", "c")}
    for index, sample in enumerate(samples):
        sample["primary_reviewer"] = "abc"[index % 3]
        packets[sample["primary_reviewer"]].append(sample)
    # Two independently selected repeats per primary reviewer. Their destination
    # differs from the primary reviewer, producing 12 files per packet.
    for reviewer in "abc":
        repeats = sorted([s for s in samples if s["primary_reviewer"] == reviewer],
                         key=lambda s: key("repeat|" + s["id"]))[:2]
        destination = "abc"[("abc".index(reviewer) + 1) % 3]
        for sample in repeats:
            sample["secondary_reviewer"] = destination
            packets[destination].append(sample)
    for reviewer, packet in packets.items():
        packet.sort(key=lambda s: key(reviewer + "|" + s["id"]))
        write(output / f"packet-{reviewer}.json", [{k: s[k] for k in
              ("id", "repository", "path", "source_path")} for s in packet])
    write(output / "sample-key.json", {"seed": SEED, "populations": populations,
                                       "samples": samples})
    print("Prepared 30 unique files, 3 blinded packets of 12, and 6 duplicate reviews.")


if __name__ == "__main__":
    if len(sys.argv) != 4:
        raise SystemExit(__doc__)
    prepare(*(Path(value) for value in sys.argv[1:]))
