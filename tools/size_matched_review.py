# SPDX-License-Identifier: MIT
"""Reproduce the 2026-10-22 size-matched research sample and comparisons.

prepare REPOS RAW OUTPUT: inspect JSON files are RAW/NAME-baseline.json.
summarize REPOS RAW OUTPUT: frozen reviews are OUTPUT/reviewer-{a,b}.json;
candidate reports are RAW/NAME-{events,breadth}.json. No third-party packages.
prepare-discordance REPOS RAW OUTPUT: prepare the registered follow-up packet,
excluding the original sample. summarize-discordance joins reviewer-c.json.
Outputs are explicit research reports, never scanner caches.
"""

import hashlib
import itertools
import json
import math
import subprocess
import sys
from pathlib import Path

SEED = "cpath-size-match-v1-20261022"
REPOSITORIES = ("serilog", "csvhelper", "dapper")
DIMENSIONS = ("attention_priority", "review_difficulty")
FORMULAS = {"baseline": "cpath.complexity-ranking.v1", "events": "cpath.research.event-count.v1",
            "breadth": "cpath.research.breadth-compressed.v1"}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def order(*parts):
    return digest("\n".join((SEED, *parts)).encode())


def read(path):
    return json.loads(path.read_text(encoding="utf-8"))


def write_new(path, value):
    # Refuse to replace a frozen protocol, selection, review, or result.
    with path.open("x", encoding="utf-8") as handle:
        json.dump(value, handle, indent=2)
        handle.write("\n")


def report(raw, repository, variant):
    data = read(raw / f"{repository}-{variant}.json")
    if (data["formula"] != FORMULAS[variant] or data["includes"] or data["excludes"]
            or data["workers"] not in (1, 2)):
        raise ValueError("unexpected formula, selection options, or worker budget")
    paths = [r["path"] for r in data["ranking"]]
    if len(paths) != len(set(paths)) or len(paths) != data["files"]:
        raise ValueError("duplicate or incomplete ranked file list")
    return data


def role(path):
    return "test_or_benchmark" if any(s in path.lower() for s in ("test", "benchmark")) else "other"


def matched_pairs(rows, repository):
    eligible = sorted((r for r in rows if r["path"].endswith(".cs") and 50 <= r["lines"] <= 1200),
                      key=lambda r: r["path"])
    candidates = []
    for a, b in itertools.combinations(eligible, 2):
        low, high = sorted((a["score"], b["score"]))
        if (max(a["lines"], b["lines"]) * 100 <= min(a["lines"], b["lines"]) * 115
                and role(a["path"]) == role(b["path"])
                and high - low >= 5 and high >= 2 * max(low, 1)):
            candidates.append((order(repository, a["path"], b["path"]), a, b))
    selected, used = [], set()
    for _, a, b in sorted(candidates, key=lambda c: c[0]):
        if a["path"] in used or b["path"] in used:
            continue
        selected.append((a, b))
        used.update((a["path"], b["path"]))
        if len(selected) == 4:
            break
    return selected, len(eligible), len(candidates)


def prepare(repos, raw, output):
    output.mkdir(parents=True, exist_ok=True)
    samples, pairs, populations = [], [], {}
    for repository in REPOSITORIES:
        baseline = report(raw, repository, "baseline")
        selected, eligible, candidates = matched_pairs(baseline["ranking"], repository)
        commit = subprocess.check_output(["git", "-C", str(repos / repository), "rev-parse", "HEAD"], text=True).strip()
        populations[repository] = {k: baseline[k] for k in ("files", "bytes", "lines", "unscored_files", "failure_counts")}
        populations[repository].update(commit=commit, eligible=eligible,
                                       candidate_pairs=candidates, selected_pairs=len(selected))
        for a, b in selected:
            ids = []
            for row in (a, b):
                sample_id = order(repository, row["path"])[:16]
                samples.append(dict(id=sample_id, repository=repository, path=row["path"],
                                    lines=row["lines"], score=row["score"], role=role(row["path"]),
                                    source_sha256=digest((repos / repository / row["path"]).read_bytes())))
                ids.append(sample_id)
            pairs.append(dict(repository=repository, ids=ids))
    samples.sort(key=lambda r: order(r["repository"], r["path"]))
    packets = {"a": [], "b": []}
    for index, sample in enumerate(samples):
        reviewer = "ab"[index % 2]
        sample["primary_reviewer"] = reviewer
        packets[reviewer].append({k: sample[k] for k in ("id", "repository", "path")})
    originals = {key: list(value) for key, value in packets.items()}
    for reviewer, other in (("a", "b"), ("b", "a")):
        for overlap in originals[other][:2]:
            packets[reviewer].append(overlap)
            next(s for s in samples if s["id"] == overlap["id"])["secondary_reviewer"] = reviewer
    for reviewer, packet in packets.items():
        packet.sort(key=lambda r: order(reviewer, r["id"]))
        write_new(output / f"packet-{reviewer}.json", packet)
    write_new(output / "sample-key.json", dict(seed=SEED, populations=populations, pairs=pairs, samples=samples))
    print(json.dumps(populations, indent=2))


def sign(value):
    return (value > 0) - (value < 0)


def compare(pairs):
    # Each item contains signed predictor and rating differences.
    wins = sum(p * r > 0 for p, r in pairs)
    losses = sum(p * r < 0 for p, r in pairs)
    ties = len(pairs) - wins - losses
    directional = wins + losses
    probability = (min(1.0, 2 * sum(math.comb(directional, i) for i in range(min(wins, losses) + 1))
                       / (2 ** directional)) if directional else None)
    return dict(pairs=len(pairs), wins=wins, losses=losses, ties=ties,
                concordance=(wins + ties / 2) / len(pairs) if pairs else None,
                two_sided_sign_p=probability)


def prepare_discordance(repos, raw, output):
    original = read(output / "sample-key.json")
    excluded = {(s["repository"], s["path"]) for s in original["samples"]}
    pairs, samples = [], {}
    for repository in REPOSITORIES:
        baseline = report(raw, repository, "baseline")["ranking"]
        breadth = {r["path"]: r for r in report(raw, repository, "breadth")["ranking"]}
        eligible = sorted((r for r in baseline if r["path"].endswith(".cs")
                           and 50 <= r["lines"] <= 1200 and (repository, r["path"]) not in excluded),
                          key=lambda r: r["path"])
        for a, b in itertools.combinations(eligible, 2):
            if (max(a["lines"], b["lines"]) * 100 > min(a["lines"], b["lines"]) * 115
                    or role(a["path"]) != role(b["path"])
                    or (a["score"] - b["score"]) * (breadth[a["path"]]["score"] - breadth[b["path"]]["score"]) >= 0):
                continue
            ids = []
            for row in (a, b):
                sample_id = order(repository, row["path"])[:16]
                samples[sample_id] = dict(id=sample_id, repository=repository, path=row["path"],
                    lines=row["lines"], baseline=row["score"], breadth=breadth[row["path"]]["score"],
                    source_sha256=digest((repos / repository / row["path"]).read_bytes()))
                ids.append(sample_id)
            pairs.append(dict(repository=repository, ids=ids))
    ordered = [samples[k] for k in sorted(samples)]
    write_new(output / "discordance-key.json", dict(pairs=pairs, samples=ordered))
    write_new(output / "packet-c.json", [{k: s[k] for k in ("id", "repository", "path")} for s in ordered])
    print(f"Follow-up: {len(pairs)} pairs, {len(samples)} distinct files")


def summarize_discordance(repos, raw, output):
    key = read(output / "discordance-key.json")
    reviews_path = output / "reviewer-c.json"
    ratings = read(reviews_path)
    assert len(ratings) == len(key["samples"])
    assert {r["id"] for r in ratings} == {s["id"] for s in key["samples"]}
    by_id = {r["id"]: r for r in ratings}
    samples = {s["id"]: s for s in key["samples"]}
    for sample_id, sample in samples.items():
        assert digest((repos / sample["repository"] / sample["path"]).read_bytes()) == sample["source_sha256"]
        rating = by_id[sample_id]
        assert all(type(rating[d]) is int and rating[d] in range(4) for d in DIMENSIONS)
        assert rating["evidence"] and rating["inspection_scope"] and rating["counterevidence"]
        for anchor in rating["evidence"]:
            assert 1 <= anchor["start_line"] <= anchor["end_line"] <= sample["lines"]
        sample["review"] = rating
    for pair in key["pairs"]:
        a, b = (samples[i] for i in pair["ids"])
        pair["direction"] = {v: sign(a[v] - b[v]) for v in ("baseline", "breadth")}
        pair["rating_direction"] = {d: sign(a["review"][d] - b["review"][d]) for d in DIMENSIONS}
    write_new(output / "formula-disagreement-results.json", dict(**key, reviewer_sha256=digest(reviews_path.read_bytes()),
              limitation="Exploratory strict-reversal selection; shared file in two pairs; one reviewer; no inferential test"))
    print(json.dumps(key["pairs"], indent=2))


def summarize(repos, raw, output):
    key = read(output / "sample-key.json")
    for repository, population in key["populations"].items():
        pin = subprocess.check_output(["git", "-C", str(repos / repository), "rev-parse", "HEAD"], text=True).strip()
        if pin != population["commit"]:
            raise ValueError(f"source revision changed: {repository}")
    ratings, hashes = {}, {}
    for reviewer in "ab":
        path = output / f"reviewer-{reviewer}.json"
        packet = read(output / f"packet-{reviewer}.json")
        rows = read(path)
        assert len(rows) == len(packet) and {r["id"] for r in rows} == {r["id"] for r in packet}
        for row in rows:
            assert all(type(row[d]) is int and row[d] in range(4) for d in DIMENSIONS)
            assert row["evidence"] and row["inspection_scope"] and row["counterevidence"]
        ratings[reviewer] = {r["id"]: r for r in rows}
        hashes[reviewer] = digest(path.read_bytes())
    candidate = {(repo, variant): {r["path"]: r["score"] for r in report(raw, repo, variant)["ranking"]}
                 for repo in REPOSITORIES for variant in ("events", "breadth")}
    joined = {}
    for sample in key["samples"]:
        source = (repos / sample["repository"] / sample["path"]).read_bytes()
        assert digest(source) == sample["source_sha256"]
        sample["predictors"] = dict(baseline=sample["score"], lines=sample["lines"],
            **{v: candidate[sample["repository"], v][sample["path"]] for v in ("events", "breadth")})
        sample["primary"] = ratings[sample["primary_reviewer"]][sample["id"]]
        if "secondary_reviewer" in sample:
            sample["secondary"] = ratings[sample["secondary_reviewer"]][sample["id"]]
        for rating in [sample["primary"]] + ([sample["secondary"]] if "secondary" in sample else []):
            for anchor in rating["evidence"]:
                assert 1 <= anchor["start_line"] <= anchor["end_line"] <= sample["lines"]
        joined[sample["id"]] = sample
    comparisons = []
    for pair in key["pairs"]:
        a, b = (joined[i] for i in pair["ids"])
        comparisons.append(dict(**pair,
            predictors={v: sign(a["predictors"][v] - b["predictors"][v]) for v in a["predictors"]},
            ratings={d: sign(a["primary"][d] - b["primary"][d]) for d in DIMENSIONS}))
    summary = {}
    for dimension in DIMENSIONS:
        summary[dimension] = {scope: {v: compare([(p["predictors"][v], p["ratings"][dimension])
            for p in comparisons if scope == "all" or p["repository"] == scope])
            for v in ("baseline", "lines", "events", "breadth")}
            for scope in ("all", *REPOSITORIES)}
        repeated = [s for s in joined.values() if "secondary" in s]
        summary[dimension]["agreement"] = dict(count=len(repeated),
            exact=sum(s["primary"][dimension] == s["secondary"][dimension] for s in repeated),
            within_one=sum(abs(s["primary"][dimension] - s["secondary"][dimension]) <= 1 for s in repeated))
    write_new(output / "results.json", dict(protocol="attention-ranking-results.md",
              reviewer_sha256=hashes, populations=key["populations"],
              summary=summary, comparisons=comparisons, files=list(joined.values())))
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    commands = {"prepare": prepare, "summarize": summarize,
                "prepare-discordance": prepare_discordance, "summarize-discordance": summarize_discordance}
    if len(sys.argv) != 5 or sys.argv[1] not in commands:
        raise SystemExit(__doc__)
    commands[sys.argv[1]](*(Path(a) for a in sys.argv[2:]))
