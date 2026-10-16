from __future__ import annotations

import json
import logging
import os
import shutil
import sqlite3
import subprocess
import threading
import time
import uuid
from contextlib import contextmanager
from pathlib import Path
from typing import Any, Iterator

try:
    from agent.memory_provider import MemoryProvider
    from tools.registry import tool_error
except ImportError:
    MemoryProvider = object  # type: ignore[assignment,misc]

    def tool_error(message: str) -> str:
        return json.dumps({"error": message})

logger = logging.getLogger(__name__)
_MAX_QUEUE_ATTEMPTS = 3
_MAX_QUEUE_PAYLOAD_BYTES = 1024 * 1024
_QUEUE_LOCKS: dict[Path, threading.Lock] = {}
_QUEUE_LOCKS_GUARD = threading.Lock()


@contextmanager
def _connection(path: Path) -> Iterator[sqlite3.Connection]:
    connection = sqlite3.connect(path)
    try:
        with connection:
            yield connection
    finally:
        connection.close()


def _validate_db_path(path: str) -> str:
    if "\0" in path or path.startswith("-"):
        raise ValueError("Invalid zkr database path")
    return path


def _schema(name: str, description: str, properties: dict[str, Any], required: list[str]) -> dict[str, Any]:
    return {
        "name": name,
        "description": description,
        "parameters": {
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": False,
        },
    }


SCHEMAS = [
    _schema(
        "zkr_search",
        "Search the user's cited long-term memories.",
        {
            "query": {"type": "string", "description": "What to recall."},
            "limit": {"type": "integer", "minimum": 1, "maximum": 50},
        },
        ["query"],
    ),
    _schema(
        "zkr_store",
        "Store an explicit fact, preference, decision, or event with source evidence.",
        {
            "content": {"type": "string", "description": "The source evidence to remember."},
            "subject": {"type": "string", "description": "Who or what the memory describes."},
            "predicate": {"type": "string", "description": "The relationship or property."},
            "value": {"type": "string", "description": "The normalized remembered value."},
            "claim_kind": {
                "type": "string",
                "enum": ["fact", "profile_fact", "preference", "task", "skill", "recommendation"],
            },
            "source_kind": {
                "type": "string",
                "enum": ["conversation", "screen", "audio", "document", "integration", "user_correction"],
            },
            "captured_at": {"type": "integer", "description": "Unix timestamp; defaults to now."},
            "valid_from": {"type": "integer", "description": "Unix timestamp; defaults to captured_at."},
            "recorded_at": {"type": "integer", "description": "Unix timestamp; defaults to now."},
        },
        ["content"],
    ),
    _schema(
        "zkr_correct",
        "Correct an accepted memory claim while retaining its history and new evidence.",
        {
            "claim_id": {"type": "string"},
            "content": {"type": "string", "description": "The correction evidence."},
            "value": {"type": "string", "description": "The corrected normalized value."},
            "valid_at": {"type": "integer", "description": "Unix timestamp; defaults to now."},
            "recorded_at": {"type": "integer", "description": "Unix timestamp; defaults to now."},
        },
        ["claim_id", "content", "value"],
    ),
    _schema(
        "zkr_delete",
        "Delete a source and make all evidence-backed claims from it unavailable.",
        {
            "source_id": {"type": "string"},
            "deleted_at": {"type": "integer", "description": "Unix timestamp; defaults to now."},
        },
        ["source_id"],
    ),
    _schema(
        "zkr_reflect",
        "Save a cited daily reflection without changing the underlying memories.",
        {
            "day": {"type": "string", "description": "ISO date for the reflection."},
            "summary": {"type": "string"},
            "evidence_ids": {"type": "array", "items": {"type": "string"}},
            "recorded_at": {"type": "integer", "description": "Unix timestamp; defaults to now."},
        },
        ["day", "summary", "evidence_ids"],
    ),
    _schema(
        "zkr_remember_project",
        "Store a cited fact or task under a project and one or more categories.",
        {
            "project": {"type": "string", "description": "Exact project name; defaults to general."},
            "category": {"type": "string", "description": "Exact category name."},
            "categories": {
                "type": "array",
                "items": {"type": "string"},
                "description": "Category labels to attach to this memory.",
            },
            "content": {"type": "string", "description": "The source evidence to remember."},
            "predicate": {"type": "string", "description": "Stable relationship or property key."},
            "value": {"type": "string", "description": "The normalized remembered value."},
            "claim_kind": {
                "type": "string",
                "enum": ["fact", "profile_fact", "preference", "task", "skill", "recommendation"],
            },
            "source_kind": {
                "type": "string",
                "enum": ["conversation", "screen", "audio", "document", "integration", "user_correction"],
            },
            "embedding": {
                "type": "object",
                "properties": {
                    "vector": {"type": "array", "items": {"type": "number"}},
                    "model": {"type": "string"},
                    "version": {"type": "string"},
                    "normalization": {"type": "string", "enum": ["none", "l2"]},
                    "distance": {"type": "string", "enum": ["cosine", "dot", "euclidean"]},
                },
                "required": ["vector"],
                "additionalProperties": False,
            },
            "captured_at": {"type": "integer", "description": "Unix timestamp; defaults to now."},
            "valid_from": {"type": "integer", "description": "Unix timestamp; defaults to captured_at."},
            "recorded_at": {"type": "integer", "description": "Unix timestamp; defaults to now."},
        },
        ["content", "predicate"],
    ),
    _schema(
        "zkr_remember_project_record",
        "Store a structured JSON record with optional connections under a project and one or more categories.",
        {
            "project": {"type": "string", "description": "Exact project name; defaults to general."},
            "category": {"type": "string", "description": "Exact category name."},
            "categories": {
                "type": "array",
                "items": {"type": "string"},
                "description": "Category labels to attach to this record.",
            },
            "content": {"type": "string", "description": "The source evidence to remember."},
            "predicate": {"type": "string", "description": "Stable record predicate key."},
            "record_type": {"type": "string", "description": "Record type name."},
            "fields": {
                "type": "object",
                "description": "Arbitrary JSON object of record fields.",
                "additionalProperties": True,
            },
            "connections": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "relation": {"type": "string"},
                        "target": {"type": "string"},
                        "weight_basis_points": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": 10000,
                        },
                        "note": {"type": "string"},
                    },
                    "required": ["relation", "target"],
                    "additionalProperties": False,
                },
            },
            "embedding": {
                "type": "object",
                "properties": {
                    "vector": {"type": "array", "items": {"type": "number"}},
                    "model": {"type": "string"},
                    "version": {"type": "string"},
                    "normalization": {"type": "string", "enum": ["none", "l2"]},
                    "distance": {"type": "string", "enum": ["cosine", "dot", "euclidean"]},
                },
                "required": ["vector"],
                "additionalProperties": False,
            },
            "claim_kind": {
                "type": "string",
                "enum": ["fact", "profile_fact", "preference", "task", "skill", "recommendation"],
            },
            "source_kind": {
                "type": "string",
                "enum": ["conversation", "screen", "audio", "document", "integration", "user_correction"],
            },
            "captured_at": {"type": "integer", "description": "Unix timestamp; defaults to now."},
            "valid_from": {"type": "integer", "description": "Unix timestamp; defaults to captured_at."},
            "recorded_at": {"type": "integer", "description": "Unix timestamp; defaults to now."},
        },
        ["content", "predicate", "record_type"],
    ),
    _schema(
        "zkr_project_memories",
        "Query current cited project and category memories with optional field, graph, and vector filters.",
        {
            "project": {"type": "string"},
            "projects": {"type": "array", "items": {"type": "string"}},
            "category": {"type": "string"},
            "categories": {"type": "array", "items": {"type": "string"}},
            "all_categories": {"type": "array", "items": {"type": "string"}},
            "predicate": {"type": "string"},
            "record_type": {"type": "string"},
            "relation": {"type": "string"},
            "target": {"type": "string"},
            "where_fields": {"type": "object", "additionalProperties": True},
            "connected_to": {"type": "string"},
            "max_hops": {"type": "integer", "minimum": 1, "maximum": 4},
            "vector_query": {"type": "string"},
            "query_embedding": {
                "type": "object",
                "properties": {
                    "vector": {"type": "array", "items": {"type": "number"}},
                    "model": {"type": "string"},
                    "version": {"type": "string"},
                },
                "required": ["vector", "model", "version"],
                "additionalProperties": False,
            },
            "limit": {"type": "integer", "minimum": 1, "maximum": 100},
        },
        [],
    ),
    _schema(
        "zkr_dream",
        "Surface emergent categories, schemas, hubs, associative connections, unresolved targets, contradictions, short-term claims, and unsummarized or stale evidence.",
        {
            "model": {"type": "string"},
            "version": {"type": "string"},
            "min_similarity_basis_points": {"type": "integer", "minimum": 0, "maximum": 10000},
            "limit": {"type": "integer", "minimum": 1, "maximum": 100},
        },
        [],
    ),
]


class ZkrMemoryProvider(MemoryProvider):
    def __init__(self) -> None:
        self._binary = os.environ.get("ZKR_BIN", "zkr")
        if Path(self._binary).name not in ("zkr", "zkr.exe"):
            raise ValueError(f"Invalid zkr binary: {self._binary}")
        self._db = Path(_validate_db_path(os.environ.get("ZKR_DB", "zkr.db")))
        self._tenant_id = os.environ.get("ZKR_TENANT_ID", "hermes")
        self._person_id = os.environ.get("ZKR_PERSON_ID", "default")
        self._queue_path = Path(f"{self._db}.queue")
        self._wake = threading.Event()
        self._stop = threading.Event()
        self._worker: threading.Thread | None = None
        self._write_enabled = True

    @property
    def name(self) -> str:
        return "zkr"

    def is_available(self) -> bool:
        return Path(self._binary).is_file() or shutil.which(self._binary) is not None

    def initialize(self, session_id: str, **kwargs: Any) -> None:
        hermes_home = Path(str(kwargs.get("hermes_home") or Path.home() / ".hermes"))
        if "ZKR_DB" not in os.environ:
            self._db = hermes_home / "zkr.db"
        self._queue_path = Path(f"{self._db}.queue")
        self._db.parent.mkdir(parents=True, exist_ok=True)
        self._tenant_id = os.environ.get(
            "ZKR_TENANT_ID", str(kwargs.get("agent_workspace") or "hermes")
        )
        self._person_id = os.environ.get(
            "ZKR_PERSON_ID",
            str(
                kwargs.get("user_id")
                or kwargs.get("user_id_alt")
                or kwargs.get("agent_identity")
                or "default"
            ),
        )
        self._write_enabled = kwargs.get("agent_context", "primary") == "primary"
        self._ensure_queue()
        self._start_worker()

    def system_prompt_block(self) -> str:
        return (
            "# zkr Memory\n"
            "Search before answering questions about the user. Store only durable facts, preferences, "
            "decisions, and events. Cite evidence IDs. Use correction instead of overwriting history."
        )

    def prefetch(self, query: str, *, session_id: str = "") -> str:
        if not query.strip():
            return ""
        try:
            result = self._run("search", {"query": query, "limit": 5})
        except RuntimeError:
            logger.debug("zkr prefetch failed", exc_info=True)
            return ""
        items = result.get("items", [])
        if not items:
            return ""
        lines = ["[zkr cited memory]"]
        for item in items:
            evidence = ", ".join(item.get("evidence_ids", []))
            lines.append(f"- {item.get('excerpt', '')} [evidence: {evidence}]")
        return "\n".join(lines)

    def sync_turn(
        self,
        user_content: str,
        assistant_content: str,
        *,
        session_id: str = "",
        messages: list[dict[str, Any]] | None = None,
    ) -> None:
        if not self._write_enabled or not user_content.strip():
            return
        text = f"User: {user_content.strip()}\nAssistant: {assistant_content.strip()}"
        self._enqueue_turn(text)

    def get_tool_schemas(self) -> list[dict[str, Any]]:
        return SCHEMAS

    def handle_tool_call(self, tool_name: str, args: dict[str, Any], **kwargs: Any) -> str:
        now = int(time.time())
        try:
            if tool_name == "zkr_search":
                result = self._run("search", args)
            elif tool_name == "zkr_store":
                captured_at = int(args.get("captured_at", now))
                valid_from = int(args.get("valid_from", captured_at))
                recorded_at = int(args.get("recorded_at", now))
                content = str(args["content"])
                result = self._run(
                    "remember",
                    {
                        "kind": args.get("source_kind", "conversation"),
                        "text": content,
                        "captured_at": captured_at,
                        "recorded_at": recorded_at,
                        "claim": {
                            "subject": args.get("subject", "user"),
                            "predicate": args.get("predicate", "remembers"),
                            "value": args.get("value", content),
                            "kind": args.get("claim_kind", "fact"),
                            "valid_from": valid_from,
                        },
                    },
                )
            elif tool_name == "zkr_correct":
                result = self._run(
                    "correct",
                    {
                        "claim_id": args["claim_id"],
                        "text": args["content"],
                        "value": args["value"],
                        "valid_at": int(args.get("valid_at", now)),
                        "recorded_at": int(args.get("recorded_at", now)),
                    },
                )
            elif tool_name == "zkr_delete":
                result = self._run(
                    "delete",
                    {
                        "source_id": args["source_id"],
                        "deleted_at": int(args.get("deleted_at", now)),
                    },
                )
            elif tool_name == "zkr_reflect":
                result = self._run(
                    "review",
                    {
                        "day": args["day"],
                        "summary": args["summary"],
                        "evidence_ids": args["evidence_ids"],
                        "recorded_at": int(args.get("recorded_at", now)),
                    },
                )
            elif tool_name == "zkr_remember_project":
                if not args.get("category") and not args.get("categories"):
                    raise ValueError("category or categories is required")
                captured_at = int(args.get("captured_at", now))
                valid_from = int(args.get("valid_from", captured_at))
                recorded_at = int(args.get("recorded_at", now))
                content = str(args["content"])
                payload: dict[str, Any] = {
                    "project": args.get("project", "general"),
                    "kind": args.get("source_kind", "conversation"),
                    "text": content,
                    "captured_at": captured_at,
                    "recorded_at": recorded_at,
                    "claim": {
                        "subject": args.get("subject", "user"),
                        "predicate": args["predicate"],
                        "value": args.get("value", content),
                        "kind": args.get("claim_kind", "fact"),
                        "valid_from": valid_from,
                    },
                }
                if "category" in args:
                    payload["category"] = args["category"]
                if "categories" in args:
                    payload["categories"] = args["categories"]
                if "embedding" in args:
                    payload["embedding"] = args["embedding"]
                result = self._run("remember-project", payload)
            elif tool_name == "zkr_remember_project_record":
                raw_record = args.get("record") if isinstance(args.get("record"), dict) else {}
                if (
                    not args.get("category")
                    and not args.get("categories")
                    and not raw_record.get("categories")
                ):
                    raise ValueError("category or categories is required")
                captured_at = int(args.get("captured_at", now))
                valid_from = int(args.get("valid_from", captured_at))
                recorded_at = int(args.get("recorded_at", now))
                content = str(args["content"])
                record: dict[str, Any] = {
                    "record_type": args.get("record_type") or raw_record["record_type"],
                    "fields": args.get("fields", raw_record.get("fields", {})),
                }
                if "categories" in raw_record:
                    record["categories"] = raw_record["categories"]
                if "connections" in args:
                    record["connections"] = args["connections"]
                elif "connections" in raw_record:
                    record["connections"] = raw_record["connections"]
                payload = {
                    "project": args.get("project", "general"),
                    "kind": args.get("source_kind", "conversation"),
                    "text": content,
                    "captured_at": captured_at,
                    "recorded_at": recorded_at,
                    "claim": {
                        "subject": args.get("subject", "user"),
                        "predicate": args["predicate"],
                        "value": args.get("value", content),
                        "kind": args.get("claim_kind", "fact"),
                        "valid_from": valid_from,
                    },
                    "record": record,
                }
                if "category" in args:
                    payload["category"] = args["category"]
                if "categories" in args:
                    payload["categories"] = args["categories"]
                if "embedding" in args:
                    payload["embedding"] = args["embedding"]
                result = self._run("remember-project-record", payload)
            elif tool_name == "zkr_project_memories":
                result = self._run(
                    "project-memories",
                    {
                        key: args[key]
                        for key in (
                            "project",
                            "projects",
                            "category",
                            "categories",
                            "all_categories",
                            "predicate",
                            "record_type",
                            "relation",
                            "target",
                            "where_fields",
                            "connected_to",
                            "max_hops",
                            "vector_query",
                            "query_embedding",
                            "limit",
                        )
                        if key in args
                    },
                )
            elif tool_name == "zkr_dream":
                result = self._run(
                    "dream",
                    {
                        key: args[key]
                        for key in (
                            "model",
                            "version",
                            "min_similarity_basis_points",
                            "limit",
                        )
                        if key in args
                    },
                )
            else:
                return tool_error(f"Unknown tool: {tool_name}")
            return json.dumps(result)
        except (KeyError, TypeError, ValueError, RuntimeError):
            logger.debug("zkr tool call failed")
            return tool_error("zkr operation failed")

    def backup_paths(self) -> list[str]:
        return [str(self._db), str(self._queue_path)]

    def shutdown(self) -> None:
        self._stop.set()
        self._wake.set()
        if self._worker is not None:
            self._worker.join(timeout=10)
            if not self._worker.is_alive():
                self._worker = None

    def _scope(self, payload: dict[str, Any]) -> dict[str, Any]:
        return {
            **payload,
            "tenant_id": self._tenant_id,
            "person_id": self._person_id,
        }

    def _run(self, command: str, payload: dict[str, Any]) -> Any:
        return self._run_payload(command, self._scope(payload))

    def _run_payload(self, command: str, payload: dict[str, Any]) -> Any:
        try:
            completed = subprocess.run(
                [self._binary, "--db", str(self._db), command],
                input=json.dumps(payload),
                capture_output=True,
                text=True,
                timeout=10,
                check=False,
            )
        except (OSError, subprocess.TimeoutExpired) as error:
            raise RuntimeError(str(error)) from error
        if completed.returncode:
            detail = completed.stderr.strip() or completed.stdout.strip() or "zkr command failed"
            raise RuntimeError(detail)
        result = json.loads(completed.stdout)
        if not isinstance(result, (dict, list)):
            raise RuntimeError("zkr returned a non-object result")
        return result

    def _ensure_queue(self) -> None:
        with _connection(self._queue_path) as connection:
            connection.execute("PRAGMA journal_mode = DELETE")
            connection.execute("PRAGMA synchronous = FULL")
            connection.execute(
                "CREATE TABLE IF NOT EXISTS pending_turns(id INTEGER PRIMARY KEY AUTOINCREMENT, payload TEXT NOT NULL, attempts INTEGER NOT NULL DEFAULT 0, last_error TEXT)"
            )
            columns = {
                row[1] for row in connection.execute("PRAGMA table_info(pending_turns)")
            }
            if "attempts" not in columns:
                connection.execute(
                    "ALTER TABLE pending_turns ADD COLUMN attempts INTEGER NOT NULL DEFAULT 0"
                )
            if "last_error" not in columns:
                connection.execute("ALTER TABLE pending_turns ADD COLUMN last_error TEXT")
            connection.execute(
                "CREATE TABLE IF NOT EXISTS failed_turns(id INTEGER PRIMARY KEY, payload TEXT NOT NULL, attempts INTEGER NOT NULL, last_error TEXT NOT NULL, failed_at INTEGER NOT NULL)"
            )

    def _start_worker(self) -> None:
        if self._worker is not None and self._worker.is_alive():
            if self._stop.is_set():
                raise RuntimeError("zkr queue worker is still shutting down")
            return
        self._stop.clear()
        self._worker = threading.Thread(
            target=self._drain_queue,
            name="zkr-hermes-store",
            daemon=True,
        )
        self._worker.start()
        self._wake.set()

    def _enqueue_turn(self, text: str) -> None:
        self._ensure_queue()
        recorded_at = int(time.time())
        payload = self._scope(
            {
                "kind": "conversation",
                "text": text,
                "captured_at": recorded_at,
                "recorded_at": recorded_at,
                "ingestion_key": f"hermes-turn:{uuid.uuid4()}",
                "claim": None,
            }
        )
        serialized = json.dumps(payload)
        if len(serialized.encode()) > _MAX_QUEUE_PAYLOAD_BYTES:
            logger.warning("skipping oversized zkr turn")
            return
        with _connection(self._queue_path) as connection:
            connection.execute(
                "INSERT INTO pending_turns(payload) VALUES(?)",
                (serialized,),
            )
        self._start_worker()
        self._wake.set()

    def _drain_queue(self) -> None:
        with _QUEUE_LOCKS_GUARD:
            queue_lock = _QUEUE_LOCKS.setdefault(self._queue_path.resolve(), threading.Lock())
        while True:
            with queue_lock:
                with _connection(self._queue_path) as connection:
                    pending = connection.execute(
                        "SELECT id, payload, attempts FROM pending_turns ORDER BY id LIMIT 1"
                    ).fetchone()
                if pending is not None:
                    item_id, payload, attempts = pending
                    try:
                        decoded = json.loads(payload)
                    except json.JSONDecodeError:
                        logger.warning("quarantining invalid zkr queue payload")
                        with _connection(self._queue_path) as connection:
                            connection.execute(
                                "INSERT OR REPLACE INTO failed_turns(id, payload, attempts, last_error, failed_at) VALUES(?, ?, 1, ?, ?)",
                                (item_id, payload, "invalid queued payload", int(time.time())),
                            )
                            connection.execute("DELETE FROM pending_turns WHERE id = ?", (item_id,))
                        continue
                    if (
                        not isinstance(decoded, dict)
                        or decoded.get("tenant_id") != self._tenant_id
                        or decoded.get("person_id") != self._person_id
                    ):
                        logger.warning("quarantining zkr queue payload outside provider scope")
                        with _connection(self._queue_path) as connection:
                            connection.execute(
                                "INSERT OR REPLACE INTO failed_turns(id, payload, attempts, last_error, failed_at) VALUES(?, ?, 1, ?, ?)",
                                (item_id, payload, "queued payload outside provider scope", int(time.time())),
                            )
                            connection.execute("DELETE FROM pending_turns WHERE id = ?", (item_id,))
                        continue
                    try:
                        self._run_payload("remember", decoded)
                    except RuntimeError:
                        logger.debug("zkr turn capture failed")
                        attempts += 1
                        with _connection(self._queue_path) as connection:
                            if attempts >= _MAX_QUEUE_ATTEMPTS:
                                connection.execute(
                                    "INSERT OR REPLACE INTO failed_turns(id, payload, attempts, last_error, failed_at) VALUES(?, ?, ?, ?, ?)",
                                    (item_id, payload, attempts, "zkr command failed", int(time.time())),
                                )
                                connection.execute(
                                    "DELETE FROM pending_turns WHERE id = ?", (item_id,)
                                )
                            else:
                                connection.execute(
                                    "UPDATE pending_turns SET attempts = ?, last_error = ? WHERE id = ?",
                                    (attempts, "zkr command failed", item_id),
                                )
                        if attempts >= _MAX_QUEUE_ATTEMPTS:
                            continue
                        if self._stop.is_set():
                            return
                    else:
                        with _connection(self._queue_path) as connection:
                            connection.execute("DELETE FROM pending_turns WHERE id = ?", (item_id,))
                        continue
            if pending is None:
                if self._stop.is_set():
                    return
                self._wake.wait(timeout=1)
                self._wake.clear()
                continue
            self._wake.wait(timeout=1)
            self._wake.clear()


def register(ctx: Any) -> None:
    ctx.register_memory_provider(ZkrMemoryProvider())
