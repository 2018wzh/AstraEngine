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
from tsuinosora_diagnostics import _rel, _write_json

__all__ = ['_convert_projectorrays_lscr_chunk', '_projectorrays_lscr_metadata_is_empty', '_convert_projectorrays_empty_lscr_metadata', '_read_projectorrays_lscr_metadata', '_build_projectorrays_script_source_index', '_find_projectorrays_lscr_script_source', '_projectorrays_chunk_scope', '_projectorrays_script_scope', '_ascii_path_segment', '_projectorrays_native_metadata_path', '_projectorrays_native_lscr_script_path']


def _convert_projectorrays_lscr_chunk(
    work_root: Path,
    alias: str,
    source: Path,
    source_relative_path: str,
    paired_json: Path,
    script_index: dict[tuple[tuple[str, ...], int, str], list[dict]],
    diagnostics: list[dict],
) -> dict | None:
    metadata = _read_projectorrays_lscr_metadata(paired_json)
    role = PROJECTORRAYS_REQUIRED_CHUNK_ROLES["Lscr"]
    if metadata is None:
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERT_LSCR_METADATA_INVALID",
                "source_alias": alias,
                "source_relative_path": source_relative_path,
                "chunk_fourcc": "Lscr",
                "role": role,
                "message": "ProjectorRays Lscr metadata JSON is required before a decompiled script can be bound",
            }
        )
        return None
    cast_id = metadata.get("castID")
    if not isinstance(cast_id, int) or cast_id <= 0:
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERT_LSCR_CAST_BINDING_MISSING",
                "source_alias": alias,
                "source_relative_path": source_relative_path,
                "chunk_fourcc": "Lscr",
                "role": role,
                "message": "ProjectorRays Lscr metadata must expose a positive castID before script source binding",
            }
        )
        return None
    cast_member_id = cast_id & 0xFFFF
    cast_library_id = (cast_id >> 16) & 0xFFFF
    if cast_member_id <= 0:
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERT_LSCR_CAST_BINDING_INVALID",
                "source_alias": alias,
                "source_relative_path": source_relative_path,
                "chunk_fourcc": "Lscr",
                "role": role,
                "message": "ProjectorRays Lscr castID did not contain a valid cast member id",
            }
        )
        return None
    script_number = metadata.get("scriptNumber")
    script_number = script_number if isinstance(script_number, int) and script_number >= 0 else None
    lookup = _find_projectorrays_lscr_script_source(
        script_index,
        source_relative_path,
        cast_member_id,
        script_number,
    )
    if lookup["status"] == "ambiguous":
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERT_LSCR_SOURCE_AMBIGUOUS",
                "source_alias": alias,
                "source_relative_path": source_relative_path,
                "chunk_fourcc": "Lscr",
                "role": role,
                "cast_library_id": cast_library_id,
                "cast_member_id": cast_member_id,
                "candidate_count": lookup["candidate_count"],
                "message": "ProjectorRays Lscr script source binding must resolve to exactly one same-scope source",
            }
        )
        return None
    if lookup["status"] == "missing":
        if _projectorrays_lscr_metadata_is_empty(metadata):
            return _convert_projectorrays_empty_lscr_metadata(
                work_root,
                alias,
                source,
                source_relative_path,
                role,
                cast_library_id,
                cast_member_id,
                script_number,
                metadata,
            )
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERT_LSCR_SOURCE_MISSING",
                "source_alias": alias,
                "source_relative_path": source_relative_path,
                "chunk_fourcc": "Lscr",
                "role": role,
                "cast_library_id": cast_library_id,
                "cast_member_id": cast_member_id,
                "message": "ProjectorRays Lscr metadata did not resolve to a same-scope decompiled script source",
            }
        )
        return None
    script_record = lookup["script"]
    if script_record["path"].stat().st_size <= 0:
        if _projectorrays_lscr_metadata_is_empty(metadata):
            return _convert_projectorrays_empty_lscr_metadata(
                work_root,
                alias,
                source,
                source_relative_path,
                role,
                cast_library_id,
                cast_member_id,
                script_number,
                metadata,
            )
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERT_LSCR_SOURCE_EMPTY",
                "source_alias": alias,
                "source_relative_path": source_relative_path,
                "chunk_fourcc": "Lscr",
                "role": role,
                "cast_library_id": cast_library_id,
                "cast_member_id": cast_member_id,
                "message": "ProjectorRays Lscr decompiled script source must be non-empty before conversion",
            }
        )
        return None
    native_path = _projectorrays_native_lscr_script_path(alias, source_relative_path, script_record["extension"])
    native_file = work_root / native_path
    native_file.parent.mkdir(parents=True, exist_ok=True)
    native_file.write_bytes(script_record["path"].read_bytes())
    method = (
        "projectorrays_lscr_decompiled_script"
        if script_record["extension"] == ".ls"
        else "projectorrays_lscr_assembly_listing"
    )
    return {
        "source_alias": alias,
        "source_relative_path": source_relative_path,
        "source_sha256": _sha256(source),
        "chunk_fourcc": "Lscr",
        "role": role,
        "native_path": native_path,
        "converted_sha256": _sha256(native_file),
        "byte_size": native_file.stat().st_size,
        "conversion_method": method,
        "cast_library_id": cast_library_id,
        "cast_member_id": cast_member_id,
        "script_number": script_number if script_number is not None else "unknown",
        "script_source_sha256": _sha256(script_record["path"]),
        "script_source_kind": script_record["kind"],
        "script_source_binding": lookup["binding"],
        "metadata_source": metadata.get("_metadata_source", "projectorrays_json"),
        "status": "converted",
    }


def _projectorrays_lscr_metadata_is_empty(metadata: dict) -> bool:
    count_fields = ("handlersCount", "literalsCount", "globalsCount", "propertiesCount")
    if any(not isinstance(metadata.get(field), int) or metadata.get(field) != 0 for field in count_fields):
        return False
    list_fields = ("handlers", "literals", "globalNameIDs", "propertyNameIDs")
    for field in list_fields:
        value = metadata.get(field)
        if value is not None and (not isinstance(value, list) or len(value) != 0):
            return False
    return True


def _convert_projectorrays_empty_lscr_metadata(
    work_root: Path,
    alias: str,
    source: Path,
    source_relative_path: str,
    role: str,
    cast_library_id: int,
    cast_member_id: int,
    script_number: int | None,
    metadata: dict,
) -> dict:
    native_path = _projectorrays_native_metadata_path(alias, source_relative_path)
    native_file = work_root / native_path
    native_payload = {
        "schema": "tsuinosora.projectorrays_empty_lscr_metadata.v1",
        "source_sha256": _sha256(source),
        "chunk_fourcc": "Lscr",
        "cast_library_id": cast_library_id,
        "cast_member_id": cast_member_id,
        "script_number": script_number if script_number is not None else "unknown",
        "script_flags": metadata.get("scriptFlags", "unknown"),
        "handler_count": 0,
        "literal_count": 0,
        "global_count": 0,
        "property_count": 0,
        "redaction": {"payload": "omitted"},
    }
    _write_json(native_file, native_payload)
    return {
        "source_alias": alias,
        "source_relative_path": source_relative_path,
        "source_sha256": _sha256(source),
        "chunk_fourcc": "Lscr",
        "role": role,
        "native_path": native_path,
        "converted_sha256": _sha256(native_file),
        "byte_size": native_file.stat().st_size,
        "conversion_method": "projectorrays_lscr_empty_script_metadata",
        "cast_library_id": cast_library_id,
        "cast_member_id": cast_member_id,
        "script_number": script_number if script_number is not None else "unknown",
        "handler_count": 0,
        "literal_count": 0,
        "global_count": 0,
        "property_count": 0,
        "script_source_binding": "empty_script_metadata",
        "metadata_source": metadata.get("_metadata_source", "projectorrays_json"),
        "status": "converted",
    }


def _read_projectorrays_lscr_metadata(path: Path) -> dict | None:
    if not path.is_file():
        return None
    try:
        text = path.read_text(encoding="utf-8")
    except UnicodeDecodeError:
        return None
    try:
        value = loads_projectorrays_json(text)
    except json.JSONDecodeError:
        return None
    if isinstance(value, dict):
        value["_metadata_source"] = "projectorrays_json"
        return value
    return None

def _build_projectorrays_script_source_index(root: Path) -> dict[tuple[tuple[str, ...], int, str], list[dict]]:
    index: dict[tuple[tuple[str, ...], int, str], list[dict]] = {}
    for path in sorted(root.rglob("*")):
        if not path.is_file() or path.suffix.lower() not in {".ls", ".lasm"}:
            continue
        match = PROJECTORRAYS_SCRIPT_SOURCE_RE.match(path.stem)
        if not match:
            continue
        member_id = int(match.group(2))
        relative_path = _rel(path, root)
        scope = _projectorrays_script_scope(relative_path)
        record = {
            "path": path,
            "member_id": member_id,
            "extension": path.suffix.lower(),
            "kind": match.group(1).lower(),
            "scope": scope,
        }
        index.setdefault((scope, member_id, path.suffix.lower()), []).append(record)
    return index


def _find_projectorrays_lscr_script_source(
    script_index: dict[tuple[tuple[str, ...], int, str], list[dict]],
    source_relative_path: str,
    cast_member_id: int,
    script_number: int | None,
) -> dict:
    scope = _projectorrays_chunk_scope(source_relative_path)
    for binding, member_id in (("cast_member", cast_member_id), ("script_number", script_number)):
        if member_id is None or member_id <= 0:
            continue
        for extension in (".ls", ".lasm"):
            candidates = script_index.get((scope, member_id, extension), [])
            if len(candidates) == 1:
                return {"status": "matched", "script": candidates[0], "binding": binding}
            if len(candidates) > 1:
                return {"status": "ambiguous", "candidate_count": len(candidates), "binding": binding}
    return {"status": "missing", "candidate_count": 0}


def _projectorrays_chunk_scope(source_relative_path: str) -> tuple[str, ...]:
    parts = source_relative_path.replace("\\", "/").split("/")
    if "chunks" in parts:
        return tuple(parts[: parts.index("chunks")])
    return tuple(parts[:-1])


def _projectorrays_script_scope(source_relative_path: str) -> tuple[str, ...]:
    parts = source_relative_path.replace("\\", "/").split("/")
    if "casts" in parts:
        return tuple(parts[: parts.index("casts")])
    return tuple(parts[:-1])


def _ascii_path_segment(value: str) -> str:
    cleaned = re.sub(r"[^A-Za-z0-9_.-]+", "_", value).strip("._")
    return cleaned or "chunk"


def _projectorrays_native_metadata_path(alias: str, source_relative_path: str) -> str:
    parts = source_relative_path.replace("\\", "/").split("/")
    parts[-1] = Path(parts[-1]).with_suffix(".json").name
    safe_parts = [_ascii_path_segment(part) for part in parts]
    return "/".join(["native-assets", "projectorrays", _ascii_path_segment(alias), *safe_parts])


def _projectorrays_native_lscr_script_path(alias: str, source_relative_path: str, extension: str) -> str:
    parts = source_relative_path.replace("\\", "/").split("/")
    suffix = extension if extension in {".ls", ".lasm"} else ".ls"
    parts[-1] = Path(parts[-1]).with_suffix(suffix).name
    safe_parts = [_ascii_path_segment(part) for part in parts]
    return "/".join(["native-assets", "projectorrays", _ascii_path_segment(alias), *safe_parts])
