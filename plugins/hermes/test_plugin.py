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

    def test_configured_scope_cannot_be_overridden_by_tool_payload(self):
        provider = plugin.ZkrMemoryProvider()
        provider._tenant_id = "configured-tenant"
        provider._person_id = "configured-person"
        self.assertEqual(
            provider._scope({"tenant_id": "other", "person_id": "other", "query": "memory"}),
            {
                "tenant_id": "configured-tenant",
                "person_id": "configured-person",
                "query": "memory",
            },
        )

    def test_uses_explicit_bitemporal_tool_fields(self):
        schemas = {schema["name"]: schema for schema in plugin.SCHEMAS}
        store = schemas["zkr_store"]["parameters"]["properties"]
        correct = schemas["zkr_correct"]["parameters"]["properties"]
        self.assertIn("captured_at", store)
        self.assertIn("valid_from", store)
        self.assertIn("recorded_at", store)
        self.assertNotIn("valid_at", store)
        self.assertIn("valid_at", correct)
        self.assertIn("recorded_at", correct)
        self.assertNotIn("occurred_at", correct)

    def test_store_preserves_captured_valid_and_recorded_times(self):
        provider = plugin.ZkrMemoryProvider()
        calls = []

        def run(command, payload):
            calls.append((command, payload))
            return {"id": "ok"}

        provider._run = run
        provider.handle_tool_call(
            "zkr_store",
            {
                "content": "The user moved in 2020.",
                "captured_at": 1_700_000_000,
                "valid_from": 1_600_000_000,
                "recorded_at": 1_700_000_100,
            },
        )
        self.assertEqual(
            calls,
            [
                (
                    "remember",
                    {
                        "kind": "conversation",
                        "text": "The user moved in 2020.",
                        "captured_at": 1_700_000_000,
                        "recorded_at": 1_700_000_100,
                        "claim": {
                            "subject": "user",
                            "predicate": "remembers",
                            "value": "The user moved in 2020.",
                            "kind": "fact",
                            "valid_from": 1_600_000_000,
                        },
                    },
                )
            ],
        )

    def test_tool_errors_are_redacted(self):
        provider = plugin.ZkrMemoryProvider()

        def run(command, payload):
            raise RuntimeError("sensitive source evidence")

        provider._run = run
        result = json.loads(provider.handle_tool_call("zkr_search", {"query": "memory"}))
        self.assertEqual(result, {"error": "zkr operation failed"})


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
                "captured_at": 1_700_000_000,
                "valid_from": 1_700_000_000,
                "recorded_at": 1_700_000_000,
            }))
            recalled = json.loads(provider.handle_tool_call("zkr_search", {"query": "concise"}))
            self.assertEqual(recalled["items"][0]["memory"]["id"], stored["claim_id"])

            corrected = json.loads(provider.handle_tool_call("zkr_correct", {
                "claim_id": stored["claim_id"],
                "content": "The user now prefers detailed reports",
                "value": "detailed reports",
                "valid_at": 1_700_000_001,
                "recorded_at": 1_700_000_001,
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

    def test_turn_queue_recovers_and_flushes_without_crossing_contexts(self):
        binary = Path(__file__).parents[2] / "target" / "debug" / "zkr"
        self.assertTrue(binary.is_file(), "run cargo build --bin zkr first")
        with tempfile.TemporaryDirectory() as directory:
            failed = plugin.ZkrMemoryProvider()
            failed._binary = str(Path(directory) / "missing-zkr")
            failed.initialize("session", hermes_home=directory, agent_context="primary")
            failed.sync_turn("Remember amber", "I will")
            failed.shutdown()
            with plugin._connection(failed._queue_path) as connection:
                self.assertEqual(
                    connection.execute("SELECT count(*) FROM pending_turns").fetchone()[0],
                    1,
                )

            recovered = plugin.ZkrMemoryProvider()
            recovered._binary = str(binary)
            recovered.initialize("session", hermes_home=directory, agent_context="primary")
            recovered.shutdown()
            self.assertIn("Remember amber", recovered.prefetch("amber"))
            with plugin._connection(recovered._queue_path) as connection:
                self.assertEqual(
                    connection.execute("SELECT count(*) FROM pending_turns").fetchone()[0],
                    0,
                )

    def test_queue_replay_is_idempotent_and_poison_does_not_block(self):
        binary = Path(__file__).parents[2] / "target" / "debug" / "zkr"
        with tempfile.TemporaryDirectory() as directory:
            provider = plugin.ZkrMemoryProvider()
            provider._binary = str(binary)
            provider._db = Path(directory) / "memory.db"
            provider._queue_path = Path(f"{provider._db}.queue")
            provider._ensure_queue()
            payload = provider._scope({
                "kind": "conversation",
                "text": "User: replay me\nAssistant: done",
                "captured_at": 1_700_000_000,
                "recorded_at": 1_700_000_000,
                "ingestion_key": "hermes-turn:stable",
                "claim": None,
            })
            with plugin._connection(provider._queue_path) as connection:
                connection.execute("INSERT INTO pending_turns(payload) VALUES('not json')")
                connection.execute(
                    "INSERT INTO pending_turns(payload) VALUES(?)", (json.dumps(payload),)
                )
                connection.execute(
                    "INSERT INTO pending_turns(payload) VALUES(?)", (json.dumps(payload),)
                )
            other = plugin.ZkrMemoryProvider()
            other._binary = str(binary)
            other._db = provider._db
            other._queue_path = provider._queue_path
            provider._start_worker()
            other._start_worker()
            provider.shutdown()
            other.shutdown()
            with plugin._connection(provider._queue_path) as connection:
                self.assertEqual(
                    connection.execute("SELECT count(*) FROM pending_turns").fetchone()[0], 0
                )
                self.assertEqual(
                    connection.execute("SELECT count(*) FROM failed_turns").fetchone()[0], 1
                )
            with plugin._connection(provider._db) as connection:
                self.assertEqual(connection.execute("SELECT count(*) FROM sources").fetchone()[0], 1)

            cron = plugin.ZkrMemoryProvider()
            cron._binary = str(binary)
            cron.initialize("cron", hermes_home=directory, agent_context="cron")
            cron.sync_turn("Do not save", "Skipped")
            cron.shutdown()
            with plugin._connection(cron._queue_path) as connection:
                self.assertEqual(
                    connection.execute("SELECT count(*) FROM pending_turns").fetchone()[0],
                    0,
                )

    def test_valid_poison_is_quarantined_without_blocking_later_turns(self):
        binary = Path(__file__).parents[2] / "target" / "debug" / "zkr"
        with tempfile.TemporaryDirectory() as directory:
            provider = plugin.ZkrMemoryProvider()
            provider._binary = str(binary)
            provider._db = Path(directory) / "memory.db"
            provider._queue_path = Path(f"{provider._db}.queue")
            provider._ensure_queue()
            poison = provider._scope({
                "kind": "conversation",
                "text": "",
                "captured_at": 1_700_000_000,
                "recorded_at": 1_700_000_000,
                "ingestion_key": "hermes-turn:poison",
                "claim": None,
            })
            valid = provider._scope({
                "kind": "conversation",
                "text": "User: remember violet\nAssistant: done",
                "captured_at": 1_700_000_001,
                "recorded_at": 1_700_000_001,
                "ingestion_key": "hermes-turn:after-poison",
                "claim": None,
            })
            with plugin._connection(provider._queue_path) as connection:
                connection.executemany(
                    "INSERT INTO pending_turns(payload) VALUES(?)",
                    [(json.dumps(poison),), (json.dumps(valid),)],
                )
            provider._start_worker()
            deadline = plugin.time.time() + 5
            while plugin.time.time() < deadline:
                with plugin._connection(provider._queue_path) as connection:
                    if connection.execute("SELECT count(*) FROM pending_turns").fetchone()[0] == 0:
                        break
                plugin.time.sleep(0.05)
            provider.shutdown()
            with plugin._connection(provider._queue_path) as connection:
                self.assertEqual(
                    connection.execute("SELECT count(*) FROM pending_turns").fetchone()[0], 0
                )
                failed = connection.execute(
                    "SELECT payload, attempts, last_error FROM failed_turns"
                ).fetchone()
            self.assertEqual(json.loads(failed[0]), poison)
            self.assertEqual(failed[1], plugin._MAX_QUEUE_ATTEMPTS)
            self.assertEqual(failed[2], "zkr command failed")
            self.assertIn("remember violet", provider.prefetch("violet"))

    def test_oversized_turn_is_not_queued(self):
        with tempfile.TemporaryDirectory() as directory:
            provider = plugin.ZkrMemoryProvider()
            provider._queue_path = Path(directory) / "zkr.queue"
            provider._ensure_queue()
            provider._start_worker = lambda: None
            provider.sync_turn("x" * plugin._MAX_QUEUE_PAYLOAD_BYTES, "")
            with plugin._connection(provider._queue_path) as connection:
                pending = connection.execute("SELECT count(*) FROM pending_turns").fetchone()
            self.assertEqual(pending, (0,))

    def test_foreign_scope_queue_payload_is_quarantined(self):
        binary = Path(__file__).parents[2] / "target" / "debug" / "zkr"
        with tempfile.TemporaryDirectory() as directory:
            provider = plugin.ZkrMemoryProvider()
            provider._binary = str(binary)
            provider._db = Path(directory) / "memory.db"
            provider._queue_path = Path(f"{provider._db}.queue")
            provider._tenant_id = "tenant-a"
            provider._person_id = "person-a"
            provider._ensure_queue()
            foreign = {
                "tenant_id": "tenant-b",
                "person_id": "person-b",
                "kind": "conversation",
                "text": "User: foreign\nAssistant: turn",
                "captured_at": 1_700_000_000,
                "recorded_at": 1_700_000_000,
                "ingestion_key": "hermes-turn:foreign",
                "claim": None,
            }
            valid = provider._scope({
                "kind": "conversation",
                "text": "User: local\nAssistant: turn",
                "captured_at": 1_700_000_001,
                "recorded_at": 1_700_000_001,
                "ingestion_key": "hermes-turn:local",
                "claim": None,
            })
            with plugin._connection(provider._queue_path) as connection:
                connection.executemany(
                    "INSERT INTO pending_turns(payload) VALUES(?)",
                    [(json.dumps(foreign),), (json.dumps(valid),)],
                )
            provider._start_worker()
            deadline = plugin.time.time() + 5
            while plugin.time.time() < deadline:
                with plugin._connection(provider._queue_path) as connection:
                    if connection.execute("SELECT count(*) FROM pending_turns").fetchone()[0] == 0:
                        break
                plugin.time.sleep(0.05)
            provider.shutdown()
            with plugin._connection(provider._queue_path) as connection:
                failed = connection.execute("SELECT last_error FROM failed_turns").fetchone()
            self.assertEqual(failed, ("queued payload outside provider scope",))
            self.assertIn("local", provider.prefetch("local"))
            self.assertEqual(provider.prefetch("foreign"), "")

    def test_legacy_queue_schema_migrates_without_losing_pending_turns(self):
        with tempfile.TemporaryDirectory() as directory:
            provider = plugin.ZkrMemoryProvider()
            provider._queue_path = Path(directory) / "legacy.queue"
            with plugin._connection(provider._queue_path) as connection:
                connection.execute(
                    "CREATE TABLE pending_turns(id INTEGER PRIMARY KEY AUTOINCREMENT, payload TEXT NOT NULL)"
                )
                connection.execute("INSERT INTO pending_turns(payload) VALUES('legacy')")
            provider._ensure_queue()
            with plugin._connection(provider._queue_path) as connection:
                columns = {
                    row[1] for row in connection.execute("PRAGMA table_info(pending_turns)")
                }
                pending = connection.execute(
                    "SELECT payload, attempts, last_error FROM pending_turns"
                ).fetchone()
                failed_table = connection.execute(
                    "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'failed_turns'"
                ).fetchone()[0]
            self.assertEqual(columns, {"id", "payload", "attempts", "last_error"})
            self.assertEqual(pending, ("legacy", 0, None))
            self.assertEqual(failed_table, 1)


if __name__ == "__main__":
    unittest.main()
