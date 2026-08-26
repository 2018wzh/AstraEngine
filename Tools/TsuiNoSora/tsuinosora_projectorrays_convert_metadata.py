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
from tsuinosora_diagnostics import _write_json
from tsuinosora_projectorrays_convert_bitmap import _projectorrays_native_metadata_path

__all__ = ['_write_projectorrays_metadata_asset', '_convert_projectorrays_empty_sound_placeholder_chunk', '_convert_projectorrays_cupt_chunk', '_convert_projectorrays_scrf_chunk', '_convert_projectorrays_info_entry_chunk', '_parse_projectorrays_info_entry_table', '_convert_projectorrays_sord_chunk', '_parse_projectorrays_sord_table', '_convert_projectorrays_fmap_chunk', '_parse_projectorrays_fmap']


def _write_projectorrays_metadata_asset(
    work_root: Path,
    alias: str,
    source: Path,
    source_relative_path: str,
    chunk_fourcc: str,
    method: str,
    metadata: dict,
) -> dict:
    role = PROJECTORRAYS_REQUIRED_CHUNK_ROLES.get(chunk_fourcc, "director_chunk")
    native_path = _projectorrays_native_metadata_path(alias, source_relative_path)
    native_file = work_root / native_path
    native_payload = {
        "schema": "tsuinosora.projectorrays_converted_chunk.v1",
        "source_alias": alias,
        "source_relative_path": source_relative_path,
        "source_sha256": _sha256(source),
        "chunk_fourcc": chunk_fourcc,
        "role": role,
        "conversion_method": method,
        **metadata,
    }
    _write_json(native_file, native_payload)
    return {
        "source_alias": alias,
        "source_relative_path": source_relative_path,
        "source_sha256": _sha256(source),
        "chunk_fourcc": chunk_fourcc,
        "role": role,
        "native_path": native_path,
        "converted_sha256": _sha256(native_file),
        "byte_size": native_file.stat().st_size,
        "conversion_method": method,
        "status": "converted",
    }


def _convert_projectorrays_empty_sound_placeholder_chunk(
    work_root: Path,
    alias: str,
    source: Path,
    source_relative_path: str,
    diagnostics: list[dict],
) -> dict | None:
    role = PROJECTORRAYS_REQUIRED_CHUNK_ROLES["snd "]
    if source.stat().st_size != 0:
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERT_SOUND_PAYLOAD_UNPROVEN",
                "source_alias": alias,
                "source_relative_path": source_relative_path,
                "chunk_fourcc": "snd ",
                "role": role,
                "message": "ProjectorRays snd chunk contains bytes and requires a proven audio decoder before conversion",
            }
        )
        return None
    return _write_projectorrays_metadata_asset(
        work_root,
        alias,
        source,
        source_relative_path,
        "snd ",
        "projectorrays_empty_sound_placeholder",
        {
            "empty_placeholder": True,
            "redaction": {
                "paths": "dump_relative_only",
                "payload": "omitted",
                "audio": "omitted",
                "commercial_text": "omitted",
            },
        },
    )


def _convert_projectorrays_cupt_chunk(
    work_root: Path,
    alias: str,
    source: Path,
    source_relative_path: str,
    diagnostics: list[dict],
) -> dict | None:
    role = PROJECTORRAYS_REQUIRED_CHUNK_ROLES["cupt"]
    payload = source.read_bytes()
    cue_point_count = int.from_bytes(payload[0:4], "big") if len(payload) >= 4 else None
    if len(payload) != 4 or cue_point_count != 0:
        diagnostic = {
            "code": "TSUI_PROJECTORRAYS_CONVERT_CUPT_UNPROVEN",
            "source_alias": alias,
            "source_relative_path": source_relative_path,
            "chunk_fourcc": "cupt",
            "role": role,
            "byte_size": len(payload),
            "message": "ProjectorRays cupt conversion currently only accepts a proven empty cue-point table",
        }
        if cue_point_count is not None:
            diagnostic["cue_point_count"] = cue_point_count
        diagnostics.append(diagnostic)
        return None
    return _write_projectorrays_metadata_asset(
        work_root,
        alias,
        source,
        source_relative_path,
        "cupt",
        "projectorrays_cue_point_table",
        {
            "cue_point_count": cue_point_count,
            "redaction": {
                "paths": "dump_relative_only",
                "payload": "omitted",
                "audio": "omitted",
                "names": "omitted",
                "commercial_text": "omitted",
            },
        },
    )


def _convert_projectorrays_scrf_chunk(
    work_root: Path,
    alias: str,
    source: Path,
    source_relative_path: str,
) -> dict:
    return _write_projectorrays_metadata_asset(
        work_root,
        alias,
        source,
        source_relative_path,
        "SCRF",
        "projectorrays_scrf_reference_skipped",
        {
            "reference_policy": "skipped_by_director_runtime",
            "source_byte_size": source.stat().st_size,
            "redaction": {
                "paths": "dump_relative_only",
                "payload": "omitted",
                "commercial_text": "omitted",
                "names": "omitted",
            },
        },
    )


def _convert_projectorrays_info_entry_chunk(
    work_root: Path,
    alias: str,
    source: Path,
    source_relative_path: str,
    chunk_fourcc: str,
    diagnostics: list[dict],
) -> dict | None:
    parsed = _parse_projectorrays_info_entry_table(source.read_bytes())
    role = PROJECTORRAYS_REQUIRED_CHUNK_ROLES[chunk_fourcc]
    if parsed is None:
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERT_INFO_TABLE_INVALID",
                "source_alias": alias,
                "source_relative_path": source_relative_path,
                "chunk_fourcc": chunk_fourcc,
                "role": role,
                "message": "ProjectorRays info-entry table did not match the supported Director layout",
            }
        )
        return None
    return _write_projectorrays_metadata_asset(
        work_root,
        alias,
        source,
        source_relative_path,
        chunk_fourcc,
        "projectorrays_info_entry_table",
        {
            "table_offset": parsed["table_offset"],
            "entry_count": parsed["entry_count"],
            "entry_lengths": parsed["entry_lengths"],
            "entry_hashes": parsed["entry_hashes"],
            "redaction": {
                "paths": "dump_relative_only",
                "payload": "omitted",
                "commercial_text": "omitted",
                "script_text": "omitted",
                "names": "omitted",
            },
        },
    )


def _parse_projectorrays_info_entry_table(payload: bytes) -> dict | None:
    if len(payload) < 6:
        return None
    table_offset = int.from_bytes(payload[0:4], "big")
    if table_offset < 4 or table_offset + 2 > len(payload):
        return None
    entry_count = int.from_bytes(payload[table_offset : table_offset + 2], "big")
    offsets_start = table_offset + 2
    offsets_end = offsets_start + (entry_count + 1) * 4
    if offsets_end > len(payload):
        return None
    offsets = [
        int.from_bytes(payload[offsets_start + index * 4 : offsets_start + (index + 1) * 4], "big")
        for index in range(entry_count + 1)
    ]
    if offsets[0] != 0 or any(right < left for left, right in zip(offsets, offsets[1:])):
        return None
    data = payload[offsets_end:]
    if offsets[-1] != len(data):
        return None
    entry_lengths = []
    entry_hashes = []
    for left, right in zip(offsets, offsets[1:]):
        entry = data[left:right]
        entry_lengths.append(len(entry))
        entry_hashes.append(_sha256_bytes(entry))
    return {
        "table_offset": table_offset,
        "entry_count": entry_count,
        "entry_lengths": entry_lengths,
        "entry_hashes": entry_hashes,
    }


def _convert_projectorrays_sord_chunk(
    work_root: Path,
    alias: str,
    source: Path,
    source_relative_path: str,
    diagnostics: list[dict],
) -> dict | None:
    parsed = _parse_projectorrays_sord_table(source.read_bytes())
    role = PROJECTORRAYS_REQUIRED_CHUNK_ROLES["Sord"]
    if parsed is None:
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERT_SORD_INVALID",
                "source_alias": alias,
                "source_relative_path": source_relative_path,
                "chunk_fourcc": "Sord",
                "role": role,
                "message": "ProjectorRays Sord score-order table did not match the supported Director layout",
            }
        )
        return None
    return _write_projectorrays_metadata_asset(
        work_root,
        alias,
        source,
        source_relative_path,
        "Sord",
        "projectorrays_score_order_table",
        {
            "entry_count": parsed["entry_count"],
            "entry_size": parsed["entry_size"],
            "referenced_members": parsed["referenced_members"],
            "redaction": {
                "paths": "dump_relative_only",
                "payload": "omitted",
                "commercial_text": "omitted",
                "names": "omitted",
            },
        },
    )


def _parse_projectorrays_sord_table(payload: bytes) -> dict | None:
    if len(payload) < 20:
        return None
    entry_count_a = int.from_bytes(payload[8:12], "big")
    entry_count_b = int.from_bytes(payload[12:16], "big")
    header_size = int.from_bytes(payload[16:18], "big")
    entry_size = int.from_bytes(payload[18:20], "big")
    if header_size != 20 or entry_count_a != entry_count_b or entry_size not in {2, 4}:
        return None
    if len(payload) != header_size + entry_count_a * entry_size:
        return None
    referenced_members = []
    offset = header_size
    for _ in range(entry_count_a):
        if entry_size == 4:
            cast_library_id = int.from_bytes(payload[offset : offset + 2], "big")
            member_id = int.from_bytes(payload[offset + 2 : offset + 4], "big")
            offset += 4
        else:
            cast_library_id = "default"
            member_id = int.from_bytes(payload[offset : offset + 2], "big")
            offset += 2
        referenced_members.append({"cast_library_id": cast_library_id, "member_id": member_id})
    return {
        "entry_count": entry_count_a,
        "entry_size": entry_size,
        "referenced_members": referenced_members,
    }


def _convert_projectorrays_fmap_chunk(
    work_root: Path,
    alias: str,
    source: Path,
    source_relative_path: str,
    diagnostics: list[dict],
) -> dict | None:
    parsed = _parse_projectorrays_fmap(source.read_bytes())
    role = PROJECTORRAYS_REQUIRED_CHUNK_ROLES["Fmap"]
    if parsed is None:
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERT_FMAP_INVALID",
                "source_alias": alias,
                "source_relative_path": source_relative_path,
                "chunk_fourcc": "Fmap",
                "role": role,
                "message": "ProjectorRays Fmap chunk did not match the supported Director font-map layout",
            }
        )
        return None
    return _write_projectorrays_metadata_asset(
        work_root,
        alias,
        source,
        source_relative_path,
        "Fmap",
        "projectorrays_font_map_v4",
        {
            "font_entry_count": parsed["font_entry_count"],
            "font_entries": parsed["font_entries"],
            "redaction": {
                "paths": "dump_relative_only",
                "payload": "omitted",
                "commercial_text": "omitted",
                "font_names": "omitted",
            },
        },
    )


def _parse_projectorrays_fmap(payload: bytes) -> dict | None:
    if len(payload) < 36:
        return None
    map_length = int.from_bytes(payload[0:4], "big")
    names_length = int.from_bytes(payload[4:8], "big")
    body_start = 8
    names_start = body_start + map_length
    if map_length < 28 or names_length < 0 or names_start + names_length != len(payload):
        return None
    entries_used = int.from_bytes(payload[16:20], "big")
    entries_total = int.from_bytes(payload[20:24], "big")
    entries_start = 36
    entries_end = entries_start + entries_used * 8
    if entries_used > entries_total or entries_end > names_start:
        return None
    font_entries = []
    for index in range(entries_used):
        offset = entries_start + index * 8
        name_offset = int.from_bytes(payload[offset : offset + 4], "big")
        platform_id = int.from_bytes(payload[offset + 4 : offset + 6], "big")
        font_id = int.from_bytes(payload[offset + 6 : offset + 8], "big")
        name_header = names_start + name_offset
        if name_header + 4 > len(payload):
            return None
        name_length = int.from_bytes(payload[name_header : name_header + 4], "big")
        name_start = name_header + 4
        name_end = name_start + name_length
        if name_end > names_start + names_length:
            return None
        font_entries.append(
            {
                "platform_id": platform_id,
                "font_id": font_id,
                "name_length": name_length,
                "name_hash": _sha256_bytes(payload[name_start:name_end]),
            }
        )
    return {"font_entry_count": entries_used, "font_entries": font_entries}
