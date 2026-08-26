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
from tsuinosora_diagnostics import _rel, _is_safe_symbol, _is_safe_report_relative_path
from tsuinosora_rendering import _read_json
from tsuinosora_constants import _read_text_lossless, _script_route_marker
from tsuinosora_diagnostics import _dedupe_diagnostics, _write_json
from tsuinosora_rendering import _report_has_path_leak, _safe_identifier
from tsuinosora_stage3_demo_slice import _demo_slice_config_diagnostics

__all__ = ['import_projectorrays_reader', '_projectorrays_route_from_script_identity', '_read_projectorrays_reader_config', '_read_demo_slice_config']


def import_projectorrays_reader(config_path: Path | str) -> dict:
    config_path = Path(config_path)
    config, diagnostics = _read_projectorrays_reader_config(config_path)
    work_root = Path(str(config.get("local_work_root", ""))) if isinstance(config, dict) and config.get("local_work_root") else None
    routes = []
    sources = []
    tool_hash = ""

    if not diagnostics:
        tool_path = Path(str(config["projectorrays_tool"]))
        dump_root = Path(str(config["dump_root"]))
        work_root = Path(str(config["local_work_root"]))
        tool_hash = _sha256(tool_path)
        unpacked_root = work_root / "unpacked"
        manifest_rel = "projectorrays/script_dump_manifest.json"
        manifest_path = unpacked_root / manifest_rel
        source_records = []

        for path in sorted(p for p in dump_root.rglob("*") if p.is_file() and p.suffix.lower() in {".ls", ".lingo", ".txt"}):
            rel = _rel(path, dump_root)
            lines = _read_text_lossless(path).splitlines()
            source_hash = _sha256(path)
            source_routes = []
            for line_no, line in enumerate(lines, start=1):
                route = _script_route_marker(line)
                if not route:
                    continue
                route["source"] = manifest_rel
                route["line"] = line_no
                route["source_hash"] = ""
                source_routes.append(route)
            if not source_routes:
                derived_route = _projectorrays_route_from_script_identity(path)
                if derived_route:
                    derived_route["source"] = manifest_rel
                    derived_route["line"] = len(source_records) + 1
                    derived_route["source_hash"] = ""
                    source_routes.append(derived_route)
            source_records.append(
                {
                    "dump_source": rel,
                    "sha256": source_hash,
                    "line_count": len(lines),
                    "route_count": len(source_routes),
                }
            )
            routes.extend(source_routes)

        manifest = {
            "schema": "tsuinosora.projectorrays_dump_manifest.v1",
            "source_count": len(source_records),
            "sources": source_records,
            "redaction": {
                "paths": "dump_relative_only",
                "payload": "omitted",
                "commercial_text": "omitted",
                "bytecode": "omitted",
            },
        }
        _write_json(manifest_path, manifest)
        manifest_hash = _sha256(manifest_path)
        for route in routes:
            route["source_hash"] = manifest_hash
        sources = [
            {
                "source": manifest_rel,
                "sha256": manifest_hash,
                "line_count": 0,
                "script_count": len(source_records),
            }
        ]
        sidecar = {
            "schema": "tsuinosora.script_source_map.v1",
            "reader": {
                "tool_id": "projectorrays",
                "tool_hash": tool_hash,
                "output_contract": "route_source_map",
            },
            "sources": sources,
            "routes": routes,
        }
        _write_json(unpacked_root / "projectorrays_script_source_map.json", sidecar)

        if not routes:
            diagnostics.append(
                {
                    "code": "TSUI_PROJECTORRAYS_ROUTES_MISSING",
                    "message": "ProjectorRays dump did not contain sanitized route markers",
                }
            )

    report = {
        "schema": "tsuinosora.projectorrays_reader_report.v1",
        "status": "blocked" if diagnostics else "pass",
        "tool": {
            "id": "projectorrays",
            "hash": tool_hash,
        },
        "source_count": len(sources),
        "route_count": len(routes),
        "source_map": "unpacked/projectorrays_script_source_map.json" if routes else "",
        "diagnostics": _dedupe_diagnostics(diagnostics),
        "redaction": {
            "paths": "alias_or_report_relative_only",
            "payload": "omitted",
            "commercial_text": "omitted",
            "bytecode": "omitted",
        },
    }
    if _report_has_path_leak(report):
        report["status"] = "blocked"
        report["diagnostics"].append(
            {
                "code": "TSUI_PROJECTORRAYS_READER_REPORT_PATH_LEAK",
                "message": "ProjectorRays reader report contains a local path-like value",
            }
        )
        report["diagnostics"] = _dedupe_diagnostics(report["diagnostics"])
    if work_root:
        _write_json(work_root / "reports" / "projectorrays_reader_report.json", report)
    return report


def _projectorrays_route_from_script_identity(path: Path) -> dict | None:
    match = PROJECTORRAYS_GO_ROUTE_SOURCE_RE.match(path.stem)
    if not match:
        return None
    token = _safe_identifier(match.group(2)).strip("_")
    if not token:
        return None
    route_id = f"classic.{token.lower()}"
    if not _is_safe_symbol(route_id):
        return None
    return {
        "route_id": route_id,
        "coverage": "covered",
        "terminal": f"ending.{_safe_identifier(route_id)}",
        "choices": [],
    }


def _read_projectorrays_reader_config(config_path: Path) -> tuple[dict, list[dict]]:
    try:
        config = _read_json(config_path)
    except (OSError, json.JSONDecodeError):
        return {}, [
            {
                "code": "TSUI_PROJECTORRAYS_CONFIG_UNREADABLE",
                "message": "ProjectorRays reader config is missing, inaccessible or not valid JSON",
            }
        ]
    diagnostics = []
    if not isinstance(config, dict):
        return {}, [
            {
                "code": "TSUI_PROJECTORRAYS_CONFIG_INVALID",
                "message": "ProjectorRays reader config must be a JSON object",
            }
        ]
    if config.get("schema") != "tsuinosora.projectorrays_reader_config.v1":
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONFIG_SCHEMA_INVALID",
                "message": "ProjectorRays reader config schema must be tsuinosora.projectorrays_reader_config.v1",
            }
        )
    for key in ["projectorrays_tool", "dump_root", "local_work_root"]:
        value = config.get(key)
        if not isinstance(value, str) or not value.strip():
            diagnostics.append(
                {
                    "code": "TSUI_PROJECTORRAYS_CONFIG_PATH_MISSING",
                    "field": key,
                    "message": "ProjectorRays reader config requires tool, dump root and local work root path fields",
                }
            )
            continue
        path = Path(value)
        if key == "projectorrays_tool" and not path.is_file():
            diagnostics.append(
                {
                    "code": "TSUI_PROJECTORRAYS_TOOL_MISSING",
                    "message": "ProjectorRays tool path is missing or inaccessible",
                }
            )
        if key == "dump_root" and not path.is_dir():
            diagnostics.append(
                {
                    "code": "TSUI_PROJECTORRAYS_DUMP_ROOT_MISSING",
                    "message": "ProjectorRays dump root is missing or inaccessible",
                }
            )
    if config.get("routes"):
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_ROUTE_EVIDENCE_REQUIRED",
                "message": "ProjectorRays reader routes must be derived from dump evidence, not config",
            }
        )
    return config, _dedupe_diagnostics(diagnostics)


def _read_demo_slice_config(config_path: Path) -> tuple[dict, list[dict]]:
    try:
        config = _read_json(config_path)
    except (OSError, json.JSONDecodeError):
        return {}, [
            {
                "code": "TSUI_DEMO_SLICE_CONFIG_UNREADABLE",
                "message": "demo-slice config is missing, inaccessible or not valid JSON",
            }
        ]
    diagnostics = _demo_slice_config_diagnostics(config)
    return config if isinstance(config, dict) else {}, diagnostics
