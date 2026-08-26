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
from director_score import DirectorScoreError, decode_director_v7_score
from tsuinosora_projectorrays_convert_bitmap import _projectorrays_native_audio_path
from tsuinosora_projectorrays_convert_metadata import _write_projectorrays_metadata_asset
from tsuinosora_projectorrays_media_video import _projectorrays_xmed_marker

__all__ = ['_convert_projectorrays_xtrl_chunk', '_parse_projectorrays_xtrl', '_convert_projectorrays_sndh_chunk', '_convert_projectorrays_snds_chunk', '_projectorrays_sound_binding_diagnostic', '_projectorrays_sound_header_diagnostic', '_parse_projectorrays_moa_sound_header', '_projectorrays_moa_pcm_to_wav', '_convert_projectorrays_vwsc_chunk', '_parse_projectorrays_vwsc', '_convert_projectorrays_xmed_chunk']


def _convert_projectorrays_xtrl_chunk(
    work_root: Path,
    alias: str,
    source: Path,
    source_relative_path: str,
    diagnostics: list[dict],
) -> dict | None:
    parsed = _parse_projectorrays_xtrl(source.read_bytes())
    role = PROJECTORRAYS_REQUIRED_CHUNK_ROLES["XTRl"]
    if parsed is None:
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERT_XTRL_INVALID",
                "source_alias": alias,
                "source_relative_path": source_relative_path,
                "chunk_fourcc": "XTRl",
                "role": role,
                "message": "ProjectorRays XTRl chunk did not match the supported length-prefixed Xtra list layout",
            }
        )
        return None
    return _write_projectorrays_metadata_asset(
        work_root,
        alias,
        source,
        source_relative_path,
        "XTRl",
        "projectorrays_xtra_list_metadata",
        {
            "format_version": parsed["format_version"],
            "declared_entry_count": parsed["declared_entry_count"],
            "record_count": parsed["record_count"],
            "record_sizes": parsed["record_sizes"],
            "record_hashes": parsed["record_hashes"],
            "redaction": {
                "paths": "dump_relative_only",
                "payload": "omitted",
                "commercial_text": "omitted",
                "xtra_names": "omitted",
            },
        },
    )


def _parse_projectorrays_xtrl(payload: bytes) -> dict | None:
    if len(payload) < 8:
        return None
    format_version = int.from_bytes(payload[0:4], "big")
    declared_entry_count = int.from_bytes(payload[4:8], "big")
    if declared_entry_count <= 0:
        return None
    offset = 8
    record_sizes = []
    record_hashes = []
    for _ in range(declared_entry_count):
        if offset + 4 > len(payload):
            return None
        record_size = int.from_bytes(payload[offset : offset + 4], "big")
        record_start = offset + 4
        record_end = record_start + record_size
        if record_size <= 0 or record_end > len(payload):
            return None
        record = payload[record_start:record_end]
        record_sizes.append(record_size)
        record_hashes.append(_sha256_bytes(record))
        offset = record_end
    if offset != len(payload):
        return None
    return {
        "format_version": format_version,
        "declared_entry_count": declared_entry_count,
        "record_count": len(record_sizes),
        "record_sizes": record_sizes,
        "record_hashes": record_hashes,
    }


def _convert_projectorrays_sndh_chunk(
    work_root: Path,
    alias: str,
    source: Path,
    source_relative_path: str,
    sound_index: dict[str, dict],
    diagnostics: list[dict],
) -> dict | None:
    role = PROJECTORRAYS_REQUIRED_CHUNK_ROLES["sndH"]
    binding = sound_index.get(source_relative_path)
    if not binding or binding.get("status") != "matched":
        diagnostics.append(_projectorrays_sound_binding_diagnostic(alias, source_relative_path, "sndH", role))
        return None
    header = _parse_projectorrays_moa_sound_header(source.read_bytes())
    if header is None:
        diagnostics.append(_projectorrays_sound_header_diagnostic(alias, source_relative_path, "sndH", role))
        return None
    sample_path = binding["sample_path"]
    sample_size = sample_path.stat().st_size
    if sample_size != header["sample_byte_size"]:
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERT_SOUND_SAMPLE_SIZE_MISMATCH",
                "source_alias": alias,
                "source_relative_path": source_relative_path,
                "chunk_fourcc": "sndH",
                "role": role,
                "declared_byte_size": header["sample_byte_size"],
                "actual_byte_size": sample_size,
                "message": "ProjectorRays sndH declared sample size must match the bound sndS chunk",
            }
        )
        return None
    return _write_projectorrays_metadata_asset(
        work_root,
        alias,
        source,
        source_relative_path,
        "sndH",
        "projectorrays_moa_sound_header",
        {
            "parent_resource_id": binding["parent_resource_id"],
            "sample_resource_id": binding["sample_resource_id"],
            "sample_source_relative_path": binding["sample_relative_path"],
            "sample_source_sha256": _sha256(sample_path),
            "sample_byte_size": sample_size,
            "sample_rate": header["sample_rate"],
            "bits_per_sample": header["bits_per_sample"],
            "bytes_per_sample": header["bytes_per_sample"],
            "channel_count": header["channel_count"],
            "bytes_per_frame": header["bytes_per_frame"],
            "frame_count": header["frame_count"],
            "redaction": {
                "paths": "dump_relative_only",
                "payload": "omitted",
                "audio": "omitted",
                "commercial_text": "omitted",
            },
        },
    )


def _convert_projectorrays_snds_chunk(
    work_root: Path,
    alias: str,
    source: Path,
    source_relative_path: str,
    sound_index: dict[str, dict],
    diagnostics: list[dict],
) -> dict | None:
    role = PROJECTORRAYS_REQUIRED_CHUNK_ROLES["sndS"]
    binding = sound_index.get(source_relative_path)
    if not binding or binding.get("status") != "matched":
        diagnostics.append(_projectorrays_sound_binding_diagnostic(alias, source_relative_path, "sndS", role))
        return None
    header_path = binding["header_path"]
    header = _parse_projectorrays_moa_sound_header(header_path.read_bytes())
    if header is None:
        diagnostics.append(_projectorrays_sound_header_diagnostic(alias, source_relative_path, "sndS", role))
        return None
    sample = source.read_bytes()
    if len(sample) != header["sample_byte_size"]:
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERT_SOUND_SAMPLE_SIZE_MISMATCH",
                "source_alias": alias,
                "source_relative_path": source_relative_path,
                "chunk_fourcc": "sndS",
                "role": role,
                "declared_byte_size": header["sample_byte_size"],
                "actual_byte_size": len(sample),
                "message": "ProjectorRays sndS byte size must match the bound sndH declaration",
            }
        )
        return None
    wav = _projectorrays_moa_pcm_to_wav(sample, header)
    if wav is None:
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERT_SOUND_PCM_UNSUPPORTED",
                "source_alias": alias,
                "source_relative_path": source_relative_path,
                "chunk_fourcc": "sndS",
                "role": role,
                "message": "ProjectorRays sndS PCM payload did not match the supported WAV conversion layout",
            }
        )
        return None
    native_path = _projectorrays_native_audio_path(alias, source_relative_path)
    native_file = work_root / native_path
    native_file.parent.mkdir(parents=True, exist_ok=True)
    native_file.write_bytes(wav)
    return {
        "source_alias": alias,
        "source_relative_path": source_relative_path,
        "source_sha256": _sha256(source),
        "chunk_fourcc": "sndS",
        "role": role,
        "native_path": native_path,
        "converted_sha256": _sha256(native_file),
        "byte_size": native_file.stat().st_size,
        "conversion_method": "projectorrays_moa_pcm_wav",
        "parent_resource_id": binding["parent_resource_id"],
        "header_resource_id": binding["header_resource_id"],
        "header_source_sha256": _sha256(header_path),
        "sample_rate": header["sample_rate"],
        "bits_per_sample": header["bits_per_sample"],
        "channel_count": header["channel_count"],
        "frame_count": header["frame_count"],
        "status": "converted",
    }


def _projectorrays_sound_binding_diagnostic(alias: str, source_relative_path: str, chunk_fourcc: str, role: str) -> dict:
    return {
        "code": "TSUI_PROJECTORRAYS_CONVERT_SOUND_BINDING_MISSING",
        "source_alias": alias,
        "source_relative_path": source_relative_path,
        "chunk_fourcc": chunk_fourcc,
        "role": role,
        "message": "ProjectorRays sound chunk must be bound through KEY_ to a matching sndH/sndS pair",
    }


def _projectorrays_sound_header_diagnostic(alias: str, source_relative_path: str, chunk_fourcc: str, role: str) -> dict:
    return {
        "code": "TSUI_PROJECTORRAYS_CONVERT_SOUND_HEADER_INVALID",
        "source_alias": alias,
        "source_relative_path": source_relative_path,
        "chunk_fourcc": chunk_fourcc,
        "role": role,
        "message": "ProjectorRays sndH chunk did not match the supported Moa PCM header layout",
    }


def _parse_projectorrays_moa_sound_header(payload: bytes) -> dict | None:
    if len(payload) != 100:
        return None
    fields = [int.from_bytes(payload[index : index + 4], "big", signed=True) for index in range(0, 52, 4)]
    compression_type = payload[52:68]
    bits_per_sample = int.from_bytes(payload[68:72], "big", signed=True)
    bytes_per_sample = int.from_bytes(payload[72:76], "big", signed=True)
    channel_count = int.from_bytes(payload[76:80], "big", signed=True)
    bytes_per_frame = int.from_bytes(payload[80:84], "big", signed=True)
    sound_header_type = payload[84:100]
    sample_byte_size = fields[1]
    playback_end = fields[8]
    frame_count = fields[10]
    sample_rate = fields[11]
    byte_rate = fields[12]
    if any(value != 0 for value in compression_type):
        return None
    if bits_per_sample not in {8, 16} or channel_count not in {1, 2}:
        return None
    if bytes_per_sample != max(1, bits_per_sample // 8):
        return None
    if bytes_per_frame != bytes_per_sample * channel_count:
        return None
    if sample_byte_size <= 0 or sample_byte_size % bytes_per_frame != 0:
        return None
    if playback_end not in {0, sample_byte_size}:
        return None
    if frame_count not in {0, sample_byte_size // bytes_per_frame}:
        return None
    if sample_rate <= 0 or byte_rate != sample_rate * bytes_per_frame:
        return None
    return {
        "sample_byte_size": sample_byte_size,
        "sample_rate": sample_rate,
        "byte_rate": byte_rate,
        "bits_per_sample": bits_per_sample,
        "bytes_per_sample": bytes_per_sample,
        "channel_count": channel_count,
        "bytes_per_frame": bytes_per_frame,
        "frame_count": sample_byte_size // bytes_per_frame,
        "sound_header_type_hash": _sha256_bytes(sound_header_type),
    }


def _projectorrays_moa_pcm_to_wav(sample: bytes, header: dict) -> bytes | None:
    bits_per_sample = header["bits_per_sample"]
    channel_count = header["channel_count"]
    sample_rate = header["sample_rate"]
    bytes_per_frame = header["bytes_per_frame"]
    if len(sample) % bytes_per_frame != 0:
        return None
    if bits_per_sample == 16:
        if len(sample) % 2 != 0:
            return None
        pcm = bytearray()
        for index in range(0, len(sample), 2):
            pcm.extend((sample[index + 1], sample[index]))
        pcm_data = bytes(pcm)
    elif bits_per_sample == 8:
        pcm_data = sample
    else:
        return None
    byte_rate = sample_rate * bytes_per_frame
    data_size = len(pcm_data)
    fmt_chunk = (
        (16).to_bytes(4, "little")
        + (1).to_bytes(2, "little")
        + channel_count.to_bytes(2, "little")
        + sample_rate.to_bytes(4, "little")
        + byte_rate.to_bytes(4, "little")
        + bytes_per_frame.to_bytes(2, "little")
        + bits_per_sample.to_bytes(2, "little")
    )
    return (
        b"RIFF"
        + (36 + data_size).to_bytes(4, "little")
        + b"WAVE"
        + b"fmt "
        + fmt_chunk
        + b"data"
        + data_size.to_bytes(4, "little")
        + pcm_data
    )


def _convert_projectorrays_vwsc_chunk(
    work_root: Path,
    alias: str,
    source: Path,
    source_relative_path: str,
    diagnostics: list[dict],
) -> dict | None:
    payload = source.read_bytes()
    parsed = _parse_projectorrays_vwsc(payload)
    role = PROJECTORRAYS_REQUIRED_CHUNK_ROLES["VWSC"]
    if parsed is None:
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERT_VWSC_INVALID",
                "source_alias": alias,
                "source_relative_path": source_relative_path,
                "chunk_fourcc": "VWSC",
                "role": role,
                "message": "ProjectorRays VWSC chunk did not match the supported Director 6 score metadata layout",
            }
        )
        return None
    try:
        score_ir = decode_director_v7_score(payload)
    except DirectorScoreError as exc:
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERT_VWSC_FRAME_DECODE_FAILED",
                "source_alias": alias,
                "source_relative_path": source_relative_path,
                "chunk_fourcc": "VWSC",
                "role": role,
                "message": str(exc),
            }
        )
        return None
    return _write_projectorrays_metadata_asset(
        work_root,
        alias,
        source,
        source_relative_path,
        "VWSC",
        "projectorrays_vwsc_score_metadata",
        {
            "score_version_marker": parsed["score_version_marker"],
            "detail_entry_count": parsed["detail_entry_count"],
            "index_entry_count": parsed["index_entry_count"],
            "max_detail_byte_size": parsed["max_detail_byte_size"],
            "frame_data_offset": parsed["frame_data_offset"],
            "zero_size_detail_count": parsed["zero_size_detail_count"],
            "score_header": parsed["score_header"],
            "score_ir": score_ir,
            "index_hash": parsed["index_hash"],
            "detail_section_hash": parsed["detail_section_hash"],
            "redaction": {
                "paths": "dump_relative_only",
                "payload": "omitted",
                "commercial_text": "omitted",
                "frame_bytes": "omitted",
                "sprite_detail_bytes": "omitted",
            },
        },
    )


def _parse_projectorrays_vwsc(payload: bytes) -> dict | None:
    if len(payload) < 36:
        return None
    frames_stream_size = int.from_bytes(payload[0:4], "big")
    score_version_marker = int.from_bytes(payload[4:8], "big", signed=True)
    list_start = int.from_bytes(payload[8:12], "big")
    if frames_stream_size != len(payload) or score_version_marker != -3:
        return None
    if list_start < 12 or list_start + 12 > len(payload):
        return None
    detail_entry_count = int.from_bytes(payload[list_start : list_start + 4], "big")
    index_entry_count = int.from_bytes(payload[list_start + 4 : list_start + 8], "big")
    max_detail_byte_size = int.from_bytes(payload[list_start + 8 : list_start + 12], "big")
    if detail_entry_count <= 0 or index_entry_count < detail_entry_count:
        return None
    index_start = list_start + 12
    index_end = index_start + index_entry_count * 4
    if index_end > len(payload):
        return None
    offsets = [
        int.from_bytes(payload[index_start + index * 4 : index_start + (index + 1) * 4], "big")
        for index in range(index_entry_count)
    ]
    frame_data_offset = index_end
    detail_section = payload[frame_data_offset:]
    if any(offset < 0 or offset > len(detail_section) for offset in offsets):
        return None
    if offsets[0] != 0:
        return None
    zero_size_detail_count = 0
    for left, right in zip(offsets, offsets[1:]):
        if right < left:
            return None
        if right == left:
            zero_size_detail_count += 1
    header_offset = frame_data_offset + offsets[0]
    if header_offset + 20 > len(payload):
        return None
    score_header = {
        "frames_stream_size": int.from_bytes(payload[header_offset : header_offset + 4], "big"),
        "frame1_offset": int.from_bytes(payload[header_offset + 4 : header_offset + 8], "big"),
        "num_frames": int.from_bytes(payload[header_offset + 8 : header_offset + 12], "big"),
        "frames_version": int.from_bytes(payload[header_offset + 12 : header_offset + 14], "big"),
        "sprite_record_size": int.from_bytes(payload[header_offset + 14 : header_offset + 16], "big"),
        "num_channels": int.from_bytes(payload[header_offset + 16 : header_offset + 18], "big"),
        "displayed_or_reserved_channels": int.from_bytes(payload[header_offset + 18 : header_offset + 20], "big"),
    }
    if score_header["frames_stream_size"] <= 0 or score_header["num_frames"] <= 0:
        return None
    return {
        "score_version_marker": score_version_marker,
        "detail_entry_count": detail_entry_count,
        "index_entry_count": index_entry_count,
        "max_detail_byte_size": max_detail_byte_size,
        "frame_data_offset": frame_data_offset,
        "zero_size_detail_count": zero_size_detail_count,
        "score_header": score_header,
        "index_hash": _sha256_bytes(payload[index_start:index_end]),
        "detail_section_hash": _sha256_bytes(detail_section),
    }


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
