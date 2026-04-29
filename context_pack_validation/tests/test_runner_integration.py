import shutil
import unittest

from context_pack_validation.runner import run_validation


@unittest.skipIf(shutil.which("cargo") is None, "cargo is required for integration smoke")
class RunnerIntegrationTests(unittest.TestCase):
    def test_smoke_fixture_reports_best_case_help(self) -> None:
        outcome = run_validation(repo_limit=1, scenario_filter="smoke")
        aggregate = outcome["aggregate"]
        runs = outcome["runs"]

        self.assertGreaterEqual(aggregate["total_runs"], 1)
        self.assertTrue(any(run["verdict"] == "helps" for run in runs))
        smoke = next(run for run in runs if run["id"] == "smoke_best_auth_panic")
        self.assertTrue(smoke["hit_at_1"])
        self.assertTrue(smoke["cli_mcp_paths_match"])
        self.assertIn("context_pack", smoke["context_pack_instruction"])
        self.assertIn("fix auth panic", smoke["mcp_call_arguments"]["goal"])
        self.assertIn("bounded", smoke["mcp_tool_description"])


if __name__ == "__main__":
    unittest.main()
