# SPDX-License-Identifier: MIT
"""Independent checks of combined-signal pairing controls and ordinal comparisons."""

import unittest

from combined_signal_review import COMPARATORS, VARIANTS, concordance, holdout_role, predictors


class CombinedSignalReviewTests(unittest.TestCase):
    def test_ties_are_not_directional_successes(self):
        actual = concordance([(1, 1), (-1, 1), (0, 1), (1, 0)])
        self.assertEqual(actual, dict(pairs=4, wins=1, losses=1, ties=2, concordance=0.5))
        self.assertIsNone(concordance([])["concordance"])

    def test_frozen_role_roots_and_documented_nested_snippet_limitation(self):
        self.assertEqual(holdout_role("polly", "src/Polly/Policy.cs"), "production")
        self.assertEqual(holdout_role("mediatr", "test/MediatR.Tests/Task.cs"), "test")
        self.assertEqual(holdout_role("litedb", "LiteDB/Engine/State.cs"), "production")
        self.assertEqual(holdout_role("litedb", "LiteDB.Tests/State.cs"), "test")
        self.assertIsNone(holdout_role("mediatr", "samples/MediatR.Sample/Task.cs"))
        self.assertIsNone(holdout_role("litedb", "LiteDB.Benchmarks/State.cs"))
        # Preserve the frozen sample; this is a path stratum, not a semantic role.
        self.assertEqual(holdout_role("polly", "src/Snippets/Docs/Example.cs"), "production")

    def test_predictors_use_each_complete_formula_maximum(self):
        row = dict(scores={v: dict(maximum=i, burden=1000-i) for i, v in enumerate(VARIANTS)},
                   lines=70, features={v: 3 for v in ("E", "arity", "S", "T", "Q", "H", "A")})
        values = predictors(row)
        self.assertEqual(set(values), set(COMPARATORS))
        self.assertEqual(values["C"], 0)
        self.assertEqual(values["KTH"], 8)
        self.assertEqual(values["lines"], 70)


if __name__ == "__main__":
    unittest.main()
