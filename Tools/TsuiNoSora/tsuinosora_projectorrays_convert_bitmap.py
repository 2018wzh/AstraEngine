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
from tsuinosora_diagnostics import _rel
from tsuinosora_projectorrays_lscr import _projectorrays_chunk_scope, _ascii_path_segment, _projectorrays_native_metadata_path, _projectorrays_native_lscr_script_path
from tsuinosora_projectorrays_validate import _projectorrays_chunk_resource_id

__all__ = ['_convert_projectorrays_bitd_chunk', '_build_projectorrays_bitd_bitmap_index', '_build_projectorrays_sound_index', '_build_projectorrays_embedded_media_index', '_parse_projectorrays_cast_member_type', '_parse_projectorrays_key_table', '_projectorrays_director_version_for_scope', '_projectorrays_chunk_path_for_scope', '_parse_projectorrays_bitmap_cast_metadata', '_decode_projectorrays_bitd_rgba', '_decode_projectorrays_packbits', '_rgb555_to_rgba', '_write_rgba_png', '_projectorrays_native_text_path', '_projectorrays_native_bitd_image_path', '_projectorrays_native_audio_path', '_projectorrays_metadata_shape_from_text', '_projectorrays_metadata_shape', '_json_shape_counts']


def _convert_projectorrays_bitd_chunk(
    work_root: Path,
    alias: str,
    source: Path,
    source_relative_path: str,
    bitd_index: dict[tuple[tuple[str, ...], int], dict],
    palette_index: dict[int, dict],
    diagnostics: list[dict],
) -> dict | None:
    role = PROJECTORRAYS_REQUIRED_CHUNK_ROLES["BITD"]
    resource_id = _projectorrays_chunk_resource_id(source)
    scope = _projectorrays_chunk_scope(source_relative_path)
    binding = bitd_index.get((scope, resource_id)) if resource_id is not None else None
    if binding is None:
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERT_BITD_BINDING_MISSING",
                "source_alias": alias,
                "source_relative_path": source_relative_path,
                "chunk_fourcc": "BITD",
                "role": role,
                "message": "ProjectorRays BITD chunk did not resolve to a same-scope bitmap CASt parent",
            }
        )
        return None
    if binding.get("status") == "ambiguous":
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERT_BITD_BINDING_AMBIGUOUS",
                "source_alias": alias,
                "source_relative_path": source_relative_path,
                "chunk_fourcc": "BITD",
                "role": role,
                "candidate_count": binding.get("candidate_count", 0),
                "message": "ProjectorRays BITD binding must resolve to exactly one bitmap CASt parent",
            }
        )
        return None
    if binding.get("status") != "matched":
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERT_BITD_CAST_METADATA_INVALID",
                "source_alias": alias,
                "source_relative_path": source_relative_path,
                "chunk_fourcc": "BITD",
                "role": role,
                "message": "ProjectorRays BITD parent CASt metadata is not a supported bitmap member",
            }
        )
        return None
    metadata = binding["metadata"]
    bpp = metadata["bits_per_pixel"]
    palette = None
    if bpp == 8:
        stored_clut_id = metadata.get("stored_clut_id")
        palette = palette_index.get(stored_clut_id) if isinstance(stored_clut_id, int) else None
        if palette is None:
            diagnostics.append(
                {
                    "code": "TSUI_PROJECTORRAYS_CONVERT_BITD_PALETTE_REQUIRED",
                    "source_alias": alias,
                    "source_relative_path": source_relative_path,
                    "chunk_fourcc": "BITD",
                    "role": role,
                    "bits_per_pixel": bpp,
                    "stored_clut_id": stored_clut_id if isinstance(stored_clut_id, int) else "unknown",
                    "message": "ProjectorRays BITD 8bpp image conversion requires proven palette binding",
                }
            )
            return None
    image = _decode_projectorrays_bitd_rgba(source.read_bytes(), metadata, palette)
    if image is None:
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERT_BITD_DECODE_FAILED",
                "source_alias": alias,
                "source_relative_path": source_relative_path,
                "chunk_fourcc": "BITD",
                "role": role,
                "bits_per_pixel": bpp,
                "message": "ProjectorRays BITD image payload could not be decoded with the supported Director bitmap codec",
            }
        )
        return None
    native_path = _projectorrays_native_bitd_image_path(alias, source_relative_path)
    native_file = work_root / native_path
    _write_rgba_png(native_file, image["width"], image["height"], image["rgba"])
    record = {
        "source_alias": alias,
        "source_relative_path": source_relative_path,
        "source_sha256": _sha256(source),
        "chunk_fourcc": "BITD",
        "role": role,
        "native_path": native_path,
        "converted_sha256": _sha256(native_file),
        "byte_size": native_file.stat().st_size,
        "conversion_method": "projectorrays_bitd_palette_png" if palette else "projectorrays_bitd_rgba_png",
        "cast_resource_id": metadata["cast_resource_id"],
        "cast_source_sha256": metadata["cast_source_sha256"],
        "width": metadata["width"],
        "height": metadata["height"],
        "pitch": metadata["pitch"],
        "bits_per_pixel": bpp,
        "status": "converted",
    }
    if palette:
        record.update(
            {
                "palette_id": palette["id"],
                "stored_clut_id": palette["stored_clut_id"],
                "director_palette_id": palette["director_palette_id"],
                "palette_sidecar_sha256": palette["sidecar_sha256"],
                "palette_color_count": 256,
            }
        )
    return record


def _build_projectorrays_bitd_bitmap_index(root: Path) -> dict[tuple[tuple[str, ...], int], dict]:
    index: dict[tuple[tuple[str, ...], int], dict] = {}
    for key_path in sorted(root.rglob("KEY_-*.bin")):
        scope = _projectorrays_chunk_scope(_rel(key_path, root))
        version = _projectorrays_director_version_for_scope(root, scope)
        for child_id, parent_id, child_tag in _parse_projectorrays_key_table(key_path):
            if child_tag != "BITD":
                continue
            key = (scope, child_id)
            cast_path = _projectorrays_chunk_path_for_scope(root, scope, "CASt", parent_id)
            if cast_path is None or version is None:
                record = {"status": "missing"}
            else:
                metadata = _parse_projectorrays_bitmap_cast_metadata(cast_path, version)
                if metadata is None:
                    record = {"status": "missing"}
                else:
                    metadata["cast_resource_id"] = parent_id
                    metadata["cast_source_sha256"] = _sha256(cast_path)
                    record = {"status": "matched", "metadata": metadata}
            if key in index:
                current = index[key]
                if current.get("status") == "ambiguous":
                    current["candidate_count"] = int(current.get("candidate_count", 2)) + 1
                else:
                    index[key] = {"status": "ambiguous", "candidate_count": 2}
            else:
                index[key] = record
    return index


def _build_projectorrays_sound_index(root: Path) -> dict[str, dict]:
    groups: dict[tuple[tuple[str, ...], int], dict] = {}
    for key_path in sorted(root.rglob("KEY_-*.bin")):
        scope = _projectorrays_chunk_scope(_rel(key_path, root))
        for child_id, parent_id, child_tag in _parse_projectorrays_key_table(key_path):
            if child_tag not in {"sndH", "sndS"}:
                continue
            child_path = _projectorrays_chunk_path_for_scope(root, scope, child_tag, child_id)
            if child_path is None:
                continue
            group = groups.setdefault((scope, parent_id), {"parent_resource_id": parent_id})
            group[child_tag] = {
                "path": child_path,
                "resource_id": child_id,
                "relative_path": _rel(child_path, root),
            }
    index: dict[str, dict] = {}
    for group in groups.values():
        header = group.get("sndH")
        sample = group.get("sndS")
        if not header or not sample:
            for child in (header, sample):
                if child:
                    index[child["relative_path"]] = {"status": "missing_pair"}
            continue
        record = {
            "status": "matched",
            "parent_resource_id": group["parent_resource_id"],
            "header_path": header["path"],
            "header_resource_id": header["resource_id"],
            "header_relative_path": header["relative_path"],
            "sample_path": sample["path"],
            "sample_resource_id": sample["resource_id"],
            "sample_relative_path": sample["relative_path"],
        }
        index[header["relative_path"]] = record
        index[sample["relative_path"]] = record
    return index


def _build_projectorrays_embedded_media_index(root: Path) -> dict[str, dict]:
    index: dict[str, dict] = {}
    for key_path in sorted(root.rglob("KEY_-*.bin")):
        scope = _projectorrays_chunk_scope(_rel(key_path, root))
        for child_id, parent_id, child_tag in _parse_projectorrays_key_table(key_path):
            if child_tag != "ediM":
                continue
            child_path = _projectorrays_chunk_path_for_scope(root, scope, "ediM", child_id)
            parent_path = _projectorrays_chunk_path_for_scope(root, scope, "CASt", parent_id)
            if child_path is None:
                continue
            child_relative_path = _rel(child_path, root)
            parent_member_type = _parse_projectorrays_cast_member_type(parent_path) if parent_path else None
            if parent_member_type is None:
                record = {"status": "missing"}
            else:
                record = {
                    "status": "matched",
                    "parent_resource_id": parent_id,
                    "parent_member_type": parent_member_type,
                }
            if child_relative_path in index:
                current = index[child_relative_path]
                if current.get("status") == "ambiguous":
                    current["candidate_count"] = int(current.get("candidate_count", 2)) + 1
                else:
                    index[child_relative_path] = {"status": "ambiguous", "candidate_count": 2}
            else:
                index[child_relative_path] = record
    return index


def _parse_projectorrays_cast_member_type(path: Path) -> int | None:
    payload = path.read_bytes()
    if len(payload) < 4:
        return None
    member_type = int.from_bytes(payload[0:4], "big")
    return member_type if member_type > 0 else None


def _parse_projectorrays_key_table(path: Path) -> list[tuple[int, int, str]]:
    payload = path.read_bytes()
    if len(payload) < 12:
        return []
    entry_size = int.from_bytes(payload[0:2], "little")
    entry_size2 = int.from_bytes(payload[2:4], "little")
    used_count = int.from_bytes(payload[8:12], "little")
    if entry_size != 12 or entry_size2 != 12:
        return []
    rows = []
    offset = 12
    for _ in range(min(used_count, (len(payload) - 12) // 12)):
        child_id = int.from_bytes(payload[offset : offset + 4], "little")
        parent_id = int.from_bytes(payload[offset + 4 : offset + 8], "little")
        tag_int = int.from_bytes(payload[offset + 8 : offset + 12], "little")
        child_tag = tag_int.to_bytes(4, "big").decode("latin1")
        rows.append((child_id, parent_id, child_tag))
        offset += 12
    return rows


def _projectorrays_director_version_for_scope(root: Path, scope: tuple[str, ...]) -> int | None:
    chunk_dir = root.joinpath(*scope, "chunks")
    if not chunk_dir.is_dir():
        return None
    for path in sorted(chunk_dir.glob("DRCF-*.json")):
        try:
            value = loads_projectorrays_json(path.read_text(encoding="utf-8"))
        except (json.JSONDecodeError, UnicodeDecodeError):
            continue
        if isinstance(value, dict) and isinstance(value.get("directorVersion"), int):
            return value["directorVersion"]
    return None


def _projectorrays_chunk_path_for_scope(root: Path, scope: tuple[str, ...], fourcc: str, resource_id: int) -> Path | None:
    path = root.joinpath(*scope, "chunks", f"{fourcc}-{resource_id}.bin")
    return path if path.is_file() else None


def _parse_projectorrays_bitmap_cast_metadata(path: Path, director_version: int) -> dict | None:
    payload = path.read_bytes()
    if len(payload) < 12:
        return None
    member_type = int.from_bytes(payload[0:4], "big")
    info_len = int.from_bytes(payload[4:8], "big")
    specific_len = int.from_bytes(payload[8:12], "big")
    if member_type != 1:
        return None
    specific_offset = 12 + info_len
    specific = payload[specific_offset : specific_offset + specific_len]
    if len(specific) != specific_len:
        return None
    if director_version < 0x4C2 or director_version >= 0x781:
        return None
    if len(specific) < 23:
        return None
    pitch_raw = int.from_bytes(specific[0:2], "big")
    top = int.from_bytes(specific[2:4], "big", signed=True)
    left = int.from_bytes(specific[4:6], "big", signed=True)
    bottom = int.from_bytes(specific[6:8], "big", signed=True)
    right = int.from_bytes(specific[8:10], "big", signed=True)
    width = right - left
    height = bottom - top
    pitch = pitch_raw & 0x3FFF if pitch_raw & 0x8000 else pitch_raw
    if width <= 0 or height <= 0 or pitch <= 0:
        return None
    bits_per_pixel = 1
    clut_cast_lib = None
    stored_clut_id = None
    director_palette_id = None
    if pitch_raw & 0x8000:
        if len(specific) < 28:
            return None
        bits_per_pixel = specific[23]
        clut_cast_lib = int.from_bytes(specific[24:26], "big", signed=True)
        stored_clut_id = int.from_bytes(specific[26:28], "big", signed=True)
        if stored_clut_id <= 0:
            director_palette_id = stored_clut_id - 1
    if bits_per_pixel not in {1, 8, 16, 32}:
        return None
    min_pitch = (width * bits_per_pixel + 7) // 8
    if pitch < min_pitch:
        return None
    return {
        "width": width,
        "height": height,
        "pitch": pitch,
        "bits_per_pixel": bits_per_pixel,
        "clut_cast_lib": clut_cast_lib,
        "stored_clut_id": stored_clut_id,
        "director_palette_id": director_palette_id,
    }


def _decode_projectorrays_bitd_rgba(payload: bytes, metadata: dict, palette: dict | None = None) -> dict | None:
    width = metadata["width"]
    height = metadata["height"]
    pitch = metadata["pitch"]
    bits_per_pixel = metadata["bits_per_pixel"]
    if bits_per_pixel not in {1, 8, 16, 32}:
        return None
    if bits_per_pixel == 8 and palette is None:
        return None
    bytes_needed = pitch * height
    skip_compression = len(payload) == bytes_needed
    if skip_compression:
        pixels = bytearray(payload)
    else:
        pixels = _decode_projectorrays_packbits(payload)
        if pixels is None:
            return None
    if len(pixels) < bytes_needed:
        pixels.extend(b"\x00" * (bytes_needed - len(pixels)))
    rgba = bytearray(width * height * 4)
    for y in range(height):
        for x in range(width):
            out = (y * width + x) * 4
            if bits_per_pixel == 1:
                source = y * pitch + (x >> 3)
                bit = 7 - (x & 7)
                color = 0xFF if pixels[source] & (1 << bit) else 0x00
                rgba[out : out + 4] = bytes((color, color, color, 0xFF))
            elif bits_per_pixel == 8:
                source = y * pitch + x
                red, green, blue = palette["colors"][pixels[source]]
                rgba[out : out + 4] = bytes((red, green, blue, 0xFF))
            elif bits_per_pixel == 16:
                if skip_compression:
                    source = y * pitch + x * 2
                    color = (pixels[source] << 8) | pixels[source + 1]
                else:
                    line = y * width * 2
                    color = (pixels[line + x] << 8) | pixels[line + width + x]
                rgba[out : out + 4] = _rgb555_to_rgba(color)
            elif bits_per_pixel == 32:
                if skip_compression:
                    source = y * pitch + x * 4
                    red = pixels[source + 1]
                    green = pixels[source + 2]
                    blue = pixels[source + 3]
                else:
                    line = y * width * 4
                    red = pixels[line + width + x]
                    green = pixels[line + 2 * width + x]
                    blue = pixels[line + 3 * width + x]
                rgba[out : out + 4] = bytes((red, green, blue, 0xFF))
    return {"width": width, "height": height, "rgba": bytes(rgba)}


def _decode_projectorrays_packbits(payload: bytes) -> bytearray | None:
    decoded = bytearray()
    offset = 0
    while offset < len(payload):
        value = payload[offset]
        offset += 1
        if value & 0x80:
            run_len = ((value ^ 0xFF) & 0xFF) + 2
            if offset >= len(payload):
                return None
            decoded.extend([payload[offset]] * run_len)
            offset += 1
        else:
            run_len = value + 1
            if offset + run_len > len(payload):
                return None
            decoded.extend(payload[offset : offset + run_len])
            offset += run_len
    return decoded


def _rgb555_to_rgba(color: int) -> bytes:
    red = ((color >> 10) & 0x1F) * 255 // 31
    green = ((color >> 5) & 0x1F) * 255 // 31
    blue = (color & 0x1F) * 255 // 31
    return bytes((red, green, blue, 0xFF))


def _write_rgba_png(path: Path, width: int, height: int, rgba: bytes) -> None:
    if width <= 0 or height <= 0 or len(rgba) != width * height * 4:
        raise ValueError("invalid RGBA PNG payload dimensions")
    path.parent.mkdir(parents=True, exist_ok=True)
    raw = bytearray()
    stride = width * 4
    for y in range(height):
        raw.append(0)
        raw.extend(rgba[y * stride : (y + 1) * stride])

    def chunk(kind: bytes, data: bytes) -> bytes:
        return (
            len(data).to_bytes(4, "big")
            + kind
            + data
            + (zlib.crc32(kind + data) & 0xFFFFFFFF).to_bytes(4, "big")
        )

    ihdr = (
        width.to_bytes(4, "big")
        + height.to_bytes(4, "big")
        + bytes([8, 6, 0, 0, 0])
    )
    path.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(bytes(raw))) + chunk(b"IEND", b""))


def _projectorrays_native_text_path(alias: str, source_relative_path: str) -> str:
    parts = source_relative_path.replace("\\", "/").split("/")
    parts[-1] = Path(parts[-1]).with_suffix(".txt").name
    safe_parts = [_ascii_path_segment(part) for part in parts]
    return "/".join(["native-assets", "projectorrays", _ascii_path_segment(alias), *safe_parts])


def _projectorrays_native_bitd_image_path(alias: str, source_relative_path: str) -> str:
    parts = source_relative_path.replace("\\", "/").split("/")
    parts[-1] = Path(parts[-1]).with_suffix(".png").name
    safe_parts = [_ascii_path_segment(part) for part in parts]
    return "/".join(["native-assets", "projectorrays", _ascii_path_segment(alias), *safe_parts])


def _projectorrays_native_audio_path(alias: str, source_relative_path: str, extension: str = ".wav") -> str:
    parts = source_relative_path.replace("\\", "/").split("/")
    suffix = extension if extension in AUDIO_EXTS else ".wav"
    parts[-1] = Path(parts[-1]).with_suffix(suffix).name
    safe_parts = [_ascii_path_segment(part) for part in parts]
    return "/".join(["native-assets", "projectorrays", _ascii_path_segment(alias), *safe_parts])


def _projectorrays_metadata_shape_from_text(text: str) -> dict | None:
    try:
        value = loads_projectorrays_json(text)
    except json.JSONDecodeError:
        return None
    if not isinstance(value, dict):
        return None
    shape = _projectorrays_metadata_shape(value)
    shape["parse_status"] = "valid_json"
    return shape

def _projectorrays_metadata_shape(value: dict) -> dict:
    counts = _json_shape_counts(value)
    member_type = value.get("type")
    if not isinstance(member_type, int):
        member_type = None
    info = value.get("info")
    member = value.get("member")
    return {
        "top_level_field_count": len(value),
        "numeric_value_count": counts["number"],
        "boolean_value_count": counts["boolean"],
        "string_value_count": counts["string"],
        "array_value_count": counts["array"],
        "object_value_count": counts["object"],
        "member_type": member_type,
        "info_field_count": len(info) if isinstance(info, dict) else 0,
        "member_field_count": len(member) if isinstance(member, dict) else 0,
    }


def _json_shape_counts(value: object) -> dict[str, int]:
    counts = {"number": 0, "boolean": 0, "string": 0, "array": 0, "object": 0}

    def visit(item: object) -> None:
        if isinstance(item, bool):
            counts["boolean"] += 1
        elif isinstance(item, (int, float)):
            counts["number"] += 1
        elif isinstance(item, str):
            counts["string"] += 1
        elif isinstance(item, list):
            counts["array"] += 1
            for child in item:
                visit(child)
        elif isinstance(item, dict):
            counts["object"] += 1
            for child in item.values():
                visit(child)

    visit(value)
    return counts
