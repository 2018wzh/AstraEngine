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
from tsuinosora_diagnostics import _rel, _is_safe_symbol, _reference_hashes, _classification_counts, _duplicate_hash_groups, _use_timing, _reference_matches, _is_unpacked_metadata_file, _asset_usage_index, _classification_conflicts, _container_source
from tsuinosora_rendering import _read_json, _report_has_path_leak
from tsuinosora_script_source_map import _duplicate_choice_diagnostics, _duplicate_route_conflict_diagnostics
from tsuinosora_script_source_routes import _forbidden_payload_key_diagnostics
from tsuinosora_visual_analysis import analyze_asset

__all__ = ['analyze_assets', 'build_route_graph_report']


def analyze_assets(root: Path | str, reference_report: dict | None) -> dict:
    root = Path(root)
    assets = []
    quarantine = []
    diagnostics = []
    for path in sorted(p for p in root.rglob("*") if p.is_file()):
        if _is_unpacked_metadata_file(path):
            continue
        rel = _rel(path, root)
        try:
            asset = analyze_asset(path, root)
        except Exception as exc:  # noqa: BLE001 - diagnostic must survive malformed local data.
            asset = {
                "relative_path": rel,
                "classification": "unknown",
                "confidence": 0.0,
                "diagnostics": [str(exc)],
            }
        assets.append(asset)
    usage_index = _asset_usage_index(root, [asset["relative_path"] for asset in assets])
    duplicate_groups = _duplicate_hash_groups(assets)
    duplicate_by_path = {
        rel: group for group in duplicate_groups for rel in group["relative_paths"]
    }
    for asset in assets:
        rel = asset["relative_path"]
        asset["container_source"] = _container_source(rel)
        asset["script_references"] = usage_index.get(rel, [])
        asset["use_timing"] = _use_timing(asset["script_references"])
        if rel in duplicate_by_path:
            asset["duplicate_hash_group"] = duplicate_by_path[rel]["duplicate_hash_group"]
            asset["duplicate_paths"] = duplicate_by_path[rel]["relative_paths"]
        asset["reference_matches"] = _reference_matches(asset, reference_report)

        asset_diagnostics = []
        if asset["classification"] == "unknown" or asset["confidence"] < 0.65:
            asset_diagnostics.append(
                {
                    "code": "TSUI_ASSET_LOW_CONFIDENCE",
                    "relative_path": rel,
                    "message": "asset classification is unknown or below confidence threshold",
                }
            )
        asset_diagnostics.extend(_classification_conflicts(asset))
        if asset_diagnostics:
            quarantine.append(
                {
                    "relative_path": rel,
                    "classification": asset["classification"],
                    "confidence": asset["confidence"],
                    "reason_codes": [diagnostic["code"] for diagnostic in asset_diagnostics],
                }
            )
            diagnostics.extend(asset_diagnostics)
    return {
        "schema": "tsuinosora.asset_analysis.v1",
        "status": "blocked" if quarantine else "pass",
        "reference_hashes": _reference_hashes(reference_report),
        "classification_counts": _classification_counts(assets),
        "duplicate_hashes": duplicate_groups,
        "assets": assets,
        "quarantine": quarantine,
        "diagnostics": diagnostics,
    }


def build_route_graph_report(root: Path | str) -> dict:
    root = Path(root)
    diagnostics = []
    routes = []
    sources = []
    for path in sorted(p for p in root.rglob("*.json") if p.is_file()):
        rel = _rel(path, root)
        try:
            value = _read_json(path)
        except json.JSONDecodeError:
            continue
        if not isinstance(value, dict) or value.get("schema") != "tsuinosora.route_graph.v1":
            continue
        payload_diagnostics = _forbidden_payload_key_diagnostics(
            value,
            rel,
            code="TSUI_ROUTE_GRAPH_PAYLOAD_FIELD",
            source_field="source",
            message="route graph sidecar must not contain script text, bytecode or payload fields",
        )
        diagnostics.extend(payload_diagnostics)
        extracted = []
        for route in value.get("routes", []):
            if not isinstance(route, dict):
                continue
            route_id = str(route.get("route_id", "")).strip()
            terminal = str(route.get("terminal", "")).strip()
            coverage = str(route.get("coverage", "unknown")).strip()
            choices = route.get("choices", [])
            route_diagnostics = []
            if not route_id or not terminal or coverage != "covered":
                route_diagnostics.append(
                    {
                        "code": "TSUI_ROUTE_GRAPH_INCOMPLETE_ROUTE",
                        "source": rel,
                        "route_id": route_id or "unknown",
                        "message": "route graph entries must include route_id, terminal and covered coverage",
                    }
                )
            if route_id and not _is_safe_symbol(route_id):
                route_diagnostics.append(
                    {
                        "code": "TSUI_ROUTE_GRAPH_ROUTE_ID_INVALID",
                        "source": rel,
                        "route_id": "invalid",
                        "message": "route graph route_id must be a safe symbol",
                    }
                )
            if terminal and not _is_safe_symbol(terminal):
                route_diagnostics.append(
                    {
                        "code": "TSUI_ROUTE_GRAPH_TERMINAL_INVALID",
                        "source": rel,
                        "route_id": route_id or "unknown",
                        "message": "route graph terminal must be a safe symbol",
                    }
                )
            if not isinstance(choices, list):
                route_diagnostics.append(
                    {
                        "code": "TSUI_ROUTE_GRAPH_CHOICES_INVALID",
                        "source": rel,
                        "route_id": route_id or "unknown",
                        "message": "route graph choices must be a list of safe symbols",
                    }
                )
                choices = []
            safe_choices = []
            for choice_index, choice in enumerate(choices):
                choice_id = str(choice).strip()
                if not choice_id:
                    continue
                if not _is_safe_symbol(choice_id):
                    route_diagnostics.append(
                        {
                            "code": "TSUI_ROUTE_GRAPH_CHOICE_INVALID",
                            "source": rel,
                            "route_id": route_id or "unknown",
                            "choice_index": choice_index,
                            "message": "route graph choice id must be a safe symbol",
                        }
                    )
                    continue
                safe_choices.append(choice_id)
            if payload_diagnostics:
                route_diagnostics.append(
                    {
                        "code": "TSUI_ROUTE_GRAPH_PAYLOAD_BLOCKED",
                        "source": rel,
                        "route_id": route_id or "unknown",
                        "message": "route graph with payload-like fields cannot prove sanitized route coverage",
                    }
                )
            if route_diagnostics:
                diagnostics.extend(route_diagnostics)
                continue
            extracted.append(
                {
                    "route_id": route_id,
                    "coverage": coverage,
                    "terminal": terminal,
                    "choices": safe_choices,
                    "source": rel,
                }
            )
        if extracted:
            sources.append(
                {
                    "source": rel,
                    "route_count": len(extracted),
                    "sha256": _sha256(path),
                }
            )
            routes.extend(extracted)
    diagnostics.extend(
        _duplicate_choice_diagnostics(
            routes,
            code="TSUI_ROUTE_GRAPH_DUPLICATE_CHOICE",
            message="route graph route choices must be unique for each route_id",
        )
    )
    diagnostics.extend(
        _duplicate_route_conflict_diagnostics(
            routes,
            code="TSUI_ROUTE_GRAPH_DUPLICATE_ROUTE_CONFLICT",
            message="route graph maps one route_id to multiple terminal or choice signatures",
        )
    )
    if not routes:
        diagnostics.append(
            {
                "code": "TSUI_ROUTE_GRAPH_MISSING",
                "message": "no covered route graph was found in unpacked assets",
            }
        )
    report = {
        "schema": "tsuinosora.route_graph_report.v1",
        "status": "blocked" if diagnostics else "pass",
        "source_count": len(sources),
        "route_count": len(routes),
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
                "code": "TSUI_ROUTE_GRAPH_REPORT_PATH_LEAK",
                "message": "route graph report contains a local path-like value",
            }
        )
    return report
