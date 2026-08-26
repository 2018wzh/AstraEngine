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
from tsuinosora_diagnostics import _dedupe_diagnostics, _is_safe_symbol, _rel, _write_json
from tsuinosora_projectorrays_convert_bitmap import _build_projectorrays_bitd_bitmap_index, _build_projectorrays_embedded_media_index, _build_projectorrays_sound_index, _convert_projectorrays_bitd_chunk, _projectorrays_metadata_shape, _projectorrays_native_metadata_path, _projectorrays_native_text_path
from tsuinosora_projectorrays_convert_metadata import _convert_projectorrays_cupt_chunk, _convert_projectorrays_empty_sound_placeholder_chunk, _convert_projectorrays_info_entry_chunk, _convert_projectorrays_scrf_chunk
from tsuinosora_projectorrays_lscr import _build_projectorrays_script_source_index, _convert_projectorrays_lscr_chunk
from tsuinosora_projectorrays_media import _convert_projectorrays_fcol_chunk, _convert_projectorrays_fmap_chunk, _convert_projectorrays_fxmp_chunk, _convert_projectorrays_sord_chunk, _convert_projectorrays_vers_chunk, _convert_projectorrays_vwlb_chunk
from tsuinosora_projectorrays_media_audio import _convert_projectorrays_sndh_chunk, _convert_projectorrays_snds_chunk, _convert_projectorrays_vwsc_chunk, _convert_projectorrays_xtrl_chunk
from tsuinosora_projectorrays_media_video import _convert_projectorrays_edim_chunk, _convert_projectorrays_xmed_chunk
from tsuinosora_projectorrays_validate import _projectorrays_chunk_fourcc
from tsuinosora_rendering import _report_has_path_leak

__all__ = ['convert_projectorrays_resources', '_projectorrays_conversion_summary', '_load_projectorrays_palette_sidecars', '_parse_projectorrays_palette_entry', '_convert_projectorrays_binary_chunk', '_convert_projectorrays_stxt_chunk', '_decode_projectorrays_stxt']


def convert_projectorrays_resources(
    work_root: Path | str,
    dump_roots: list[tuple[str, Path]],
    palette_sidecars: list[Path] | None = None,
) -> dict:
    work_root = Path(work_root)
    diagnostics = []
    resources = []
    scanned_binary_count = 0
    palette_index = _load_projectorrays_palette_sidecars(palette_sidecars or [], diagnostics)
    for alias, root in dump_roots:
        if not _is_safe_symbol(alias):
            diagnostics.append(
                {
                    "code": "TSUI_PROJECTORRAYS_CONVERT_ALIAS_INVALID",
                    "alias": alias or "unknown",
                    "message": "ProjectorRays conversion root alias must be a safe symbol",
                }
            )
            continue
        if not root.is_dir():
            diagnostics.append(
                {
                    "code": "TSUI_PROJECTORRAYS_CONVERT_ROOT_MISSING",
                    "alias": alias,
                    "message": "ProjectorRays conversion root is missing or inaccessible",
                }
            )
            continue
        script_index = _build_projectorrays_script_source_index(root)
        bitd_index = _build_projectorrays_bitd_bitmap_index(root)
        sound_index = _build_projectorrays_sound_index(root)
        embedded_media_index = _build_projectorrays_embedded_media_index(root)
        for source in sorted(path for path in root.rglob("*.bin") if path.is_file()):
            scanned_binary_count += 1
            resource = _convert_projectorrays_binary_chunk(
                work_root,
                alias,
                root,
                source,
                diagnostics,
                script_index,
                bitd_index,
                sound_index,
                embedded_media_index,
                palette_index,
            )
            if resource:
                resources.append(resource)
    report = {
        "schema": "tsuinosora.projectorrays_converted_resources.v1",
        "status": "pass" if scanned_binary_count == len(resources) and not diagnostics else "blocked",
        "scanned_binary_count": scanned_binary_count,
        "converted_count": len(resources),
        "resources": resources,
        "diagnostics": _dedupe_diagnostics(diagnostics),
        "redaction": {
            "paths": "work_root_relative_or_dump_relative_only",
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
                "code": "TSUI_PROJECTORRAYS_CONVERT_REPORT_PATH_LEAK",
                "message": "ProjectorRays converted resource report contains a local path-like value",
            }
        )
        report["diagnostics"] = _dedupe_diagnostics(report["diagnostics"])
    _write_json(work_root / "reports" / "projectorrays_converted_resources.json", report)
    return report


def _projectorrays_conversion_summary(report: dict) -> dict:
    converted_by_chunk: dict[str, int] = {}
    diagnostic_by_code: dict[str, int] = {}
    diagnostic_by_chunk: dict[str, int] = {}
    for resource in report.get("resources", []):
        if isinstance(resource, dict):
            chunk = str(resource.get("chunk_fourcc", "unknown"))
            converted_by_chunk[chunk] = converted_by_chunk.get(chunk, 0) + 1
    for diagnostic in report.get("diagnostics", []):
        if isinstance(diagnostic, dict):
            code = str(diagnostic.get("code", "unknown"))
            chunk = str(diagnostic.get("chunk_fourcc", "unknown"))
            diagnostic_by_code[code] = diagnostic_by_code.get(code, 0) + 1
            diagnostic_by_chunk[chunk] = diagnostic_by_chunk.get(chunk, 0) + 1
    return {
        "schema": "tsuinosora.projectorrays_converted_resources.summary.v1",
        "status": report.get("status", "blocked"),
        "scanned_binary_count": report.get("scanned_binary_count", 0),
        "converted_count": report.get("converted_count", 0),
        "diagnostic_count": len(report.get("diagnostics", [])),
        "converted_by_chunk": dict(sorted(converted_by_chunk.items())),
        "diagnostics_by_code": dict(sorted(diagnostic_by_code.items())),
        "diagnostics_by_chunk": dict(sorted(diagnostic_by_chunk.items())),
    }


def _load_projectorrays_palette_sidecars(paths: list[Path], diagnostics: list[dict]) -> dict[int, dict]:
    palette_index: dict[int, dict] = {}
    for ordinal, path in enumerate(paths, start=1):
        sidecar_id = f"palette_sidecar_{ordinal}"
        try:
            value = json.loads(Path(path).read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError, UnicodeDecodeError):
            diagnostics.append(
                {
                    "code": "TSUI_PROJECTORRAYS_PALETTE_SIDECAR_INVALID",
                    "sidecar": sidecar_id,
                    "message": "ProjectorRays palette sidecar must be readable JSON",
                }
            )
            continue
        if not isinstance(value, dict) or value.get("schema") != "tsuinosora.projectorrays_palette_sidecar.v1":
            diagnostics.append(
                {
                    "code": "TSUI_PROJECTORRAYS_PALETTE_SIDECAR_SCHEMA_INVALID",
                    "sidecar": sidecar_id,
                    "message": "ProjectorRays palette sidecar schema is missing or unsupported",
                }
            )
            continue
        palettes = value.get("palettes")
        if not isinstance(palettes, list) or not palettes:
            diagnostics.append(
                {
                    "code": "TSUI_PROJECTORRAYS_PALETTE_SIDECAR_EMPTY",
                    "sidecar": sidecar_id,
                    "message": "ProjectorRays palette sidecar must contain at least one palette entry",
                }
            )
            continue
        sidecar_hash = _sha256(Path(path))
        for index, palette in enumerate(palettes):
            entry_id = f"{sidecar_id}:{index}"
            parsed = _parse_projectorrays_palette_entry(palette, sidecar_hash)
            if parsed is None:
                diagnostics.append(
                    {
                        "code": "TSUI_PROJECTORRAYS_PALETTE_ENTRY_INVALID",
                        "sidecar": sidecar_id,
                        "entry": index,
                        "message": "ProjectorRays palette entry must declare safe id, clut ids and 256 RGB colors",
                    }
                )
                continue
            key = parsed["stored_clut_id"]
            if key in palette_index:
                diagnostics.append(
                    {
                        "code": "TSUI_PROJECTORRAYS_PALETTE_ENTRY_DUPLICATE",
                        "sidecar": sidecar_id,
                        "entry": index,
                        "stored_clut_id": key,
                        "message": "ProjectorRays palette sidecars must not declare the same stored clut id twice",
                    }
                )
                continue
            parsed["entry_id"] = entry_id
            palette_index[key] = parsed
    return palette_index


def _parse_projectorrays_palette_entry(value: object, sidecar_hash: str) -> dict | None:
    if not isinstance(value, dict):
        return None
    palette_id = value.get("id")
    stored_clut_id = value.get("stored_clut_id")
    director_palette_id = value.get("director_palette_id")
    colors = value.get("colors")
    if not isinstance(palette_id, str) or not _is_safe_symbol(palette_id):
        return None
    if not isinstance(stored_clut_id, int) or not isinstance(director_palette_id, int):
        return None
    if not isinstance(colors, list) or len(colors) != 256:
        return None
    parsed_colors: list[tuple[int, int, int]] = []
    for color in colors:
        if (
            not isinstance(color, list)
            or len(color) != 3
            or any(not isinstance(channel, int) or channel < 0 or channel > 255 for channel in color)
        ):
            return None
        parsed_colors.append((color[0], color[1], color[2]))
    return {
        "id": palette_id,
        "stored_clut_id": stored_clut_id,
        "director_palette_id": director_palette_id,
        "colors": tuple(parsed_colors),
        "sidecar_sha256": sidecar_hash,
    }


def _convert_projectorrays_binary_chunk(
    work_root: Path,
    alias: str,
    root: Path,
    source: Path,
    diagnostics: list[dict],
    script_index: dict[tuple[tuple[str, ...], int, str], list[dict]],
    bitd_index: dict[tuple[tuple[str, ...], int], dict],
    sound_index: dict[str, dict],
    embedded_media_index: dict[str, dict],
    palette_index: dict[int, dict],
) -> dict | None:
    source_relative_path = _rel(source, root)
    chunk_fourcc = _projectorrays_chunk_fourcc(source)
    role = PROJECTORRAYS_REQUIRED_CHUNK_ROLES.get(chunk_fourcc, "director_chunk")
    paired_json = source.with_suffix(".json")
    if chunk_fourcc == "BITD":
        return _convert_projectorrays_bitd_chunk(
            work_root,
            alias,
            source,
            source_relative_path,
            bitd_index,
            palette_index,
            diagnostics,
        )
    if chunk_fourcc == "STXT":
        return _convert_projectorrays_stxt_chunk(work_root, alias, source, source_relative_path, diagnostics)
    if chunk_fourcc == "Lscr":
        return _convert_projectorrays_lscr_chunk(
            work_root,
            alias,
            source,
            source_relative_path,
            paired_json,
            script_index,
            diagnostics,
        )
    if chunk_fourcc == "snd ":
        return _convert_projectorrays_empty_sound_placeholder_chunk(
            work_root,
            alias,
            source,
            source_relative_path,
            diagnostics,
        )
    if chunk_fourcc == "cupt":
        return _convert_projectorrays_cupt_chunk(work_root, alias, source, source_relative_path, diagnostics)
    if chunk_fourcc == "SCRF":
        return _convert_projectorrays_scrf_chunk(work_root, alias, source, source_relative_path)
    if chunk_fourcc in {"Cinf", "VWFI"}:
        return _convert_projectorrays_info_entry_chunk(
            work_root,
            alias,
            source,
            source_relative_path,
            chunk_fourcc,
            diagnostics,
        )
    if chunk_fourcc == "Sord":
        return _convert_projectorrays_sord_chunk(work_root, alias, source, source_relative_path, diagnostics)
    if chunk_fourcc == "Fmap":
        return _convert_projectorrays_fmap_chunk(work_root, alias, source, source_relative_path, diagnostics)
    if chunk_fourcc == "VWLB":
        return _convert_projectorrays_vwlb_chunk(work_root, alias, source, source_relative_path, diagnostics)
    if chunk_fourcc == "FCOL":
        return _convert_projectorrays_fcol_chunk(work_root, alias, source, source_relative_path, diagnostics)
    if chunk_fourcc == "FXmp":
        return _convert_projectorrays_fxmp_chunk(work_root, alias, source, source_relative_path, diagnostics)
    if chunk_fourcc == "VERS":
        return _convert_projectorrays_vers_chunk(work_root, alias, source, source_relative_path, diagnostics)
    if chunk_fourcc == "XTRl":
        return _convert_projectorrays_xtrl_chunk(work_root, alias, source, source_relative_path, diagnostics)
    if chunk_fourcc == "sndH":
        return _convert_projectorrays_sndh_chunk(work_root, alias, source, source_relative_path, sound_index, diagnostics)
    if chunk_fourcc == "sndS":
        return _convert_projectorrays_snds_chunk(work_root, alias, source, source_relative_path, sound_index, diagnostics)
    if chunk_fourcc == "VWSC":
        return _convert_projectorrays_vwsc_chunk(work_root, alias, source, source_relative_path, diagnostics)
    if chunk_fourcc == "XMED":
        return _convert_projectorrays_xmed_chunk(work_root, alias, source, source_relative_path, diagnostics)
    if chunk_fourcc == "ediM":
        return _convert_projectorrays_edim_chunk(
            work_root,
            alias,
            source,
            source_relative_path,
            embedded_media_index,
            diagnostics,
        )
    if chunk_fourcc not in PROJECTORRAYS_JSON_METADATA_CHUNKS or not paired_json.is_file():
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERT_UNSUPPORTED_CHUNK",
                "source_alias": alias,
                "source_relative_path": source_relative_path,
                "chunk_fourcc": chunk_fourcc,
                "role": role,
                "message": "ProjectorRays chunk requires a dedicated converter before it can be counted as converted",
            }
        )
        return None
    try:
        metadata_text = paired_json.read_text(encoding="utf-8")
    except UnicodeDecodeError:
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERT_JSON_INVALID",
                "source_alias": alias,
                "source_relative_path": source_relative_path,
                "chunk_fourcc": chunk_fourcc,
                "message": "ProjectorRays paired metadata JSON could not be parsed",
            }
        )
        return None
    try:
        metadata_value = loads_projectorrays_json(metadata_text)
    except json.JSONDecodeError:
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERT_JSON_INVALID",
                "source_alias": alias,
                "source_relative_path": source_relative_path,
                "chunk_fourcc": chunk_fourcc,
                "message": "ProjectorRays paired metadata JSON could not be parsed",
            }
        )
        return None
    metadata_shape = _projectorrays_metadata_shape(metadata_value)
    if metadata_shape is None:
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERT_JSON_SHAPE_INVALID",
                "source_alias": alias,
                "source_relative_path": source_relative_path,
                "chunk_fourcc": chunk_fourcc,
                "message": "ProjectorRays paired metadata JSON must be an object",
            }
        )
        return None
    native_path = _projectorrays_native_metadata_path(alias, source_relative_path)
    native_file = work_root / native_path
    native_payload = {
        "schema": "tsuinosora.projectorrays_converted_chunk.v1",
        "source_alias": alias,
        "source_relative_path": source_relative_path,
        "source_sha256": _sha256(source),
        "chunk_fourcc": chunk_fourcc,
        "role": role,
        "conversion_method": "projectorrays_json_metadata",
        "metadata_shape": metadata_shape,
        "redaction": {
            "paths": "dump_relative_only",
            "payload": "omitted",
            "commercial_text": "omitted",
            "script_text": "omitted",
            "names": "omitted",
        },
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
        "conversion_method": "projectorrays_json_metadata",
        "status": "converted",
    }


def _convert_projectorrays_stxt_chunk(
    work_root: Path,
    alias: str,
    source: Path,
    source_relative_path: str,
    diagnostics: list[dict],
) -> dict | None:
    payload = source.read_bytes()
    decoded = _decode_projectorrays_stxt(payload)
    if decoded is None:
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERT_STXT_INVALID",
                "source_alias": alias,
                "source_relative_path": source_relative_path,
                "chunk_fourcc": "STXT",
                "role": PROJECTORRAYS_REQUIRED_CHUNK_ROLES["STXT"],
                "message": "ProjectorRays STXT chunk did not match the expected header or CP932 text payload",
            }
        )
        return None
    native_path = _projectorrays_native_text_path(alias, source_relative_path)
    native_file = work_root / native_path
    native_file.parent.mkdir(parents=True, exist_ok=True)
    native_file.write_text(decoded, encoding="utf-8")
    return {
        "source_alias": alias,
        "source_relative_path": source_relative_path,
        "source_sha256": _sha256(source),
        "chunk_fourcc": "STXT",
        "role": PROJECTORRAYS_REQUIRED_CHUNK_ROLES["STXT"],
        "native_path": native_path,
        "converted_sha256": _sha256(native_file),
        "byte_size": native_file.stat().st_size,
        "conversion_method": "projectorrays_stxt_cp932_text",
        "status": "converted",
    }


def _decode_projectorrays_stxt(payload: bytes) -> str | None:
    if len(payload) < 12:
        return None
    header_size = int.from_bytes(payload[0:4], "big")
    text_size = int.from_bytes(payload[4:8], "big")
    trailer_size = int.from_bytes(payload[8:12], "big")
    if header_size != 12 or text_size < 0 or trailer_size < 0:
        return None
    if len(payload) != header_size + text_size + trailer_size:
        return None
    text_payload = payload[header_size : header_size + text_size]
    try:
        decoded = text_payload.decode("cp932")
    except UnicodeDecodeError:
        return None
    return decoded.replace("\r\n", "\n").replace("\r", "\n")
