import unittest

from context_pack_validation.metrics import (
    Oracle,
    RunComparison,
    classify_verdict,
    score_pack,
)


class MetricsTests(unittest.TestCase):
    def test_score_pack_reports_rank_and_precision(self) -> None:
        oracle = Oracle(
            required_files=["src/auth.rs"],
            helpful_files=["tests/auth_test.rs"],
            bad_files=["docs/auth.md"],
        )
        scored = score_pack(
            ["tests/auth_test.rs", "src/auth.rs", "docs/auth.md"],
            oracle,
            pack_tokens=120,
            baseline_tokens=420,
            pack_latency_ms=10.0,
            baseline_latency_ms=30.0,
            pack_tool_calls=1,
            baseline_tool_calls=4,
        )

        self.assertFalse(scored.hit_at_1)
        self.assertTrue(scored.hit_at_4)
        self.assertAlmostEqual(scored.mrr, 0.5)
        self.assertAlmostEqual(scored.precision_at_pack, 2 / 3)
        self.assertFalse(scored.misleading_top1)
        self.assertEqual(scored.verdict, "helps")

    def test_misleading_top1_is_hurts_even_when_cheap(self) -> None:
        oracle = Oracle(required_files=["src/real.rs"], bad_files=["docs/update.md"])
        scored = score_pack(
            ["docs/update.md", "src/real.rs"],
            oracle,
            pack_tokens=50,
            baseline_tokens=500,
            pack_latency_ms=5.0,
            baseline_latency_ms=20.0,
            pack_tool_calls=1,
            baseline_tool_calls=3,
        )

        self.assertTrue(scored.misleading_top1)
        self.assertEqual(scored.verdict, "hurts")

    def test_verdict_can_be_neutral_for_missing_required_without_bad_top1(self) -> None:
        comparison = RunComparison(
            hit_at_1=False,
            hit_at_4=False,
            mrr=0.0,
            oracle_coverage=0.0,
            precision_at_pack=0.0,
            misleading_top1=False,
            expansion_needed=True,
            pack_tokens=80,
            baseline_tokens=80,
            token_reduction_ratio=0.0,
            pack_tool_calls=1,
            baseline_tool_calls=1,
            tool_call_reduction=0,
            pack_latency_ms=5.0,
            baseline_latency_ms=5.0,
            latency_ratio=1.0,
            verdict="",
        )

        self.assertEqual(classify_verdict(comparison), "neutral")


if __name__ == "__main__":
    unittest.main()
