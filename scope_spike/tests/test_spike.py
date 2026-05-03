from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from scope_spike.analyze import analyze_trace
from scope_spike.capture import parse_claude_session
from scope_spike.oracle import build_oracle
from scope_spike.report import generate_report
from scope_spike.spike_runner import run_trace


FIXTURE = Path(__file__).parent / "fixtures" / "sample_trace.jsonl"


class ScopeSpikeTests(unittest.TestCase):
    def test_parse_claude_session_extracts_tool_calls(self) -> None:
        parsed = parse_claude_session(FIXTURE)
        self.assertEqual(parsed.session_id, "sample-session")
        self.assertEqual(parsed.cwd, "/repo")
        self.assertEqual(len(parsed.tool_calls), 6)
        self.assertEqual(parsed.tool_calls[0].name, "Read")
        self.assertEqual(parsed.tool_calls[-1].name, "Edit")

    def test_analyze_trace_classifies_useful_and_redundant_reads(self) -> None:
        parsed = parse_claude_session(FIXTURE)
        analysis = analyze_trace(parsed)
        self.assertEqual(analysis["files_useful"], 1)
        self.assertGreaterEqual(analysis["files_wasted"], 3)
        self.assertGreater(analysis["redundant_read_tokens"], 0)
        self.assertGreater(analysis["wasted_read_tokens"], 0)
        core = next(item for item in analysis["file_details"] if item["path"] == "src/core.py")
        self.assertTrue(core["was_useful"])
        self.assertGreaterEqual(core["read_count"], 3)

    def test_oracle_and_report_are_generated(self) -> None:
        parsed = parse_claude_session(FIXTURE)
        analysis = analyze_trace(parsed)
        oracle = build_oracle(analysis)
        self.assertIn(oracle["verdict"], {"PASS", "MARGINAL", "FAIL"})
        self.assertGreaterEqual(oracle["eliminated_tokens"], 0)
        report = generate_report(
            task_description="sample trace",
            label="fixture",
            trace_analysis=analysis,
            oracle=oracle,
        )
        self.assertIn("Scope Validation Report", report)
        self.assertIn("ORACLE (perfect Scope)", report)

    def test_runner_writes_json_and_markdown_reports(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            result = run_trace(
                trace_path=FIXTURE,
                task_description="sample trace",
                label="fixture-run",
                results_dir=tmpdir,
            )
            json_path = Path(result["json_path"])
            md_path = Path(result["md_path"])
            self.assertTrue(json_path.exists())
            self.assertTrue(md_path.exists())
            payload = json.loads(json_path.read_text())
            self.assertEqual(payload["label"], "fixture-run")
            self.assertIn("oracle", payload)


if __name__ == "__main__":
    unittest.main()
