import tempfile
import unittest
from pathlib import Path

from context_pack_validation.report import aggregate_runs, render_public_summary
from context_pack_validation.runner import load_scenarios


class ScenarioAndReportTests(unittest.TestCase):
    def test_load_scenarios_validates_required_fields(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            scenario_file = Path(tmp) / "scenarios.yaml"
            scenario_file.write_text(
                """
{
  "version": 1,
  "scenarios": [
    {
      "id": "best_auth_panic",
      "group": "best",
      "repo": "fixture:best_auth",
      "agent_prompt": "Fix the auth panic for service accounts. Start by asking TriSeek for a small context pack.",
      "goal": "fix auth panic",
      "intent": "bugfix",
      "oracle": {
        "required_files": ["src/auth.rs"],
        "helpful_files": ["tests/auth_test.rs"],
        "bad_files": []
      },
      "baseline_steps": [
        {"tool": "search_content", "arguments": {"query": "auth", "mode": "literal"}}
      ]
    }
  ]
}
""",
                encoding="utf-8",
            )

            scenarios = load_scenarios(scenario_file)

        self.assertEqual(len(scenarios), 1)
        self.assertEqual(scenarios[0].id, "best_auth_panic")
        self.assertIn("context pack", scenarios[0].agent_prompt)
        self.assertEqual(scenarios[0].oracle.required_files, ["src/auth.rs"])

    def test_public_summary_includes_help_and_failure_breakdowns(self) -> None:
        runs = [
            {
                "id": "best_auth_panic",
                "group": "best",
                "verdict": "helps",
                "hit_at_4": True,
                "pack_tokens": 100,
                "baseline_tokens": 400,
                "pack_tool_calls": 1,
                "baseline_tool_calls": 4,
            },
            {
                "id": "worst_vague_update",
                "group": "worst",
                "verdict": "hurts",
                "hit_at_4": False,
                "pack_tokens": 100,
                "baseline_tokens": 80,
                "pack_tool_calls": 1,
                "baseline_tool_calls": 1,
            },
        ]
        aggregate = aggregate_runs(runs)
        summary = render_public_summary(runs, aggregate)

        self.assertEqual(aggregate["verdict_counts"]["helps"], 1)
        self.assertEqual(aggregate["verdict_counts"]["hurts"], 1)
        self.assertIn("Where It Helps", summary)
        self.assertIn("Where It Does Not Help", summary)
        self.assertIn("best_auth_panic", summary)
        self.assertIn("worst_vague_update", summary)


if __name__ == "__main__":
    unittest.main()
