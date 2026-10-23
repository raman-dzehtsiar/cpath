# SPDX-License-Identifier: MIT
"""Check matching boundaries and statistical tie handling independently."""

import unittest
from pathlib import Path
from unittest.mock import patch

from size_matched_review import compare, matched_pairs, report


def row(path, lines, score):
    return dict(path=path, lines=lines, score=score)


class MatchedResearchTests(unittest.TestCase):
    def test_size_and_score_boundaries_do_not_relax_to_fill_sample(self):
        a = row("src/A.cs", 100, 10)
        b = row("src/B.cs", 115, 5)
        pairs, eligible, candidates = matched_pairs([a, b], "repo")
        self.assertEqual((len(pairs), eligible, candidates), (1, 2, 1))
        for other in [row("src/B.cs", 116, 5), row("src/B.cs", 100, 6),
                      row("tests/B.cs", 100, 5), row("src/B.cpp", 100, 5)]:
            self.assertEqual(matched_pairs([a, other], "repo")[0], [])

    def test_matching_is_input_order_independent_and_disjoint(self):
        rows = [row(f"src/{i}.cs", 100, i * 5) for i in range(12)]
        forward = matched_pairs(rows, "repo")[0]
        reverse = matched_pairs(list(reversed(rows)), "repo")[0]
        self.assertEqual(forward, reverse)
        self.assertEqual(len(forward), 4)
        self.assertEqual(len({r["path"] for pair in forward for r in pair}), 8)

    def test_zero_predictor_or_judgment_differences_are_ties(self):
        result = compare([(1, 1), (-1, 1), (0, 1), (1, 0)])
        self.assertEqual((result["wins"], result["losses"], result["ties"]), (1, 1, 2))
        self.assertEqual(result["concordance"], 0.5)
        self.assertEqual(result["two_sided_sign_p"], 1)
        self.assertIsNone(compare([(0, 1), (1, 0)])["two_sided_sign_p"])
        self.assertIsNone(compare([])["concordance"])

    def test_sign_test_counts_only_directional_pairs(self):
        # Four successes have exact two-sided p = 2/16, regardless of ties.
        result = compare([(1, 1)] * 4 + [(0, 1)] * 20)
        self.assertEqual(result["two_sided_sign_p"], 0.125)

    def test_report_rejects_swapped_formulas_and_duplicate_files(self):
        data = dict(formula="cpath.complexity-ranking.v1", includes=[], excludes=[],
                    workers=2, files=1, ranking=[row("A.cs", 100, 10)])
        with patch("size_matched_review.read", return_value=data):
            with self.assertRaises(ValueError):
                report(Path("."), "repo", "breadth")
            self.assertEqual(report(Path("."), "repo", "baseline"), data)
            data["ranking"].append(row("A.cs", 100, 10))
            data["files"] = 2
            with self.assertRaises(ValueError):
                report(Path("."), "repo", "baseline")


if __name__ == "__main__":
    unittest.main()
