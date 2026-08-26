from __future__ import annotations

import hashlib
import io
import json
import os
import re
import shutil
import struct
import subprocess
import sys
import zlib
from collections import deque
from pathlib import Path

from tsuinosora_constants import *
from tsuinosora_diagnostics import _is_safe_report_relative_path, _is_safe_symbol, _is_sanitized_sha256, _nonnegative_int, _positive_int
from tsuinosora_rendering import _read_json

__all__ = ['_script_source_map_sidecar_routes', '_lingo_bytecode_resource_index', '_script_source_map_route', '_script_source_map_payload_key_diagnostics', '_forbidden_payload_key_diagnostics']


def _script_source_map_sidecar_routes(
    value: dict,
    map_source: str,
    root: Path,
) -> tuple[list[dict], list[dict], list[dict], list[dict]]:
    diagnostics = _script_source_map_payload_key_diagnostics(value, map_source)
    sources = []
    routes = []
    readers = []
    declared_source_hashes = {}
    declared_source_line_counts = {}
    lingo_bytecode_resources_by_source = {}

    reader = value.get("reader", {})
    if reader and not isinstance(reader, dict):
        diagnostics.append(
            {
                "code": "TSUI_SCRIPT_SOURCE_MAP_READER_INVALID",
                "source_map": map_source,
                "message": "script source map reader metadata must be an object",
            }
        )
    elif reader:
        reader_valid = True
        tool_id = str(reader.get("tool_id", ""))
        tool_hash = str(reader.get("tool_hash", ""))
        output_contract = str(reader.get("output_contract", ""))
        if not _is_safe_symbol(tool_id):
            diagnostics.append(
                {
                    "code": "TSUI_SCRIPT_SOURCE_MAP_READER_ID_INVALID",
                    "source_map": map_source,
                    "field": "reader.tool_id",
                    "message": "reader tool_id must be a sanitized tool identity",
                }
            )
            reader_valid = False
        if not tool_hash:
            diagnostics.append(
                {
                    "code": "TSUI_SCRIPT_SOURCE_MAP_READER_HASH_MISSING",
                    "source_map": map_source,
                    "field": "reader.tool_hash",
                    "message": "reader hash evidence is required when reader metadata is present",
                }
            )
            reader_valid = False
        elif not _is_sanitized_sha256(tool_hash):
            diagnostics.append(
                {
                    "code": "TSUI_SCRIPT_SOURCE_MAP_READER_HASH_INVALID",
                    "source_map": map_source,
                    "field": "reader.tool_hash",
                    "message": "reader hash evidence must be a sanitized sha256 value",
                }
            )
            reader_valid = False
        if output_contract and not _is_safe_symbol(output_contract):
            diagnostics.append(
                {
                    "code": "TSUI_SCRIPT_SOURCE_MAP_READER_CONTRACT_INVALID",
                    "source_map": map_source,
                    "field": "reader.output_contract",
                    "message": "reader output_contract must be a sanitized contract id",
                }
            )
            reader_valid = False
        if reader_valid:
            readers.append(
                {
                    "source_map": map_source,
                    "tool_id": tool_id,
                    "tool_hash": tool_hash,
                    "output_contract": output_contract,
                }
            )

    raw_sources = value.get("sources", [])
    if raw_sources and not isinstance(raw_sources, list):
        diagnostics.append(
            {
                "code": "TSUI_SCRIPT_SOURCE_MAP_SOURCES_INVALID",
                "source_map": map_source,
                "message": "script source map sources must be a list",
            }
        )
        raw_sources = []
    for index, raw_source in enumerate(raw_sources):
        if not isinstance(raw_source, dict):
            diagnostics.append(
                {
                    "code": "TSUI_SCRIPT_SOURCE_MAP_SOURCE_INVALID",
                    "source_map": map_source,
                    "index": index,
                    "message": "script source map source entry must be an object",
                }
            )
            continue
        source = str(raw_source.get("source", ""))
        source_invalid = False
        if not _is_safe_report_relative_path(source):
            diagnostics.append(
                {
                    "code": "TSUI_SCRIPT_SOURCE_MAP_SOURCE_INVALID",
                    "source_map": map_source,
                    "index": index,
                    "message": "script source map source must be report-relative",
                }
            )
            source_invalid = True
        digest = str(raw_source.get("sha256", ""))
        digest_invalid = False
        if digest and not _is_sanitized_sha256(digest):
            diagnostics.append(
                {
                    "code": "TSUI_SCRIPT_SOURCE_MAP_HASH_INVALID",
                    "source_map": map_source,
                    "index": index,
                    "field": "sha256",
                    "message": "script source map source hash must be a sanitized sha256 value",
                }
            )
            digest_invalid = True
        line_count = _nonnegative_int(raw_source.get("line_count", 0))
        if source_invalid or digest_invalid:
            continue
        if not digest:
            diagnostics.append(
                {
                    "code": "TSUI_SCRIPT_SOURCE_MAP_HASH_MISSING",
                    "source_map": map_source,
                    "index": index,
                    "field": "sha256",
                    "message": "script source map source hash is required to prove route coverage",
                }
            )
            continue
        source_path = root / source
        if source_path.is_file():
            actual_digest = _sha256(source_path)
            if digest != actual_digest:
                diagnostics.append(
                    {
                        "code": "TSUI_SCRIPT_SOURCE_MAP_SOURCE_HASH_MISMATCH",
                        "source_map": map_source,
                        "index": index,
                        "source": source,
                        "message": "script source map source hash does not match the report-relative source file",
                    }
                )
                continue
            lingo_bytecode_resources = _lingo_bytecode_resource_index(source_path)
            if lingo_bytecode_resources:
                lingo_bytecode_resources_by_source[source] = lingo_bytecode_resources
        existing_digest = declared_source_hashes.get(source)
        if existing_digest and existing_digest != digest:
            diagnostics.append(
                {
                    "code": "TSUI_SCRIPT_SOURCE_MAP_SOURCE_HASH_CONFLICT",
                    "source_map": map_source,
                    "index": index,
                    "source": source,
                    "message": "script source map source hash must be stable for each source",
                }
            )
            continue
        declared_source_hashes[source] = digest
        declared_source_line_counts[source] = line_count
        sources.append(
            {
                "source": source,
                "sha256": digest,
                "line_count": line_count,
                "route_marker_count": 0,
                "script_count": _nonnegative_int(raw_source.get("script_count", 0)),
                "source_map": map_source,
            }
        )

    raw_routes = value.get("routes", [])
    if raw_routes and not isinstance(raw_routes, list):
        diagnostics.append(
            {
                "code": "TSUI_SCRIPT_SOURCE_MAP_ROUTES_INVALID",
                "source_map": map_source,
                "message": "script source map routes must be a list",
            }
        )
        raw_routes = []
    for index, raw_route in enumerate(raw_routes):
        if not isinstance(raw_route, dict):
            diagnostics.append(
                {
                    "code": "TSUI_SCRIPT_SOURCE_MAP_ROUTE_INVALID",
                    "source_map": map_source,
                    "index": index,
                    "message": "script source map route entry must be an object",
                }
            )
            continue
        route, route_diagnostics = _script_source_map_route(raw_route, map_source, index)
        diagnostics.extend(route_diagnostics)
        if route_diagnostics:
            continue
        declared_hash = declared_source_hashes.get(route["source"])
        if not declared_hash:
            diagnostics.append(
                {
                    "code": "TSUI_SCRIPT_SOURCE_MAP_ROUTE_SOURCE_UNDECLARED",
                    "source_map": map_source,
                    "index": index,
                    "source": route["source"],
                    "message": "script source map route source must match a declared source entry",
                }
            )
            continue
        if route["source_hash"] != declared_hash:
            diagnostics.append(
                {
                    "code": "TSUI_SCRIPT_SOURCE_MAP_ROUTE_HASH_MISMATCH",
                    "source_map": map_source,
                    "index": index,
                    "source": route["source"],
                    "message": "script source map route hash must match the declared source hash",
                }
            )
            continue
        declared_line_count = declared_source_line_counts.get(route["source"], 0)
        if declared_line_count and route["line"] > declared_line_count:
            diagnostics.append(
                {
                    "code": "TSUI_SCRIPT_SOURCE_MAP_ROUTE_LINE_OUT_OF_RANGE",
                    "source_map": map_source,
                    "index": index,
                    "source": route["source"],
                    "line": route["line"],
                    "line_count": declared_line_count,
                    "message": "script source map route line must be inside the declared source line range",
                }
            )
            continue
        script_resources = lingo_bytecode_resources_by_source.get(route["source"], {})
        if script_resources:
            script_resource_id = route.get("script_resource_id")
            script_payload_sha256 = str(route.get("script_payload_sha256", ""))
            if script_resource_id is None or not script_payload_sha256:
                diagnostics.append(
                    {
                        "code": "TSUI_SCRIPT_SOURCE_MAP_SCRIPT_RESOURCE_REQUIRED",
                        "source_map": map_source,
                        "index": index,
                        "source": route["source"],
                        "message": "Lingo bytecode coverage requires script resource id and payload hash evidence",
                    }
                )
                continue
            script_resource = script_resources.get(int(script_resource_id))
            if not script_resource:
                diagnostics.append(
                    {
                        "code": "TSUI_SCRIPT_SOURCE_MAP_SCRIPT_RESOURCE_UNKNOWN",
                        "source_map": map_source,
                        "index": index,
                        "source": route["source"],
                        "script_resource_id": script_resource_id,
                        "message": "Lingo bytecode route references a script resource that is not present in the Director Lingo map",
                    }
                )
                continue
            if script_payload_sha256 != script_resource["payload_sha256"]:
                diagnostics.append(
                    {
                        "code": "TSUI_SCRIPT_SOURCE_MAP_SCRIPT_HASH_MISMATCH",
                        "source_map": map_source,
                        "index": index,
                        "source": route["source"],
                        "script_resource_id": script_resource_id,
                        "message": "Lingo bytecode route hash does not match the Director Lingo map script payload hash",
                    }
                )
                continue
            route["script_entry_id"] = script_resource.get("entry_id", "")
        routes.append(route)

    return sources, routes, readers, diagnostics


def _lingo_bytecode_resource_index(source_path: Path) -> dict[int, dict]:
    try:
        value = _read_json(source_path)
    except json.JSONDecodeError:
        return {}
    if not isinstance(value, dict) or value.get("schema") != "tsuinosora.director_lingo_map.v1":
        return {}
    resources = {}
    for raw_resource in value.get("resources", []):
        if not isinstance(raw_resource, dict):
            continue
        if raw_resource.get("tag") != "Lscr" or not raw_resource.get("requires_bytecode_reader"):
            continue
        try:
            resource_id = int(raw_resource.get("resource_id"))
        except (TypeError, ValueError):
            continue
        payload_sha256 = str(raw_resource.get("payload_sha256", ""))
        if not _is_sanitized_sha256(payload_sha256):
            continue
        resources[resource_id] = {
            "resource_id": resource_id,
            "entry_id": str(raw_resource.get("entry_id", "")).strip(),
            "payload_sha256": payload_sha256,
        }
    return resources


def _script_source_map_route(raw_route: dict, map_source: str, index: int) -> tuple[dict | None, list[dict]]:
    diagnostics = []
    route_id = str(raw_route.get("route_id", ""))
    terminal = str(raw_route.get("terminal", route_id))
    source = str(raw_route.get("source", ""))
    coverage = str(raw_route.get("coverage", "covered"))
    line = _positive_int(raw_route.get("line", 0))
    source_hash = str(raw_route.get("source_hash", ""))
    script_resource_id = None
    if "script_resource_id" in raw_route:
        try:
            parsed_script_resource_id = int(raw_route.get("script_resource_id"))
        except (TypeError, ValueError):
            parsed_script_resource_id = -1
        if parsed_script_resource_id < 0:
            diagnostics.append(
                {
                    "code": "TSUI_SCRIPT_SOURCE_MAP_SCRIPT_RESOURCE_INVALID",
                    "source_map": map_source,
                    "index": index,
                    "message": "script_resource_id must be a non-negative Director Lingo resource id",
                }
            )
        else:
            script_resource_id = parsed_script_resource_id
    script_payload_sha256 = str(raw_route.get("script_payload_sha256", ""))
    if script_payload_sha256 and not _is_sanitized_sha256(script_payload_sha256):
        diagnostics.append(
            {
                "code": "TSUI_SCRIPT_SOURCE_MAP_SCRIPT_HASH_INVALID",
                "source_map": map_source,
                "index": index,
                "field": "script_payload_sha256",
                "message": "script_payload_sha256 must be a sanitized sha256 value",
            }
        )

    if not _is_safe_symbol(route_id):
        diagnostics.append(
            {
                "code": "TSUI_SCRIPT_SOURCE_MAP_ROUTE_ID_INVALID",
                "source_map": map_source,
                "index": index,
                "message": "script source map route_id must be a safe symbol",
            }
        )
    if terminal and not _is_safe_symbol(terminal):
        diagnostics.append(
            {
                "code": "TSUI_SCRIPT_SOURCE_MAP_TERMINAL_INVALID",
                "source_map": map_source,
                "index": index,
                "message": "script source map terminal must be a safe symbol",
            }
        )
    if not _is_safe_report_relative_path(source):
        diagnostics.append(
            {
                "code": "TSUI_SCRIPT_SOURCE_MAP_SOURCE_INVALID",
                "source_map": map_source,
                "index": index,
                "message": "script source map route source must be report-relative",
            }
        )
    if line <= 0:
        diagnostics.append(
            {
                "code": "TSUI_SCRIPT_SOURCE_MAP_LINE_INVALID",
                "source_map": map_source,
                "index": index,
                "message": "script source map line must be a positive integer",
            }
        )
    if coverage != "covered":
        diagnostics.append(
            {
                "code": "TSUI_SCRIPT_SOURCE_MAP_COVERAGE_INVALID",
                "source_map": map_source,
                "index": index,
                "message": "script source map routes must prove covered coverage",
            }
        )
    if source_hash and not _is_sanitized_sha256(source_hash):
        diagnostics.append(
            {
                "code": "TSUI_SCRIPT_SOURCE_MAP_HASH_INVALID",
                "source_map": map_source,
                "index": index,
                "field": "source_hash",
                "message": "script source map route hash must be a sanitized sha256 value",
            }
        )

    raw_choices = raw_route.get("choices", [])
    if raw_choices is None:
        raw_choices = []
    if not isinstance(raw_choices, list):
        diagnostics.append(
            {
                "code": "TSUI_SCRIPT_SOURCE_MAP_CHOICES_INVALID",
                "source_map": map_source,
                "index": index,
                "message": "script source map choices must be a list of safe symbols",
            }
        )
        raw_choices = []
    choices = []
    for choice_index, choice in enumerate(raw_choices):
        choice_id = str(choice)
        if not _is_safe_symbol(choice_id):
            diagnostics.append(
                {
                    "code": "TSUI_SCRIPT_SOURCE_MAP_CHOICE_INVALID",
                    "source_map": map_source,
                    "index": index,
                    "choice_index": choice_index,
                    "message": "script source map choice id must be a safe symbol",
                }
            )
            continue
        choices.append(choice_id)

    if diagnostics:
        return None, diagnostics
    route = {
        "route_id": route_id,
        "coverage": "covered",
        "terminal": terminal or route_id,
        "choices": choices,
        "source": source,
        "line": line,
        "source_hash": source_hash,
        "source_map": map_source,
    }
    if script_resource_id is not None:
        route["script_resource_id"] = script_resource_id
    if script_payload_sha256:
        route["script_payload_sha256"] = script_payload_sha256
    return route, diagnostics


def _script_source_map_payload_key_diagnostics(value, map_source: str) -> list[dict]:
    return _forbidden_payload_key_diagnostics(
        value,
        map_source,
        code="TSUI_SCRIPT_SOURCE_MAP_PAYLOAD_FIELD",
        source_field="source_map",
        message="script source map sidecar must not contain script text, bytecode or payload fields",
    )


def _forbidden_payload_key_diagnostics(
    value,
    source: str,
    *,
    code: str,
    source_field: str,
    message: str,
) -> list[dict]:
    diagnostics = []

    def walk(node, path: str):
        if isinstance(node, dict):
            for key, child in node.items():
                key_name = str(key)
                field = f"{path}.{key_name}" if path else key_name
                if key_name.lower() in SCRIPT_SOURCE_MAP_FORBIDDEN_KEYS:
                    if path == "redaction" and child == "omitted":
                        continue
                    diagnostics.append(
                        {
                            "code": code,
                            source_field: source,
                            "field": field,
                            "message": message,
                        }
                    )
                    continue
                walk(child, field)
        elif isinstance(node, list):
            for index, child in enumerate(node):
                walk(child, f"{path}[{index}]")

    walk(value, "")
    return diagnostics
