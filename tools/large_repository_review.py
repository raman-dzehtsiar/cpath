# SPDX-License-Identifier: MIT
"""Frozen large-repository validation: prepare|summarize STUDY_ROOT.

Reads raw/{roslyn,aspnetcore}.json and repos/{roslyn,aspnetcore}.
prepare freezes review packets and holdout-key.json; summarize validates frozen
inputs and reviewer-{a,b,c}.json before writing results.json. Outputs are never
overwritten. Optional validation-checks.json records independently completed
operational checks; absent checks cannot pass the overall research gate.
New studies fingerprint the consolidated docs/attention-ranking-results.md methodology. Historical
results retain their original protocol hashes and are not rewritten on import.
"""

import hashlib
import itertools
import json
import subprocess
import sys
from fractions import Fraction
from pathlib import Path, PurePosixPath

from combined_signal_review import DIMENSIONS, VARIANTS, sign

SEED = "cpath-phase3-large-v1-20261022"
REPOSITORIES = ("roslyn", "aspnetcore")
COMPARATORS = ("C", "B", "K", "KH", "CS", "lines", "S", "H")
ROLES = ("production", "test")
POOL_LIMIT = 512
SHARED_EXCLUDES = ("**/*.c", "**/*.h", "**/*.cpp")
EXCLUDED_DIRS = frozenset("sample samples example examples doc docs benchmark benchmarks "
    "perf performance testasset testassets testsite testsites testapplication "
    "testapplications testutilities testutils mocks ref testfiles testdata testresources".split())
TEST_DIRS = frozenset("test tests unittest unittests integrationtest integrationtests "
    "functionaltest functionaltests csharptest csharptest2 codeanalysistest coretest "
    "protocolunittests unittesting".split())
TEST_SUFFIXES = (".test", ".tests", ".unittests", ".integrationtests")
CHECK_NAMES = ("structural_controls", "coverage", "determinism", "resources")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def read(path):
    return json.loads(path.read_text(encoding="utf-8"))


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("x", encoding="utf-8") as handle:
        json.dump(value, handle, indent=2)
        handle.write("\n")


def sha(data):
    return hashlib.sha256(data).hexdigest()


def order(*parts):
    return sha("\n".join((SEED, *parts)).encode())


def role(path):
    directories = PurePosixPath(path).parts[:-1]
    folded = [part.casefold() for part in directories]
    if any(part in EXCLUDED_DIRS for part in folded):
        return None
    if any(part in TEST_DIRS or part.endswith(TEST_SUFFIXES) for part in folded):
        return "test"
    return "production" if folded and folded[0] == "src" else None


def checked_source(root, relative):
    path = PurePosixPath(relative)
    require(not path.is_absolute() and ".." not in path.parts and str(path) == relative,
            f"Noncanonical source path: {relative}")
    source = root / relative
    require(source.is_file() and source.resolve().is_relative_to(root.resolve()),
            f"Source escapes repository or is missing: {relative}")
    return source


def pin(root):
    return subprocess.check_output(["git", "-C", str(root), "rev-parse", "HEAD"], text=True).strip()


def report(root, repository):
    data = read(root / "raw" / f"{repository}.json")
    require(data["schema"] == "cpath.research.signals.v1", "Unexpected research schema")
    require(data["formula_family"] == "cpath.research.phase2-formulas.v1", "Formula drift")
    require(data["includes"] == [] and tuple(data["excludes"]) == SHARED_EXCLUDES,
            "Expected frozen C# scope without generated-selection overrides")
    require(Path(data["root"]).resolve() == (root / "repos" / repository).resolve(), "Root mismatch")
    require(data["workers"] in (1, 2), "Worker budget mismatch")
    rows = data["files"]
    require(len(rows) == data["analyzed_files"], "Analyzed count mismatch")
    require(len({r["path"] for r in rows}) == len(rows), "Duplicate scored path")
    for row in rows:
        require(set(row["scores"]) == set(VARIANTS), "Formula inventory drift")
        require(type(row["lines"]) is int and row["lines"] >= 0, "Invalid line count")
        require(all(type(value) is int and value >= 0 for value in predictors(row).values()),
                "Invalid predictor value")
    return data


def predictors(row):
    return {**{v: row["scores"][v]["maximum"] for v in COMPARATORS[:5]},
            "lines": row["lines"], "S": row["features"]["S"], "H": row["features"]["H"]}


def sample(root, repository, row):
    source = checked_source(root / "repos" / repository, row["path"])
    return dict(id=order(repository, row["path"])[:16], repository=repository,
                path=row["path"], role=role(row["path"]), lines=row["lines"],
                source_sha256=sha(source.read_bytes()), predictors=predictors(row))


def challenge_pairs(repository, eligible, used):
    candidates, pools = [], {}
    for selected_role in ROLES:
        unused = [r for r in eligible if role(r["path"]) == selected_role and r["path"] not in used]
        pool = sorted(unused, key=lambda r: order("challenge-pool", repository, r["path"]))[:POOL_LIMIT]
        count = 0
        for a, b in itertools.combinations(sorted(pool, key=lambda r: r["path"]), 2):
            if max(a["lines"], b["lines"]) * 10 > min(a["lines"], b["lines"]) * 13:
                continue
            if ((a["scores"]["C"]["maximum"] - b["scores"]["C"]["maximum"])
                    * (a["scores"]["KH"]["maximum"] - b["scores"]["KH"]["maximum"]) >= 0):
                continue
            count += 1
            candidates.append((order("challenge-pair", repository, a["path"], b["path"]), a, b))
        pools[selected_role] = dict(eligible_unused=len(unused), pool=len(pool), reversal_pairs=count)
    selected, unavailable = [], set(used)
    for _, a, b in sorted(candidates, key=lambda item: item[0]):
        if a["path"] in unavailable or b["path"] in unavailable:
            continue
        selected.append((a, b))
        unavailable.update((a["path"], b["path"]))
        if len(selected) == 2:
            break
    return selected, pools


def assignments(pairs, samples):
    pairs.sort(key=lambda p: order("assignment", p["repository"], *p["ids"]))
    packets = {reviewer: [] for reviewer in "abc"}
    for index, pair in enumerate(pairs):
        reviewer = "abc"[index % 3]
        pair["primary_reviewer"] = reviewer
        for sample_id in pair["ids"]:
            samples[sample_id].update(primary_reviewer=reviewer, stratum=pair["stratum"])
            packets[reviewer].append(sample_id)
    overlapped = set()
    for reviewer in "abc":
        for preferred in ("neutral", "challenge"):
            choices = [p for p in pairs if p["primary_reviewer"] != reviewer and tuple(p["ids"]) not in overlapped]
            # First pick must be neutral; second prefers challenge but may use neutral.
            if preferred == "neutral":
                choices = [p for p in choices if p["stratum"] == "neutral"]
            choices.sort(key=lambda p: (p["stratum"] != preferred,
                                       order("overlap", reviewer, *p["ids"])))
            if not choices:
                continue
            pair = choices[0]
            overlapped.add(tuple(pair["ids"]))
            pair["secondary_reviewer"] = reviewer
            for sample_id in pair["ids"]:
                samples[sample_id]["secondary_reviewer"] = reviewer
                packets[reviewer].append(sample_id)
    return {reviewer: [{k: samples[sample_id][k] for k in ("id", "repository", "path")}
                      for sample_id in sorted(ids, key=lambda i: order("packet", reviewer, i))]
            for reviewer, ids in packets.items()}


def prepare(root):
    outputs = [root / "holdout-key.json", *[root / "reviews" / f"packet-{r}.json" for r in "abc"]]
    require(not any(path.exists() for path in outputs), "Refusing to replace frozen selection outputs")
    samples, pairs, populations, hashes = {}, [], {}, {}
    for repository in REPOSITORIES:
        data = report(root, repository)
        hashes[repository] = sha((root / "raw" / f"{repository}.json").read_bytes())
        repo_root = root / "repos" / repository
        status = subprocess.check_output(["git", "-C", str(repo_root), "status", "--porcelain"], text=True)
        require(not status, f"Source checkout is not clean: {repository}")
        eligible = [r for r in data["files"] if r["path"].endswith(".cs") and 40 <= r["lines"] <= 800
                    and role(r["path"]) is not None]
        population = {k: data[k] for k in ("analyzed_files", "unscored_files", "failure_counts")}
        population.update(commit=pin(repo_root), root=str(repo_root.resolve()), source_checkout_clean=True,
                          excluded_or_unknown_role=sum(role(r["path"]) is None for r in data["files"]), roles={})
        used = set()

        def add_pair(a, b, stratum):
            members = [sample(root, repository, row) for row in (a, b)]
            for member in members:
                require(member["id"] not in samples, "Duplicate sample or hash collision")
                samples[member["id"]] = member
                used.add(member["path"])
            pairs.append(dict(repository=repository, role=members[0]["role"], stratum=stratum,
                              ids=[member["id"] for member in members]))

        for selected_role in ROLES:
            rows = [r for r in eligible if role(r["path"]) == selected_role]
            chosen = sorted(rows, key=lambda r: order("neutral", repository, r["path"]))[:8]
            chosen = chosen[:len(chosen) - len(chosen) % 2]
            population["roles"][selected_role] = dict(eligible=len(rows), selected=len(chosen))
            chosen.sort(key=lambda r: (r["lines"], r["path"]))
            for a, b in zip(chosen[::2], chosen[1::2]):
                add_pair(a, b, "neutral")
        selected, pools = challenge_pairs(repository, eligible, used)
        population["challenge"] = dict(pools=pools, selected_pairs=len(selected), pool_limit=POOL_LIMIT)
        for a, b in selected:
            add_pair(a, b, "challenge")
        populations[repository] = population
    packets = assignments(pairs, samples)
    for reviewer, packet in packets.items():
        write(root / "reviews" / f"packet-{reviewer}.json", packet)
    key = dict(schema="cpath.research.phase3-validation.v1", seed=SEED, frozen_candidate="KH",
               comparators=COMPARATORS, includes=[], excludes=SHARED_EXCLUDES,
               populations=populations, pairs=pairs, samples=list(samples.values()),
               raw_report_sha256=hashes,
               packet_sha256={r: sha((root / "reviews" / f"packet-{r}.json").read_bytes()) for r in "abc"},
               role_policy=dict(excluded_dirs=sorted(EXCLUDED_DIRS), test_dirs=sorted(TEST_DIRS),
                                test_suffixes=TEST_SUFFIXES),
               protocol_sha256=sha((Path(__file__).resolve().parents[1]
                   / "docs/attention-ranking-results.md").read_bytes()))
    write(root / "holdout-key.json", key)
    print(json.dumps(dict(populations=populations, assignments={r: len(p) for r, p in packets.items()}), indent=2))


def concordance(comparisons):
    wins = sum(x * y > 0 for x, y in comparisons)
    losses = sum(x * y < 0 for x, y in comparisons)
    ties = len(comparisons) - wins - losses
    return dict(pairs=len(comparisons), wins=wins, losses=losses, ties=ties,
                judgment_tied=sum(y == 0 for _, y in comparisons), predictor_tied=sum(x == 0 for x, _ in comparisons),
                both_tied=sum(x == y == 0 for x, y in comparisons),
                concordance=float(Fraction(2 * wins + ties, 2 * len(comparisons))) if comparisons else None)


def comparison(a, b, variant, dimension, rating="primary"):
    return sign(a["predictors"][variant] - b["predictors"][variant]), sign(a[rating][dimension] - b[rating][dimension])


def validate_review(row, sample_record):
    require(all(type(row.get(d)) is int and row[d] in range(4) for d in DIMENSIONS), "Invalid ordinal rating")
    require(row.get("confidence") in ("high", "medium", "low"), "Invalid confidence")
    for field in ("actual_role", "hardest_behavior", "counterevidence", "inspection_scope"):
        require(isinstance(row.get(field), str) and row[field].strip(), f"Missing review {field}")
    require(isinstance(row.get("evidence"), list) and row["evidence"], "Missing line evidence")
    for anchor in row["evidence"]:
        require(type(anchor.get("start_line")) is int and type(anchor.get("end_line")) is int
                and 1 <= anchor["start_line"] <= anchor["end_line"] <= sample_record["lines"], "Invalid source anchor")
        require(isinstance(anchor.get("observation"), str) and anchor["observation"].strip(), "Missing anchor observation")


def endpoint(pairs, samples, dimension, rating="primary"):
    return {v: concordance([comparison(samples[p["ids"][0]], samples[p["ids"][1]], v, dimension, rating)
                           for p in pairs]) for v in COMPARATORS}


def macro(table):
    result = {}
    for variant in COMPARATORS:
        rows = [table[repository][variant] for repository in REPOSITORIES]
        available = [r for r in rows if r["pairs"]]
        exact = (sum((Fraction(2 * r["wins"] + r["ties"], 2 * r["pairs"]) for r in available), Fraction())
                 / len(available)) if available else None
        result[variant] = dict(repositories=len(available), exact=str(exact) if exact is not None else None,
                               concordance=float(exact) if exact is not None else None)
    return result


def gate(neutral, checks):
    macros = neutral["macro"]
    complete = all(macros[v]["repositories"] == len(REPOSITORIES) for v in ("C", "KH", "CS"))
    beats = {v: complete and Fraction(macros["KH"]["exact"]) > Fraction(macros[v]["exact"])
             for v in ("C", "CS")}
    deltas, missing = {}, []
    for scope in (*REPOSITORIES, *ROLES):
        a, b = neutral[scope]["KH"], neutral[scope]["C"]
        if not a["pairs"] or not b["pairs"]:
            missing.append(scope)
            continue
        deltas[scope] = float(Fraction(2 * a["wins"] + a["ties"], 2 * a["pairs"])
                              - Fraction(2 * b["wins"] + b["ties"], 2 * b["pairs"]))
    reversals = [scope for scope, delta in deltas.items() if delta < 0]
    statistical = complete and all(beats.values()) and not reversals and not missing
    operational = all(checks.get(name) is True for name in CHECK_NAMES) if checks is not None else None
    return dict(frozen_candidate="KH", strictly_beats=beats, subgroup_deltas=deltas,
                observed_reversals=reversals, missing_subgroups=missing, statistical_gate=statistical,
                operational_gate=operational, promising=statistical and operational is True,
                operational_checks_pending=checks is None, supported_default="C", product_promotion=False,
                interpretation="Fresh agent agreement only; no candidate reselection, equivalence claim or product promotion.")


def summarize(root):
    require(not (root / "results.json").exists(), "Refusing to replace results")
    key_path = root / "holdout-key.json"
    key = read(key_path)
    require(key["schema"] == "cpath.research.phase3-validation.v1" and key["seed"] == SEED
            and key["frozen_candidate"] == "KH" and tuple(key["comparators"]) == COMPARATORS, "Frozen protocol mismatch")
    protocol_path = Path(__file__).resolve().parents[1] / "docs/attention-ranking-results.md"
    require(key["protocol_sha256"] == sha(protocol_path.read_bytes()), "Frozen protocol document changed")
    samples = {sample["id"]: sample for sample in key["samples"]}
    require(len(samples) == len(key["samples"]), "Duplicate frozen sample")
    for repository in REPOSITORIES:
        require(key["raw_report_sha256"][repository] == sha((root / "raw" / f"{repository}.json").read_bytes()),
                "Raw report changed")
        require(key["populations"][repository]["root"] == str((root / "repos" / repository).resolve()), "Source root changed")
        require(key["populations"][repository]["commit"] == pin(root / "repos" / repository), "Source revision changed")
    reviewer_hashes = {}
    for reviewer in "abc":
        packet_path = root / "reviews" / f"packet-{reviewer}.json"
        require(key["packet_sha256"][reviewer] == sha(packet_path.read_bytes()), "Review packet changed")
        packet = read(packet_path)
        expected = {sample_id for sample_id, record in samples.items()
                    if reviewer in (record["primary_reviewer"], record.get("secondary_reviewer"))}
        require({r["id"] for r in packet} == expected and len(packet) == len(expected), "Packet assignment mismatch")
        require(all(all(record[field] == samples[record["id"]][field] for field in ("repository", "path"))
                    for record in packet), "Packet source identity mismatch")
        rows_path = root / "reviews" / f"reviewer-{reviewer}.json"
        rows = read(rows_path)
        require(isinstance(rows, list) and len(rows) == len(packet), "Review count mismatch")
        require({r["id"] for r in rows} == {r["id"] for r in packet}
                and len({r["id"] for r in rows}) == len(rows), "Missing or duplicate review IDs")
        for row in rows:
            sample_record = samples[row["id"]]
            validate_review(row, sample_record)
            if sample_record["primary_reviewer"] == reviewer:
                sample_record["primary"] = row
            else:
                require(sample_record.get("secondary_reviewer") == reviewer, "Unexpected secondary reviewer")
                sample_record["secondary"] = row
        reviewer_hashes[reviewer] = sha(rows_path.read_bytes())
    for sample_record in samples.values():
        source = checked_source(root / "repos" / sample_record["repository"], sample_record["path"])
        require(sha(source.read_bytes()) == sample_record["source_sha256"], "Reviewed source changed")
    summary, overlap, pair_details = {}, {}, []
    for stratum in ("neutral", "challenge"):
        pairs = [p for p in key["pairs"] if p["stratum"] == stratum]
        summary[stratum], overlap[stratum] = {}, {}
        repeated = [p for p in pairs if all("secondary" in samples[i] for i in p["ids"])]
        for dimension in DIMENSIONS:
            table = {"all": endpoint(pairs, samples, dimension)}
            for scope in (*REPOSITORIES, *ROLES, "low_baseline"):
                chosen = [p for p in pairs if p["repository"] == scope or p["role"] == scope
                          or (scope == "low_baseline" and all(samples[i]["predictors"]["C"] <= 5 for i in p["ids"]))]
                table[scope] = endpoint(chosen, samples, dimension)
            table["macro"] = macro(table)
            summary[stratum][dimension] = table
            directions = [tuple(sign(samples[p["ids"][0]][rating][dimension] - samples[p["ids"][1]][rating][dimension])
                                for rating in ("primary", "secondary")) for p in repeated]
            overlap[stratum][dimension] = dict(pairs=len(repeated),
                judgment_direction_agreement=sum(a == b for a, b in directions),
                primary=endpoint(repeated, samples, dimension), secondary=endpoint(repeated, samples, dimension, "secondary"))
            repeated_result = overlap[stratum][dimension]
            deltas = {rating: (repeated_result[rating]["KH"]["concordance"]
                              - repeated_result[rating]["C"]["concordance"]) if repeated else None
                      for rating in ("primary", "secondary")}
            repeated_result["KH_minus_C"] = deltas
            repeated_result["candidate_reference_direction_changes"] = (
                sign(deltas["primary"]) != sign(deltas["secondary"])) if repeated else None
        for pair in pairs:
            a, b = [samples[i] for i in pair["ids"]]
            pair_details.append(dict(**pair, comparisons={dimension: {v: dict(predictor_direction=x,
                judgment_direction=y, credit=0.5 if x == 0 or y == 0 else int(x == y))
                for v in COMPARATORS for x, y in [comparison(a, b, v, dimension)]} for dimension in DIMENSIONS}))
    repeats = [s for s in samples.values() if "secondary" in s]
    agreement = {d: dict(count=len(repeats), exact=sum(s["primary"][d] == s["secondary"][d] for s in repeats),
                        within_one=sum(abs(s["primary"][d] - s["secondary"][d]) <= 1 for s in repeats)) for d in DIMENSIONS}
    checks_path = root / "validation-checks.json"
    checks = read(checks_path) if checks_path.exists() else None
    if checks is not None:
        require(all(type(checks.get(name)) is bool for name in CHECK_NAMES) and checks.get("evidence"),
                "Operational checks require booleans and evidence")
    decision = gate(summary["neutral"]["attention_priority"], checks)
    result = dict(**key, summary=summary, overlap=overlap, agreement=agreement, pair_details=pair_details,
                  decision=decision, reviewer_sha256=reviewer_hashes, holdout_key_sha256=sha(key_path.read_bytes()),
                  validation_checks=checks, validation_checks_sha256=sha(checks_path.read_bytes()) if checks is not None else None,
                  primary_dimensions_identical=sum(s["primary"][DIMENSIONS[0]] == s["primary"][DIMENSIONS[1]] for s in samples.values()),
                  limitation="Agent judgments on a bounded fresh sample; large scan coverage does not establish full-ranking usefulness.")
    write(root / "results.json", result)
    print(json.dumps(dict(decision=decision, neutral=summary["neutral"]["attention_priority"]["macro"]), indent=2))


if __name__ == "__main__":
    commands = {"prepare": prepare, "summarize": summarize}
    if len(sys.argv) != 3 or sys.argv[1] not in commands:
        raise SystemExit(__doc__)
    commands[sys.argv[1]](Path(sys.argv[2]).resolve())
