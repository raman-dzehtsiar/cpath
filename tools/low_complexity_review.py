# SPDX-License-Identifier: MIT
"""Prepare/summarize frozen low-complexity matched region reviews.

Usage: python3 tools/low_complexity_review.py {prepare,summarize} STUDY_ROOT
Inputs are explicit research reports, never reusable production scan state.
Outputs refuse replacement. Python standard library only.
"""

import hashlib
import json
import subprocess
import sys
from collections import Counter
from pathlib import Path

from large_repository_review import SHARED_EXCLUDES, role as coarse_role

SEED = "cpath-phase3-lowc-v1-20261022"
REPOSITORIES = ("roslyn", "aspnetcore")
RULES = ("R", "X", "F", "E", "W", "D")
SYNTAX = dict(R="usings", X="catches", F="finallys", E="catches", W="awaits", D="locks")
SYNTAX_NAMES = tuple(sorted(set(SYNTAX.values())))
DIMENSIONS = ("attention_priority", "review_difficulty")
ASSESSMENTS = ("useful_review_question", "routine", "intentional", "unclear")
LIMITATION = (
    "Signal-enriched matched sample from at most five retained callable records per file; "
    "not a population-recall, precision or defect-accuracy estimate. Agent judgments, "
    "not measured maintainer outcomes. Controls match existing syntax-family presence."
)


def read(path):
    with path.open(encoding="utf-8") as handle:
        return json.load(handle)


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("x", encoding="utf-8") as handle:
        json.dump(value, handle, indent=2)
        handle.write("\n")


def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            value.update(block)
    return value.hexdigest()


def order(*items):
    return hashlib.sha256("\n".join((SEED, *map(str, items))).encode()).hexdigest()


def identity(repository, path, method, evidence_index):
    # Distinct same-line lambdas can share every human-readable location field.
    return order(repository, path, method["name"], method["start"], method["end"], evidence_index)[:20]


def eligible(method):
    return (0 <= method["signals"]["baseline"] <= 5
            and 1 <= method["signals"]["span_lines"] <= 40
            and not method["lowc"]["incomplete"])


def can_pair(positive, negative, rule):
    return (positive["repository"] == negative["repository"]
            and positive["role"] == negative["role"]
            and positive["path"] != negative["path"]
            and positive["C"] == negative["C"]
            and max(positive["span_lines"], negative["span_lines"])
            <= 2 * min(positive["span_lines"], negative["span_lines"])
            and negative["syntax"][SYNTAX[rule]] > 0
            and not any(negative["counts"].values()))


def choose_pairs(candidates):
    """At most two disjoint pairs/rule, preferring one from each repository."""
    pairs, used_files = [], set()
    populations = {rule: {} for rule in RULES}
    # Index by every matching dimension, including syntax and small span. This
    # avoids scanning all negative records for each unmatched positive record.
    negatives = {}
    for row in candidates:
        if not any(row["counts"].values()):
            for syntax in SYNTAX_NAMES:
                if row["syntax"][syntax] > 0:
                    key = (row["repository"], row["role"], row["C"], syntax, row["span_lines"])
                    negatives.setdefault(key, []).append(row)
    for pool in negatives.values():
        pool.sort(key=lambda row: order("negative", row["id"]))
    for rule in RULES:
        positives = {repo: sorted(
            [row for row in candidates if row["repository"] == repo and row["counts"][rule] > 0],
            key=lambda row: order("positive", rule, row["id"])) for repo in REPOSITORIES}
        for repo in REPOSITORIES:
            populations[rule][repo] = dict(positive_regions=len(positives[repo]), selected_pairs=0)

        def select(repository):
            for positive in positives[repository]:
                if (repository, positive["path"]) in used_files:
                    continue
                possible = []
                for span in range((positive["span_lines"] + 1) // 2,
                                  min(40, positive["span_lines"] * 2) + 1):
                    key = (repository, positive["role"], positive["C"], SYNTAX[rule], span)
                    for negative in negatives.get(key, []):
                        if ((repository, negative["path"]) not in used_files
                                and negative["path"] != positive["path"]):
                            possible.append(negative)
                            break
                if not possible:
                    continue
                negative = min(possible, key=lambda row: order("negative", row["id"]))
                assert can_pair(positive, negative, rule)
                used_files.update((repository, row["path"]) for row in (positive, negative))
                pairs.append(dict(rule=rule, repository=repository, role=positive["role"],
                                  positive=positive["id"], negative=negative["id"],
                                  ids=[positive["id"], negative["id"]]))
                populations[rule][repository]["selected_pairs"] += 1
                return True
            return False

        selected = sum(select(repo) for repo in REPOSITORIES)
        if selected < 2:
            for repository in sorted(REPOSITORIES, key=lambda repo: order("fallback", rule, repo)):
                if selected == 2:
                    break
                selected += select(repository)
    return pairs, populations


def assign(pairs, samples):
    packets = {reviewer: [] for reviewer in "abc"}
    pairs.sort(key=lambda pair: order("assignment", pair["rule"], *pair["ids"]))
    for index, pair in enumerate(pairs):
        reviewer = "abc"[index % 3]
        pair["primary_reviewer"] = reviewer
        for sample_id in pair["ids"]:
            samples[sample_id]["primary_reviewer"] = reviewer
            packets[reviewer].append(sample_id)
    # Two complete overlap pairs add at most four reviews: at most 28 total.
    for pair in sorted(pairs, key=lambda pair: order("overlap", *pair["ids"]))[:2]:
        reviewer = min((r for r in "abc" if r != pair["primary_reviewer"]),
                       key=lambda r: (len(packets[r]), order("secondary", r, *pair["ids"])))
        pair["secondary_reviewer"] = reviewer
        for sample_id in pair["ids"]:
            samples[sample_id]["secondary_reviewer"] = reviewer
            packets[reviewer].append(sample_id)
    for reviewer, packet in packets.items():
        packet.sort(key=lambda sample_id: order("packet", reviewer, sample_id))
    return packets


def prepare(root):
    outputs = [root / "lowc-key.json", *(root / f"lowc-packet-{r}.json" for r in "abc")]
    assert not any(path.exists() for path in outputs), "Refusing to replace frozen outputs"
    holdout_path = root / "holdout-key.json"
    main_key = read(holdout_path)
    excluded = {(sample["repository"], sample["path"]) for sample in main_key["samples"]}
    candidates, populations, raw_hashes, roots, pins = [], {}, {}, {}, {}
    for repository in REPOSITORIES:
        report_path = root / "raw" / f"{repository}.json"
        data = read(report_path)
        assert data["schema"] == "cpath.research.signals.v1"
        assert data["formula_family"] == "cpath.research.phase2-formulas.v1"
        assert data["lowc_contract"] == "cpath.research.lowc-obligations.v1"
        assert data["includes"] == [] and tuple(data["excludes"]) == SHARED_EXCLUDES
        assert Path(data["root"]).resolve() == (root / "repos" / repository).resolve()
        assert len(data["files"]) == data["analyzed_files"]
        assert len({row["path"] for row in data["files"]}) == len(data["files"])
        roots[repository] = str(Path(data["root"]).resolve())
        pins[repository] = subprocess.check_output(
            ["git", "-C", roots[repository], "rev-parse", "HEAD"], text=True).strip()
        raw_hashes[repository] = digest(report_path)
        counts = Counter()
        abstentions = Counter()
        all_callable_counts, all_rule_methods, all_abstentions = Counter(), Counter(), Counter()
        for row in data["files"]:
            assert len(row["methods"]) <= 5, "Evidence frame must remain bounded"
            aggregate = row["lowc"]
            all_callable_counts.update({name: aggregate[name] for name in (
                "eligible_methods", "eligible_with_findings", "eligible_incomplete", "incomplete_methods")})
            all_rule_methods.update(aggregate["rule_methods"])
            all_abstentions.update(aggregate["abstentions"])
            for evidence_index, method in enumerate(row["methods"]):
                counts["retained_callables"] += 1
                assert set(method["lowc"]["counts"]) == set(RULES)
                abstentions.update(method["lowc"]["abstentions"])
                if method["lowc"]["incomplete"]:
                    counts["hard_budget_incomplete"] += 1
                if not eligible(method):
                    continue
                counts["lowc_complete_retained_callables"] += 1
                if (repository, row["path"]) in excluded:
                    counts["excluded_main_sample_callables"] += 1
                    continue
                role = coarse_role(row["path"])
                if role is None:
                    counts["excluded_role_callables"] += 1
                    continue
                signals = method["signals"]
                record = dict(id=identity(repository, row["path"], method, evidence_index), repository=repository,
                              path=row["path"], name=method["name"], start=method["start"],
                              end=method["end"], C=signals["baseline"],
                              file_C=row["baseline_max"], file_lines=row["lines"], role=role,
                              span_lines=signals["span_lines"], counts=method["lowc"]["counts"],
                              syntax={name: signals[name] for name in SYNTAX_NAMES},
                              evidence=method["lowc"]["evidence"],
                              abstentions=method["lowc"]["abstentions"])
                assert record["span_lines"] == record["end"] - record["start"] + 1
                candidates.append(record)
                counts["eligible_retained_callables"] += 1
        populations[repository] = dict(counts=counts, retained_abstentions=abstentions,
                                       all_callable_counts=all_callable_counts,
                                       all_eligible_rule_methods=all_rule_methods,
                                       all_callable_abstentions=all_abstentions,
                                       analyzed_files=data["analyzed_files"],
                                       unscored_files=data["unscored_files"])
        del data
    assert len({row["id"] for row in candidates}) == len(candidates)
    pairs, rule_populations = choose_pairs(candidates)
    wanted = {sample_id for pair in pairs for sample_id in pair["ids"]}
    samples = {row["id"]: row for row in candidates if row["id"] in wanted}
    for sample in samples.values():
        source_path = Path(roots[sample["repository"]]) / sample["path"]
        assert source_path.resolve().is_relative_to(Path(roots[sample["repository"]]))
        sample["source_sha256"] = digest(source_path)
    packets = assign(pairs, samples)
    rubric = dict(
        instructions="Inspect the indicated callable region plus necessary nearby source; do not consult research outputs, detectors or other reviews. Do not assume a bug. Identify the behavior-changing edit obligation, including reasons this region is routine or intentional.",
        scale={"0": "routine", "1": "localized care", "2": "careful interacting invariants", "3": "intensive reasoning across many invariants"},
        required_fields=["id", *DIMENSIONS, "hardest_behavior", "change_obligation", "obligation_assessment",
                         "review_question", "counterevidence", "confidence", "actual_role", "inspection_scope", "evidence"],
        obligation_assessment_values=list(ASSESSMENTS),
        evidence="Nonempty list of {start_line, end_line, observation}; use actual source line numbers.",
        confidence_values=["low", "medium", "high"])
    for reviewer, ids in packets.items():
        visible = [{key: samples[sample_id][key] for key in
                    ("id", "repository", "path", "name", "start", "end")} for sample_id in ids]
        write(root / f"lowc-packet-{reviewer}.json", dict(roots=roots, rubric=rubric, samples=visible))
    key = dict(schema="cpath.research.phase3-lowc-sample.v1", seed=SEED,
               rule_order=list(RULES), populations=populations, rule_populations=rule_populations,
               repository_roots=roots, repository_pins=pins, pairs=pairs, samples=list(samples.values()),
               raw_report_sha256=raw_hashes, main_holdout_sha256=digest(holdout_path),
               packet_sha256={r: digest(root / f"lowc-packet-{r}.json") for r in "abc"},
               assignments={r: len(ids) for r, ids in packets.items()}, limitation=LIMITATION)
    write(root / "lowc-key.json", key)
    print(json.dumps(dict(pairs=len(pairs), unique_regions=len(samples), assignments=key["assignments"],
                         rule_populations=rule_populations), indent=2))


def pair_concordance(differences):
    wins = sum(value > 0 for value in differences)
    losses = sum(value < 0 for value in differences)
    ties = len(differences) - wins - losses
    return dict(pairs=len(differences), wins=wins, losses=losses, ties=ties,
                concordance=(wins + ties / 2) / len(differences) if differences else None)


def summarize(root):
    assert not (root / "lowc-results.json").exists(), "Refusing to replace frozen results"
    key = read(root / "lowc-key.json")
    assert key["main_holdout_sha256"] == digest(root / "holdout-key.json")
    for repository in REPOSITORIES:
        assert key["raw_report_sha256"][repository] == digest(root / "raw" / f"{repository}.json")
        pin = subprocess.check_output(["git", "-C", key["repository_roots"][repository],
                                       "rev-parse", "HEAD"], text=True).strip()
        assert pin == key["repository_pins"][repository]
    ratings, review_hashes = {}, {}
    for reviewer in "abc":
        packet_path = root / f"lowc-packet-{reviewer}.json"
        assert digest(packet_path) == key["packet_sha256"][reviewer]
        packet = read(packet_path)
        review_path = root / "reviews" / f"lowc-reviewer-{reviewer}.json"
        rows = read(review_path) if packet["samples"] else []
        assert len(rows) == len(packet["samples"])
        assert {row["id"] for row in rows} == {sample["id"] for sample in packet["samples"]}
        for row in rows:
            assert all(type(row[field]) is int and row[field] in range(4) for field in DIMENSIONS)
            assert row["confidence"] in ("low", "medium", "high")
            assert row["obligation_assessment"] in ASSESSMENTS
            assert all(row[field] for field in ("evidence", "hardest_behavior", "change_obligation",
                                               "review_question", "counterevidence", "actual_role", "inspection_scope"))
        ratings[reviewer] = {row["id"]: row for row in rows}
        review_hashes[reviewer] = digest(review_path) if rows else None
    samples = {sample["id"]: sample for sample in key["samples"]}
    for sample_id, sample in samples.items():
        source_path = Path(key["repository_roots"][sample["repository"]]) / sample["path"]
        assert digest(source_path) == sample["source_sha256"]
        sample["primary"] = ratings[sample["primary_reviewer"]][sample_id]
        if "secondary_reviewer" in sample:
            sample["secondary"] = ratings[sample["secondary_reviewer"]][sample_id]
        for rating in [sample["primary"]] + ([sample["secondary"]] if "secondary" in sample else []):
            for evidence in rating["evidence"]:
                assert 1 <= evidence["start_line"] <= evidence["end_line"] <= sample["file_lines"]
                assert evidence["observation"]
    summaries = {}
    for scope in ("all", *RULES, *REPOSITORIES):
        pairs = [pair for pair in key["pairs"] if scope == "all" or scope in (pair["rule"], pair["repository"])]
        summaries[scope] = {dimension: pair_concordance([
            samples[pair["positive"]]["primary"][dimension] - samples[pair["negative"]]["primary"][dimension]
            for pair in pairs]) for dimension in DIMENSIONS}
        summaries[scope]["baseline_C"] = pair_concordance([0] * len(pairs))
        summaries[scope]["span_control"] = {dimension: pair_concordance([
            (samples[pair["positive"]]["span_lines"] - samples[pair["negative"]]["span_lines"])
            * (samples[pair["positive"]]["primary"][dimension] - samples[pair["negative"]]["primary"][dimension])
            for pair in pairs]) for dimension in DIMENSIONS}
        for group in ("positive", "negative"):
            chosen = [samples[pair[group]]["primary"] for pair in pairs]
            summaries[scope][group] = dict(count=len(chosen),
                assessments=dict(Counter(row["obligation_assessment"] for row in chosen)),
                attention_at_least_two=sum(row["attention_priority"] >= 2 for row in chosen),
                file_baseline_at_most_five=sum(samples[pair[group]]["file_C"] <= 5 for pair in pairs))
    repeated = [sample for sample in samples.values() if "secondary" in sample]
    agreement = {dimension: dict(count=len(repeated), exact=sum(
        sample["primary"][dimension] == sample["secondary"][dimension] for sample in repeated),
        within_one=sum(abs(sample["primary"][dimension] - sample["secondary"][dimension]) <= 1
                       for sample in repeated)) for dimension in DIMENSIONS}
    overlap_pairs = [pair for pair in key["pairs"] if "secondary_reviewer" in pair]
    overlap = {which: {dimension: pair_concordance([
        samples[pair["positive"]][which][dimension] - samples[pair["negative"]][which][dimension]
        for pair in overlap_pairs]) for dimension in DIMENSIONS} for which in ("primary", "secondary")}
    write(root / "lowc-results.json", dict(**key, summary=summaries, agreement=agreement,
          overlap=overlap, reviewer_sha256=review_hashes, lowc_key_sha256=digest(root / "lowc-key.json")))
    print(json.dumps(summaries, indent=2))


if __name__ == "__main__":
    commands = dict(prepare=prepare, summarize=summarize)
    if len(sys.argv) != 3 or sys.argv[1] not in commands:
        raise SystemExit(__doc__)
    commands[sys.argv[1]](Path(sys.argv[2]))
