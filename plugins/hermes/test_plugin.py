import importlib.util
import json
import sys
import tempfile
import types
import unittest
from pathlib import Path


memory_provider = types.ModuleType("agent.memory_provider")
memory_provider.MemoryProvider = object
agent = types.ModuleType("agent")
agent.memory_provider = memory_provider
registry = types.ModuleType("tools.registry")
registry.tool_error = lambda message: json.dumps({"error": message})
tools = types.ModuleType("tools")
tools.registry = registry
sys.modules.update(
    {
        "agent": agent,
        "agent.memory_provider": memory_provider,
        "tools": tools,
        "tools.registry": registry,
    }
)
spec = importlib.util.spec_from_file_location("zkr_hermes", Path(__file__).with_name("__init__.py"))
plugin = importlib.util.module_from_spec(spec)
spec.loader.exec_module(plugin)


class PluginTest(unittest.TestCase):
    def test_native_tool_mapping_and_prefetch(self):
        provider = plugin.ZkrMemoryProvider()
        calls = []

        def run(command, payload):
            calls.append((command, payload))
            if command == "search":
                return {
                    "items": [
                        {
                            "excerpt": "User prefers concise reports",
                            "evidence_ids": ["e-1"],
                        }
                    ]
                }
            return {"id": "ok"}

        provider._run = run
        result = json.loads(
            provider.handle_tool_call(
                "zkr_reflect",
                {"day": "2026-07-21", "summary": "Built zkr", "evidence_ids": ["e-1"]},
            )
        )
        self.assertEqual(result, {"id": "ok"})
        self.assertEqual(calls[0][0], "review")
        self.assertIn("[evidence: e-1]", provider.prefetch("report style"))
        self.assertEqual({schema["name"] for schema in provider.get_tool_schemas()}, {
            "zkr_search", "zkr_store", "zkr_correct", "zkr_delete", "zkr_reflect"
        })

    def test_compiled_cli_sqlite_round_trip(self):
        binary = Path(__file__).parents[2] / "target" / "debug" / "zkr"
        self.assertTrue(binary.is_file(), "run cargo build --bin zkr first")
        with tempfile.TemporaryDirectory() as directory:
            provider = plugin.ZkrMemoryProvider()
            provider._binary = str(binary)
            provider._db = Path(directory) / "memory.db"
            provider._tenant_id = "test-tenant"
            provider._person_id = "test-person"

            stored = json.loads(provider.handle_tool_call("zkr_store", {
                "content": "The user prefers concise reports",
                "subject": "user",
                "predicate": "prefers",
                "value": "concise reports",
                "occurred_at": 1_700_000_000,
            }))
            recalled = json.loads(provider.handle_tool_call("zkr_search", {"query": "concise"}))
            self.assertEqual(recalled["items"][0]["memory"]["id"], stored["claim_id"])

            corrected = json.loads(provider.handle_tool_call("zkr_correct", {
                "claim_id": stored["claim_id"],
                "content": "The user now prefers detailed reports",
                "value": "detailed reports",
                "occurred_at": 1_700_000_001,
            }))
            reflected = json.loads(provider.handle_tool_call("zkr_reflect", {
                "day": "2026-07-21",
                "summary": "The reporting preference changed.",
                "evidence_ids": [corrected["evidence_id"]],
                "recorded_at": 1_700_000_002,
            }))
            deleted = json.loads(provider.handle_tool_call("zkr_delete", {
                "source_id": corrected["source_id"],
                "deleted_at": 1_700_000_003,
            }))

            self.assertTrue(reflected["id"])
            self.assertEqual(deleted["claim_count"], 1)
            self.assertEqual(
                json.loads(provider.handle_tool_call("zkr_search", {"query": "detailed"}))["items"],
                [],
            )


if __name__ == "__main__":
    unittest.main()
