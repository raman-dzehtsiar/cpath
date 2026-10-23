# SPDX-License-Identifier: MIT
"""Independent checks of large-repository selection, uncertainty and admission gates."""

import unittest
from pathlib import Path
from unittest.mock import patch

from large_repository_review import (CHECK_NAMES, COMPARATORS, REPOSITORIES, assignments,
                            challenge_pairs, concordance, gate, macro, report, role, validate_review)


def row(path, c, kh, lines=100):
    return dict(path=path, lines=lines, scores={"C": {"maximum": c}, "KH": {"maximum": kh}})


class LargeRepositoryReviewTests(unittest.TestCase):
    def test_shared_exclusions_preserve_default_generated_selection(self):
        root = Path("/synthetic/large-repository-review")
        data = dict(schema="cpath.research.signals.v1", formula_family="cpath.research.phase2-formulas.v1",
                    includes=[], excludes=["**/*.c", "**/*.h", "**/*.cpp"],
                    root=str(root / "repos/roslyn"), workers=2, analyzed_files=0, files=[],
                    evidence_formula=None, evidence_selection="lowc", lowc_contract="research")
        with patch("large_repository_review.read", return_value=data):
            self.assertIs(report(root, "roslyn"), data)
            data["includes"] = ["**/*.cs"]
            with self.assertRaisesRegex(ValueError, "scope"):
                report(root, "roslyn")
            data["includes"] = []
            data["excludes"] = []
            with self.assertRaisesRegex(ValueError, "scope"):
                report(root, "roslyn")

    def test_role_exclusions_precede_test_and_production(self):
        for segment in ("CSharpTest", "CSharpTest2", "CodeAnalysisTest", "CoreTest",
                        "ProtocolUnitTests", "UnitTesting", "Feature.UnitTests", "Feature.IntegrationTests"):
            self.assertEqual(role(f"src/{segment}/Behavior.cs"), "test")
        for segment in ("docs", "samples", "TestFiles", "testdata", "testresources", "ref"):
            self.assertIsNone(role(f"src/Tests/{segment}/Behavior.cs"))
        self.assertEqual(role("src/Compiler/TestingBehavior.cs"), "production")
        self.assertIsNone(role("eng/Compiler.cs"))

    def test_challenge_pool_is_bounded_disjoint_and_requires_strict_reversal(self):
        values = [row(f"src/a{i}.cs", 5, 5) for i in range(30)]
        values += [row("src/reverse.cs", 2, 8), row("src/tied.cs", 5, 9)]
        with patch("large_repository_review.POOL_LIMIT", 4):
            pairs, pools = challenge_pairs("roslyn", values, {"src/reverse.cs"})
        self.assertEqual(pools["production"]["pool"], 4)
        self.assertEqual(pools["production"]["eligible_unused"], 31)
        self.assertEqual(pairs, [])
        pairs, _ = challenge_pairs("roslyn", [row("src/a.cs", 1, 8), row("src/b.cs", 5, 2),
                                             row("src/c.cs", 9, 0, 131)], set())
        self.assertEqual(len(pairs), 1)
        self.assertEqual({r["path"] for r in pairs[0]}, {"src/a.cs", "src/b.cs"})

    def test_pair_assignments_preserve_blinding_and_pair_complete_overlap(self):
        pairs, samples = [], {}
        for index in range(20):
            ids = [f"{index}a", f"{index}b"]
            pairs.append(dict(repository=REPOSITORIES[index % 2], role="production",
                              stratum="challenge" if index < 4 else "neutral", ids=ids))
            for sample_id in ids:
                samples[sample_id] = dict(id=sample_id, repository=REPOSITORIES[index % 2], path=f"src/{sample_id}.cs")
        packets = assignments(pairs, samples)
        self.assertEqual(sum(map(len, packets.values())), 52)
        self.assertEqual(sum("secondary_reviewer" in pair for pair in pairs), 6)
        for reviewer, packet in packets.items():
            self.assertTrue(all(set(record) == {"id", "repository", "path"} for record in packet))
            secondary = [pair for pair in pairs if pair.get("secondary_reviewer") == reviewer]
            self.assertEqual(len(secondary), 2)
            self.assertTrue(any(pair["stratum"] == "neutral" for pair in secondary))
            for pair in secondary:
                self.assertNotEqual(pair["primary_reviewer"], reviewer)
                self.assertTrue(all(samples[i]["secondary_reviewer"] == reviewer for i in pair["ids"]))

    def test_macro_weights_repositories_equally_and_distinguishes_ties(self):
        small = concordance([(1, 1)])
        large = concordance([(-1, 1)] * 9)
        table = {repository: {v: value for v in COMPARATORS}
                 for repository, value in zip(REPOSITORIES, (small, large))}
        self.assertEqual(macro(table)["C"]["exact"], "1/2")
        ties = concordance([(1, 1), (-1, 1), (0, 1), (1, 0), (0, 0)])
        self.assertEqual((ties["wins"], ties["losses"], ties["ties"]), (1, 1, 3))
        self.assertEqual((ties["judgment_tied"], ties["predictor_tied"], ties["both_tied"]), (2, 2, 1))

    def test_gate_does_not_promote_reselected_or_unverified_candidate(self):
        baseline, better = concordance([(1, 1), (-1, 1)]), concordance([(1, 1), (1, 1)])
        table = {scope: {v: better if v == "KH" else baseline for v in COMPARATORS}
                 for scope in (*REPOSITORIES, "production", "test")}
        table["macro"] = macro(table)
        pending = gate(table, None)
        self.assertTrue(pending["statistical_gate"])
        self.assertFalse(pending["promising"])
        self.assertTrue(pending["operational_checks_pending"])
        checks = dict.fromkeys(CHECK_NAMES, True)
        self.assertTrue(gate(table, checks)["promising"])
        self.assertFalse(gate(table, checks)["product_promotion"])
        table["test"]["KH"] = concordance([(-1, 1), (-1, 1)])
        self.assertEqual(gate(table, checks)["observed_reversals"], ["test"])
        self.assertFalse(gate(table, checks)["promising"])

    def test_review_rejects_boolean_rating_and_out_of_source_anchor(self):
        rating = dict(attention_priority=1, review_difficulty=2, confidence="medium",
                      actual_role="production", hardest_behavior="ordering", counterevidence="single caller",
                      inspection_scope="whole file", evidence=[dict(start_line=1, end_line=5, observation="shared state")])
        validate_review(rating, dict(lines=5))
        with self.assertRaisesRegex(ValueError, "anchor"):
            validate_review(rating, dict(lines=4))
        rating["attention_priority"] = True
        with self.assertRaisesRegex(ValueError, "ordinal"):
            validate_review(rating, dict(lines=5))


if __name__ == "__main__":
    unittest.main()
