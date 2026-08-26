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
from tsuinosora_projectorrays_convert_bitmap import _projectorrays_native_audio_path
from tsuinosora_projectorrays_convert_metadata import _write_projectorrays_metadata_asset

__all__ = ['_convert_projectorrays_xmed_chunk', '_projectorrays_xmed_marker', '_convert_projectorrays_edim_chunk', '_projectorrays_edim_macrz_mp3_stream', '_projectorrays_header_u32', '_projectorrays_header_u16', '_parse_contiguous_mp3_frames', '_parse_mp3_frame_header', '_mp3_bit_rate_kbps', '_parse_projectorrays_edim_macrz_header']


def _convert_projectorrays_xmed_chunk(
    work_root: Path,
    alias: str,
    source: Path,
    source_relative_path: str,
    diagnostics: list[dict],
) -> dict | None:
    payload = source.read_bytes()
    marker = _projectorrays_xmed_marker(payload)
    role = PROJECTORRAYS_REQUIRED_CHUNK_ROLES["XMED"]
    if marker is None:
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERT_XMED_INVALID",
                "source_alias": alias,
                "source_relative_path": source_relative_path,
                "chunk_fourcc": "XMED",
                "role": role,
                "message": "ProjectorRays XMED chunk did not expose a supported metadata marker",
            }
        )
        return None
    printable_count = sum(1 for value in payload if value in (9, 10, 13) or 32 <= value <= 126)
    return _write_projectorrays_metadata_asset(
        work_root,
        alias,
        source,
        source_relative_path,
        "XMED",
        "projectorrays_xmed_metadata",
        {
            "format_marker": marker,
            "byte_size": len(payload),
            "printable_byte_count": printable_count,
            "metadata_hash": _sha256_bytes(payload),
            "redaction": {
                "paths": "dump_relative_only",
                "payload": "omitted",
                "commercial_text": "omitted",
                "xtra_names": "omitted",
                "media_bytes": "omitted",
            },
        },
    )


def _projectorrays_xmed_marker(payload: bytes) -> str | None:
    if payload.startswith(b"PFR1"):
        return "PFR1"
    if payload.startswith(b"FFFF0000"):
        return "FFFF0000"
    return None


def _convert_projectorrays_edim_chunk(
    work_root: Path,
    alias: str,
    source: Path,
    source_relative_path: str,
    embedded_media_index: dict[str, dict],
    diagnostics: list[dict],
) -> dict | None:
    role = PROJECTORRAYS_REQUIRED_CHUNK_ROLES["ediM"]
    binding = embedded_media_index.get(source_relative_path)
    if not binding or binding.get("status") == "missing":
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERT_EDIM_BINDING_MISSING",
                "source_alias": alias,
                "source_relative_path": source_relative_path,
                "chunk_fourcc": "ediM",
                "role": role,
                "message": "ProjectorRays ediM chunk did not resolve to a same-scope sound cast parent",
            }
        )
        return None
    if binding.get("status") == "ambiguous":
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERT_EDIM_BINDING_AMBIGUOUS",
                "source_alias": alias,
                "source_relative_path": source_relative_path,
                "chunk_fourcc": "ediM",
                "role": role,
                "candidate_count": binding.get("candidate_count", 2),
                "message": "ProjectorRays ediM binding must resolve to exactly one same-scope sound cast parent",
            }
        )
        return None
    media = source.read_bytes()
    parsed = _parse_projectorrays_edim_macrz_header(media)
    if parsed is None:
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERT_EDIM_CONTAINER_UNSUPPORTED",
                "source_alias": alias,
                "source_relative_path": source_relative_path,
                "chunk_fourcc": "ediM",
                "role": role,
                "parent_resource_id": binding["parent_resource_id"],
                "parent_member_type": binding["parent_member_type"],
                "message": "ProjectorRays ediM chunk did not expose a supported embedded media container signature",
            }
        )
        return None
    stream = _projectorrays_edim_macrz_mp3_stream(media, parsed)
    if stream is None:
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERT_EDIM_MACRZ_MP3_STREAM_INVALID",
                "source_alias": alias,
                "source_relative_path": source_relative_path,
                "chunk_fourcc": "ediM",
                "role": role,
                "parent_resource_id": binding["parent_resource_id"],
                "parent_member_type": binding["parent_member_type"],
                "codec_marker": parsed["codec_marker"],
                "byte_size": parsed["byte_size"],
                "macrz_signature_offset": parsed["macrz_signature_offset"],
                "header_u32_words": parsed["header_u32_words"],
                "header_u16_words": parsed["header_u16_words"],
                "macrz_guid_sha256": parsed["macrz_guid_sha256"],
                "macrz_body_byte_size": parsed["macrz_body_byte_size"],
                "message": "ProjectorRays ediM MACRZ media did not contain a verified contiguous MP3 stream",
            }
        )
        return None
    native_path = _projectorrays_native_audio_path(alias, source_relative_path, ".mp3")
    native_file = work_root / native_path
    native_file.parent.mkdir(parents=True, exist_ok=True)
    stream_bytes = media[stream["offset"] :]
    native_file.write_bytes(stream_bytes)
    return {
        "source_alias": alias,
        "source_relative_path": source_relative_path,
        "source_sha256": _sha256(source),
        "chunk_fourcc": "ediM",
        "role": role,
        "native_path": native_path,
        "converted_sha256": _sha256(native_file),
        "byte_size": native_file.stat().st_size,
        "conversion_method": "projectorrays_edim_macrz_mp3_extract",
        "parent_resource_id": binding["parent_resource_id"],
        "parent_member_type": binding["parent_member_type"],
        "codec_marker": parsed["codec_marker"],
        "macrz_signature_offset": parsed["macrz_signature_offset"],
        "macrz_guid_sha256": parsed["macrz_guid_sha256"],
        "media_codec": "mp3",
        "media_stream_offset": stream["offset"],
        "media_stream_byte_size": len(stream_bytes),
        "media_stream_sha256": _sha256_bytes(stream_bytes),
        "frame_count": stream["frame_count"],
        "sample_rate": stream["sample_rate"],
        "bitrate_kbps": stream["bitrate_kbps"],
        "channel_count": stream["channel_count"],
        "mpeg_version": stream["mpeg_version"],
        "mpeg_layer": stream["mpeg_layer"],
        "status": "converted",
    }


def _projectorrays_edim_macrz_mp3_stream(payload: bytes, parsed: dict) -> dict | None:
    expected_sample_rate = _projectorrays_header_u32(parsed, 2)
    expected_bit_rate = _projectorrays_header_u32(parsed, 3)
    expected_channel_count = _projectorrays_header_u16(parsed, 1)
    scan_start = int(parsed["macrz_signature_offset"]) + len("MACRZ") + 16
    scan_end = min(len(payload) - 4, scan_start + 4096)
    for offset in range(scan_start, scan_end):
        if _parse_mp3_frame_header(payload, offset) is None:
            continue
        chain = _parse_contiguous_mp3_frames(payload, offset)
        if chain is None:
            continue
        if expected_sample_rate and chain["sample_rate"] != expected_sample_rate:
            continue
        if expected_bit_rate and chain["bitrate_kbps"] * 1000 != expected_bit_rate:
            continue
        if expected_channel_count and chain["channel_count"] != expected_channel_count:
            continue
        return chain
    return None


def _projectorrays_header_u32(parsed: dict, index: int) -> int | None:
    words = parsed.get("header_u32_words")
    if not isinstance(words, list) or index >= len(words):
        return None
    value = words[index]
    return value if isinstance(value, int) and value > 0 else None


def _projectorrays_header_u16(parsed: dict, index: int) -> int | None:
    words = parsed.get("header_u16_words")
    if not isinstance(words, list) or index >= len(words):
        return None
    value = words[index]
    return value if isinstance(value, int) and value > 0 else None


def _parse_contiguous_mp3_frames(payload: bytes, offset: int) -> dict | None:
    cursor = offset
    frame_count = 0
    first: dict | None = None
    while cursor + 4 <= len(payload):
        frame = _parse_mp3_frame_header(payload, cursor)
        if frame is None or cursor + frame["frame_length"] > len(payload):
            return None
        if first is None:
            first = frame
        elif (
            frame["mpeg_version"] != first["mpeg_version"]
            or frame["mpeg_layer"] != first["mpeg_layer"]
            or frame["sample_rate"] != first["sample_rate"]
            or frame["bitrate_kbps"] != first["bitrate_kbps"]
            or frame["channel_count"] != first["channel_count"]
        ):
            return None
        frame_count += 1
        cursor += frame["frame_length"]
    if first is None or cursor != len(payload) or frame_count < 2:
        return None
    return {
        "offset": offset,
        "frame_count": frame_count,
        "sample_rate": first["sample_rate"],
        "bitrate_kbps": first["bitrate_kbps"],
        "channel_count": first["channel_count"],
        "mpeg_version": first["mpeg_version"],
        "mpeg_layer": first["mpeg_layer"],
    }


def _parse_mp3_frame_header(payload: bytes, offset: int) -> dict | None:
    if offset < 0 or offset + 4 > len(payload):
        return None
    word = int.from_bytes(payload[offset : offset + 4], "big")
    if (word & 0xFFE00000) != 0xFFE00000:
        return None
    version_bits = (word >> 19) & 0x03
    layer_bits = (word >> 17) & 0x03
    bit_rate_index = (word >> 12) & 0x0F
    sample_rate_index = (word >> 10) & 0x03
    padding = (word >> 9) & 0x01
    if version_bits == 0x01 or layer_bits == 0 or bit_rate_index in {0, 0x0F} or sample_rate_index == 0x03:
        return None
    mpeg_version = {0x03: "mpeg1", 0x02: "mpeg2", 0x00: "mpeg25"}[version_bits]
    mpeg_layer = {0x03: "layer1", 0x02: "layer2", 0x01: "layer3"}[layer_bits]
    sample_rate = {
        "mpeg1": [44100, 48000, 32000],
        "mpeg2": [22050, 24000, 16000],
        "mpeg25": [11025, 12000, 8000],
    }[mpeg_version][sample_rate_index]
    bit_rate = _mp3_bit_rate_kbps(mpeg_version, mpeg_layer, bit_rate_index)
    if bit_rate is None:
        return None
    if mpeg_layer == "layer1":
        frame_length = ((12000 * bit_rate) // sample_rate + padding) * 4
    elif mpeg_layer == "layer3" and mpeg_version != "mpeg1":
        frame_length = (72000 * bit_rate) // sample_rate + padding
    else:
        frame_length = (144000 * bit_rate) // sample_rate + padding
    if frame_length < 4:
        return None
    channel_mode = (word >> 6) & 0x03
    return {
        "mpeg_version": mpeg_version,
        "mpeg_layer": mpeg_layer,
        "bitrate_kbps": bit_rate,
        "sample_rate": sample_rate,
        "frame_length": frame_length,
        "channel_count": 1 if channel_mode == 0x03 else 2,
    }


def _mp3_bit_rate_kbps(mpeg_version: str, mpeg_layer: str, index: int) -> int | None:
    tables = {
        ("mpeg1", "layer1"): [None, 32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448, None],
        ("mpeg1", "layer2"): [None, 32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384, None],
        ("mpeg1", "layer3"): [None, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, None],
        ("mpeg2", "layer1"): [None, 32, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 224, 256, None],
        ("mpeg2", "layer2"): [None, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, None],
        ("mpeg2", "layer3"): [None, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, None],
        ("mpeg25", "layer1"): [None, 32, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 224, 256, None],
        ("mpeg25", "layer2"): [None, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, None],
        ("mpeg25", "layer3"): [None, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, None],
    }
    table = tables.get((mpeg_version, mpeg_layer))
    if table is None or index >= len(table):
        return None
    return table[index]


def _parse_projectorrays_edim_macrz_header(payload: bytes) -> dict | None:
    marker = b"MACRZ"
    signature_offset = payload.find(marker)
    if signature_offset < 0 or signature_offset < 4 or signature_offset + len(marker) > len(payload):
        return None
    header_u16_start = max(signature_offset - 4, 0)
    aligned_u32_end = header_u16_start - (header_u16_start % 4)
    header_u32_words = [
        int.from_bytes(payload[offset : offset + 4], "big")
        for offset in range(0, aligned_u32_end, 4)
    ]
    header_u16_words = [
        int.from_bytes(payload[offset : offset + 2], "big")
        for offset in range(aligned_u32_end, signature_offset, 2)
        if offset + 2 <= signature_offset
    ]
    guid_start = signature_offset + len(marker)
    guid_end = min(guid_start + 16, len(payload))
    return {
        "codec_marker": "MACRZ",
        "byte_size": len(payload),
        "macrz_signature_offset": signature_offset,
        "header_u32_words": header_u32_words,
        "header_u16_words": header_u16_words,
        "macrz_guid_sha256": _sha256_bytes(payload[guid_start:guid_end]),
        "macrz_body_byte_size": len(payload) - signature_offset,
    }
