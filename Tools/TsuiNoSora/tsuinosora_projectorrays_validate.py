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
from tsuinosora_diagnostics import _dedupe_diagnostics, _is_safe_report_relative_path, _is_safe_symbol, _is_sanitized_sha256, _positive_int
from tsuinosora_rendering import _read_json

__all__ = ['_projectorrays_converted_resource_evidence', '_validate_projectorrays_converted_resource', '_projectorrays_chunk_fourcc', '_projectorrays_chunk_resource_id', '_is_safe_projectorrays_fourcc']


def _projectorrays_converted_resource_evidence(
    work_root: Path,
    binary_chunks: dict[tuple[str, str], dict],
) -> tuple[list[dict], dict[str, int], list[dict]]:
    evidence_path = work_root / "reports" / "projectorrays_converted_resources.json"
    if not evidence_path.exists():
        return [], {}, []
    diagnostics = []
    try:
        evidence = _read_json(evidence_path)
    except (json.JSONDecodeError, UnicodeDecodeError):
        return (
            [],
            {},
            [
                {
                    "code": "TSUI_PROJECTORRAYS_CONVERTED_EVIDENCE_INVALID",
                    "message": "ProjectorRays converted resource evidence must be valid JSON",
                }
            ],
        )
    if not isinstance(evidence, dict) or evidence.get("schema") != "tsuinosora.projectorrays_converted_resources.v1":
        return (
            [],
            {},
            [
                {
                    "code": "TSUI_PROJECTORRAYS_CONVERTED_SCHEMA_INVALID",
                    "message": "ProjectorRays converted resource evidence schema is invalid",
                }
            ],
        )
    raw_resources = evidence.get("resources", [])
    if not isinstance(raw_resources, list):
        return (
            [],
            {},
            [
                {
                    "code": "TSUI_PROJECTORRAYS_CONVERTED_RESOURCES_INVALID",
                    "message": "ProjectorRays converted resource evidence resources must be a list",
                }
            ],
        )
    converted = []
    converted_counts: dict[str, int] = {}
    seen_sources = set()
    for index, raw in enumerate(raw_resources):
        record = _validate_projectorrays_converted_resource(work_root, binary_chunks, raw, index, diagnostics)
        if not record:
            continue
        source_key = (record["source_alias"], record["source_relative_path"])
        if source_key in seen_sources:
            diagnostics.append(
                {
                    "code": "TSUI_PROJECTORRAYS_CONVERTED_SOURCE_DUPLICATE",
                    "index": index,
                    "message": "ProjectorRays converted resource evidence must not duplicate a source chunk",
                }
            )
            continue
        seen_sources.add(source_key)
        converted.append(record)
        fourcc = record["chunk_fourcc"]
        converted_counts[fourcc] = converted_counts.get(fourcc, 0) + 1
    return converted, converted_counts, _dedupe_diagnostics(diagnostics)


def _validate_projectorrays_converted_resource(
    work_root: Path,
    binary_chunks: dict[tuple[str, str], dict],
    raw: object,
    index: int,
    diagnostics: list[dict],
) -> dict | None:
    if not isinstance(raw, dict):
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERTED_RESOURCE_INVALID",
                "index": index,
                "message": "ProjectorRays converted resource evidence entries must be objects",
            }
        )
        return None
    source_alias = str(raw.get("source_alias", "")).strip()
    source_relative_path = str(raw.get("source_relative_path", "")).strip()
    source_key = (source_alias, source_relative_path)
    source = binary_chunks.get(source_key)
    raw_chunk_fourcc = raw.get("chunk_fourcc", "")
    chunk_fourcc = raw_chunk_fourcc if isinstance(raw_chunk_fourcc, str) else str(raw_chunk_fourcc)
    role = str(raw.get("role", "")).strip()
    native_path = str(raw.get("native_path", "")).strip()
    source_sha256 = str(raw.get("source_sha256", "")).strip()
    converted_sha256 = str(raw.get("converted_sha256", "")).strip()
    conversion_method = str(raw.get("conversion_method", "")).strip()
    byte_size = _positive_int(raw.get("byte_size", 0))
    entry_diagnostics = []
    if not _is_safe_symbol(source_alias) or not _is_safe_report_relative_path(source_relative_path):
        entry_diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERTED_SOURCE_INVALID",
                "index": index,
                "message": "converted resource evidence must reference a dump-root relative source chunk",
            }
        )
    elif not source:
        entry_diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERTED_SOURCE_MISSING",
                "index": index,
                "source_alias": source_alias,
                "source_relative_path": source_relative_path,
                "message": "converted resource evidence references an unknown ProjectorRays binary chunk",
            }
        )
    if source and chunk_fourcc != source["chunk_fourcc"]:
        entry_diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERTED_CHUNK_FOURCC_MISMATCH",
                "index": index,
                "source_alias": source_alias,
                "source_relative_path": source_relative_path,
                "message": "converted resource evidence chunk fourcc does not match the source chunk",
            }
        )
    expected_role = PROJECTORRAYS_REQUIRED_CHUNK_ROLES.get(chunk_fourcc, "director_chunk")
    if role != expected_role:
        entry_diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERTED_ROLE_MISMATCH",
                "index": index,
                "message": "converted resource evidence role does not match the chunk role",
            }
        )
    if not _is_sanitized_sha256(source_sha256) or (source and source_sha256 != source["source_sha256"]):
        entry_diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERTED_SOURCE_HASH_MISMATCH",
                "index": index,
                "message": "converted resource evidence source hash does not match the source chunk",
            }
        )
    if not _is_safe_report_relative_path(native_path) or not native_path.startswith("native-assets/"):
        entry_diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERTED_NATIVE_PATH_INVALID",
                "index": index,
                "message": "converted resource evidence native path must be under native-assets",
            }
        )
        native_file = None
    else:
        native_file = work_root / native_path
    if native_file is not None and not native_file.is_file():
        entry_diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERTED_NATIVE_MISSING",
                "index": index,
                "native_path": native_path,
                "message": "converted resource evidence native asset is missing",
            }
        )
    if native_file is not None and native_file.is_file():
        native_hash = _sha256(native_file)
        native_size = native_file.stat().st_size
        if not _is_sanitized_sha256(converted_sha256) or converted_sha256 != native_hash:
            entry_diagnostics.append(
                {
                    "code": "TSUI_PROJECTORRAYS_CONVERTED_HASH_MISMATCH",
                    "index": index,
                    "native_path": native_path,
                    "message": "converted resource evidence hash does not match the native asset",
                }
            )
        if byte_size <= 0 or byte_size != native_size:
            entry_diagnostics.append(
                {
                    "code": "TSUI_PROJECTORRAYS_CONVERTED_BYTE_SIZE_MISMATCH",
                    "index": index,
                    "native_path": native_path,
                    "message": "converted resource evidence byte size does not match the native asset",
                }
            )
    forbidden_methods = {"", "hash_only", "route_only", "raw_chunk_copy", "raw_copy", "none"}
    if not _is_safe_symbol(conversion_method) or conversion_method in forbidden_methods:
        entry_diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERTED_METHOD_INVALID",
                "index": index,
                "message": "converted resource evidence must name a real converter method and cannot be raw chunk copy",
            }
        )
    if raw.get("status") != "converted":
        entry_diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERTED_STATUS_INVALID",
                "index": index,
                "message": "converted resource evidence status must be converted",
            }
        )
    diagnostics.extend(entry_diagnostics)
    if entry_diagnostics or not source:
        return None
    return {
        "source_alias": source_alias,
        "source_relative_path": source_relative_path,
        "source_sha256": source_sha256,
        "chunk_fourcc": chunk_fourcc,
        "role": role,
        "native_path": native_path,
        "converted_sha256": converted_sha256,
        "byte_size": byte_size,
        "conversion_method": conversion_method,
        "status": "converted",
    }


def _projectorrays_chunk_fourcc(path: Path) -> str:
    name = path.stem
    if "-" not in name:
        return "unknown"
    fourcc = name.rsplit("-", 1)[0]
    return fourcc if _is_safe_projectorrays_fourcc(fourcc) else "unknown"


def _projectorrays_chunk_resource_id(path: Path) -> int | None:
    name = path.stem
    if "-" not in name:
        return None
    raw_id = name.rsplit("-", 1)[1]
    return int(raw_id) if raw_id.isdigit() else None


def _is_safe_projectorrays_fourcc(value: str) -> bool:
    if not value or len(value) > 8:
        return False
    return all(32 <= ord(char) <= 126 and char not in "\\/:*" for char in value)

