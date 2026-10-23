# SPDX-License-Identifier: MIT
"""Focused checks of frozen low-C matching and blinded packet allocation."""

import unittest
from unittest.mock import patch

from low_complexity_review import (
    RULES, SYNTAX, assign, can_pair, choose_pairs, eligible, identity, pair_concordance,
)


def candidate(identifier, repository="roslyn", rule=None, syntax=None, **overrides):
    value = dict(id=identifier, repository=repository, path=f"src/{identifier}.cs",
                 role="production", C=1, span_lines=10,
                 counts={name: int(name == rule) for name in RULES},
                 syntax={name: int(name == syntax) for name in set(SYNTAX.values())})
    value.update(overrides)
    return value


class LowCResearchTests(unittest.TestCase):
    def test_same_line_lambdas_have_distinct_stable_identities(self):
        method = dict(name="<lambda>", start=12, end=12)
        first = identity("roslyn", "src/File.cs", method, 0)
        self.assertEqual(first, identity("roslyn", "src/File.cs", method, 0))
        self.assertNotEqual(first, identity("roslyn", "src/File.cs", method, 1))

    def test_hard_limits_and_incomplete_are_not_zero(self):
        method = dict(signals=dict(baseline=5, span_lines=40), lowc=dict(incomplete=False))
        self.assertTrue(eligible(method))
        method["lowc"]["incomplete"] = True
        self.assertFalse(eligible(method))
        method["lowc"]["incomplete"] = False
        method["signals"]["baseline"] = 6
        self.assertFalse(eligible(method))
        method["signals"].update(baseline=0, span_lines=41)
        self.assertFalse(eligible(method))

    def test_match_requires_same_syntax_not_just_same_complexity(self):
        positive = candidate("p", rule="W", syntax="awaits")
        self.assertFalse(can_pair(positive, candidate("n"), "W"))
        self.assertTrue(can_pair(positive, candidate("n", syntax="awaits"), "W"))
        self.assertFalse(can_pair(positive, candidate("n", syntax="awaits", C=0), "W"))
        self.assertFalse(can_pair(positive, candidate("n", syntax="awaits", span_lines=21), "W"))
        self.assertFalse(can_pair(positive, candidate("n", syntax="awaits", role="test"), "W"))
        self.assertFalse(can_pair(positive, candidate("n", syntax="awaits", rule="E"), "W"))

    def test_prefer_both_repositories_and_deterministic_under_input_order(self):
        candidates = [candidate("rp1", rule="E"), candidate("rp2", rule="E"),
                      candidate("rn1", syntax="catches"), candidate("rn2", syntax="catches"),
                      candidate("ap", repository="aspnetcore", rule="E"),
                      candidate("an", repository="aspnetcore", syntax="catches")]
        first, _ = choose_pairs(candidates)
        second, _ = choose_pairs(list(reversed(candidates)))
        self.assertEqual(first, second)
        self.assertEqual(len(first), 2)
        self.assertEqual({pair["repository"] for pair in first}, {"roslyn", "aspnetcore"})

    def test_scarcity_is_not_relaxed_and_fallback_is_same_repository(self):
        candidates = [candidate("p1", rule="R"), candidate("p2", rule="R"),
                      candidate("n1", syntax="usings"), candidate("n2", syntax="usings")]
        pairs, populations = choose_pairs(candidates)
        self.assertEqual(len(pairs), 2)
        self.assertEqual(populations["R"]["aspnetcore"]["selected_pairs"], 0)
        self.assertEqual(populations["R"]["roslyn"]["selected_pairs"], 2)
        pairs, _ = choose_pairs([candidate("p", rule="F"), candidate("n", syntax="usings")])
        self.assertEqual(pairs, [])

    def test_same_file_is_not_reused_across_rules(self):
        positive = candidate("both", rule="R")
        positive["counts"]["E"] = 1
        candidates = [positive, candidate("rnegative", syntax="usings"),
                      candidate("enegative", syntax="catches"),
                      candidate("other-method", rule="E", path=positive["path"])]
        pairs, _ = choose_pairs(candidates)
        self.assertEqual(len(pairs), 1)
        self.assertEqual(pairs[0]["rule"], "R")

    def test_assignments_preserve_pairs_and_overlap_budget(self):
        pairs = [dict(rule=rule, repository=repo, role="production",
                      positive=f"{repo}-{rule}-p", negative=f"{repo}-{rule}-n",
                      ids=[f"{repo}-{rule}-p", f"{repo}-{rule}-n"])
                 for repo in ("roslyn", "aspnetcore") for rule in RULES]
        samples = {sample_id: dict(id=sample_id) for pair in pairs for sample_id in pair["ids"]}
        packets = assign(pairs, samples)
        self.assertEqual(sum(map(len, packets.values())), 28)
        self.assertEqual(sum("secondary_reviewer" in pair for pair in pairs), 2)
        for pair in pairs:
            for key in ("primary_reviewer", "secondary_reviewer"):
                if key in pair:
                    self.assertTrue(set(pair["ids"]).issubset(packets[pair[key]]))
            if "secondary_reviewer" in pair:
                self.assertNotEqual(pair["primary_reviewer"], pair["secondary_reviewer"])

    def test_ties_are_not_directional_wins(self):
        self.assertEqual(pair_concordance([2, -1, 0, 0]),
                         dict(pairs=4, wins=1, losses=1, ties=2, concordance=0.5))
        self.assertIsNone(pair_concordance([])["concordance"])

    def test_sixth_rule_requires_lock_present_control(self):
        candidates = [candidate("p", rule="D", syntax="locks", C=0),
                      candidate("plain", C=0), candidate("locked", syntax="locks", C=0)]
        pairs, _ = choose_pairs(candidates)
        self.assertEqual(len(pairs), 1)
        self.assertEqual(pairs[0]["rule"], "D")
        self.assertEqual(pairs[0]["negative"], "locked")

    def test_unmatched_syntax_buckets_do_not_do_all_pairs_checks(self):
        candidates = [candidate(f"p{i}", rule="D", syntax="locks") for i in range(100)]
        candidates += [candidate(f"n{i}", syntax="usings") for i in range(1000)]
        with patch("low_complexity_review.can_pair", side_effect=AssertionError("All-pairs check")):
            pairs, _ = choose_pairs(candidates)
        self.assertEqual(pairs, [])


if __name__ == "__main__":
    unittest.main()
