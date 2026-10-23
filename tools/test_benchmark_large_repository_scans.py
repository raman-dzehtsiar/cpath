# SPDX-License-Identifier: MIT
"""Checks for benchmark scope, complete-score parity and immutable semantics."""

import unittest
from pathlib import Path

from benchmark_large_repository_scans import command, comparable, coverage, digest, expected_diagnostics, grouped, semantic
from large_repository_review import REPOSITORIES


class LargeRepositoryBenchmarkTests(unittest.TestCase):
    def test_component_and_flat_path_order_can_change_truncated_membership(self):
        failures = [dict(path=f"src/A/{i:02}.cs", kind="parse") for i in range(31)]
        failures += [dict(path="src/A.Z/file.cs", kind="parse"), dict(path="src/A/Z/file.cs", kind="parse")]
        product = expected_diagnostics(failures, False)
        research = expected_diagnostics(failures, True)
        self.assertEqual(len(product), 32)
        self.assertEqual(len(research), 32)
        self.assertIn("src/A/Z/file.cs", [row["path"] for row in product])
        self.assertNotIn("src/A.Z/file.cs", [row["path"] for row in product])
        self.assertIn("src/A.Z/file.cs", [row["path"] for row in research])
        self.assertNotIn("src/A/Z/file.cs", [row["path"] for row in research])

    def test_shared_scope_and_lowc_mode_are_explicit(self):
        research = command(Path("tool"), Path("repo"), 2, True)
        product = command(Path("tool"), Path("repo"), 1, False)
        expected = ["--exclude", "**/*.c", "--exclude", "**/*.h", "--exclude", "**/*.cpp"]
        self.assertEqual(research, ["tool", "repo", "2", "--lowc", *expected])
        self.assertEqual(product, ["tool", "repo", "1", *expected])

    def test_complete_scores_determine_parity_not_retained_evidence(self):
        research = dict(analyzed_files=1, files=[dict(path="x.cs", baseline_max=20, baseline_total=45,
                        scores={"C": dict(maximum=20, burden=8)}, methods=[dict(score=0)])])
        product = dict(files=1, ranking=[dict(path="x.cs", complexity=20, complexity_total=45, complexity_burden=8)])
        self.assertEqual(comparable(research, True), comparable(product, False))
        research["files"][0]["scores"]["C"]["maximum"] = 19
        with self.assertRaisesRegex(ValueError, "baseline"):
            comparable(research, True)

    def test_semantics_exclude_only_worker_and_time_metadata(self):
        first = dict(workers=1, seconds={"total": 1.0}, methods=[dict(rule="D", line=2)], includes=[])
        second = dict(first, workers=2, seconds={"total": 0.5})
        self.assertEqual(digest(semantic(first)), digest(semantic(second)))
        second["methods"] = [dict(rule="D", line=3)]
        self.assertNotEqual(digest(semantic(first)), digest(semantic(second)))
        a = dict(unscored_files=1, failure_counts={"Parse": 1}, diagnostics=[dict(path="bad.cs", kind="Parse", message="A")])
        b = dict(a, diagnostics=[dict(path="bad.cs", kind="Parse", message="B")])
        self.assertEqual(coverage(a), coverage(b))
        self.assertNotEqual(digest(semantic(a)), digest(semantic(b)))

    def test_groups_require_three_complete_repetitions(self):
        rows = [dict(repository=repository, tool=tool, workers=workers,
                     pipeline_seconds=value, wall_seconds=value, user_seconds=value,
                     system_seconds=0, peak_rss_kib=value * 100)
                for repository in REPOSITORIES for tool in ("inspect", "syntax_signal_experiment")
                for workers in (1, 2) for value in (3, 1, 2)]
        result = grouped(rows)
        self.assertEqual(len(rows), 24)
        self.assertEqual(len(result), 8)
        self.assertTrue(all(group["median_pipeline_seconds"] == 2 and group["max_peak_rss_kib"] == 300
                            for group in result))
        with self.assertRaisesRegex(ValueError, "Incomplete"):
            grouped(rows[:-1])


if __name__ == "__main__":
    unittest.main()
