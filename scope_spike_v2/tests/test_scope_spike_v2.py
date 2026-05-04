from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from scope_spike_v2.evaluate import _metrics, _ranked_files, load_repo_index


class ScopeSpikeV2Tests(unittest.TestCase):
    def test_metrics_capture_recall_and_mrr(self) -> None:
        metrics = _metrics(
            ["src/auth.rs", "src/users.rs", "tests/users.rs"],
            {"src/users.rs", "tests/users.rs"},
        )
        self.assertAlmostEqual(metrics.recall_at_5, 1.0)
        self.assertEqual(metrics.first_hit_rank, 2)
        self.assertAlmostEqual(metrics.mrr, 0.5)

    def test_ranker_prefers_auth_related_files(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            root = Path(tmpdir)
            (root / "src").mkdir()
            (root / "tests").mkdir()
            (root / "src" / "auth.rs").write_text(
                "pub fn oauth_login() {}\nuse crate::routes::users;\n"
            )
            (root / "src" / "users.rs").write_text(
                "pub fn add_oauth_to_users_endpoint() {}\n"
            )
            (root / "tests" / "users_test.rs").write_text(
                "#[test]\nfn oauth_users_route() {}\n"
            )
            (root / "README.md").write_text("project overview\n")

            index = load_repo_index(str(root.resolve()))
            ranked = _ranked_files(
                index,
                "Add OAuth to the users endpoint and update tests",
                mode="scope",
                max_files=3,
            )
            top_paths = [row.path for row in ranked]
            self.assertIn("src/users.rs", top_paths[:2])
            self.assertIn("tests/users_test.rs", top_paths)


if __name__ == "__main__":
    unittest.main()

