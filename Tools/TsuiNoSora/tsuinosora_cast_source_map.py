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
from tsuinosora_director_core import _cast_member_from_map, _safe_atlas_parts
from tsuinosora_diagnostics import _is_unpacked_metadata_file, _rel
from tsuinosora_director_core import _safe_symbol_list
from tsuinosora_rendering import _read_json, _report_has_path_leak, _safe_identifier
from tsuinosora_visual_analysis import analyze_asset
from tsuinosora_script_source_routes import _forbidden_payload_key_diagnostics

__all__ = ['build_cast_source_map_report', '_cast_source_map_payload_key_diagnostics', '_cast_members_from_director_cast_report', '_asset_hash_index', '_director_child_source_from_hash', '_director_child_kind']


def build_cast_source_map_report(root: Path | str) -> dict:
    root = Path(root)
    diagnostics = []
    sources = []
    members = []
    asset_index = {
        _rel(path, root): path
        for path in sorted(p for p in root.rglob("*") if p.is_file() and not _is_unpacked_metadata_file(p))
    }

    for path in sorted(p for p in root.rglob("*.json") if p.is_file()):
        rel = _rel(path, root)
        try:
            value = _read_json(path)
        except json.JSONDecodeError:
            continue
        if not isinstance(value, dict):
            continue
        schema = value.get("schema")
        if schema == "tsuinosora.cast_map.v1":
            payload_diagnostics = _cast_source_map_payload_key_diagnostics(value, rel)
            diagnostics.extend(payload_diagnostics)
            source_members = []
            for raw_member in value.get("members", []):
                if not isinstance(raw_member, dict):
                    continue
                member, member_diagnostics = _cast_member_from_map(raw_member, rel, asset_index)
                if member:
                    source_members.append(member)
                    members.append(member)
                diagnostics.extend(member_diagnostics)
            sources.append(
                {
                    "source": rel,
                    "sha256": _sha256(path),
                    "member_count": len(source_members),
                }
            )
        elif schema == "tsuinosora.director_cast_map.v1":
            diagnostics.extend(_cast_source_map_payload_key_diagnostics(value, rel))
            source_members, member_diagnostics = _cast_members_from_director_cast_report(value, rel, root, asset_index)
            members.extend(source_members)
            diagnostics.extend(member_diagnostics)
            sources.append(
                {
                    "source": rel,
                    "sha256": _sha256(path),
                    "member_count": len(source_members),
                    "source_schema": schema,
                }
            )

    if not members:
        diagnostics.append(
            {
                "code": "TSUI_CAST_SOURCE_MAP_MISSING",
                "message": "no tsuinosora.cast_map.v1 or tsuinosora.director_cast_map.v1 members were found in unpacked assets",
            }
        )

    report = {
        "schema": "tsuinosora.cast_source_map_report.v1",
        "status": "blocked" if diagnostics else "pass",
        "source_count": len(sources),
        "member_count": len(members),
        "sources": sources,
        "members": members,
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
                "code": "TSUI_CAST_SOURCE_MAP_REPORT_PATH_LEAK",
                "message": "cast source map report contains a local path-like value",
            }
        )
    return report


def _cast_source_map_payload_key_diagnostics(value, map_source: str) -> list[dict]:
    return _forbidden_payload_key_diagnostics(
        value,
        map_source,
        code="TSUI_CAST_SOURCE_MAP_PAYLOAD_FIELD",
        source_field="source",
        message="cast source map sidecar must not contain commercial text, bytecode or payload fields",
    )


def _cast_members_from_director_cast_report(
    report: dict,
    map_source: str,
    root: Path,
    asset_index: dict[str, Path],
) -> tuple[list[dict], list[dict]]:
    diagnostics = [
        diagnostic
        for diagnostic in report.get("diagnostics", [])
        if isinstance(diagnostic, dict)
    ]
    members = []
    asset_hash_index = _asset_hash_index(asset_index)
    for container in report.get("containers", []):
        if not isinstance(container, dict):
            continue
        if container.get("status") != "pass":
            diagnostics.extend(
                diagnostic
                for diagnostic in container.get("diagnostics", [])
                if isinstance(diagnostic, dict)
            )
        source_container = str(container.get("relative_path", "")).strip()
        container_id = _safe_identifier(Path(source_container).with_suffix("").as_posix())
        for director_member in container.get("members", []):
            if not isinstance(director_member, dict):
                continue
            member_id = str(director_member.get("member_id", "")).strip()
            for child in director_member.get("child_resources", []):
                if not isinstance(child, dict):
                    continue
                child_resource_id = child.get("resource_id")
                try:
                    entry_id = f"{container_id}.{int(child_resource_id):04d}"
                except (TypeError, ValueError):
                    diagnostics.append(
                        {
                            "code": "TSUI_CAST_DIRECTOR_CHILD_RESOURCE_ID_INVALID",
                            "source": map_source,
                            "member_id": member_id or "unknown",
                            "message": "Director child resource id is not numeric",
                        }
                    )
                    continue
                source, source_diagnostics = _director_child_source_from_hash(
                    child,
                    container_id,
                    map_source,
                    member_id,
                    asset_hash_index,
                )
                diagnostics.extend(source_diagnostics)
                if not source:
                    continue
                metadata_kind = str(director_member.get("kind", "")).strip()
                if metadata_kind and metadata_kind not in CAST_MEMBER_KINDS:
                    diagnostics.append(
                        {
                            "code": "TSUI_CAST_DIRECTOR_MEMBER_KIND_INVALID",
                            "source": map_source,
                            "member_id": member_id or "unknown",
                            "message": "Director cast member kind is not part of the allowed classification set",
                        }
                    )
                route_ids = _safe_symbol_list(director_member.get("route_ids", []))
                if route_ids is None:
                    diagnostics.append(
                        {
                            "code": "TSUI_CAST_DIRECTOR_MEMBER_ROUTE_ID_INVALID",
                            "source": map_source,
                            "member_id": member_id or "unknown",
                            "message": "Director cast member route_ids must be safe symbols",
                        }
                    )
                    route_ids = []
                command_ids = _safe_symbol_list(director_member.get("command_ids", []))
                if command_ids is None:
                    diagnostics.append(
                        {
                            "code": "TSUI_CAST_DIRECTOR_MEMBER_COMMAND_ID_INVALID",
                            "source": map_source,
                            "member_id": member_id or "unknown",
                            "message": "Director cast member command_ids must be safe symbols",
                        }
                    )
                    command_ids = []
                parts = []
                if "parts" in director_member:
                    parts, part_diagnostics = _safe_atlas_parts(
                        director_member.get("parts"),
                        source=map_source,
                        owner_id=member_id or "unknown",
                        source_field="source",
                        code_prefix="TSUI_CAST_DIRECTOR_MEMBER",
                    )
                    diagnostics.extend(part_diagnostics)
                elif metadata_kind == "character_atlas":
                    diagnostics.append(
                        {
                            "code": "TSUI_CAST_DIRECTOR_MEMBER_ATLAS_PARTS_MISSING",
                            "source": map_source,
                            "member_id": member_id or "unknown",
                            "message": "character_atlas director member must include crop/part records",
                        }
                    )
                kind = metadata_kind if metadata_kind in CAST_MEMBER_KINDS else _director_child_kind(child, asset_index[source], root)
                raw_member = {
                    "member_id": member_id,
                    "kind": kind,
                    "source": source,
                    "container_entry_id": entry_id,
                    "director_child_resource_id": child_resource_id,
                    "director_child_tag": child.get("tag", ""),
                    "director_child_payload_sha256": child.get("payload_sha256", ""),
                    "route_ids": route_ids,
                    "command_ids": command_ids,
                }
                if parts:
                    raw_member["parts"] = parts
                member, member_diagnostics = _cast_member_from_map(raw_member, map_source, asset_index)
                if member:
                    members.append(member)
                diagnostics.extend(member_diagnostics)
    return members, diagnostics


def _asset_hash_index(asset_index: dict[str, Path]) -> dict[str, list[str]]:
    index: dict[str, list[str]] = {}
    for rel, path in asset_index.items():
        index.setdefault(_sha256(path), []).append(rel)
    return {digest: sorted(paths) for digest, paths in index.items()}


def _director_child_source_from_hash(
    child: dict,
    container_id: str,
    map_source: str,
    member_id: str,
    asset_hash_index: dict[str, list[str]],
) -> tuple[str, list[dict]]:
    diagnostics = []
    payload_hash = str(child.get("payload_sha256", "")).strip()
    if not payload_hash.startswith("sha256:"):
        return "", [
            {
                "code": "TSUI_CAST_DIRECTOR_CHILD_HASH_MISSING",
                "source": map_source,
                "member_id": member_id or "unknown",
                "resource_id": child.get("resource_id", "unknown"),
                "message": "Director child resource requires a sanitized payload hash",
            }
        ]
    candidates = asset_hash_index.get(payload_hash, [])
    container_prefix = f"containers/{container_id}/"
    scoped_candidates = [candidate for candidate in candidates if candidate.startswith(container_prefix)]
    if not scoped_candidates:
        return "", [
            {
                "code": "TSUI_CAST_DIRECTOR_CHILD_SOURCE_MISSING",
                "source": map_source,
                "member_id": member_id or "unknown",
                "resource_id": child.get("resource_id", "unknown"),
                "payload_sha256": payload_hash,
                "message": "Director child resource was not found among extracted readable assets",
            }
        ]
    if len(scoped_candidates) > 1:
        diagnostics.append(
            {
                "code": "TSUI_CAST_DIRECTOR_CHILD_SOURCE_AMBIGUOUS",
                "source": map_source,
                "member_id": member_id or "unknown",
                "resource_id": child.get("resource_id", "unknown"),
                "candidate_count": len(scoped_candidates),
                "message": "Director child resource hash matches multiple extracted assets in the same container",
            }
        )
    return scoped_candidates[0], diagnostics


def _director_child_kind(child: dict, source_path: Path, root: Path) -> str:
    tag = str(child.get("tag", "")).strip()
    if tag in SCRIPT_TEXT_CHUNK_IDS:
        return "script"
    if source_path.suffix.lower() in AUDIO_EXTS:
        return "audio"
    if source_path.suffix.lower() in MOVIE_EXTS:
        return "movie"
    if source_path.suffix.lower() in FONT_EXTS:
        return "font"
    if source_path.suffix.lower() in IMAGE_EXTS:
        classification = analyze_asset(source_path, root).get("classification", "unknown")
        if classification in CAST_MEMBER_KINDS:
            return classification
    return "unknown"
