# SPDX-License-Identifier: MIT
"""Frozen combined-signal selection and evaluation, using explicit research outputs.

develop STUDY_ROOT: read raw/{serilog,csvhelper,dapper}.json and size-matched review labels.
prepare STUDY_ROOT: freeze neutral/challenge sources and blinded review packets.
summarize STUDY_ROOT: join frozen reviewer-{a,b,c}.json and report all formulas.
sensitivity STUDY_ROOT: descriptive audits of the frozen results; no reselection.
Commands refuse to replace existing outputs. No third-party dependencies.
"""

import hashlib
import itertools
import json
import subprocess
import sys
from fractions import Fraction
from pathlib import Path

SEED = "cpath-phase2-holdout-v1-20261022"
VARIANTS = ("C", "B", "K", "CT", "KT", "CH", "KH", "CTH", "KTH", "KQH", "KTHA", "CS")
FAMILIES = dict(zip(VARIANTS, (0, 0, 1, 1, 2, 1, 2, 2, 3, 3, 4, 1)))
CONTROL_EXCLUSIONS = {"KQH": "fails scope-local alpha-renaming invariance",
                      "KTHA": "changes on a behavior-preserving user-defined API rename"}
DEVELOPMENT = ("serilog", "csvhelper", "dapper")
HOLDOUT = ("polly", "mediatr", "litedb")
DIMENSIONS = ("attention_priority", "review_difficulty")
COMPARATORS = (*VARIANTS, "lines", "E", "arity", "S", "T", "Q", "H", "A")
EVIDENCE = Path(__file__).resolve().parents[1] / "docs/evidence"


def read(path):
    return json.loads(path.read_text(encoding="utf-8"))


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("x", encoding="utf-8") as handle:
        json.dump(value, handle, indent=2)
        handle.write("\n")


def sha(data):
    return hashlib.sha256(data).hexdigest()


def order(*values):
    return sha("\n".join((SEED, *values)).encode())


def sign(value):
    return (value > 0) - (value < 0)


def coarse_role(path):
    return "test" if any(s in path.lower() for s in ("test", "benchmark")) else "other"


def holdout_role(repository, path):
    # Frozen path-root strata, not semantic roles: src/ also admits nested docs
    # snippets. Keep this rule unchanged to reproduce the original sample.
    parts = path.lower().split("/")
    if repository in ("polly", "mediatr"):
        if parts[0] in ("test", "tests"):
            return "test"
        if parts[0] == "src":
            return "production"
    if repository == "litedb":
        if parts[0] == "litedb.tests":
            return "test"
        if parts[0] == "litedb":
            return "production"
    return None


def report(root, repository):
    path = root / "raw" / f"{repository}.json"
    data = read(path)
    assert data["schema"] == "cpath.research.signals.v1"
    assert data["formula_family"] == "cpath.research.phase2-formulas.v1"
    assert data["includes"] == [] and data["excludes"] == []
    rows = data["files"]
    assert len(rows) == data["analyzed_files"]
    assert len({r["path"] for r in rows}) == len(rows)
    assert all(set(r["scores"]) == set(VARIANTS) for r in rows)
    assert data["workers"] in (1, 2)
    return data


def predictors(row):
    return {**{v: row["scores"][v]["maximum"] for v in VARIANTS},
            "lines": row["lines"], **{v: row["features"][v] for v in COMPARATORS[13:]}}


def concordance(comparisons):
    wins = sum(p * r > 0 for p, r in comparisons)
    losses = sum(p * r < 0 for p, r in comparisons)
    ties = len(comparisons) - wins - losses
    return dict(pairs=len(comparisons), wins=wins, losses=losses, ties=ties,
                concordance=float(Fraction(2 * wins + ties, 2 * len(comparisons))) if comparisons else None)


def compare_samples(a, b, variant, dimension, reviewer_field="rating"):
    return (sign(a["predictors"][variant] - b["predictors"][variant]),
            sign(a[reviewer_field][dimension] - b[reviewer_field][dimension]))


def development(root):
    matched_reviews = EVIDENCE / "size-matched-review"
    originals = read(matched_reviews / "results.json")["files"]
    originals += read(matched_reviews / "formula-disagreement-results.json")["samples"]
    raw_reports = {repo: report(root, repo) for repo in DEVELOPMENT}
    reports = {repo: {r["path"]: r for r in raw_reports[repo]["files"]} for repo in DEVELOPMENT}
    samples = []
    for old in originals:
        repo, path = old["repository"], old["path"]
        row = reports[repo][path]
        assert sha((Path(raw_reports[repo]["root"]) / path).read_bytes()) == old["source_sha256"]
        assert row["scores"]["C"]["maximum"] == old.get("score", old.get("baseline"))
        samples.append(dict(id=old["id"], repository=repo, path=path, role=coarse_role(path),
                            source_sha256=old["source_sha256"], predictors=predictors(row),
                            rating=old.get("primary", old.get("review"))))
    samples.sort(key=lambda s: (s["repository"], s["path"]))
    # Freeze the pair list before calculating/selecting candidate fit.
    pairs = [(a, b) for a, b in itertools.combinations(samples, 2)
             if a["repository"] == b["repository"] and a["role"] == b["role"]]
    write(root / "development-pairs.json", [dict(repository=a["repository"], role=a["role"], ids=[a["id"], b["id"]])
                                            for a, b in pairs])
    table, exact = {}, {}
    for v in COMPARATORS:
        by_repo = {repo: concordance([compare_samples(a, b, v, "attention_priority")
                    for a, b in pairs if a["repository"] == repo]) for repo in DEVELOPMENT}
        assert all(r["pairs"] for r in by_repo.values())
        score = sum((Fraction(2 * r["wins"] + r["ties"], 2 * r["pairs"]) for r in by_repo.values()), Fraction()) / len(DEVELOPMENT)
        exact[v] = score
        table[v] = dict(macro_concordance=float(score), exact_macro=str(score), per_repository=by_repo,
                        pooled=concordance([compare_samples(a, b, v, "attention_priority") for a, b in pairs]),
                        difficulty=concordance([compare_samples(a, b, v, "review_difficulty") for a, b in pairs]))
    eligible = [v for v in VARIANTS if v not in CONTROL_EXCLUSIONS]
    winner = min(eligible, key=lambda v: (-exact[v], FAMILIES[v], VARIANTS.index(v)))
    by_id = {s["id"]: s for s in samples}
    strata = {"original_matched": read(matched_reviews / "results.json")["comparisons"],
              "original_targeted": read(matched_reviews / "formula-disagreement-results.json")["pairs"]}
    stratum_results = {name: {v: concordance([compare_samples(by_id[p["ids"][0]], by_id[p["ids"][1]],
                       v, "attention_priority") for p in selected]) for v in COMPARATORS}
                       for name, selected in strata.items()}
    result = dict(seed=SEED, winner=winner, winner_selected_before_holdout=True,
                  exclusions=CONTROL_EXCLUSIONS, table=table, samples=samples, development_strata=stratum_results,
                  raw_report_sha256={r: sha((root / "raw" / f"{r}.json").read_bytes()) for r in DEVELOPMENT},
                  limitation="Size-matched review labels reused as development only; induced pairs share files and are not independent")
    write(root / "development.json", result)
    print(json.dumps(dict(winner=winner, table={v: r["macro_concordance"] for v, r in table.items()}), indent=2))


def source_record(root, repository, row, role):
    digest = sha((root / "repos" / repository / row["path"]).read_bytes())
    manifest = read(root / "scan-input-manifest.json")[repository]
    assert digest == manifest["source_sha256"][row["path"]]
    return dict(id=order(repository, row["path"])[:16], repository=repository, path=row["path"],
                role=role, lines=row["lines"], predictors=predictors(row),
                source_sha256=digest)


def prepare(root):
    dev = read(root / "development.json")
    winner = dev["winner"]
    pairs, samples, populations = [], {}, {}
    for repository in HOLDOUT:
        data = report(root, repository)
        assert Path(data["root"]).resolve() == (root / "repos" / repository).resolve()
        manifest = read(root / "scan-input-manifest.json")[repository]
        assert manifest["raw_sha256"] == sha((root / "raw" / f"{repository}.json").read_bytes())
        rows = data["files"]
        population = {k: data[k] for k in ("analyzed_files", "unscored_files", "failure_counts")}
        population["commit"] = subprocess.check_output(["git", "-C", str(root / "repos" / repository), "rev-parse", "HEAD"], text=True).strip()
        population["roles"] = {}
        for role in ("production", "test"):
            eligible = [r for r in rows if r["path"].endswith(".cs") and 50 <= r["lines"] <= 600
                        and holdout_role(repository, r["path"]) == role]
            chosen = sorted(eligible, key=lambda r: order("neutral", repository, r["path"]))[:6]
            # Only complete pairs. A shortfall is recorded, never substituted.
            if len(chosen) % 2:
                chosen = chosen[:-1]
            population["roles"][role] = dict(eligible=len(eligible), selected=len(chosen))
            chosen.sort(key=lambda r: (r["lines"], r["path"]))
            for a, b in zip(chosen[::2], chosen[1::2]):
                members = [source_record(root, repository, row, role) for row in (a, b)]
                for sample in members:
                    samples[sample["id"]] = sample
                pairs.append(dict(repository=repository, role=role, stratum="neutral", ids=[s["id"] for s in members]))
        unused = [r for r in rows if 50 <= r["lines"] <= 800 and r["path"].endswith(".cs")
                  and holdout_role(repository, r["path"]) is not None and order(repository, r["path"])[:16] not in samples]
        candidates = []
        for a, b in itertools.combinations(sorted(unused, key=lambda r: r["path"]), 2):
            if (holdout_role(repository, a["path"]) != holdout_role(repository, b["path"])
                    or max(a["lines"], b["lines"]) * 10 > min(a["lines"], b["lines"]) * 13):
                continue
            if ((a["scores"]["C"]["maximum"] - b["scores"]["C"]["maximum"])
                    * (a["scores"][winner]["maximum"] - b["scores"][winner]["maximum"]) < 0):
                candidates.append((order("challenge", repository, a["path"], b["path"]), a, b))
        selected = 0
        for _, a, b in sorted(candidates, key=lambda item: item[0]):
            if any(order(repository, r["path"])[:16] in samples for r in (a, b)):
                continue
            role = holdout_role(repository, a["path"])
            members = [source_record(root, repository, row, role) for row in (a, b)]
            for sample in members:
                samples[sample["id"]] = sample
            pairs.append(dict(repository=repository, role=role, stratum="challenge", ids=[s["id"] for s in members]))
            selected += 1
            if selected == 2:
                break
        population["challenge"] = dict(eligible_pairs=len(candidates), selected_pairs=selected)
        populations[repository] = population
    pairs.sort(key=lambda p: order("assignment", p["repository"], *p["ids"]))
    packets = {r: [] for r in "abc"}
    for index, pair in enumerate(pairs):
        reviewer = "abc"[index % 3]
        pair["primary_reviewer"] = reviewer
        for sample_id in pair["ids"]:
            sample = samples[sample_id]
            sample.update(primary_reviewer=reviewer, stratum=pair["stratum"])
            packets[reviewer].append({k: sample[k] for k in ("id", "repository", "path")})
    overlapped = set()
    for reviewer in "abc":
        count = 0
        for pair in sorted(pairs, key=lambda p: order("overlap", reviewer, *p["ids"])):
            pair_id = tuple(pair["ids"])
            if pair["primary_reviewer"] == reviewer or pair_id in overlapped:
                continue
            overlapped.add(pair_id)
            for sample_id in pair["ids"]:
                sample = samples[sample_id]
                sample["secondary_reviewer"] = reviewer
                packets[reviewer].append({k: sample[k] for k in ("id", "repository", "path")})
            count += 1
            if count == 2:
                break
    for reviewer, packet in packets.items():
        packet.sort(key=lambda s: order("packet", reviewer, s["id"]))
        write(root / "reviews" / f"packet-{reviewer}.json", packet)
    write(root / "holdout-key.json", dict(seed=SEED, frozen_winner=winner,
          development_sha256=sha((root / "development.json").read_bytes()), populations=populations,
          pairs=pairs, samples=list(samples.values()),
          raw_report_sha256={r: sha((root / "raw" / f"{r}.json").read_bytes()) for r in HOLDOUT},
          packet_sha256={r: sha((root / "reviews" / f"packet-{r}.json").read_bytes()) for r in "abc"}))
    print(json.dumps(dict(winner=winner, populations=populations, assignments={r: len(p) for r, p in packets.items()}), indent=2))


def summarize(root):
    key = read(root / "holdout-key.json")
    assert key["development_sha256"] == sha((root / "development.json").read_bytes())
    for repository, digest in key["raw_report_sha256"].items():
        assert digest == sha((root / "raw" / f"{repository}.json").read_bytes())
        pin = subprocess.check_output(["git", "-C", str(root / "repos" / repository), "rev-parse", "HEAD"], text=True).strip()
        assert pin == key["populations"][repository]["commit"]
    ratings, hashes = {}, {}
    for reviewer in "abc":
        path = root / "reviews" / f"reviewer-{reviewer}.json"
        rows = read(path)
        packet = read(root / "reviews" / f"packet-{reviewer}.json")
        assert key["packet_sha256"][reviewer] == sha((root / "reviews" / f"packet-{reviewer}.json").read_bytes())
        assert len(rows) == len(packet) and {r["id"] for r in rows} == {r["id"] for r in packet}
        for row in rows:
            assert all(type(row[d]) is int and row[d] in range(4) for d in DIMENSIONS)
            assert row["evidence"] and row["inspection_scope"] and row["counterevidence"]
            assert row["confidence"] in ("high", "medium", "low")
            assert row["actual_role"] and row["hardest_behavior"]
        ratings[reviewer] = {r["id"]: r for r in rows}
        hashes[reviewer] = sha(path.read_bytes())
    samples = {s["id"]: s for s in key["samples"]}
    for sample_id, sample in samples.items():
        assert sha((root / "repos" / sample["repository"] / sample["path"]).read_bytes()) == sample["source_sha256"]
        sample["primary"] = ratings[sample["primary_reviewer"]][sample_id]
        if "secondary_reviewer" in sample:
            sample["secondary"] = ratings[sample["secondary_reviewer"]][sample_id]
        for rating in [sample["primary"]] + ([sample["secondary"]] if "secondary" in sample else []):
            for anchor in rating["evidence"]:
                assert 1 <= anchor["start_line"] <= anchor["end_line"] <= sample["lines"]
    summary = {}
    for stratum in ("neutral", "challenge"):
        pairs = [p for p in key["pairs"] if p["stratum"] == stratum]
        summary[stratum] = {}
        for dimension in DIMENSIONS:
            dimension_result = {}
            for scope in ("all", *HOLDOUT, "production", "test", "low_baseline"):
                eligible = [p for p in pairs if (scope == "all" or scope == p["repository"]
                            or scope == p["role"] or (scope == "low_baseline"
                            and all(samples[i]["predictors"]["C"] <= 5 for i in p["ids"])))]
                dimension_result[scope] = {v: concordance([compare_samples(samples[p["ids"][0]],
                    samples[p["ids"][1]], v, dimension, "primary") for p in eligible]) for v in COMPARATORS}
            summary[stratum][dimension] = dimension_result
    repeats = [s for s in samples.values() if "secondary" in s]
    agreement = {d: dict(count=len(repeats), exact=sum(s["primary"][d] == s["secondary"][d] for s in repeats),
                        within_one=sum(abs(s["primary"][d] - s["secondary"][d]) <= 1 for s in repeats)) for d in DIMENSIONS}
    write(root / "results.json", dict(**key, summary=summary, agreement=agreement, reviewer_sha256=hashes,
                                      limitation="Agent judgments; one frozen development winner; all other holdout variants descriptive only"))
    print(json.dumps({s: {v: summary[s]["attention_priority"]["all"][v]
                         for v in ("C", key["frozen_winner"], "CS", "lines")} for s in summary}, indent=2))


def sensitivity(root):
    result = read(root / "results.json")
    samples = {s["id"]: s for s in result["samples"]}
    winner = result["frozen_winner"]
    neutral = result["summary"]["neutral"]["attention_priority"]
    baseline_score = neutral["all"]["C"]["concordance"]
    leader_score = neutral["all"][winner]["concordance"]
    size_score = neutral["all"]["CS"]["concordance"]
    reversals = [scope for scope in (*HOLDOUT, "production", "test")
                 if neutral[scope][winner]["concordance"] < neutral[scope]["C"]["concordance"]]
    pair_details, overlap, without_snippets = [], {}, {}
    for pair in result["pairs"]:
        a, b = [samples[i] for i in pair["ids"]]
        comparisons = {v: compare_samples(a, b, v, "attention_priority", "primary") for v in COMPARATORS}
        pair_details.append(dict(**pair, comparisons={v: dict(predictor_direction=x, judgment_direction=y,
                             credit=0.5 if x == 0 or y == 0 else int(x == y))
                             for v, (x, y) in comparisons.items()}))
    for stratum in ("neutral", "challenge"):
        pairs = [p for p in result["pairs"] if p["stratum"] == stratum]
        repeated = [p for p in pairs if all("secondary" in samples[i] for i in p["ids"])]
        overlap[stratum] = {rating: {v: concordance([compare_samples(samples[p["ids"][0]],
            samples[p["ids"][1]], v, "attention_priority", rating) for p in repeated])
            for v in COMPARATORS} for rating in ("primary", "secondary")}
        eligible = [p for p in pairs if not any("/Snippets/Docs/" in samples[i]["path"] for i in p["ids"])]
        without_snippets[stratum] = {v: concordance([compare_samples(samples[p["ids"][0]],
            samples[p["ids"][1]], v, "attention_priority", "primary") for p in eligible]) for v in COMPARATORS}
    tie_breakdown = {}
    for stratum in ("neutral", "challenge"):
        pairs = [p for p in pair_details if p["stratum"] == stratum]
        tie_breakdown[stratum] = {}
        for v in COMPARATORS:
            values = [p["comparisons"][v] for p in pairs]
            tie_breakdown[stratum][v] = dict(
                judgment_tied=sum(x["judgment_direction"] == 0 for x in values),
                predictor_tied=sum(x["predictor_direction"] == 0 for x in values),
                both_tied=sum(x["judgment_direction"] == x["predictor_direction"] == 0 for x in values))
    decision = dict(frozen_leader=winner, strictly_beats_C=leader_score > baseline_score,
                    strictly_beats_CS=leader_score > size_score, observed_reversals=reversals,
                    supported_default="C", promote=False,
                    reason="The frozen leader does not improve neutral concordance over C; do not reselect from holdout.")
    assert not decision["strictly_beats_C"], "Review gate for a changed dataset; do not silently reuse this decision"
    write(root / "sensitivity.json", dict(results_sha256=sha((root / "results.json").read_bytes()),
        decision=decision, primary_dimensions_identical=sum(s["primary"][DIMENSIONS[0]] == s["primary"][DIMENSIONS[1]]
                                                         for s in samples.values()),
        pair_details=pair_details, tie_breakdown=tie_breakdown, overlap=overlap,
        without_documentation_snippets=without_snippets,
        limitation="Post-label descriptive audit. Original pairs, labels and selection remain authoritative; no hypothesis test or reselection."))


if __name__ == "__main__":
    commands = {"develop": development, "prepare": prepare, "summarize": summarize, "sensitivity": sensitivity}
    if len(sys.argv) != 3 or sys.argv[1] not in commands:
        raise SystemExit(__doc__)
    commands[sys.argv[1]](Path(sys.argv[2]))
