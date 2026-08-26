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
from tsuinosora_projectorrays_convert_metadata import _write_projectorrays_metadata_asset

__all__ = ['_convert_projectorrays_sord_chunk', '_parse_projectorrays_sord_table', '_convert_projectorrays_fmap_chunk', '_parse_projectorrays_fmap', '_convert_projectorrays_vwlb_chunk', '_parse_projectorrays_vwlb', '_convert_projectorrays_fcol_chunk', '_convert_projectorrays_fxmp_chunk', '_convert_projectorrays_vers_chunk', '_parse_projectorrays_vers']


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


def _convert_projectorrays_vwlb_chunk(
    work_root: Path,
    alias: str,
    source: Path,
    source_relative_path: str,
    diagnostics: list[dict],
) -> dict | None:
    parsed = _parse_projectorrays_vwlb(source.read_bytes())
    role = PROJECTORRAYS_REQUIRED_CHUNK_ROLES["VWLB"]
    if parsed is None:
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERT_VWLB_INVALID",
                "source_alias": alias,
                "source_relative_path": source_relative_path,
                "chunk_fourcc": "VWLB",
                "role": role,
                "message": "ProjectorRays VWLB chunk did not match the supported Director label-table layout",
            }
        )
        return None
    return _write_projectorrays_metadata_asset(
        work_root,
        alias,
        source,
        source_relative_path,
        "VWLB",
        "projectorrays_score_label_table",
        {
            "label_count": parsed["label_count"],
            "labels": parsed["labels"],
            "redaction": {
                "paths": "dump_relative_only",
                "payload": "omitted",
                "commercial_text": "omitted",
                "label_text": "omitted",
                "comments": "omitted",
            },
        },
    )


def _parse_projectorrays_vwlb(payload: bytes) -> dict | None:
    if len(payload) < 6:
        return None
    table_count = int.from_bytes(payload[0:2], "big") + 1
    table_end = table_count * 4 + 2
    if table_count < 1 or table_end > len(payload):
        return None
    pairs = []
    offset = 2
    for _ in range(table_count):
        frame = int.from_bytes(payload[offset : offset + 2], "big")
        label_offset = int.from_bytes(payload[offset + 2 : offset + 4], "big") + table_end
        if label_offset > len(payload):
            return None
        pairs.append((frame, label_offset))
        offset += 4
    labels = []
    for (frame, start), (_, end) in zip(pairs, pairs[1:]):
        if end < start:
            return None
        segment = payload[start:end]
        labels.append({"frame": frame, "byte_size": len(segment), "label_hash": _sha256_bytes(segment)})
    return {"label_count": len(labels), "labels": labels}


def _convert_projectorrays_fcol_chunk(
    work_root: Path,
    alias: str,
    source: Path,
    source_relative_path: str,
    diagnostics: list[dict],
) -> dict | None:
    payload = source.read_bytes()
    role = PROJECTORRAYS_REQUIRED_CHUNK_ROLES["FCOL"]
    if len(payload) == 0 or len(payload) % 2 != 0:
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERT_FCOL_INVALID",
                "source_alias": alias,
                "source_relative_path": source_relative_path,
                "chunk_fourcc": "FCOL",
                "role": role,
                "message": "ProjectorRays FCOL color table must contain an even number of 16-bit words",
            }
        )
        return None
    return _write_projectorrays_metadata_asset(
        work_root,
        alias,
        source,
        source_relative_path,
        "FCOL",
        "projectorrays_fixed_color_table",
        {
            "word_count": len(payload) // 2,
            "table_hash": _sha256_bytes(payload),
            "redaction": {
                "paths": "dump_relative_only",
                "payload": "omitted",
                "colors": "omitted",
                "commercial_text": "omitted",
            },
        },
    )


def _convert_projectorrays_fxmp_chunk(
    work_root: Path,
    alias: str,
    source: Path,
    source_relative_path: str,
    diagnostics: list[dict],
) -> dict | None:
    payload = source.read_bytes()
    role = PROJECTORRAYS_REQUIRED_CHUNK_ROLES["FXmp"]
    try:
        decoded = payload.decode("cp932")
    except UnicodeDecodeError:
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERT_FXMP_INVALID",
                "source_alias": alias,
                "source_relative_path": source_relative_path,
                "chunk_fourcc": "FXmp",
                "role": role,
                "message": "ProjectorRays FXmp text map must be decodable before redacted metadata conversion",
            }
        )
        return None
    normalized = decoded.replace("\r\n", "\n").replace("\r", "\n")
    line_count = len([line for line in normalized.split("\n") if line])
    return _write_projectorrays_metadata_asset(
        work_root,
        alias,
        source,
        source_relative_path,
        "FXmp",
        "projectorrays_fxmp_text_map_metadata",
        {
            "line_count": line_count,
            "map_hash": _sha256_bytes(payload),
            "redaction": {
                "paths": "dump_relative_only",
                "payload": "omitted",
                "commercial_text": "omitted",
                "font_names": "omitted",
                "font_map_lines": "omitted",
            },
        },
    )


def _convert_projectorrays_vers_chunk(
    work_root: Path,
    alias: str,
    source: Path,
    source_relative_path: str,
    diagnostics: list[dict],
) -> dict | None:
    parsed = _parse_projectorrays_vers(source.read_bytes())
    role = PROJECTORRAYS_REQUIRED_CHUNK_ROLES["VERS"]
    if parsed is None:
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERT_VERS_INVALID",
                "source_alias": alias,
                "source_relative_path": source_relative_path,
                "chunk_fourcc": "VERS",
                "role": role,
                "message": "ProjectorRays VERS chunk did not match the supported fixed-width version table layout",
            }
        )
        return None
    return _write_projectorrays_metadata_asset(
        work_root,
        alias,
        source,
        source_relative_path,
        "VERS",
        "projectorrays_version_table",
        {
            "table_version": parsed["table_version"],
            "entry_count": parsed["entry_count"],
            "entries": parsed["entries"],
            "redaction": {
                "paths": "dump_relative_only",
                "payload": "omitted",
                "commercial_text": "omitted",
            },
        },
    )


def _parse_projectorrays_vers(payload: bytes) -> dict | None:
    if len(payload) < 4:
        return None
    table_version = int.from_bytes(payload[0:2], "big")
    entry_count = int.from_bytes(payload[2:4], "big")
    if len(payload) != 4 + entry_count * 8:
        return None
    entries = []
    offset = 4
    for _ in range(entry_count):
        entries.append(
            {
                "director_version": int.from_bytes(payload[offset : offset + 2], "big"),
                "minor": int.from_bytes(payload[offset + 2 : offset + 4], "big"),
                "major": int.from_bytes(payload[offset + 4 : offset + 6], "big"),
                "build": int.from_bytes(payload[offset + 6 : offset + 8], "big"),
            }
        )
        offset += 8
    return {"table_version": table_version, "entry_count": entry_count, "entries": entries}
