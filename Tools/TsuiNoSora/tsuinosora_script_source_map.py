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
from tsuinosora_constants import _read_text_lossless, _script_route_marker
from tsuinosora_diagnostics import _is_unpacked_metadata_file, _rel
from tsuinosora_rendering import _read_json, _report_has_path_leak
from tsuinosora_script_source_routes import _script_source_map_sidecar_routes

__all__ = ['build_script_source_map_report', '_duplicate_choice_diagnostics', '_duplicate_route_conflict_diagnostics', '_dedupe_script_source_routes']


def build_script_source_map_report(root: Path | str) -> dict:
    root = Path(root)
    diagnostics = []
    sources = []
    routes = []
    readers = []
    lingo_bytecode_requirements = []
    for path in sorted(p for p in root.rglob("*") if p.is_file() and p.suffix.lower() in TEXT_EXTS):
        if path.suffix.lower() == ".json" and _is_unpacked_metadata_file(path):
            continue
        rel = _rel(path, root)
        text = _read_text_lossless(path)
        line_count = len(text.splitlines())
        source_routes = []
        for line_no, line in enumerate(text.splitlines(), start=1):
            route = _script_route_marker(line)
            if not route:
                continue
            route["source"] = rel
            route["line"] = line_no
            source_routes.append(route)
            routes.append(route)
        sources.append(
            {
                "source": rel,
                "sha256": _sha256(path),
                "line_count": line_count,
                "route_marker_count": len(source_routes),
            }
        )
    for path in sorted(p for p in root.rglob("*.json") if p.is_file()):
        rel = _rel(path, root)
        try:
            value = _read_json(path)
        except json.JSONDecodeError:
            continue
        if not isinstance(value, dict):
            continue
        if value.get("schema") == "tsuinosora.script_source_map.v1":
            sidecar_sources, sidecar_routes, sidecar_readers, sidecar_diagnostics = _script_source_map_sidecar_routes(value, rel, root)
            sources.extend(sidecar_sources)
            routes.extend(sidecar_routes)
            readers.extend(sidecar_readers)
            diagnostics.extend(sidecar_diagnostics)
            continue
        if value.get("schema") != "tsuinosora.director_lingo_map.v1":
            continue
        unsupported_count = int(value.get("unsupported_script_count", 0))
        script_count = int(value.get("script_count", 0))
        if script_count:
            sources.append(
                {
                    "source": rel,
                    "sha256": _sha256(path),
                    "line_count": 0,
                    "route_marker_count": 0,
                    "lingo_script_count": script_count,
                    "unsupported_script_count": unsupported_count,
                }
            )
        if unsupported_count:
            required_scripts = []
            for resource in value.get("resources", []):
                if not isinstance(resource, dict):
                    continue
                if resource.get("tag") != "Lscr" or not resource.get("requires_bytecode_reader"):
                    continue
                try:
                    resource_id = int(resource.get("resource_id"))
                except (TypeError, ValueError):
                    continue
                required_scripts.append(
                    {
                        "resource_id": resource_id,
                        "entry_id": str(resource.get("entry_id", "")),
                        "payload_sha256": str(resource.get("payload_sha256", "")),
                    }
                )
            lingo_bytecode_requirements.append(
                {
                    "source": rel,
                    "source_hash": _sha256(path),
                    "script_count": script_count,
                    "unsupported_script_count": unsupported_count,
                    "required_scripts": required_scripts,
                }
            )
    covered_source_hashes = {
        (str(route.get("source", "")), str(route.get("source_hash", "")))
        for route in routes
        if route.get("source") and route.get("source_hash")
    }
    for requirement in lingo_bytecode_requirements:
        if (requirement["source"], requirement["source_hash"]) in covered_source_hashes:
            covered_scripts = {
                (
                    str(route.get("source", "")),
                    str(route.get("source_hash", "")),
                    int(route.get("script_resource_id")),
                    str(route.get("script_payload_sha256", "")),
                )
                for route in routes
                if route.get("source") == requirement["source"]
                and route.get("source_hash") == requirement["source_hash"]
                and route.get("script_resource_id") is not None
                and route.get("script_payload_sha256")
            }
            for script in requirement.get("required_scripts", []):
                expected = (
                    requirement["source"],
                    requirement["source_hash"],
                    int(script["resource_id"]),
                    str(script["payload_sha256"]),
                )
                if expected in covered_scripts:
                    continue
                diagnostics.append(
                    {
                        "code": "TSUI_SCRIPT_SOURCE_MAP_LINGO_BYTECODE_RESOURCE_UNCOVERED",
                        "source": requirement["source"],
                        "script_resource_id": int(script["resource_id"]),
                        "script_payload_sha256": str(script["payload_sha256"]),
                        "message": "Director Lingo bytecode route coverage must bind every unsupported Lscr resource id and payload hash",
                    }
                )
            continue
        diagnostics.append(
            {
                "code": "TSUI_SCRIPT_SOURCE_MAP_LINGO_BYTECODE_UNSUPPORTED",
                "source": requirement["source"],
                "script_count": requirement["script_count"],
                "unsupported_script_count": requirement["unsupported_script_count"],
                "message": "Director Lingo bytecode requires a complete Lctx/Lnam/Lscr source-map reader before route coverage can be proven",
            }
        )
    diagnostics.extend(
        _duplicate_route_conflict_diagnostics(
            routes,
            code="TSUI_SCRIPT_SOURCE_MAP_DUPLICATE_ROUTE_CONFLICT",
            message="script source map maps one route_id to multiple terminal or choice signatures",
        )
    )
    diagnostics.extend(
        _duplicate_choice_diagnostics(
            routes,
            code="TSUI_SCRIPT_SOURCE_MAP_DUPLICATE_CHOICE",
            message="script source map route choices must be unique for each route_id",
        )
    )
    routes = _dedupe_script_source_routes(routes)
    if not routes:
        diagnostics.append(
            {
                "code": "TSUI_SCRIPT_SOURCE_MAP_ROUTE_MISSING",
                "message": "no route markers were found in readable script text",
            }
        )
    report = {
        "schema": "tsuinosora.script_source_map_report.v1",
        "status": "blocked" if diagnostics else "pass",
        "source_count": len(sources),
        "route_count": len(routes),
        "reader_count": len(readers),
        "readers": readers,
        "sources": sources,
        "routes": routes,
        "diagnostics": diagnostics,
        "redaction": {
            "paths": "report_relative_only",
            "payload": "omitted",
            "commercial_text": "omitted",
        },
    }
    if _report_has_path_leak(report):
        report["status"] = "blocked"
        report["diagnostics"].append(
            {
                "code": "TSUI_SCRIPT_SOURCE_MAP_REPORT_PATH_LEAK",
                "message": "script source map report contains a local path-like value",
            }
        )
    return report


def _duplicate_choice_diagnostics(routes: list[dict], *, code: str, message: str) -> list[dict]:
    diagnostics = []
    for route in routes:
        seen: set[str] = set()
        reported: set[str] = set()
        for choice in [str(value).strip() for value in route.get("choices", []) or []]:
            if not choice:
                continue
            if choice not in seen:
                seen.add(choice)
                continue
            if choice in reported:
                continue
            reported.add(choice)
            diagnostics.append(
                {
                    "code": code,
                    "route_id": str(route.get("route_id", "unknown")).strip() or "unknown",
                    "source": str(route.get("source", "")).strip(),
                    "choice": choice,
                    "message": message,
                }
            )
    return diagnostics


def _duplicate_route_conflict_diagnostics(routes: list[dict], *, code: str, message: str) -> list[dict]:
    diagnostics = []
    first_by_route_id: dict[str, dict] = {}
    reported: set[str] = set()
    for route in routes:
        route_id = str(route.get("route_id", "")).strip()
        if not route_id:
            continue
        signature = {
            "terminal": str(route.get("terminal", "")).strip(),
            "choices": [str(choice).strip() for choice in route.get("choices", []) or []],
        }
        current = {
            "route_id": route_id,
            "source": str(route.get("source", "")).strip(),
            **signature,
        }
        first = first_by_route_id.get(route_id)
        if first is None:
            first_by_route_id[route_id] = current
            continue
        if first["terminal"] == signature["terminal"] and first["choices"] == signature["choices"]:
            continue
        if route_id in reported:
            continue
        reported.add(route_id)
        diagnostics.append(
            {
                "code": code,
                "route_id": route_id,
                "source": current["source"],
                "first_source": first["source"],
                "terminal": current["terminal"],
                "first_terminal": first["terminal"],
                "choice_count": len(signature["choices"]),
                "first_choice_count": len(first["choices"]),
                "message": message,
            }
        )
    return diagnostics


def _dedupe_script_source_routes(routes: list[dict]) -> list[dict]:
    by_key: dict[tuple[str, str, tuple[str, ...]], dict] = {}
    order: list[tuple[str, str, tuple[str, ...]]] = []
    for route in routes:
        key = (
            str(route.get("route_id", "")),
            str(route.get("terminal", "")),
            tuple(str(choice) for choice in route.get("choices", [])),
        )
        if key not in by_key:
            by_key[key] = route
            order.append(key)
            continue
        current = by_key[key]
        if route.get("source_map") and not current.get("source_map"):
            by_key[key] = route
    return [by_key[key] for key in order]
