from __future__ import annotations

import json
import logging
import os
import shutil
import subprocess
import threading
import time
from pathlib import Path
from typing import Any

from agent.memory_provider import MemoryProvider
from tools.registry import tool_error

logger = logging.getLogger(__name__)


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
            "kind": {
                "type": "string",
                "enum": ["conversation", "screen", "audio", "document", "integration", "user_correction"],
            },
            "occurred_at": {"type": "integer", "description": "Unix timestamp; defaults to now."},
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
            "occurred_at": {"type": "integer", "description": "Unix timestamp; defaults to now."},
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
]


class ZkrMemoryProvider(MemoryProvider):
    def __init__(self) -> None:
        self._binary = os.environ.get("ZKR_BIN", "zkr")
        self._db = Path(os.environ.get("ZKR_DB", "zkr.db"))
        self._tenant_id = os.environ.get("ZKR_TENANT_ID", "hermes")
        self._person_id = os.environ.get("ZKR_PERSON_ID", "default")

    @property
    def name(self) -> str:
        return "zkr"

    def is_available(self) -> bool:
        return Path(self._binary).is_file() or shutil.which(self._binary) is not None

    def initialize(self, session_id: str, **kwargs: Any) -> None:
        hermes_home = Path(str(kwargs.get("hermes_home") or Path.home() / ".hermes"))
        if "ZKR_DB" not in os.environ:
            self._db = hermes_home / "zkr.db"
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
        if not user_content.strip():
            return
        text = f"User: {user_content.strip()}\nAssistant: {assistant_content.strip()}"
        threading.Thread(
            target=self._store_turn,
            args=(text,),
            name="zkr-hermes-store",
            daemon=True,
        ).start()

    def get_tool_schemas(self) -> list[dict[str, Any]]:
        return SCHEMAS

    def handle_tool_call(self, tool_name: str, args: dict[str, Any], **kwargs: Any) -> str:
        now = int(time.time())
        try:
            if tool_name == "zkr_search":
                result = self._run("search", args)
            elif tool_name == "zkr_store":
                occurred_at = int(args.get("occurred_at", now))
                content = str(args["content"])
                result = self._run(
                    "remember",
                    {
                        "kind": args.get("kind", "conversation"),
                        "text": content,
                        "captured_at": occurred_at,
                        "claim": {
                            "subject": args.get("subject", "user"),
                            "predicate": args.get("predicate", "remembers"),
                            "value": args.get("value", content),
                            "valid_from": occurred_at,
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
                        "occurred_at": int(args.get("occurred_at", now)),
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
            else:
                return tool_error(f"Unknown tool: {tool_name}")
            return json.dumps(result)
        except (KeyError, TypeError, ValueError, RuntimeError) as error:
            return tool_error(str(error))

    def backup_paths(self) -> list[str]:
        return [str(self._db)]

    def _scope(self, payload: dict[str, Any]) -> dict[str, Any]:
        return {
            "tenant_id": self._tenant_id,
            "person_id": self._person_id,
            **payload,
        }

    def _run(self, command: str, payload: dict[str, Any]) -> dict[str, Any]:
        completed = subprocess.run(
            [self._binary, "--db", str(self._db), command],
            input=json.dumps(self._scope(payload)),
            capture_output=True,
            text=True,
            timeout=10,
            check=False,
        )
        if completed.returncode:
            detail = completed.stderr.strip() or completed.stdout.strip() or "zkr command failed"
            raise RuntimeError(detail)
        result = json.loads(completed.stdout)
        if not isinstance(result, dict):
            raise RuntimeError("zkr returned a non-object result")
        return result

    def _store_turn(self, text: str) -> None:
        try:
            self._run(
                "remember",
                {
                    "kind": "conversation",
                    "text": text,
                    "captured_at": int(time.time()),
                    "claim": None,
                },
            )
        except RuntimeError:
            logger.debug("zkr turn capture failed", exc_info=True)


def register(ctx: Any) -> None:
    ctx.register_memory_provider(ZkrMemoryProvider())
