import tempfile
import unittest
from pathlib import Path
from subprocess import CompletedProcess

from context_pack_validation.agent_smoke import (
    extract_codex_tool_uses,
    extract_claude_tool_uses,
    run_agent_smoke,
)


class AgentSmokeTests(unittest.TestCase):
    def test_disabled_agent_smoke_records_skip(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            result = run_agent_smoke(results_root=Path(tmp) / "skip")

        self.assertTrue(result["skipped"])
        self.assertIn("optional", result["reason"])

    def test_extract_claude_tool_uses_from_stream_json(self) -> None:
        stream = "\n".join(
            [
                '{"type":"assistant","message":{"content":[{"type":"tool_use","name":"mcp__triseek__context_pack"}]}}',
                '{"type":"assistant","message":{"content":[{"type":"tool_use","name":"mcp__triseek__search_content"}]}}',
                '{"type":"result","subtype":"success"}',
            ]
        )

        self.assertEqual(
            extract_claude_tool_uses(stream),
            ["mcp__triseek__context_pack", "mcp__triseek__search_content"],
        )

    def test_extract_codex_tool_uses_from_json_events(self) -> None:
        stream = "\n".join(
            [
                '{"type":"thread.started","thread_id":"t"}',
                '{"type":"item.started","item":{"id":"a","type":"mcp_tool_call","server":"triseek","tool":"context_pack","status":"in_progress"}}',
                '{"type":"item.completed","item":{"id":"a","type":"mcp_tool_call","server":"triseek","tool":"context_pack","status":"completed","result":{}}}',
                '{"type":"item.completed","item":{"id":"b","type":"mcp_tool_call","server":"triseek","tool":"find_files","status":"failed","error":{"message":"cancelled"}}}',
                '{"type":"item.completed","item":{"id":"c","type":"tool_call","name":"mcp__triseek__search_content","status":"completed","result":{}}}',
                '{"type":"item.completed","item":{"type":"agent_message","text":"done"}}',
            ]
        )

        self.assertEqual(
            extract_codex_tool_uses(stream),
            ["mcp__triseek__context_pack", "mcp__triseek__search_content"],
        )

    def test_enabled_agent_smoke_runs_real_harness_command_and_records_evidence(self) -> None:
        calls = []

        def fake_runner(command, **kwargs):
            calls.append((command, kwargs))
            stdout = "\n".join(
                [
                    '{"type":"assistant","message":{"content":[{"type":"tool_use","name":"mcp__triseek__context_pack"}]}}',
                    '{"type":"assistant","message":{"content":[{"type":"tool_use","name":"mcp__triseek__search_content"}]}}',
                    '{"type":"assistant","message":{"content":[{"type":"tool_use","name":"mcp__triseek__memo_check"}]}}',
                    '{"type":"result","subtype":"success","result":"{\\"triseek_agent_smoke\\":\\"ok\\"}"}',
                ]
            )
            return CompletedProcess(command, 0, stdout=stdout, stderr="")

        with tempfile.TemporaryDirectory() as tmp:
            result = run_agent_smoke(
                results_root=Path(tmp) / "enabled",
                enable_agent_smoke=True,
                harness="claude",
                command_runner=fake_runner,
                triseek_bin=Path("/tmp/triseek"),
                manage_daemon=False,
            )

        self.assertEqual(result["aggregate"]["total_runs"], 1)
        self.assertEqual(result["aggregate"]["verdict_counts"]["helps"], 1)
        self.assertEqual(result["runs"][0]["tool_uses"], [
            "mcp__triseek__context_pack",
            "mcp__triseek__search_content",
            "mcp__triseek__memo_check",
        ])
        self.assertIn("--verbose", calls[0][0])
        self.assertIn("--mcp-config", calls[0][0])
        self.assertEqual(calls[0][1]["cwd"], result["runs"][0]["repo_root"])
        self.assertIn("TRISEEK_HOME", calls[0][1]["env"])

    def test_enabled_codex_smoke_uses_codex_exec_with_triseek_config(self) -> None:
        calls = []

        def fake_runner(command, **kwargs):
            calls.append((command, kwargs))
            stdout = "\n".join(
                [
                    '{"type":"item.started","item":{"type":"tool_call","name":"mcp__triseek__context_pack"}}',
                    '{"type":"item.completed","item":{"type":"tool_call","name":"mcp__triseek__context_pack","status":"completed","result":{}}}',
                    '{"type":"item.completed","item":{"type":"tool_call","name":"mcp__triseek__search_content","status":"completed","result":{}}}',
                    '{"type":"item.completed","item":{"type":"tool_call","name":"mcp__triseek__memo_check","status":"completed","result":{}}}',
                    '{"type":"item.completed","item":{"type":"agent_message","text":"{\\"triseek_agent_smoke\\":\\"ok\\"}"}}',
                ]
            )
            return CompletedProcess(command, 0, stdout=stdout, stderr="")

        with tempfile.TemporaryDirectory() as tmp:
            result = run_agent_smoke(
                results_root=Path(tmp) / "codex",
                enable_agent_smoke=True,
                harness="codex",
                command_runner=fake_runner,
                triseek_bin=Path("/tmp/triseek"),
                manage_daemon=False,
            )

        self.assertEqual(result["aggregate"]["verdict_counts"]["helps"], 1)
        self.assertEqual(calls[0][0][:3], ["codex", "exec", "--json"])
        self.assertIn("--ignore-user-config", calls[0][0])
        self.assertIn("--dangerously-bypass-approvals-and-sandbox", calls[0][0])
        self.assertTrue(
            any(str(arg).startswith("mcp_servers.triseek.env=") for arg in calls[0][0])
        )
        self.assertEqual(calls[0][1]["cwd"], result["runs"][0]["repo_root"])


if __name__ == "__main__":
    unittest.main()
