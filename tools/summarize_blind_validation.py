# Copyright (c) 2026 Raman Dzehtsiar
# SPDX-License-Identifier: MIT
"""Join frozen blind reviews to the sampling key and compute protocol summaries.

Usage: python tools/summarize_blind_validation.py PACKETS REVIEWS OUTPUT_JSON
No third-party packages required. Does not modify input ratings.
"""

import hashlib
import json
import math
import statistics
import sys
from pathlib import Path


def average_ranks(values):
    ordered = sorted(range(len(values)), key=lambda i: values[i])
    ranks = [0.0] * len(values)
    start = 0
    while start < len(ordered):
        end = start + 1
        while end < len(ordered) and values[ordered[end]] == values[ordered[start]]:
            end += 1
        mean = (start + 1 + end) / 2
        for index in ordered[start:end]:
            ranks[index] = mean
        start = end
    return ranks


def spearman(xs, ys):
    x, y = average_ranks(xs), average_ranks(ys)
    mx, my = statistics.mean(x), statistics.mean(y)
    covariance = sum((a - mx) * (b - my) for a, b in zip(x, y))
    scale = math.sqrt(sum((a - mx) ** 2 for a in x) * sum((b - my) ** 2 for b in y))
    return covariance / scale if scale else None


def summarize(packets, reviews, output):
    key = json.loads((packets / "sample-key.json").read_text(encoding="utf-8"))
    raw = {}
    hashes = {}
    for reviewer in "abc":
        path = reviews / f"reviewer-{reviewer}.json"
        hashes[reviewer] = hashlib.sha256(path.read_bytes()).hexdigest()
        ratings = json.loads(path.read_text(encoding="utf-8-sig"))
        packet = json.loads((packets / f"packet-{reviewer}.json").read_text(encoding="utf-8"))
        assert len(ratings) == len(packet) == 12
        assert {r["id"] for r in ratings} == {r["id"] for r in packet}
        for rating in ratings:
            for field in ("attention_priority", "review_difficulty"):
                assert type(rating[field]) is int and rating[field] in range(4)
            assert rating["confidence"] in ("high", "medium", "low")
            assert rating["evidence"] and rating["inspection_scope"]
        raw[reviewer] = {r["id"]: r for r in ratings}
    joined = []
    for sample in key["samples"]:
        source = Path(sample["source_path"])
        data = source.read_bytes()
        assert hashlib.sha256(data).hexdigest() == sample["source_sha256"], source
        encoding = "utf-16" if data[:2] in (b"\xff\xfe", b"\xfe\xff") else "utf-8-sig"
        lines = len(data.decode(encoding).splitlines())
        row = {k: v for k, v in sample.items() if k != "source_path"}
        row["source_lines"] = lines
        row["primary"] = raw[row["primary_reviewer"]][row["id"]]
        if "secondary_reviewer" in row:
            row["secondary"] = raw[row["secondary_reviewer"]][row["id"]]
        for rating in [row["primary"]] + ([row["secondary"]] if "secondary" in row else []):
            for anchor in rating["evidence"]:
                assert 1 <= anchor["start_line"] <= anchor["end_line"] <= lines, (row["id"], anchor)
        joined.append(row)
    summary = {}
    for dimension in ("attention_priority", "review_difficulty"):
        bands = {}
        for band in ("top", "middle", "low"):
            ratings = [r["primary"][dimension] for r in joined if r["stratum"] == band]
            bands[band] = {"count": len(ratings), "positive": sum(v >= 2 for v in ratings),
                           "mean": statistics.mean(ratings), "ratings": ratings}
        comparisons = []
        for repo in key["populations"]:
            top = [r for r in joined if r["repository"] == repo and r["stratum"] == "top"]
            low = [r for r in joined if r["repository"] == repo and r["stratum"] == "low"]
            for a in top:
                for b in low:
                    delta = a["primary"][dimension] - b["primary"][dimension]
                    comparisons.append({"top": a["id"], "low": b["id"], "delta": delta})
        repeats = [r for r in joined if "secondary" in r]
        differences = [{"id": r["id"], "primary": r["primary"][dimension],
                        "secondary": r["secondary"][dimension]} for r in repeats]
        summary[dimension] = {"strata": bands,
            "spearman_printed_score": spearman([r["score"] for r in joined],
                                                [r["primary"][dimension] for r in joined]),
            "top_low_pairs": comparisons,
            "top_low_concordance": statistics.mean(1 if p["delta"] > 0 else 0.5 if p["delta"] == 0 else 0 for p in comparisons),
            "repeat_agreement": {"count": len(repeats),
                "exact": sum(d["primary"] == d["secondary"] for d in differences),
                "within_one": sum(abs(d["primary"] - d["secondary"]) <= 1 for d in differences),
                "pairs": differences}}
    result = {"protocol": "blind-validation-protocol.md", "seed": key["seed"],
              "reviewer_artifact_sha256": hashes, "populations": key["populations"],
              "summary": summary, "files": sorted(joined, key=lambda r: (r["repository"], r["rank"]))}
    # Added after the blind ratings were frozen. These are explicitly exploratory
    # diagnostics, not endpoints pre-specified in the protocol.
    nonzero = [r for r in joined if r["score"] > 0]
    result["exploratory_post_review"] = {
        "source_line_count_spearman_attention": spearman(
            [r["source_lines"] for r in joined], [r["primary"]["attention_priority"] for r in joined]),
        "zero_score_files": len(joined) - len(nonzero),
        "nonzero_subset_count": len(nonzero),
        "nonzero_subset_score_spearman_attention": spearman(
            [r["score"] for r in nonzero], [r["primary"]["attention_priority"] for r in nonzero]),
    }
    output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8", newline="\n")
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    if len(sys.argv) != 4:
        raise SystemExit(__doc__)
    summarize(*(Path(value) for value in sys.argv[1:]))
