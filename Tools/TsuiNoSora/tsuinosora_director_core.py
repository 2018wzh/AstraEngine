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
from tsuinosora_constants import _fourcc, _slice_embedded_payload, _slice_metadata_json_payload, _slice_script_text_payload, _decode_script_text
from tsuinosora_diagnostics import _is_safe_symbol, _is_safe_report_relative_path, _is_sanitized_sha256, _rel, _write_json, _read_json, _format_probe, _format_counts, _edition_fingerprint, _blocked_extract_report, _blocked_asset_analysis, _dedupe_diagnostics
from tsuinosora_rendering import _safe_identifier, _read_json, _write_json, _report_has_path_leak
__all__ = ['_parse_director_key_table', '_empty_director_key_table', '_parse_director_cas_table', '_director_cast_member_skeleton', '_apply_director_cast_member_metadata', '_parse_director_cast_member_metadata', '_decode_json_payload', '_safe_symbol_list', '_safe_int_point', '_safe_int_bounds', '_safe_atlas_parts', '_parse_director_cast_member_metadata', '_read_director_resource_map', '_read_director_cast_map', '_read_director_lingo_map', '_parse_lingo_context_table', '_parse_lingo_name_table', '_director_resource_payloads_by_id', '_director_resource_data', '_append_director_child_resource', '_blocked_director_resource_map', '_safe_atlas_parts', '_append_director_child_resource', '_blocked_director_resource_map', '_director_resource_map_summary', '_mapped_director_resource_chunks', '_linear_riff_chunks', '_extract_payload_from_container_chunk', '_cast_member_from_map', 'extract_readable_assets', '_extract_readable_container', 'build_director_resource_map_report', 'build_director_cast_map_report', 'build_director_lingo_map_report', '_director_cast_map_report_for_container', '_director_lingo_map_report_for_container', '_director_lingo_source_map_from_extracted_scripts', '_decode_xfir_riff_payload', 'build_director_resource_map_report', 'build_director_cast_map_report', 'build_director_lingo_map_report', '_director_cast_map_report_for_container', '_director_lingo_map_report_for_container', '_director_lingo_source_map_from_extracted_scripts']
from tsuinosora_script_source_routes import _forbidden_payload_key_diagnostics

def _parse_director_key_table(payload: bytes, endian: str, relative_path: str, resource_id: int) -> dict:
    diagnostics = []
    relationships = []
    if len(payload) < 12:
        diagnostics.append(
            {
                "code": "TSUI_DIRECTOR_CAST_KEY_TABLE_TRUNCATED",
                "relative_path": relative_path,
                "resource_id": resource_id,
                "message": "KEY* payload is too small for the table header",
            }
        )
        return {
            "table": _empty_director_key_table(resource_id),
            "relationships": [],
            "diagnostics": diagnostics,
        }
    entry_size, entry_size_2 = struct.unpack(endian + "HH", payload[:4])
    entry_count, used_count = struct.unpack(endian + "II", payload[4:12])
    table = {
        "key_resource_id": resource_id,
        "entry_size": entry_size,
        "entry_size_2": entry_size_2,
        "entry_count": entry_count,
        "used_count": used_count,
        "child_tag_counts": {},
    }
    if entry_size != 12 or entry_size_2 != 12:
        diagnostics.append(
            {
                "code": "TSUI_DIRECTOR_CAST_KEY_ENTRY_SIZE_INVALID",
                "relative_path": relative_path,
                "resource_id": resource_id,
                "entry_size": entry_size,
                "entry_size_2": entry_size_2,
                "message": "KEY* entries must be 12 bytes: child index, parent index and child tag",
            }
        )
        return {
            "table": table,
            "relationships": [],
            "diagnostics": diagnostics,
        }
    if used_count > entry_count:
        diagnostics.append(
            {
                "code": "TSUI_DIRECTOR_CAST_KEY_USED_COUNT_INVALID",
                "relative_path": relative_path,
                "resource_id": resource_id,
                "message": "KEY* used entry count exceeds declared entry count",
            }
        )
    expected_size = 12 + used_count * entry_size
    if expected_size > len(payload):
        diagnostics.append(
            {
                "code": "TSUI_DIRECTOR_CAST_KEY_TABLE_TRUNCATED",
                "relative_path": relative_path,
                "resource_id": resource_id,
                "message": "KEY* used entries extend beyond the payload",
            }
        )
        used_count = max((len(payload) - 12) // entry_size, 0)

    for index in range(used_count):
        offset = 12 + index * entry_size
        child_index, parent_index = struct.unpack(endian + "II", payload[offset : offset + 8])
        child_tag = _fourcc(payload[offset + 8 : offset + 12])
        relationships.append(
            {
                "key_resource_id": resource_id,
                "entry_index": index,
                "child_resource_id": child_index,
                "parent_resource_id": parent_index,
                "child_tag": child_tag,
            }
        )
        table["child_tag_counts"][child_tag] = table["child_tag_counts"].get(child_tag, 0) + 1
    table["child_tag_counts"] = dict(sorted(table["child_tag_counts"].items()))
    return {
        "table": table,
        "relationships": relationships,
        "diagnostics": diagnostics,
    }

def _empty_director_key_table(resource_id: int) -> dict:
    return {
        "key_resource_id": resource_id,
        "entry_size": 0,
        "entry_size_2": 0,
        "entry_count": 0,
        "used_count": 0,
        "child_tag_counts": {},
    }

def _parse_director_cas_table(payload: bytes, relative_path: str, resource_id: int) -> dict:
    diagnostics = []
    if len(payload) % 4:
        diagnostics.append(
            {
                "code": "TSUI_DIRECTOR_CAST_CAS_TABLE_UNALIGNED",
                "relative_path": relative_path,
                "resource_id": resource_id,
                "message": "CAS* payload size is not aligned to 32-bit cast resource ids",
            }
        )
    readable = len(payload) - (len(payload) % 4)
    cast_resource_ids = [
        struct.unpack(">I", payload[offset : offset + 4])[0]
        for offset in range(0, readable, 4)
    ]
    return {
        "cast_resource_ids": cast_resource_ids,
        "diagnostics": diagnostics,
    }

def _director_cast_member_skeleton(
    relative_path: str,
    resource_map: dict,
    cast_resource_id: int,
    cast_slot: int,
    library_resource_id: int,
    cast_resource: dict,
) -> dict:
    container_id = _safe_identifier(Path(relative_path).with_suffix("").as_posix())
    return {
        "member_id": f"{container_id}.cast.{library_resource_id}.{cast_slot if cast_slot >= 0 else cast_resource_id}",
        "source_container": relative_path,
        "cast_resource_id": cast_resource_id,
        "cast_slot": cast_slot,
        "library_resource_id": library_resource_id,
        "cast_payload_sha256": cast_resource.get("payload_sha256", ""),
        "director_version": resource_map.get("director_version", 0),
        "child_resources": [],
        "coverage_status": "mapped",
    }

def _apply_director_cast_member_metadata(
    member: dict,
    payload: bytes,
    diagnostics: list[dict],
    relative_path: str,
) -> None:
    metadata, metadata_diagnostics = _parse_director_cast_member_metadata(
        payload,
        relative_path,
        int(member["cast_resource_id"]),
    )
    diagnostics.extend(metadata_diagnostics)
    member.update(metadata)

def _parse_director_cast_member_metadata(
    payload: bytes,
    relative_path: str,
    cast_resource_id: int,
) -> tuple[dict, list[dict]]:
    decoded = _decode_json_payload(payload)
    if decoded is None:
        return {}, []
    value, normalized = decoded
    if value.get("schema") != DIRECTOR_CAST_MEMBER_METADATA_SCHEMA:
        return {}, []

    diagnostics = _forbidden_payload_key_diagnostics(
        value,
        relative_path,
        code="TSUI_DIRECTOR_CAST_METADATA_PAYLOAD_FIELD",
        source_field="relative_path",
        message="Director cast member metadata must not contain commercial text, bytecode or payload fields",
    )
    metadata = {
        "cast_metadata_schema": DIRECTOR_CAST_MEMBER_METADATA_SCHEMA,
        "cast_metadata_sha256": _sha256_bytes(normalized.encode("utf-8")),
    }
    kind = str(value.get("kind", "")).strip()
    if kind:
        if kind in CAST_MEMBER_KINDS:
            metadata["kind"] = kind
        else:
            diagnostics.append(
                {
                    "code": "TSUI_DIRECTOR_CAST_METADATA_KIND_INVALID",
                    "relative_path": relative_path,
                    "cast_resource_id": cast_resource_id,
                    "message": "Director cast member metadata kind is not in the allowed classification set",
                }
            )

    route_ids = _safe_symbol_list(value.get("route_ids", []))
    if route_ids is None:
        diagnostics.append(
            {
                "code": "TSUI_DIRECTOR_CAST_METADATA_ROUTE_ID_INVALID",
                "relative_path": relative_path,
                "cast_resource_id": cast_resource_id,
                "message": "Director cast member metadata route_ids must be safe symbols",
            }
        )
    elif route_ids:
        metadata["route_ids"] = route_ids

    command_ids = _safe_symbol_list(value.get("command_ids", []))
    if command_ids is None:
        diagnostics.append(
            {
                "code": "TSUI_DIRECTOR_CAST_METADATA_COMMAND_ID_INVALID",
                "relative_path": relative_path,
                "cast_resource_id": cast_resource_id,
                "message": "Director cast member metadata command_ids must be safe symbols",
            }
        )
    elif command_ids:
        metadata["command_ids"] = command_ids

    if "anchor" in value:
        anchor = _safe_int_point(value.get("anchor"))
        if anchor is None:
            diagnostics.append(
                {
                    "code": "TSUI_DIRECTOR_CAST_METADATA_ANCHOR_INVALID",
                    "relative_path": relative_path,
                    "cast_resource_id": cast_resource_id,
                    "message": "Director cast member metadata anchor must contain numeric x and y values",
                }
            )
        else:
            metadata["anchor"] = anchor
    if "bounds" in value:
        bounds = _safe_int_bounds(value.get("bounds"))
        if bounds is None:
            diagnostics.append(
                {
                    "code": "TSUI_DIRECTOR_CAST_METADATA_BOUNDS_INVALID",
                    "relative_path": relative_path,
                    "cast_resource_id": cast_resource_id,
                    "message": "Director cast member metadata bounds must contain non-negative numeric x, y, width and height values",
                }
            )
        else:
            metadata["bounds"] = bounds
    if "parts" in value:
        parts, part_diagnostics = _safe_atlas_parts(
            value.get("parts"),
            source=relative_path,
            owner_id=cast_resource_id,
            source_field="relative_path",
            code_prefix="TSUI_DIRECTOR_CAST_METADATA",
        )
        diagnostics.extend(part_diagnostics)
        if parts:
            metadata["parts"] = parts
    elif kind == "character_atlas":
        diagnostics.append(
            {
                "code": "TSUI_DIRECTOR_CAST_METADATA_ATLAS_PARTS_MISSING",
                "relative_path": relative_path,
                "cast_resource_id": cast_resource_id,
                "message": "character_atlas metadata must include crop/part records",
            }
        )
    return metadata, diagnostics

def _decode_json_payload(payload: bytes) -> tuple[dict, str] | None:
    stripped = payload.strip(b"\x00\r\n\t ")
    if not stripped or b"{" not in stripped:
        return None
    offset = stripped.find(b"{")
    decoded = _decode_script_text(stripped[offset:])
    if decoded is None:
        return None
    text, _encoding = decoded
    try:
        value = json.loads(text)
    except json.JSONDecodeError:
        return None
    if not isinstance(value, dict):
        return None
    normalized = json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    return value, normalized

def _safe_symbol_list(value) -> list[str] | None:
    if value in (None, ""):
        return []
    if not isinstance(value, list):
        return None
    symbols = []
    for item in value:
        symbol = str(item).strip()
        if not symbol or not _is_safe_symbol(symbol):
            return None
        symbols.append(symbol)
    return symbols

def _safe_int_point(value) -> dict | None:
    if not isinstance(value, dict):
        return None
    if not all(key in value for key in ("x", "y")):
        return None
    try:
        return {"x": int(value["x"]), "y": int(value["y"])}
    except (TypeError, ValueError):
        return None

def _safe_int_bounds(value) -> dict | None:
    if not isinstance(value, dict):
        return None
    if not all(key in value for key in ("x", "y", "width", "height")):
        return None
    try:
        bounds = {
            "x": int(value["x"]),
            "y": int(value["y"]),
            "width": int(value["width"]),
            "height": int(value["height"]),
        }
    except (TypeError, ValueError):
        return None
    if bounds["width"] < 0 or bounds["height"] < 0:
        return None
    return bounds

def _safe_atlas_parts(
    value,
    *,
    source: str,
    owner_id,
    source_field: str,
    code_prefix: str,
) -> tuple[list[dict], list[dict]]:
    diagnostics = []
    if not isinstance(value, list) or not value:
        diagnostics.append(
            {
                "code": f"{code_prefix}_ATLAS_PARTS_MISSING",
                source_field: source,
                "owner_id": owner_id,
                "message": "character_atlas metadata must include crop/part records",
            }
        )
        return [], diagnostics

    parts = []
    for index, raw_part in enumerate(value):
        if not isinstance(raw_part, dict):
            diagnostics.append(
                {
                    "code": f"{code_prefix}_ATLAS_PART_INVALID",
                    source_field: source,
                    "owner_id": owner_id,
                    "part_index": index,
                    "message": "atlas part must be an object",
                }
            )
            continue
        part_id = str(raw_part.get("part_id", "")).strip()
        pose_id = str(raw_part.get("pose_id", "")).strip()
        expression_id = str(raw_part.get("expression_id", "")).strip()
        layer = str(raw_part.get("layer", "character")).strip()
        fallback = str(raw_part.get("fallback", "nearest_pose")).strip()
        crop = _safe_int_bounds(raw_part.get("crop"))
        anchor = _safe_int_point(raw_part.get("anchor"))
        mouth_eye_state_compatible = raw_part.get("mouth_eye_state_compatible", True)
        if not part_id or not _is_safe_symbol(part_id):
            diagnostics.append(
                {
                    "code": f"{code_prefix}_ATLAS_PART_ID_INVALID",
                    source_field: source,
                    "owner_id": owner_id,
                    "part_index": index,
                    "message": "atlas part_id must be a safe symbol",
                }
            )
        if not pose_id or not _is_safe_symbol(pose_id):
            diagnostics.append(
                {
                    "code": f"{code_prefix}_ATLAS_POSE_ID_INVALID",
                    source_field: source,
                    "owner_id": owner_id,
                    "part_index": index,
                    "message": "atlas pose_id must be a safe symbol",
                }
            )
        if not expression_id or not _is_safe_symbol(expression_id):
            diagnostics.append(
                {
                    "code": f"{code_prefix}_ATLAS_EXPRESSION_ID_INVALID",
                    source_field: source,
                    "owner_id": owner_id,
                    "part_index": index,
                    "message": "atlas expression_id must be a safe symbol",
                }
            )
        if not layer or not _is_safe_symbol(layer):
            diagnostics.append(
                {
                    "code": f"{code_prefix}_ATLAS_LAYER_INVALID",
                    source_field: source,
                    "owner_id": owner_id,
                    "part_index": index,
                    "message": "atlas layer must be a safe symbol",
                }
            )
        if not fallback or not _is_safe_symbol(fallback):
            diagnostics.append(
                {
                    "code": f"{code_prefix}_ATLAS_FALLBACK_INVALID",
                    source_field: source,
                    "owner_id": owner_id,
                    "part_index": index,
                    "message": "atlas fallback must be a safe symbol",
                }
            )
        if crop is None:
            diagnostics.append(
                {
                    "code": f"{code_prefix}_ATLAS_CROP_INVALID",
                    source_field: source,
                    "owner_id": owner_id,
                    "part_index": index,
                    "message": "atlas crop must contain non-negative numeric x, y, width and height values",
                }
            )
        if anchor is None:
            diagnostics.append(
                {
                    "code": f"{code_prefix}_ATLAS_ANCHOR_INVALID",
                    source_field: source,
                    "owner_id": owner_id,
                    "part_index": index,
                    "message": "atlas anchor must contain numeric x and y values",
                }
            )
        if not isinstance(mouth_eye_state_compatible, bool):
            diagnostics.append(
                {
                    "code": f"{code_prefix}_ATLAS_STATE_COMPAT_INVALID",
                    source_field: source,
                    "owner_id": owner_id,
                    "part_index": index,
                    "message": "atlas mouth_eye_state_compatible must be boolean",
                }
            )
        if diagnostics and any(diagnostic.get("part_index") == index for diagnostic in diagnostics):
            continue
        parts.append(
            {
                "part_id": part_id,
                "pose_id": pose_id,
                "expression_id": expression_id,
                "anchor": anchor,
                "crop": crop,
                "layer": layer,
                "mouth_eye_state_compatible": mouth_eye_state_compatible,
                "fallback": fallback,
            }
        )
    return parts, diagnostics

def _parse_director_cast_member_metadata(
    payload: bytes,
    relative_path: str,
    cast_resource_id: int,
) -> tuple[dict, list[dict]]:
    decoded = _decode_json_payload(payload)
    if decoded is None:
        return {}, []
    value, normalized = decoded
    if value.get("schema") != DIRECTOR_CAST_MEMBER_METADATA_SCHEMA:
        return {}, []

    diagnostics = _forbidden_payload_key_diagnostics(
        value,
        relative_path,
        code="TSUI_DIRECTOR_CAST_METADATA_PAYLOAD_FIELD",
        source_field="relative_path",
        message="Director cast member metadata must not contain commercial text, bytecode or payload fields",
    )
    metadata = {
        "cast_metadata_schema": DIRECTOR_CAST_MEMBER_METADATA_SCHEMA,
        "cast_metadata_sha256": _sha256_bytes(normalized.encode("utf-8")),
    }
    kind = str(value.get("kind", "")).strip()
    if kind:
        if kind in CAST_MEMBER_KINDS:
            metadata["kind"] = kind
        else:
            diagnostics.append(
                {
                    "code": "TSUI_DIRECTOR_CAST_METADATA_KIND_INVALID",
                    "relative_path": relative_path,
                    "cast_resource_id": cast_resource_id,
                    "message": "Director cast member metadata kind is not in the allowed classification set",
                }
            )

    route_ids = _safe_symbol_list(value.get("route_ids", []))
    if route_ids is None:
        diagnostics.append(
            {
                "code": "TSUI_DIRECTOR_CAST_METADATA_ROUTE_ID_INVALID",
                "relative_path": relative_path,
                "cast_resource_id": cast_resource_id,
                "message": "Director cast member metadata route_ids must be safe symbols",
            }
        )
    elif route_ids:
        metadata["route_ids"] = route_ids

    command_ids = _safe_symbol_list(value.get("command_ids", []))
    if command_ids is None:
        diagnostics.append(
            {
                "code": "TSUI_DIRECTOR_CAST_METADATA_COMMAND_ID_INVALID",
                "relative_path": relative_path,
                "cast_resource_id": cast_resource_id,
                "message": "Director cast member metadata command_ids must be safe symbols",
            }
        )
    elif command_ids:
        metadata["command_ids"] = command_ids

    if "anchor" in value:
        anchor = _safe_int_point(value.get("anchor"))
        if anchor is None:
            diagnostics.append(
                {
                    "code": "TSUI_DIRECTOR_CAST_METADATA_ANCHOR_INVALID",
                    "relative_path": relative_path,
                    "cast_resource_id": cast_resource_id,
                    "message": "Director cast member metadata anchor must contain numeric x and y values",
                }
            )
        else:
            metadata["anchor"] = anchor
    if "bounds" in value:
        bounds = _safe_int_bounds(value.get("bounds"))
        if bounds is None:
            diagnostics.append(
                {
                    "code": "TSUI_DIRECTOR_CAST_METADATA_BOUNDS_INVALID",
                    "relative_path": relative_path,
                    "cast_resource_id": cast_resource_id,
                    "message": "Director cast member metadata bounds must contain non-negative numeric x, y, width and height values",
                }
            )
        else:
            metadata["bounds"] = bounds
    if "parts" in value:
        parts, part_diagnostics = _safe_atlas_parts(
            value.get("parts"),
            source=relative_path,
            owner_id=cast_resource_id,
            source_field="relative_path",
            code_prefix="TSUI_DIRECTOR_CAST_METADATA",
        )
        diagnostics.extend(part_diagnostics)
        if parts:
            metadata["parts"] = parts
    elif kind == "character_atlas":
        diagnostics.append(
            {
                "code": "TSUI_DIRECTOR_CAST_METADATA_ATLAS_PARTS_MISSING",
                "relative_path": relative_path,
                "cast_resource_id": cast_resource_id,
                "message": "character_atlas metadata must include crop/part records",
            }
        )
    return metadata, diagnostics

def _read_director_resource_map(path: Path, relative_path: str) -> dict:
    diagnostics = []
    raw_data = path.read_bytes()
    original_file_size = len(raw_data)
    decoded = _decode_xfir_riff_payload(raw_data)
    decoded_from_xfir = False
    if original_file_size >= 4 and raw_data[:4] == b"XFIR":
        if not decoded:
            return _blocked_director_resource_map(
                relative_path,
                "TSUI_DIRECTOR_RESOURCE_MAP_XFIR_READER_REQUIRED",
                "Shockwave XFIR containers require a dedicated verified resource-map reader",
                imap_found=False,
                signature="XFIR",
                file_size=original_file_size,
            )
        data = decoded["data"]
        decoded_from_xfir = True
    else:
        data = raw_data
    file_size = len(data)
    if file_size >= 4 and data[:4] == b"XFIR":
        return _blocked_director_resource_map(
            relative_path,
            "TSUI_DIRECTOR_RESOURCE_MAP_XFIR_READER_REQUIRED",
            "Shockwave XFIR containers require a dedicated verified resource-map reader",
            imap_found=False,
            signature="XFIR",
            file_size=file_size,
        )
    if file_size < 12 or data[:4] not in READABLE_RIFF_SIGNATURES:
        return _blocked_director_resource_map(
            relative_path,
            "TSUI_DIRECTOR_RESOURCE_MAP_UNRECOGNIZED",
            "container is not RIFF/RIFX Director data",
            imap_found=False,
        )

    signature = data[:4]
    endian = ">" if signature == b"RIFX" else "<"
    declared_size = struct.unpack(endian + "I", data[4:8])[0]
    form_type = _fourcc(data[8:12])
    if declared_size + 8 != file_size:
        return _blocked_director_resource_map(
            relative_path,
            "TSUI_DIRECTOR_RESOURCE_MAP_SIZE_MISMATCH",
            "container declared size does not match readable file size",
            imap_found=file_size >= 16 and data[12:16] == b"imap",
            signature=_fourcc(signature),
            form_type=form_type,
            declared_size=declared_size,
            file_size=file_size,
        )

    if file_size < 32 or data[12:16] != b"imap":
        return _blocked_director_resource_map(
            relative_path,
            "TSUI_DIRECTOR_RESOURCE_MAP_IMAP_MISSING",
            "Director initial map chunk was not found at the fixed imap offset",
            imap_found=False,
            signature=_fourcc(signature),
            form_type=form_type,
            declared_size=declared_size,
            file_size=file_size,
            diagnostics=diagnostics,
        )

    imap_size = struct.unpack(endian + "I", data[16:20])[0]
    if 20 + imap_size > file_size or imap_size < 12:
        return _blocked_director_resource_map(
            relative_path,
            "TSUI_DIRECTOR_RESOURCE_MAP_IMAP_TRUNCATED",
            "Director imap chunk is truncated or too small",
            imap_found=True,
            signature=_fourcc(signature),
            form_type=form_type,
            declared_size=declared_size,
            file_size=file_size,
            diagnostics=diagnostics,
        )

    map_version = struct.unpack(endian + "I", data[20:24])[0]
    mmap_offset = struct.unpack(endian + "I", data[24:28])[0]
    director_version = struct.unpack(endian + "I", data[28:32])[0]
    if mmap_offset < 12 or mmap_offset + 32 > file_size:
        return _blocked_director_resource_map(
            relative_path,
            "TSUI_DIRECTOR_RESOURCE_MAP_MMAP_OFFSET_INVALID",
            "Director mmap offset is outside the readable container",
            imap_found=True,
            signature=_fourcc(signature),
            form_type=form_type,
            declared_size=declared_size,
            file_size=file_size,
            map_version=map_version,
            director_version=director_version,
            mmap_offset=mmap_offset,
            diagnostics=diagnostics,
        )
    if data[mmap_offset : mmap_offset + 4] != b"mmap":
        return _blocked_director_resource_map(
            relative_path,
            "TSUI_DIRECTOR_RESOURCE_MAP_MMAP_MISSING",
            "Director mmap chunk was not found at the imap-provided offset",
            imap_found=True,
            signature=_fourcc(signature),
            form_type=form_type,
            declared_size=declared_size,
            file_size=file_size,
            map_version=map_version,
            director_version=director_version,
            mmap_offset=mmap_offset,
            diagnostics=diagnostics,
        )

    mmap_size = struct.unpack(endian + "I", data[mmap_offset + 4 : mmap_offset + 8])[0]
    mmap_payload = mmap_offset + 8
    mmap_end = mmap_payload + mmap_size
    if mmap_end > file_size or mmap_size < 24:
        return _blocked_director_resource_map(
            relative_path,
            "TSUI_DIRECTOR_RESOURCE_MAP_MMAP_TRUNCATED",
            "Director mmap chunk is truncated or too small",
            imap_found=True,
            signature=_fourcc(signature),
            form_type=form_type,
            declared_size=declared_size,
            file_size=file_size,
            map_version=map_version,
            director_version=director_version,
            mmap_offset=mmap_offset,
            diagnostics=diagnostics,
        )

    mmap_header_size = struct.unpack(endian + "H", data[mmap_payload : mmap_payload + 2])[0]
    mmap_entry_size = struct.unpack(endian + "H", data[mmap_payload + 2 : mmap_payload + 4])[0]
    total_count = struct.unpack(endian + "I", data[mmap_payload + 4 : mmap_payload + 8])[0]
    resource_count = struct.unpack(endian + "I", data[mmap_payload + 8 : mmap_payload + 12])[0]
    if mmap_entry_size < 20:
        return _blocked_director_resource_map(
            relative_path,
            "TSUI_DIRECTOR_RESOURCE_MAP_ENTRY_SIZE_INVALID",
            "Director mmap entry size is smaller than the required resource fields",
            imap_found=True,
            signature=_fourcc(signature),
            form_type=form_type,
            declared_size=declared_size,
            file_size=file_size,
            map_version=map_version,
            director_version=director_version,
            mmap_offset=mmap_offset,
            mmap_header_size=mmap_header_size,
            mmap_entry_size=mmap_entry_size,
            diagnostics=diagnostics,
        )

    header_size = max(mmap_header_size, 24)
    entries_start = mmap_payload + header_size
    if entries_start + resource_count * mmap_entry_size > mmap_end:
        diagnostics.append(
            {
                "code": "TSUI_DIRECTOR_RESOURCE_MAP_ENTRIES_TRUNCATED",
                "relative_path": relative_path,
                "resource_count": resource_count,
                "message": "Director mmap resource entries extend beyond the mmap chunk",
            }
        )
        resource_count = max((mmap_end - entries_start) // mmap_entry_size, 0)

    resources = []
    free_resource_count = 0
    tag_counts: dict[str, int] = {}
    for resource_id in range(resource_count):
        entry_offset = entries_start + resource_id * mmap_entry_size
        tag_bytes = data[entry_offset : entry_offset + 4]
        tag = _fourcc(tag_bytes)
        size = struct.unpack(endian + "I", data[entry_offset + 4 : entry_offset + 8])[0]
        chunk_offset = struct.unpack(endian + "I", data[entry_offset + 8 : entry_offset + 12])[0]
        flags = struct.unpack(endian + "H", data[entry_offset + 12 : entry_offset + 14])[0]
        unknown = struct.unpack(endian + "H", data[entry_offset + 14 : entry_offset + 16])[0]
        next_free_resource_id = struct.unpack(endian + "I", data[entry_offset + 16 : entry_offset + 20])[0]
        if tag_bytes == b"\x00\x00\x00\x00" and size == 0 and chunk_offset == 0:
            free_resource_count += 1
            continue
        resource = {
            "resource_id": resource_id,
            "tag": tag,
            "size": size,
            "chunk_offset": chunk_offset,
            "flags": flags,
            "unknown": unknown,
            "next_free_resource_id": next_free_resource_id,
            "coverage_status": "mapped",
            "payload_sha256": "",
        }
        if chunk_offset + 8 > file_size:
            resource["coverage_status"] = "broken"
            diagnostics.append(
                {
                    "code": "TSUI_DIRECTOR_RESOURCE_MAP_RESOURCE_OFFSET_INVALID",
                    "relative_path": relative_path,
                    "resource_id": resource_id,
                    "tag": tag,
                    "message": "Director resource offset is outside the readable container",
                }
            )
        else:
            actual_tag = _fourcc(data[chunk_offset : chunk_offset + 4])
            actual_size = struct.unpack(endian + "I", data[chunk_offset + 4 : chunk_offset + 8])[0]
            resource["actual_tag"] = actual_tag
            resource["actual_size"] = actual_size
            if actual_tag != tag or actual_size != size:
                resource["coverage_status"] = "broken"
                diagnostics.append(
                    {
                        "code": "TSUI_DIRECTOR_RESOURCE_MAP_RESOURCE_MISMATCH",
                        "relative_path": relative_path,
                        "resource_id": resource_id,
                        "tag": tag,
                        "actual_tag": actual_tag,
                        "message": "Director mmap entry does not match the chunk header at its offset",
                    }
                )
            elif chunk_offset + 8 + actual_size > file_size:
                resource["coverage_status"] = "broken"
                diagnostics.append(
                    {
                        "code": "TSUI_DIRECTOR_RESOURCE_MAP_RESOURCE_TRUNCATED",
                        "relative_path": relative_path,
                        "resource_id": resource_id,
                        "tag": tag,
                        "message": "Director resource payload extends beyond the readable container",
                    }
                )
            else:
                payload = data[chunk_offset + 8 : chunk_offset + 8 + actual_size]
                resource["payload_sha256"] = _sha256_bytes(payload)
        resources.append(resource)
        tag_counts[tag] = tag_counts.get(tag, 0) + 1

    return {
        "schema": "tsuinosora.director_resource_map.v1",
        "status": "blocked" if diagnostics else "pass",
        "relative_path": relative_path,
        "imap_found": True,
        "container_format": "XFIR" if decoded_from_xfir else _fourcc(signature),
        "decoded_container_format": _fourcc(signature) if decoded_from_xfir else "",
        "form_type": form_type,
        "endianness": "big" if endian == ">" else "little",
        "declared_size": declared_size,
        "file_size": original_file_size if decoded_from_xfir else file_size,
        "decoded_size": file_size if decoded_from_xfir else 0,
        "decoded_sha256": _sha256_bytes(data) if decoded_from_xfir else "",
        "sha256": _sha256(path),
        "map_version": map_version,
        "director_version": director_version,
        "mmap_offset": mmap_offset,
        "mmap_header_size": mmap_header_size,
        "mmap_entry_size": mmap_entry_size,
        "total_count": total_count,
        "resource_count": len(resources),
        "free_resource_count": free_resource_count,
        "tag_counts": dict(sorted(tag_counts.items())),
        "resources": resources,
        "diagnostics": diagnostics,
    }

def _read_director_cast_map(path: Path, relative_path: str) -> dict:
    resource_map = _read_director_resource_map(path, relative_path)
    diagnostics = list(resource_map.get("diagnostics", []))
    if resource_map.get("status") != "pass":
        return {
            "relative_path": relative_path,
            "status": "blocked",
            "resource_map_status": resource_map.get("status", "blocked"),
            "key_table_count": 0,
            "cas_library_count": 0,
            "member_count": 0,
            "key_tables": [],
            "cas_libraries": [],
            "members": [],
            "diagnostics": diagnostics,
        }

    endian = ">" if resource_map.get("endianness") == "big" else "<"
    payloads = _director_resource_payloads_by_id(path, endian, resource_map, diagnostics)
    resources_by_id = {int(resource["resource_id"]): resource for resource in resource_map.get("resources", [])}
    key_tables = []
    relationships = []
    for resource in resource_map.get("resources", []):
        if resource.get("tag") != "KEY*":
            continue
        resource_id = int(resource["resource_id"])
        parsed = _parse_director_key_table(
            payloads.get(resource_id, b""),
            endian,
            relative_path,
            resource_id,
        )
        key_tables.append(parsed["table"])
        relationships.extend(parsed["relationships"])
        diagnostics.extend(parsed["diagnostics"])

    cas_library_by_resource = {
        rel["child_resource_id"]: rel["parent_resource_id"]
        for rel in relationships
        if rel.get("child_tag") == "CAS*"
    }
    cas_libraries = []
    members: dict[int, dict] = {}
    for resource in resource_map.get("resources", []):
        if resource.get("tag") != "CAS*":
            continue
        resource_id = int(resource["resource_id"])
        parsed = _parse_director_cas_table(
            payloads.get(resource_id, b""),
            relative_path,
            resource_id,
        )
        diagnostics.extend(parsed["diagnostics"])
        lib_resource_id = cas_library_by_resource.get(resource_id, 0)
        cas_libraries.append(
            {
                "cas_resource_id": resource_id,
                "library_resource_id": lib_resource_id,
                "cast_resource_count": len(parsed["cast_resource_ids"]),
                "cast_resource_ids_hash": _sha256_bytes(
                    ",".join(str(value) for value in parsed["cast_resource_ids"]).encode("ascii")
                ),
            }
        )
        for slot, cast_resource_id in enumerate(parsed["cast_resource_ids"]):
            if cast_resource_id == 0:
                continue
            if cast_resource_id not in resources_by_id or resources_by_id[cast_resource_id].get("tag") != "CASt":
                diagnostics.append(
                    {
                        "code": "TSUI_DIRECTOR_CAST_MAP_CAST_RESOURCE_MISSING",
                        "relative_path": relative_path,
                        "cas_resource_id": resource_id,
                        "cast_resource_id": cast_resource_id,
                        "message": "CAS* table references a missing CASt resource",
                    }
                )
                continue
            if cast_resource_id in members:
                diagnostics.append(
                    {
                        "code": "TSUI_DIRECTOR_CAST_DUPLICATE_MEMBER_BINDING",
                        "relative_path": relative_path,
                        "cas_resource_id": resource_id,
                        "cast_resource_id": cast_resource_id,
                        "existing_library_resource_id": members[cast_resource_id]["library_resource_id"],
                        "existing_cast_slot": members[cast_resource_id]["cast_slot"],
                        "library_resource_id": lib_resource_id,
                        "cast_slot": slot,
                        "message": "CAS* tables map the same CASt resource to multiple cast members",
                    }
                )
                continue
            member = _director_cast_member_skeleton(
                relative_path,
                resource_map,
                cast_resource_id,
                slot,
                lib_resource_id,
                resources_by_id[cast_resource_id],
            )
            _apply_director_cast_member_metadata(
                member,
                payloads.get(cast_resource_id, b""),
                diagnostics,
                relative_path,
            )
            members[cast_resource_id] = member

    cast_resource_ids = {rid for rid, resource in resources_by_id.items() if resource.get("tag") == "CASt"}
    for relationship in relationships:
        child_id = relationship["child_resource_id"]
        parent_id = relationship["parent_resource_id"]
        child_tag = relationship["child_tag"]
        if parent_id in cast_resource_ids:
            if parent_id not in members:
                member = _director_cast_member_skeleton(
                    relative_path,
                    resource_map,
                    parent_id,
                    -1,
                    0,
                    resources_by_id[parent_id],
                )
                _apply_director_cast_member_metadata(
                    member,
                    payloads.get(parent_id, b""),
                    diagnostics,
                    relative_path,
                )
                members[parent_id] = member
            _append_director_child_resource(
                members[parent_id],
                resources_by_id,
                child_id,
                child_tag,
                relative_path,
                diagnostics,
            )
        elif child_id in cast_resource_ids:
            if child_id not in members:
                member = _director_cast_member_skeleton(
                    relative_path,
                    resource_map,
                    child_id,
                    -1,
                    0,
                    resources_by_id[child_id],
                )
                _apply_director_cast_member_metadata(
                    member,
                    payloads.get(child_id, b""),
                    diagnostics,
                    relative_path,
                )
                members[child_id] = member
            _append_director_child_resource(
                members[child_id],
                resources_by_id,
                parent_id,
                child_tag,
                relative_path,
                diagnostics,
            )

    if not key_tables:
        diagnostics.append(
            {
                "code": "TSUI_DIRECTOR_CAST_KEY_TABLE_MISSING",
                "relative_path": relative_path,
                "message": "Director cast map requires a KEY* resource",
            }
        )
    if not cas_libraries:
        diagnostics.append(
            {
                "code": "TSUI_DIRECTOR_CAST_CAS_TABLE_MISSING",
                "relative_path": relative_path,
                "message": "Director cast map requires a CAS* resource",
            }
        )

    member_list = list(sorted(members.values(), key=lambda member: member["cast_resource_id"]))
    return {
        "relative_path": relative_path,
        "status": "blocked" if diagnostics else "pass",
        "resource_map_status": resource_map.get("status", "blocked"),
        "key_table_count": len(key_tables),
        "cas_library_count": len(cas_libraries),
        "member_count": len(member_list),
        "key_tables": key_tables,
        "cas_libraries": cas_libraries,
        "members": member_list,
        "diagnostics": diagnostics,
    }

def _read_director_lingo_map(
    path: Path,
    relative_path: str,
    extracted_script_entries: set[str] | None = None,
) -> dict:
    extracted_script_entries = extracted_script_entries or set()
    resource_map = _read_director_resource_map(path, relative_path)
    diagnostics = list(resource_map.get("diagnostics", []))
    if resource_map.get("status") != "pass":
        return {
            "relative_path": relative_path,
            "status": "blocked",
            "resource_map_status": resource_map.get("status", "blocked"),
            "context_count": 0,
            "context_entry_count": 0,
            "name_count": 0,
            "name_entry_count": 0,
            "script_count": 0,
            "unsupported_script_count": 0,
            "resources": [],
            "diagnostics": diagnostics,
        }

    endian = ">" if resource_map.get("endianness") == "big" else "<"
    payloads = _director_resource_payloads_by_id(path, endian, resource_map, diagnostics)
    container_id = _safe_identifier(Path(relative_path).with_suffix("").as_posix())
    resources = []
    counts = {
        "context_count": 0,
        "context_entry_count": 0,
        "name_count": 0,
        "name_entry_count": 0,
        "script_count": 0,
        "unsupported_script_count": 0,
    }
    for resource in resource_map.get("resources", []):
        tag = resource.get("tag", "")
        if tag not in DIRECTOR_LINGO_CHUNK_IDS:
            continue
        resource_id = int(resource["resource_id"])
        entry_id = f"{container_id}.{resource_id:04d}"
        payload = payloads.get(resource_id, b"")
        context_table = None
        if tag == "Lctx":
            context_table, context_diagnostics = _parse_lingo_context_table(payload, relative_path, resource_id)
            diagnostics.extend(context_diagnostics)
        name_table = None
        if tag == "Lnam":
            name_table, name_diagnostics = _parse_lingo_name_table(payload, relative_path, resource_id)
            diagnostics.extend(name_diagnostics)
        script_text_extractable = tag == "Lscr" and _slice_script_text_payload(payload, "Lscr") is not None
        script_text_extracted = entry_id in extracted_script_entries
        requires_bytecode_reader = tag == "Lscr" and not script_text_extractable and not script_text_extracted
        if tag == "Lctx":
            counts["context_count"] += 1
            counts["context_entry_count"] += int(context_table["entry_count"] if context_table else 0)
        elif tag == "Lnam":
            counts["name_count"] += 1
            counts["name_entry_count"] += int(name_table["entry_count"] if name_table else 0)
        elif tag == "Lscr":
            counts["script_count"] += 1
            if requires_bytecode_reader:
                counts["unsupported_script_count"] += 1
        entry = {
            "resource_id": resource_id,
            "entry_id": entry_id,
            "tag": tag,
            "size": resource.get("size", 0),
            "payload_sha256": resource.get("payload_sha256", ""),
            "coverage_status": resource.get("coverage_status", "mapped"),
            "script_text_extractable": script_text_extractable,
            "script_text_extracted": script_text_extracted,
            "requires_bytecode_reader": requires_bytecode_reader,
        }
        if context_table is not None:
            entry.update(context_table)
        if name_table is not None:
            entry.update(name_table)
        resources.append(entry)

    return {
        "relative_path": relative_path,
        "status": "blocked" if diagnostics else "pass",
        "resource_map_status": resource_map.get("status", "blocked"),
        **counts,
        "resources": resources,
        "diagnostics": diagnostics,
    }

def _parse_lingo_context_table(payload: bytes, relative_path: str, resource_id: int) -> tuple[dict, list[dict]]:
    diagnostics = []
    if len(payload) % 4:
        diagnostics.append(
            {
                "code": "TSUI_DIRECTOR_LINGO_CONTEXT_TABLE_UNALIGNED",
                "relative_path": relative_path,
                "resource_id": resource_id,
                "message": "Lctx payload size is not aligned to 32-bit context entries",
            }
        )
    entry_count = len(payload) // 4 if payload else 0
    return {
        "entry_count": entry_count,
        "entry_table_sha256": _sha256_bytes(payload),
    }, diagnostics

def _parse_lingo_name_table(payload: bytes, relative_path: str, resource_id: int) -> tuple[dict, list[dict]]:
    diagnostics = []
    if payload and not payload.endswith(b"\x00"):
        diagnostics.append(
            {
                "code": "TSUI_DIRECTOR_LINGO_NAME_TABLE_UNTERMINATED",
                "relative_path": relative_path,
                "resource_id": resource_id,
                "message": "Lnam payload is not a null-terminated sanitized name table",
            }
        )
    entries = [entry for entry in payload.split(b"\x00") if entry]
    return {
        "entry_count": len(entries),
        "entry_table_sha256": _sha256_bytes(payload),
    }, diagnostics

def _director_resource_payloads_by_id(
    path: Path,
    endian: str,
    resource_map: dict,
    diagnostics: list[dict],
) -> dict[int, bytes]:
    data = _director_resource_data(path, resource_map)
    payloads: dict[int, bytes] = {}
    relative_path = str(resource_map.get("relative_path", "unknown"))
    for resource in resource_map.get("resources", []):
        if resource.get("coverage_status") != "mapped":
            continue
        resource_id = int(resource["resource_id"])
        offset = int(resource["chunk_offset"])
        size = int(resource["size"])
        if offset + 8 + size > len(data):
            diagnostics.append(
                {
                    "code": "TSUI_DIRECTOR_CAST_RESOURCE_PAYLOAD_TRUNCATED",
                    "relative_path": relative_path,
                    "resource_id": resource_id,
                    "message": "mapped Director resource payload is truncated",
                }
            )
            continue
        header = data[offset : offset + 8]
        chunk_id = _fourcc(header[:4])
        chunk_size = struct.unpack(endian + "I", header[4:8])[0]
        if chunk_id != resource.get("tag") or chunk_size != size:
            diagnostics.append(
                {
                    "code": "TSUI_DIRECTOR_CAST_RESOURCE_HEADER_MISMATCH",
                    "relative_path": relative_path,
                    "resource_id": resource_id,
                    "message": "mapped Director resource header does not match the resource map",
                }
            )
            continue
        payloads[resource_id] = data[offset + 8 : offset + 8 + size]
    return payloads

def _director_resource_data(path: Path, resource_map: dict) -> bytes:
    data = path.read_bytes()
    if resource_map.get("container_format") == "XFIR" and resource_map.get("decoded_container_format"):
        decoded = _decode_xfir_riff_payload(data)
        if decoded:
            return decoded["data"]
    return data

def _append_director_child_resource(
    member: dict,
    resources_by_id: dict[int, dict],
    child_resource_id: int,
    expected_tag: str,
    relative_path: str,
    diagnostics: list[dict],
) -> None:
    child = resources_by_id.get(child_resource_id)
    if not child:
        diagnostics.append(
            {
                "code": "TSUI_DIRECTOR_CAST_CHILD_RESOURCE_MISSING",
                "relative_path": relative_path,
                "cast_resource_id": member["cast_resource_id"],
                "child_resource_id": child_resource_id,
                "message": "KEY* references a missing child resource",
            }
        )
        return
    if expected_tag and child.get("tag") != expected_tag:
        diagnostics.append(
            {
                "code": "TSUI_DIRECTOR_CAST_CHILD_TAG_MISMATCH",
                "relative_path": relative_path,
                "cast_resource_id": member["cast_resource_id"],
                "child_resource_id": child_resource_id,
                "expected_tag": expected_tag,
                "actual_tag": child.get("tag", ""),
                "message": "KEY* child tag does not match the mapped child resource",
            }
        )
        return
    entry = {
        "resource_id": child_resource_id,
        "tag": child.get("tag", ""),
        "size": child.get("size", 0),
        "payload_sha256": child.get("payload_sha256", ""),
        "coverage_status": child.get("coverage_status", "mapped"),
    }
    if entry not in member["child_resources"]:
        member["child_resources"].append(entry)

def _blocked_director_resource_map(
    relative_path: str,
    code: str,
    message: str,
    *,
    imap_found: bool,
    signature: str = "unknown",
    form_type: str = "",
    declared_size: int = 0,
    file_size: int = 0,
    map_version: int = 0,
    director_version: int = 0,
    mmap_offset: int = 0,
    mmap_header_size: int = 0,
    mmap_entry_size: int = 0,
    diagnostics: list[dict] | None = None,
) -> dict:
    all_diagnostics = list(diagnostics or [])
    all_diagnostics.append(
        {
            "code": code,
            "relative_path": relative_path,
            "message": message,
        }
    )
    return {
        "schema": "tsuinosora.director_resource_map.v1",
        "status": "blocked",
        "relative_path": relative_path,
        "imap_found": imap_found,
        "container_format": signature,
        "form_type": form_type,
        "declared_size": declared_size,
        "file_size": file_size,
        "map_version": map_version,
        "director_version": director_version,
        "mmap_offset": mmap_offset,
        "mmap_header_size": mmap_header_size,
        "mmap_entry_size": mmap_entry_size,
        "total_count": 0,
        "resource_count": 0,
        "free_resource_count": 0,
        "tag_counts": {},
        "resources": [],
        "diagnostics": all_diagnostics,
    }

def _director_resource_map_summary(resource_map: dict) -> dict:
    return {
        "schema": resource_map.get("schema", "tsuinosora.director_resource_map.v1"),
        "status": resource_map.get("status", "blocked"),
        "imap_found": resource_map.get("imap_found", False),
        "resource_count": resource_map.get("resource_count", 0),
        "free_resource_count": resource_map.get("free_resource_count", 0),
        "tag_counts": resource_map.get("tag_counts", {}),
        "diagnostic_codes": [diagnostic.get("code", "") for diagnostic in resource_map.get("diagnostics", [])],
    }

def _mapped_director_resource_chunks(
    path: Path,
    relative_path: str,
    endian: str,
    resource_map: dict,
    diagnostics: list[dict],
    data: bytes | None = None,
) -> list[dict]:
    data = data if data is not None else _director_resource_data(path, resource_map)
    chunks = []
    for resource in resource_map.get("resources", []):
        if resource.get("coverage_status") != "mapped":
            continue
        offset = int(resource["chunk_offset"])
        size = int(resource["size"])
        if offset + 8 + size > len(data):
            diagnostics.append(
                {
                    "code": "TSUI_EXTRACT_RESOURCE_PAYLOAD_TRUNCATED",
                    "relative_path": relative_path,
                    "resource_id": resource["resource_id"],
                    "message": "mapped Director resource payload is truncated",
                }
            )
            continue
        chunk_header = data[offset : offset + 8]
        chunk_id = _fourcc(chunk_header[:4])
        chunk_size = struct.unpack(endian + "I", chunk_header[4:8])[0]
        if chunk_id != resource["tag"] or chunk_size != size:
            diagnostics.append(
                {
                    "code": "TSUI_EXTRACT_RESOURCE_HEADER_MISMATCH",
                    "relative_path": relative_path,
                    "resource_id": resource["resource_id"],
                    "message": "mapped Director resource header changed between map and extraction",
                }
            )
            continue
        chunks.append(
            {
                "resource_id": resource["resource_id"],
                "chunk_id": chunk_id,
                "chunk_offset": offset,
                "chunk_size": chunk_size,
                "payload": data[offset + 8 : offset + 8 + chunk_size],
            }
        )
    return chunks

def _linear_riff_chunks(handle, relative_path: str, endian: str, file_size: int, diagnostics: list[dict]) -> list[dict]:
    chunks = []
    offset = 12
    while offset + 8 <= file_size:
        handle.seek(offset)
        chunk_header = handle.read(8)
        if len(chunk_header) < 8:
            break
        chunk_id = _fourcc(chunk_header[:4])
        chunk_size = struct.unpack(endian + "I", chunk_header[4:8])[0]
        payload_offset = offset + 8
        next_offset = payload_offset + chunk_size + (chunk_size % 2)
        if payload_offset + chunk_size > file_size:
            diagnostics.append(
                {
                    "code": "TSUI_EXTRACT_CONTAINER_CHUNK_TRUNCATED",
                    "relative_path": relative_path,
                    "chunk_id": chunk_id,
                    "chunk_offset": offset,
                    "chunk_size": chunk_size,
                    "message": "chunk payload extends beyond the readable container",
                }
            )
            break
        handle.seek(payload_offset)
        chunks.append(
            {
                "chunk_id": chunk_id,
                "chunk_offset": offset,
                "chunk_size": chunk_size,
                "payload": handle.read(chunk_size),
            }
        )
        offset = next_offset
    return chunks

def _extract_payload_from_container_chunk(
    *,
    payload: bytes,
    chunk_id: str,
    output_index: int,
    entry: dict,
    container_id: str,
    unpacked_root: Path,
    source_container: str,
) -> list[dict]:
    files = []
    metadata_payload = _slice_metadata_json_payload(payload)
    if metadata_payload:
        metadata_json, schema, payload_inner_offset = metadata_payload
        output_name = f"{output_index:04d}_{_safe_identifier(chunk_id)}.json"
        output_rel = f"containers/{container_id}/{output_name}"
        dest = unpacked_root / output_rel
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_text(metadata_json, encoding="utf-8")
        payload_bytes = metadata_json.encode("utf-8")
        entry["format_probe"] = "metadata_json"
        entry["metadata_schema"] = schema
        entry["coverage_status"] = "extracted"
        entry["output_relative_path"] = f"unpacked/{output_rel}"
        entry["payload_inner_offset"] = payload_inner_offset
        files.append(
            {
                "relative_path": output_rel,
                "output_relative_path": f"unpacked/{output_rel}",
                "source_container": source_container,
                "container_entry_id": entry["entry_id"],
                "chunk_id": chunk_id,
                "size": len(payload_bytes),
                "sha256": _sha256_bytes(payload_bytes),
                "format_probe": "metadata_json",
                "metadata_schema": schema,
            }
        )
        return files

    sliced = _slice_embedded_payload(payload)
    if sliced:
        embedded_payload, extension, format_probe, payload_inner_offset = sliced
        output_name = f"{output_index:04d}_{_safe_identifier(chunk_id)}.{extension}"
        output_rel = f"containers/{container_id}/{output_name}"
        dest = unpacked_root / output_rel
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(embedded_payload)
        entry["format_probe"] = format_probe
        entry["coverage_status"] = "extracted"
        entry["output_relative_path"] = f"unpacked/{output_rel}"
        entry["payload_inner_offset"] = payload_inner_offset
        files.append(
            {
                "relative_path": output_rel,
                "output_relative_path": f"unpacked/{output_rel}",
                "source_container": source_container,
                "container_entry_id": entry["entry_id"],
                "chunk_id": chunk_id,
                "size": len(embedded_payload),
                "sha256": _sha256_bytes(embedded_payload),
                "format_probe": format_probe,
            }
        )
        return files

    text_payload = _slice_script_text_payload(payload, chunk_id)
    if text_payload:
        text, encoding, payload_inner_offset = text_payload
        output_name = f"{output_index:04d}_{_safe_identifier(chunk_id)}.ls"
        output_rel = f"containers/{container_id}/{output_name}"
        dest = unpacked_root / output_rel
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_text(text, encoding="utf-8")
        payload_bytes = text.encode("utf-8")
        entry["format_probe"] = "script_text"
        entry["coverage_status"] = "extracted"
        entry["output_relative_path"] = f"unpacked/{output_rel}"
        entry["payload_inner_offset"] = payload_inner_offset
        entry["source_encoding"] = encoding
        entry["line_count"] = len(text.splitlines())
        files.append(
            {
                "relative_path": output_rel,
                "output_relative_path": f"unpacked/{output_rel}",
                "source_container": source_container,
                "container_entry_id": entry["entry_id"],
                "chunk_id": chunk_id,
                "size": len(payload_bytes),
                "sha256": _sha256_bytes(payload_bytes),
                "format_probe": "script_text",
                "line_count": len(text.splitlines()),
                "payload_inner_offset": payload_inner_offset,
            }
        )
    return files

def _cast_member_from_map(
    raw_member: dict,
    map_source: str,
    asset_index: dict[str, Path],
) -> tuple[dict | None, list[dict]]:
    diagnostics = []
    member_id = str(raw_member.get("member_id", "")).strip()
    kind = str(raw_member.get("kind", "unknown")).strip()
    source = str(raw_member.get("source", "")).strip()
    declared_source_hash = str(raw_member.get("source_hash", "")).strip()
    container_entry_id = str(raw_member.get("container_entry_id", "")).strip()
    director_child_resource_id = raw_member.get("director_child_resource_id")
    director_child_tag = str(raw_member.get("director_child_tag", ""))
    director_child_payload_sha256 = str(raw_member.get("director_child_payload_sha256", "")).strip()
    command_ids = [str(value).strip() for value in raw_member.get("command_ids", []) if str(value).strip()]
    route_ids = [str(value).strip() for value in raw_member.get("route_ids", []) if str(value).strip()]
    parts = []

    if not member_id or not _is_safe_symbol(member_id):
        diagnostics.append(
            {
                "code": "TSUI_CAST_MEMBER_ID_INVALID",
                "source": map_source,
                "member_id": member_id or "unknown",
                "message": "cast member requires a safe member_id",
            }
        )
        return None, diagnostics
    if kind not in CAST_MEMBER_KINDS:
        diagnostics.append(
            {
                "code": "TSUI_CAST_MEMBER_KIND_INVALID",
                "source": map_source,
                "member_id": member_id,
                "kind": kind,
                "message": "cast member kind is not part of the allowed classification set",
            }
        )
    if source and not _is_safe_report_relative_path(source):
        diagnostics.append(
            {
                "code": "TSUI_CAST_MEMBER_SOURCE_PATH_INVALID",
                "source": map_source,
                "member_id": member_id,
                "message": "cast member source must be report-relative",
            }
        )
    if source and source not in asset_index:
        diagnostics.append(
            {
                "code": "TSUI_CAST_MEMBER_SOURCE_MISSING",
                "source": map_source,
                "member_id": member_id,
                "member_source": source,
                "message": "cast member source is not present in unpacked assets",
            }
        )
    actual_source_hash = _sha256(asset_index[source]) if source in asset_index else ""
    if declared_source_hash:
        if not _is_sanitized_sha256(declared_source_hash):
            diagnostics.append(
                {
                    "code": "TSUI_CAST_MEMBER_SOURCE_HASH_INVALID",
                    "source": map_source,
                    "member_id": member_id or "unknown",
                    "message": "cast member source_hash must be a sanitized sha256 digest",
                }
            )
        elif actual_source_hash and declared_source_hash != actual_source_hash:
            diagnostics.append(
                {
                    "code": "TSUI_CAST_MEMBER_SOURCE_HASH_MISMATCH",
                    "source": map_source,
                    "member_id": member_id or "unknown",
                    "message": "cast member source_hash does not match the extracted source asset",
                }
            )
    if not source and not container_entry_id:
        diagnostics.append(
            {
                "code": "TSUI_CAST_MEMBER_SOURCE_UNMAPPED",
                "source": map_source,
                "member_id": member_id,
                "message": "cast member requires a source path or container entry id",
            }
        )
    if container_entry_id and not _is_safe_symbol(container_entry_id):
        diagnostics.append(
            {
                "code": "TSUI_CAST_MEMBER_ENTRY_ID_INVALID",
                "source": map_source,
                "member_id": member_id,
                "message": "container entry id must be a safe symbolic id",
            }
        )
    director_child_resource_id_value = None
    if director_child_resource_id not in (None, ""):
        try:
            director_child_resource_id_value = int(director_child_resource_id)
        except (TypeError, ValueError):
            diagnostics.append(
                {
                    "code": "TSUI_CAST_DIRECTOR_CHILD_RESOURCE_ID_INVALID",
                    "source": map_source,
                    "member_id": member_id or "unknown",
                    "message": "Director child resource id must be numeric",
                }
            )
    if director_child_tag and not re.match(r"^[\x20-\x7e]{4}$", director_child_tag):
        diagnostics.append(
            {
                "code": "TSUI_CAST_DIRECTOR_CHILD_TAG_INVALID",
                "source": map_source,
                "member_id": member_id or "unknown",
                "message": "Director child resource tag must be a sanitized FourCC",
            }
        )
    if director_child_payload_sha256 and not _is_sanitized_sha256(director_child_payload_sha256):
        diagnostics.append(
            {
                "code": "TSUI_CAST_DIRECTOR_CHILD_HASH_INVALID",
                "source": map_source,
                "member_id": member_id or "unknown",
                "message": "Director child resource hash must be a sanitized sha256 digest",
            }
        )
    for route_id in route_ids:
        if not _is_safe_symbol(route_id):
            diagnostics.append(
                {
                    "code": "TSUI_CAST_MEMBER_ROUTE_ID_INVALID",
                    "source": map_source,
                    "member_id": member_id,
                    "route_id": route_id,
                    "message": "route id must be a safe symbolic id",
                }
            )
    for command_id in command_ids:
        if not _is_safe_symbol(command_id):
            diagnostics.append(
                {
                    "code": "TSUI_CAST_MEMBER_COMMAND_ID_INVALID",
                    "source": map_source,
                    "member_id": member_id,
                    "command_id": command_id,
                    "message": "command id must be a safe symbolic id",
                }
            )
    if "parts" in raw_member:
        parts, part_diagnostics = _safe_atlas_parts(
            raw_member.get("parts"),
            source=map_source,
            owner_id=member_id or "unknown",
            source_field="source",
            code_prefix="TSUI_CAST_MEMBER",
        )
        diagnostics.extend(part_diagnostics)
    elif kind == "character_atlas":
        diagnostics.append(
            {
                "code": "TSUI_CAST_MEMBER_ATLAS_PARTS_MISSING",
                "source": map_source,
                "member_id": member_id,
                "message": "character_atlas cast member must include crop/part records",
            }
        )

    member = {
        "member_id": member_id,
        "kind": kind if kind in CAST_MEMBER_KINDS else "unknown",
        "source": source,
        "source_hash": actual_source_hash,
        "container_entry_id": container_entry_id,
        "route_ids": route_ids,
        "command_ids": command_ids,
        "coverage_status": "mapped" if source in asset_index or container_entry_id else "manual_review",
        "map_source": map_source,
    }
    if director_child_resource_id_value is not None:
        member["director_child_resource_id"] = director_child_resource_id_value
    if director_child_tag:
        member["director_child_tag"] = director_child_tag
    if director_child_payload_sha256:
        member["director_child_payload_sha256"] = director_child_payload_sha256
    if parts:
        member["parts"] = parts
    return member, diagnostics

def extract_readable_assets(
    source_root: Path | str,
    work_root: Path | str,
    source_alias: str = "original_install_root",
) -> dict:
    source_root = Path(source_root)
    work_root = Path(work_root)
    reports_root = work_root / "reports"
    unpacked_root = work_root / "unpacked"
    diagnostics = []
    extracted = []
    skipped = []
    containers = []

    if not source_root.exists():
        report = _blocked_extract_report(
            source_alias,
            "TSUI_EXTRACT_SOURCE_MISSING",
            "source root does not exist or is not accessible",
        )
        _write_json(reports_root / "extract_report.json", report)
        return report
    if not source_root.is_dir():
        report = _blocked_extract_report(
            source_alias,
            "TSUI_EXTRACT_SOURCE_NOT_DIRECTORY",
            "source root must be a directory",
        )
        _write_json(reports_root / "extract_report.json", report)
        return report

    files = sorted(p for p in source_root.rglob("*") if p.is_file())
    for path in files:
        rel = _rel(path, source_root)
        ext = path.suffix.lower()
        probe = _format_probe(path)
        if not _is_safe_report_relative_path(rel):
            diagnostics.append(
                {
                    "code": "TSUI_EXTRACT_UNSAFE_RELATIVE_PATH",
                    "source_alias": source_alias,
                    "relative_path": rel,
                    "message": "source entry is not a safe report-relative path",
                }
            )
            skipped.append(
                {
                    "relative_path": rel,
                    "format_probe": probe,
                    "reason": "unsafe_relative_path",
                }
            )
            continue
        if ext in READABLE_EXTRACT_EXTS:
            output_rel = f"unpacked/{rel}"
            dest = unpacked_root / rel
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(path, dest)
            extracted.append(
                {
                    "relative_path": rel,
                    "output_relative_path": output_rel,
                    "size": path.stat().st_size,
                    "sha256": _sha256(path),
                    "format_probe": probe,
                }
            )
        elif ext in DIRECTOR_CONTAINER_EXTS:
            container_report = _extract_readable_container(path, source_root, unpacked_root)
            containers.append(container_report)
            extracted.extend(container_report.get("files", []))
            diagnostics.extend(container_report.get("diagnostics", []))
            if container_report.get("status") != "pass":
                skipped.append(
                    {
                        "relative_path": rel,
                        "format_probe": probe,
                        "reason": container_report.get("block_reason", "director_reader_required"),
                    }
                )
        else:
            skipped.append(
                {
                    "relative_path": rel,
                    "format_probe": probe,
                    "reason": "unsupported_or_irrelevant_format",
                }
            )

    protected_count = sum(1 for entry in skipped if entry["reason"] == "director_reader_required")
    if protected_count:
        diagnostics.append(
            {
                "code": "TSUI_EXTRACT_DIRECTOR_READER_REQUIRED",
                "source_alias": source_alias,
                "container_count": protected_count,
                "message": "Director/Shockwave containers require a real reader before full conversion can pass",
            }
        )
    if not extracted:
        diagnostics.append(
            {
                "code": "TSUI_EXTRACT_NO_READABLE_ASSETS",
                "source_alias": source_alias,
                "message": "no directly readable sidecar assets were extracted",
            }
        )

    report = {
        "schema": "tsuinosora.extract_report.v1",
        "status": "blocked" if diagnostics else "pass",
        "source_alias": source_alias,
        "output_alias": "local_work_root/unpacked",
        "input_file_count": len(files),
        "extracted_count": len(extracted),
        "skipped_count": len(skipped),
        "container_count": len(containers),
        "container_entry_count": sum(container.get("entry_count", 0) for container in containers),
        "protected_container_count": protected_count,
        "format_counts": _format_counts(
            [{"format_probe": entry["format_probe"]} for entry in extracted + skipped]
        ),
        "containers": containers,
        "files": extracted,
        "skipped": skipped,
        "diagnostics": diagnostics,
        "redaction": {
            "paths": "alias_or_report_relative_only",
            "payload": "omitted",
            "commercial_text": "omitted",
            "screenshots": "omitted",
            "audio": "omitted",
            "movie": "omitted",
        },
    }
    if _report_has_path_leak(report):
        report["status"] = "blocked"
        report["diagnostics"].append(
            {
                "code": "TSUI_EXTRACT_REPORT_PATH_LEAK",
                "message": "extract report contains a local path-like value",
            }
        )
    _write_json(reports_root / "extract_report.json", report)
    return report

def _extract_readable_container(path: Path, source_root: Path, unpacked_root: Path) -> dict:
    rel = _rel(path, source_root)
    container_id = _safe_identifier(Path(rel).with_suffix("").as_posix())
    diagnostics = []
    entries = []
    files = []
    generated_reports = []

    raw_data = path.read_bytes()
    decoded = _decode_xfir_riff_payload(raw_data)
    decoded_from_xfir = False
    if len(raw_data) >= 4 and raw_data[:4] == b"XFIR":
        if not decoded:
            return {
                "relative_path": rel,
                "status": "blocked",
                "block_reason": "director_reader_required",
                "container_format": "XFIR",
                "entry_count": 0,
                "readable_payload_count": 0,
                "sha256": _sha256(path),
                "entries": [],
                "files": [],
                "diagnostics": [
                    {
                        "code": "TSUI_EXTRACT_DIRECTOR_XFIR_READER_REQUIRED",
                        "relative_path": rel,
                        "message": "Shockwave XFIR containers require a dedicated verified reader before payload extraction",
                    }
                ],
            }
        data = decoded["data"]
        decoded_from_xfir = True
    else:
        data = raw_data

    with io.BytesIO(data) as handle:
        header = handle.read(12)
        if len(header) < 12 or header[:4] not in READABLE_RIFF_SIGNATURES:
            return {
                "relative_path": rel,
                "status": "blocked",
                "block_reason": "director_reader_required",
                "container_format": "unknown",
                "entry_count": 0,
                "readable_payload_count": 0,
                "sha256": _sha256(path),
                "entries": [],
                "files": [],
                "diagnostics": [
                    {
                        "code": "TSUI_EXTRACT_CONTAINER_UNRECOGNIZED",
                        "relative_path": rel,
                        "message": "container is not a readable RIFF/RIFX Director container",
                    }
                ],
            }
        signature = header[:4]
        endian = ">" if signature == b"RIFX" else "<"
        declared_size = struct.unpack(endian + "I", header[4:8])[0]
        form_type = _fourcc(header[8:12])
        file_size = len(data)
        original_file_size = len(raw_data)
        container_size_matches = declared_size + 8 == file_size
        if not container_size_matches:
            diagnostics.append(
                {
                    "code": "TSUI_EXTRACT_CONTAINER_SIZE_MISMATCH",
                    "relative_path": rel,
                    "declared_size": declared_size,
                    "file_size": file_size,
                    "message": "container declared size does not match readable file size",
                }
            )

        if not container_size_matches:
            chunk_records = []
            extraction_mode = "container_size_mismatch"
            resource_map = _blocked_director_resource_map(
                rel,
                "TSUI_DIRECTOR_RESOURCE_MAP_SIZE_MISMATCH",
                "container declared size does not match readable file size",
                imap_found=False,
                signature=_fourcc(signature),
                form_type=form_type,
                declared_size=declared_size,
                file_size=file_size,
            )
        else:
            resource_map = _read_director_resource_map(path, rel)
        if container_size_matches and resource_map.get("status") == "pass":
            chunk_records = _mapped_director_resource_chunks(
                path,
                rel,
                endian,
                resource_map,
                diagnostics,
                data=data,
            )
            extraction_mode = "director_resource_map"
        elif container_size_matches and resource_map.get("imap_found"):
            diagnostics.extend(resource_map.get("diagnostics", []))
            chunk_records = []
            extraction_mode = "director_resource_map"
        elif container_size_matches:
            chunk_records = _linear_riff_chunks(handle, rel, endian, file_size, diagnostics)
            extraction_mode = "linear_chunk_scan"

        for index, record in enumerate(chunk_records, start=1):
            chunk_id = record["chunk_id"]
            payload = record["payload"]
            entry = {
                "entry_id": f"{container_id}.{record.get('resource_id', index):04d}",
                "chunk_id": chunk_id,
                "chunk_offset": record["chunk_offset"],
                "chunk_size": record["chunk_size"],
                "payload_sha256": _sha256_bytes(payload),
                "format_probe": "unknown",
                "coverage_status": "manual_review",
            }
            if "resource_id" in record:
                entry["resource_id"] = record["resource_id"]
            files.extend(
                _extract_payload_from_container_chunk(
                    payload=payload,
                    chunk_id=chunk_id,
                    output_index=index,
                    entry=entry,
                    container_id=container_id,
                    unpacked_root=unpacked_root,
                    source_container=rel,
                )
            )
            entries.append(entry)

        director_cast_map = _director_cast_map_report_for_container(path, rel, resource_map)
        if director_cast_map:
            if director_cast_map.get("status") != "pass":
                diagnostics.extend(director_cast_map.get("diagnostics", []))
            else:
                output_rel = f"containers/{container_id}/director_cast_map.json"
                _write_json(unpacked_root / output_rel, director_cast_map)
                generated_reports.append(
                    {
                        "relative_path": output_rel,
                        "schema": "tsuinosora.director_cast_map.v1",
                        "member_count": director_cast_map.get("member_count", 0),
                    }
                )

        director_lingo_map = _director_lingo_map_report_for_container(path, rel, resource_map, files)
        if director_lingo_map:
            if director_lingo_map.get("status") != "pass":
                diagnostics.extend(director_lingo_map.get("diagnostics", []))
            else:
                output_rel = f"containers/{container_id}/director_lingo_map.json"
                _write_json(unpacked_root / output_rel, director_lingo_map)
                generated_reports.append(
                    {
                        "relative_path": output_rel,
                        "schema": "tsuinosora.director_lingo_map.v1",
                        "script_count": director_lingo_map.get("script_count", 0),
                        "unsupported_script_count": director_lingo_map.get("unsupported_script_count", 0),
                    }
                )
                source_map_report = _director_lingo_source_map_from_extracted_scripts(
                    unpacked_root=unpacked_root,
                    container_id=container_id,
                    lingo_map_report=director_lingo_map,
                    lingo_map_relative_path=output_rel,
                    extracted_files=files,
                    source_container=rel,
                )
                if source_map_report:
                    source_map_rel = f"containers/{container_id}/director_lingo_source_map.json"
                    _write_json(unpacked_root / source_map_rel, source_map_report)
                    generated_reports.append(
                        {
                            "relative_path": source_map_rel,
                            "schema": "tsuinosora.script_source_map.v1",
                            "route_count": len(source_map_report.get("routes", [])),
                        }
                    )

    if not files:
        diagnostics.append(
            {
                "code": "TSUI_EXTRACT_CONTAINER_NO_READABLE_PAYLOADS",
                "relative_path": rel,
                "message": "container parsed, but no directly readable embedded payload was found",
            }
        )

    return {
        "relative_path": rel,
        "status": "blocked" if diagnostics else "pass",
        "block_reason": "director_reader_required" if diagnostics else "",
        "container_format": "XFIR" if decoded_from_xfir else _fourcc(signature),
        "decoded_container_format": _fourcc(signature) if decoded_from_xfir else "",
        "form_type": form_type,
        "declared_size": declared_size,
        "file_size": original_file_size if decoded_from_xfir else file_size,
        "decoded_size": file_size if decoded_from_xfir else 0,
        "sha256": _sha256(path),
        "decoded_sha256": _sha256_bytes(data) if decoded_from_xfir else "",
        "entry_count": len(entries),
        "readable_payload_count": len(files),
        "extraction_mode": extraction_mode,
        "director_resource_map": _director_resource_map_summary(resource_map),
        "entries": entries,
        "files": files,
        "generated_reports": generated_reports,
        "diagnostics": diagnostics,
    }

def build_director_resource_map_report(root: Path | str) -> dict:
    root = Path(root)
    diagnostics = []
    containers = []
    tag_counts: dict[str, int] = {}
    for path in sorted(p for p in root.rglob("*") if p.is_file() and p.suffix.lower() in DIRECTOR_CONTAINER_EXTS):
        rel = _rel(path, root)
        container = _read_director_resource_map(path, rel)
        containers.append(container)
        for tag, count in container.get("tag_counts", {}).items():
            tag_counts[tag] = tag_counts.get(tag, 0) + count
        if container.get("status") != "pass":
            diagnostics.extend(container.get("diagnostics", []))

    if not containers:
        diagnostics.append(
            {
                "code": "TSUI_DIRECTOR_RESOURCE_MAP_CONTAINER_MISSING",
                "message": "no Director/Shockwave container was found for resource map preflight",
            }
        )

    report = {
        "schema": "tsuinosora.director_resource_map.v1",
        "status": "blocked" if diagnostics else "pass",
        "container_count": len(containers),
        "resource_count": sum(container.get("resource_count", 0) for container in containers),
        "free_resource_count": sum(container.get("free_resource_count", 0) for container in containers),
        "tag_counts": dict(sorted(tag_counts.items())),
        "containers": containers,
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
                "code": "TSUI_DIRECTOR_RESOURCE_MAP_REPORT_PATH_LEAK",
                "message": "Director resource map report contains a local path-like value",
            }
        )
    return report

def build_director_cast_map_report(root: Path | str) -> dict:
    root = Path(root)
    diagnostics = []
    containers = []
    total_members = 0
    for path in sorted(p for p in root.rglob("*") if p.is_file() and p.suffix.lower() in DIRECTOR_CONTAINER_EXTS):
        rel = _rel(path, root)
        container = _read_director_cast_map(path, rel)
        containers.append(container)
        total_members += container.get("member_count", 0)
        if container.get("status") != "pass":
            diagnostics.extend(container.get("diagnostics", []))

    if not containers:
        diagnostics.append(
            {
                "code": "TSUI_DIRECTOR_CAST_MAP_CONTAINER_MISSING",
                "message": "no Director/Shockwave container was found for cast map preflight",
            }
        )
    if containers and total_members == 0:
        diagnostics.append(
            {
                "code": "TSUI_DIRECTOR_CAST_MAP_MEMBER_MISSING",
                "message": "Director KEY*/CAS* preflight did not map any cast member",
            }
        )

    report = {
        "schema": "tsuinosora.director_cast_map.v1",
        "status": "blocked" if diagnostics else "pass",
        "container_count": len(containers),
        "member_count": total_members,
        "containers": containers,
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
                "code": "TSUI_DIRECTOR_CAST_MAP_REPORT_PATH_LEAK",
                "message": "Director cast map report contains a local path-like value",
            }
        )
    return report

def build_director_lingo_map_report(root: Path | str) -> dict:
    root = Path(root)
    diagnostics = []
    containers = []
    totals = {
        "context_count": 0,
        "context_entry_count": 0,
        "name_count": 0,
        "name_entry_count": 0,
        "script_count": 0,
        "unsupported_script_count": 0,
    }
    for path in sorted(p for p in root.rglob("*") if p.is_file() and p.suffix.lower() in DIRECTOR_CONTAINER_EXTS):
        rel = _rel(path, root)
        container = _read_director_lingo_map(path, rel)
        containers.append(container)
        for key in totals:
            totals[key] += int(container.get(key, 0))
        if container.get("status") != "pass":
            diagnostics.extend(container.get("diagnostics", []))

    if not containers:
        diagnostics.append(
            {
                "code": "TSUI_DIRECTOR_LINGO_MAP_CONTAINER_MISSING",
                "message": "no Director/Shockwave container was found for Lingo map preflight",
            }
        )
    if containers and totals["context_count"] + totals["name_count"] + totals["script_count"] == 0:
        diagnostics.append(
            {
                "code": "TSUI_DIRECTOR_LINGO_MAP_RESOURCE_MISSING",
                "message": "Director Lingo map preflight did not find Lctx, Lnam or Lscr resources",
            }
        )

    report = {
        "schema": "tsuinosora.director_lingo_map.v1",
        "status": "blocked" if diagnostics else "pass",
        "container_count": len(containers),
        **totals,
        "containers": containers,
        "diagnostics": diagnostics,
        "redaction": {
            "paths": "report_relative_only",
            "payload": "omitted",
            "commercial_text": "omitted",
            "lingo_names": "omitted",
            "bytecode": "omitted",
        },
    }
    if _report_has_path_leak(report):
        report["status"] = "blocked"
        report["diagnostics"].append(
            {
                "code": "TSUI_DIRECTOR_LINGO_MAP_REPORT_PATH_LEAK",
                "message": "Director Lingo map report contains a local path-like value",
            }
        )
    return report

def _director_cast_map_report_for_container(path: Path, relative_path: str, resource_map: dict) -> dict | None:
    tag_counts = resource_map.get("tag_counts", {})
    if not any(tag_counts.get(tag, 0) for tag in ("KEY*", "CAS*")):
        return None

    container = _read_director_cast_map(path, relative_path)
    report = {
        "schema": "tsuinosora.director_cast_map.v1",
        "status": container.get("status", "blocked"),
        "container_count": 1,
        "member_count": container.get("member_count", 0),
        "containers": [container],
        "diagnostics": list(container.get("diagnostics", [])),
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
                "code": "TSUI_DIRECTOR_CAST_MAP_REPORT_PATH_LEAK",
                "message": "Director cast map report contains a local path-like value",
            }
        )
    return report

def _director_lingo_map_report_for_container(
    path: Path,
    relative_path: str,
    resource_map: dict,
    extracted_files: list[dict],
) -> dict | None:
    tag_counts = resource_map.get("tag_counts", {})
    if not any(tag_counts.get(tag, 0) for tag in DIRECTOR_LINGO_CHUNK_IDS):
        return None
    extracted_script_entries = {
        file["container_entry_id"]
        for file in extracted_files
        if file.get("format_probe") == "script_text" and "container_entry_id" in file
    }
    container = _read_director_lingo_map(path, relative_path, extracted_script_entries)
    report = {
        "schema": "tsuinosora.director_lingo_map.v1",
        "status": container.get("status", "blocked"),
        "container_count": 1,
        "context_count": container.get("context_count", 0),
        "name_count": container.get("name_count", 0),
        "script_count": container.get("script_count", 0),
        "unsupported_script_count": container.get("unsupported_script_count", 0),
        "containers": [container],
        "diagnostics": list(container.get("diagnostics", [])),
        "redaction": {
            "paths": "report_relative_only",
            "payload": "omitted",
            "commercial_text": "omitted",
            "lingo_names": "omitted",
            "bytecode": "omitted",
        },
    }
    if _report_has_path_leak(report):
        report["status"] = "blocked"
        report["diagnostics"].append(
            {
                "code": "TSUI_DIRECTOR_LINGO_MAP_REPORT_PATH_LEAK",
                "message": "Director Lingo map report contains a local path-like value",
            }
        )
    return report

def _director_lingo_source_map_from_extracted_scripts(
    *,
    unpacked_root: Path,
    container_id: str,
    lingo_map_report: dict,
    lingo_map_relative_path: str,
    extracted_files: list[dict],
    source_container: str,
) -> dict | None:
    routes = []
    lingo_map_path = unpacked_root / lingo_map_relative_path
    source_hash = _sha256(lingo_map_path) if lingo_map_path.exists() else _sha256_bytes(
        json.dumps(lingo_map_report, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")
    )
    script_files = [
        file
        for file in extracted_files
        if file.get("format_probe") == "script_text"
        and file.get("source_container") == source_container
        and _is_safe_report_relative_path(str(file.get("relative_path", "")))
    ]
    for file in script_files:
        script_path = unpacked_root / str(file["relative_path"])
        if not script_path.exists():
            continue
        for line_no, line in enumerate(_read_text_lossless(script_path).splitlines(), start=1):
            route = _script_route_marker(line)
            if not route:
                continue
            route["source"] = lingo_map_relative_path
            route["line"] = line_no
            route["source_hash"] = source_hash
            routes.append(route)

    if not routes:
        return None

    return {
        "schema": "tsuinosora.script_source_map.v1",
        "reader": {
            "tool_id": "astra.tsui.director_lingo_source_map",
            "tool_hash": _sha256(Path(__file__)),
            "output_contract": "route_source_map",
            "container_id": container_id,
        },
        "sources": [
            {
                "source": lingo_map_relative_path,
                "sha256": source_hash,
                "line_count": 0,
                "script_count": int(lingo_map_report.get("script_count", 0)),
            }
        ],
        "routes": routes,
    }

def _decode_xfir_riff_payload(data: bytes) -> dict | None:
    if len(data) < 20 or data[:4] != b"XFIR":
        return None
    payload_size = struct.unpack("<I", data[4:8])[0]
    payload_start = 8
    payload_end = payload_start + payload_size
    if payload_size < 12 or payload_end != len(data):
        return None
    payload = data[payload_start:payload_end]
    if payload[:4] not in READABLE_RIFF_SIGNATURES:
        return None
    return {
        "data": payload,
        "payload_size": payload_size,
        "decoded_container_format": _fourcc(payload[:4]),
        "decoded_sha256": _sha256_bytes(payload),
    }

def build_director_resource_map_report(root: Path | str) -> dict:
    root = Path(root)
    diagnostics = []
    containers = []
    tag_counts: dict[str, int] = {}
    for path in sorted(p for p in root.rglob("*") if p.is_file() and p.suffix.lower() in DIRECTOR_CONTAINER_EXTS):
        rel = _rel(path, root)
        container = _read_director_resource_map(path, rel)
        containers.append(container)
        for tag, count in container.get("tag_counts", {}).items():
            tag_counts[tag] = tag_counts.get(tag, 0) + count
        if container.get("status") != "pass":
            diagnostics.extend(container.get("diagnostics", []))

    if not containers:
        diagnostics.append(
            {
                "code": "TSUI_DIRECTOR_RESOURCE_MAP_CONTAINER_MISSING",
                "message": "no Director/Shockwave container was found for resource map preflight",
            }
        )

    report = {
        "schema": "tsuinosora.director_resource_map.v1",
        "status": "blocked" if diagnostics else "pass",
        "container_count": len(containers),
        "resource_count": sum(container.get("resource_count", 0) for container in containers),
        "free_resource_count": sum(container.get("free_resource_count", 0) for container in containers),
        "tag_counts": dict(sorted(tag_counts.items())),
        "containers": containers,
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
                "code": "TSUI_DIRECTOR_RESOURCE_MAP_REPORT_PATH_LEAK",
                "message": "Director resource map report contains a local path-like value",
            }
        )
    return report

def build_director_cast_map_report(root: Path | str) -> dict:
    root = Path(root)
    diagnostics = []
    containers = []
    total_members = 0
    for path in sorted(p for p in root.rglob("*") if p.is_file() and p.suffix.lower() in DIRECTOR_CONTAINER_EXTS):
        rel = _rel(path, root)
        container = _read_director_cast_map(path, rel)
        containers.append(container)
        total_members += container.get("member_count", 0)
        if container.get("status") != "pass":
            diagnostics.extend(container.get("diagnostics", []))

    if not containers:
        diagnostics.append(
            {
                "code": "TSUI_DIRECTOR_CAST_MAP_CONTAINER_MISSING",
                "message": "no Director/Shockwave container was found for cast map preflight",
            }
        )
    if containers and total_members == 0:
        diagnostics.append(
            {
                "code": "TSUI_DIRECTOR_CAST_MAP_MEMBER_MISSING",
                "message": "Director KEY*/CAS* preflight did not map any cast member",
            }
        )

    report = {
        "schema": "tsuinosora.director_cast_map.v1",
        "status": "blocked" if diagnostics else "pass",
        "container_count": len(containers),
        "member_count": total_members,
        "containers": containers,
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
                "code": "TSUI_DIRECTOR_CAST_MAP_REPORT_PATH_LEAK",
                "message": "Director cast map report contains a local path-like value",
            }
        )
    return report

def build_director_lingo_map_report(root: Path | str) -> dict:
    root = Path(root)
    diagnostics = []
    containers = []
    totals = {
        "context_count": 0,
        "context_entry_count": 0,
        "name_count": 0,
        "name_entry_count": 0,
        "script_count": 0,
        "unsupported_script_count": 0,
    }
    for path in sorted(p for p in root.rglob("*") if p.is_file() and p.suffix.lower() in DIRECTOR_CONTAINER_EXTS):
        rel = _rel(path, root)
        container = _read_director_lingo_map(path, rel)
        containers.append(container)
        for key in totals:
            totals[key] += int(container.get(key, 0))
        if container.get("status") != "pass":
            diagnostics.extend(container.get("diagnostics", []))

    if not containers:
        diagnostics.append(
            {
                "code": "TSUI_DIRECTOR_LINGO_MAP_CONTAINER_MISSING",
                "message": "no Director/Shockwave container was found for Lingo map preflight",
            }
        )
    if containers and totals["context_count"] + totals["name_count"] + totals["script_count"] == 0:
        diagnostics.append(
            {
                "code": "TSUI_DIRECTOR_LINGO_MAP_RESOURCE_MISSING",
                "message": "Director Lingo map preflight did not find Lctx, Lnam or Lscr resources",
            }
        )

    report = {
        "schema": "tsuinosora.director_lingo_map.v1",
        "status": "blocked" if diagnostics else "pass",
        "container_count": len(containers),
        **totals,
        "containers": containers,
        "diagnostics": diagnostics,
        "redaction": {
            "paths": "report_relative_only",
            "payload": "omitted",
            "commercial_text": "omitted",
            "lingo_names": "omitted",
            "bytecode": "omitted",
        },
    }
    if _report_has_path_leak(report):
        report["status"] = "blocked"
        report["diagnostics"].append(
            {
                "code": "TSUI_DIRECTOR_LINGO_MAP_REPORT_PATH_LEAK",
                "message": "Director Lingo map report contains a local path-like value",
            }
        )
    return report

def _director_cast_map_report_for_container(path: Path, relative_path: str, resource_map: dict) -> dict | None:
    tag_counts = resource_map.get("tag_counts", {})
    if not any(tag_counts.get(tag, 0) for tag in ("KEY*", "CAS*")):
        return None

    container = _read_director_cast_map(path, relative_path)
    report = {
        "schema": "tsuinosora.director_cast_map.v1",
        "status": container.get("status", "blocked"),
        "container_count": 1,
        "member_count": container.get("member_count", 0),
        "containers": [container],
        "diagnostics": list(container.get("diagnostics", [])),
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
                "code": "TSUI_DIRECTOR_CAST_MAP_REPORT_PATH_LEAK",
                "message": "Director cast map report contains a local path-like value",
            }
        )
    return report

def _director_lingo_map_report_for_container(
    path: Path,
    relative_path: str,
    resource_map: dict,
    extracted_files: list[dict],
) -> dict | None:
    tag_counts = resource_map.get("tag_counts", {})
    if not any(tag_counts.get(tag, 0) for tag in DIRECTOR_LINGO_CHUNK_IDS):
        return None
    extracted_script_entries = {
        file["container_entry_id"]
        for file in extracted_files
        if file.get("format_probe") == "script_text" and "container_entry_id" in file
    }
    container = _read_director_lingo_map(path, relative_path, extracted_script_entries)
    report = {
        "schema": "tsuinosora.director_lingo_map.v1",
        "status": container.get("status", "blocked"),
        "container_count": 1,
        "context_count": container.get("context_count", 0),
        "name_count": container.get("name_count", 0),
        "script_count": container.get("script_count", 0),
        "unsupported_script_count": container.get("unsupported_script_count", 0),
        "containers": [container],
        "diagnostics": list(container.get("diagnostics", [])),
        "redaction": {
            "paths": "report_relative_only",
            "payload": "omitted",
            "commercial_text": "omitted",
            "lingo_names": "omitted",
            "bytecode": "omitted",
        },
    }
    if _report_has_path_leak(report):
        report["status"] = "blocked"
        report["diagnostics"].append(
            {
                "code": "TSUI_DIRECTOR_LINGO_MAP_REPORT_PATH_LEAK",
                "message": "Director Lingo map report contains a local path-like value",
            }
        )
    return report

def _director_lingo_source_map_from_extracted_scripts(
    *,
    unpacked_root: Path,
    container_id: str,
    lingo_map_report: dict,
    lingo_map_relative_path: str,
    extracted_files: list[dict],
    source_container: str,
) -> dict | None:
    routes = []
    lingo_map_path = unpacked_root / lingo_map_relative_path
    source_hash = _sha256(lingo_map_path) if lingo_map_path.exists() else _sha256_bytes(
        json.dumps(lingo_map_report, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")
    )
    script_files = [
        file
        for file in extracted_files
        if file.get("format_probe") == "script_text"
        and file.get("source_container") == source_container
        and _is_safe_report_relative_path(str(file.get("relative_path", "")))
    ]
    for file in script_files:
        script_path = unpacked_root / str(file["relative_path"])
        if not script_path.exists():
            continue
        for line_no, line in enumerate(_read_text_lossless(script_path).splitlines(), start=1):
            route = _script_route_marker(line)
            if not route:
                continue
            route["source"] = lingo_map_relative_path
            route["line"] = line_no
            route["source_hash"] = source_hash
            routes.append(route)

    if not routes:
        return None

    return {
        "schema": "tsuinosora.script_source_map.v1",
        "reader": {
            "tool_id": "astra.tsui.director_lingo_source_map",
            "tool_hash": _sha256(Path(__file__)),
            "output_contract": "route_source_map",
            "container_id": container_id,
        },
        "sources": [
            {
                "source": lingo_map_relative_path,
                "sha256": source_hash,
                "line_count": 0,
                "script_count": int(lingo_map_report.get("script_count", 0)),
            }
        ],
        "routes": routes,
    }