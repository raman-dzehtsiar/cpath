# Copyright (c) 2026 Raman Dzehtsiar
# SPDX-License-Identifier: MIT
"""Independent arithmetic checks for the blind-review summary helpers."""

import unittest

from summarize_blind_validation import average_ranks, spearman


class RankStatisticsTests(unittest.TestCase):
    def test_ties_receive_average_one_based_ranks(self):
        self.assertEqual(average_ranks([30, 10, 10, 20]), [4, 1.5, 1.5, 3])

    def test_monotonic_and_reversed_order(self):
        self.assertAlmostEqual(spearman([0, 2, 9], [1, 4, 7]), 1)
        self.assertAlmostEqual(spearman([0, 2, 9], [7, 4, 1]), -1)

    def test_tied_rank_correlation_has_independent_expected_value(self):
        # Ranks x=[1.5,1.5,3], y=[1,2.5,2.5]; centered dot=0.75,
        # each squared norm=1.5, so rho=0.5.
        self.assertAlmostEqual(spearman([0, 0, 8], [0, 3, 3]), 0.5)

    def test_constant_ratings_do_not_invent_a_correlation(self):
        self.assertIsNone(spearman([0, 1, 2], [2, 2, 2]))


if __name__ == "__main__":
    unittest.main()
