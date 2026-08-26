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
from tsuinosora_diagnostics import _rel, _reference_hashes, _is_safe_symbol, _is_safe_report_relative_path, _is_sanitized_sha256, _positive_int, _nonnegative_int, _write_json, _blocked_asset_analysis, _classification_counts, _duplicate_hash_groups, _source_root_diagnostics, _empty_inventory, _dedupe_diagnostics
from tsuinosora_rendering import _read_json, _report_has_path_leak
from tsuinosora_visual_analysis import build_source_inventory
from tsuinosora_director_core import build_director_resource_map_report, build_director_cast_map_report, build_director_lingo_map_report
from tsuinosora_director_core import extract_readable_assets
from tsuinosora_cast_source_map import build_cast_source_map_report
from tsuinosora_route_graph import analyze_assets
from tsuinosora_native_conversion import build_conversion_report, rearrange_native_assets
from tsuinosora_projectorrays_convert import convert_projectorrays_resources
from tsuinosora_projectorrays_reader import import_projectorrays_reader
from tsuinosora_projectorrays_validate import _projectorrays_chunk_fourcc, _projectorrays_converted_resource_evidence

__all__ = ['_extract_diagnostics_after_external_reader', '_external_reader_satisfies_director_preflight', '_projectorrays_converted_resources_available', '_read_projectorrays_converted_resources_report', '_projectorrays_converted_asset_reports', '_projectorrays_asset_analysis_record', '_run_projectorrays_from_demo_config', 'build_projectorrays_full_dump_report']


def _extract_diagnostics_after_external_reader(diagnostics: list[dict], external_reader_report: dict | None) -> list[dict]:
    if not _external_reader_satisfies_director_preflight(external_reader_report):
        return list(diagnostics)
    reader_covered_codes = {
        "TSUI_EXTRACT_CONTAINER_UNRECOGNIZED",
        "TSUI_EXTRACT_DIRECTOR_READER_REQUIRED",
    }
    return [
        diagnostic
        for diagnostic in diagnostics
        if diagnostic.get("code") not in reader_covered_codes
    ]


def _external_reader_satisfies_director_preflight(report: dict | None) -> bool:
    return (
        isinstance(report, dict)
        and report.get("schema") == "tsuinosora.projectorrays_reader_report.v1"
        and report.get("status") == "pass"
        and _nonnegative_int(report.get("source_count", 0)) > 0
        and _nonnegative_int(report.get("route_count", 0)) > 0
        and bool(report.get("source_map"))
    )


def _projectorrays_converted_resources_available(work_root: Path | str) -> bool:
    report = _read_projectorrays_converted_resources_report(Path(work_root))
    return bool(
        isinstance(report, dict)
        and report.get("status") == "pass"
        and isinstance(report.get("resources"), list)
        and len(report.get("resources", [])) > 0
    )


def _read_projectorrays_converted_resources_report(work_root: Path) -> dict | None:
    path = work_root / "reports" / "projectorrays_converted_resources.json"
    try:
        report = _read_json(path)
    except (OSError, json.JSONDecodeError, UnicodeDecodeError):
        return None
    if not isinstance(report, dict) or report.get("schema") != "tsuinosora.projectorrays_converted_resources.v1":
        return None
    return report


def _projectorrays_converted_asset_reports(
    work_root: Path,
    reference_report: dict | None,
    external_reader_report: dict | None,
) -> tuple[dict | None, dict | None, dict | None, list[dict]]:
    if not _external_reader_satisfies_director_preflight(external_reader_report):
        return None, None, None, []
    report = _read_projectorrays_converted_resources_report(work_root)
    if report is None:
        return None, None, None, []
    diagnostics = []
    if report.get("status") != "pass":
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERTED_ASSETS_BLOCKED",
                "message": "ProjectorRays converted native asset evidence must pass before it can feed Stage 3",
            }
        )
        return None, None, None, diagnostics
    raw_resources = report.get("resources", [])
    if not isinstance(raw_resources, list) or not raw_resources:
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONVERTED_ASSETS_MISSING",
                "message": "ProjectorRays converted native asset evidence must include at least one resource",
            }
        )
        return None, None, None, diagnostics
    assets = []
    native_resources = []
    members = []
    for index, raw in enumerate(raw_resources):
        if not isinstance(raw, dict):
            diagnostics.append(
                {
                    "code": "TSUI_PROJECTORRAYS_CONVERTED_ASSET_INVALID",
                    "index": index,
                    "message": "ProjectorRays converted native asset entry must be an object",
                }
            )
            continue
        native_path = str(raw.get("native_path", "")).strip()
        source_alias = str(raw.get("source_alias", "")).strip()
        source_relative_path = str(raw.get("source_relative_path", "")).strip()
        source_sha256 = str(raw.get("source_sha256", "")).strip()
        converted_sha256 = str(raw.get("converted_sha256", "")).strip()
        byte_size = _positive_int(raw.get("byte_size", 0))
        role = str(raw.get("role", "")).strip()
        chunk_fourcc = str(raw.get("chunk_fourcc", "")).strip()
        source_key = f"{source_alias}/{source_relative_path}"
        if (
            not _is_safe_report_relative_path(native_path)
            or not native_path.startswith("native-assets/")
            or not _is_safe_symbol(source_alias)
            or not _is_safe_report_relative_path(source_relative_path)
            or not _is_sanitized_sha256(source_sha256)
            or not _is_sanitized_sha256(converted_sha256)
            or byte_size <= 0
            or not _is_safe_report_relative_path(source_key)
        ):
            diagnostics.append(
                {
                    "code": "TSUI_PROJECTORRAYS_CONVERTED_ASSET_EVIDENCE_INVALID",
                    "index": index,
                    "message": "ProjectorRays converted native asset evidence must use safe relative paths, hashes and byte size",
                }
            )
            continue
        native_file = work_root / native_path
        if not native_file.is_file():
            diagnostics.append(
                {
                    "code": "TSUI_PROJECTORRAYS_CONVERTED_ASSET_MISSING",
                    "index": index,
                    "native_path": native_path,
                    "message": "ProjectorRays converted native asset file is missing",
                }
            )
            continue
        if _sha256(native_file) != converted_sha256 or native_file.stat().st_size != byte_size:
            diagnostics.append(
                {
                    "code": "TSUI_PROJECTORRAYS_CONVERTED_ASSET_HASH_MISMATCH",
                    "index": index,
                    "native_path": native_path,
                    "message": "ProjectorRays converted native asset hash or byte size does not match the file",
                }
            )
            continue
        asset = _projectorrays_asset_analysis_record(native_file, work_root, raw)
        asset["relative_path"] = native_path
        asset["sha256"] = converted_sha256
        asset["projectorrays_source"] = source_key
        asset["source_sha256"] = source_sha256
        asset["role"] = role
        asset["chunk_fourcc"] = chunk_fourcc
        classification = str(asset.get("classification", "script")).strip()
        if classification not in CAST_MEMBER_KINDS:
            classification = "script"
            asset["classification"] = classification
            asset["confidence"] = max(float(asset.get("confidence", 0.0)), 0.95)
        assets.append(asset)
        native_resources.append(
            {
                "source": source_key,
                "native_path": native_path,
                "classification": classification,
                "source_hash": source_sha256,
                "converted_hash": converted_sha256,
                "byte_size": byte_size,
                "coverage_status": "converted",
                "role": role,
                "chunk_fourcc": chunk_fourcc,
            }
        )
        members.append(
            {
                "member_id": f"projectorrays.{index + 1:04d}",
                "kind": classification,
                "source": source_key,
                "source_hash": source_sha256,
                "route_ids": [],
                "command_ids": [],
            }
        )
    if diagnostics:
        return None, None, None, diagnostics
    asset_analysis = {
        "schema": "tsuinosora.asset_analysis.v1",
        "status": "pass",
        "reference_hashes": _reference_hashes(reference_report),
        "classification_counts": _classification_counts(assets),
        "duplicate_hashes": _duplicate_hash_groups(assets),
        "assets": assets,
        "quarantine": [],
        "diagnostics": [],
        "redaction": {
            "paths": "report_relative_only",
            "payload": "omitted",
            "commercial_text": "omitted",
            "screenshots": "omitted",
            "audio": "omitted",
            "movie": "omitted",
        },
    }
    native_asset_report = {
        "schema": "tsuinosora.native_asset_rearrange_report.v1",
        "status": "pass",
        "output_root": "local_work_root/native-assets",
        "converted_assets": len(native_resources),
        "resources": native_resources,
        "diagnostics": [],
        "redaction": {
            "paths": "report_relative_only",
            "payload": "omitted",
            "commercial_text": "omitted",
            "screenshots": "omitted",
            "audio": "omitted",
            "movie": "omitted",
        },
    }
    converted_report_path = work_root / "reports" / "projectorrays_converted_resources.json"
    cast_source_map_report = {
        "schema": "tsuinosora.cast_source_map_report.v1",
        "status": "pass",
        "source_count": 1,
        "member_count": len(members),
        "sources": [
            {
                "source": "reports/projectorrays_converted_resources.json",
                "sha256": _sha256(converted_report_path),
                "member_count": len(members),
                "source_schema": "tsuinosora.projectorrays_converted_resources.v1",
            }
        ],
        "members": members,
        "diagnostics": [],
        "redaction": {
            "paths": "report_relative_only",
            "payload": "omitted",
            "commercial_text": "omitted",
            "bytecode": "omitted",
        },
    }
    generated = {"asset": asset_analysis, "native": native_asset_report, "cast": cast_source_map_report}
    if _report_has_path_leak(generated):
        return None, None, None, [
            {
                "code": "TSUI_PROJECTORRAYS_CONVERTED_ASSET_REPORT_PATH_LEAK",
                "message": "ProjectorRays converted native asset reports contain a local path-like value",
            }
        ]
    return asset_analysis, native_asset_report, cast_source_map_report, []


def _projectorrays_asset_analysis_record(native_file: Path, work_root: Path, resource: dict) -> dict:
    suffix = native_file.suffix.lower()
    role = str(resource.get("role", "script")).strip()
    chunk_fourcc = str(resource.get("chunk_fourcc", "")).strip()
    if suffix in IMAGE_EXTS:
        classification = "cg"
    elif suffix in AUDIO_EXTS:
        classification = "audio"
    elif suffix in MOVIE_EXTS:
        classification = "movie"
    elif suffix in FONT_EXTS:
        classification = "font"
    elif chunk_fourcc in {"Lscr", "STXT", "SCRF"} or "script" in role:
        classification = "script"
    else:
        classification = "script"
    asset = {
        "classification": classification,
        "confidence": 0.95,
    }
    width = _positive_int(resource.get("width", 0))
    height = _positive_int(resource.get("height", 0))
    if width and height:
        asset["dimensions"] = {"width": width, "height": height}
    bits_per_pixel = _positive_int(resource.get("bits_per_pixel", 0))
    if bits_per_pixel:
        asset["bits_per_pixel"] = bits_per_pixel
    return asset

def _run_projectorrays_from_demo_config(config: dict) -> dict | None:
    if not config.get("projectorrays_tool") and not config.get("projectorrays_dump_root"):
        return None
    work_root = Path(str(config.get("local_work_root", "")))
    reader_config = {
        "schema": "tsuinosora.projectorrays_reader_config.v1",
        "projectorrays_tool": str(config.get("projectorrays_tool", "")),
        "dump_root": str(config.get("projectorrays_dump_root", "")),
        "local_work_root": str(work_root),
    }
    config_path = work_root / "reports" / "projectorrays_reader.config.local.json"
    _write_json(config_path, reader_config)
    try:
        return import_projectorrays_reader(config_path)
    finally:
        try:
            config_path.unlink()
        except OSError:
            pass

def build_projectorrays_full_dump_report(work_root: Path | str, dump_roots: list[tuple[str, Path]]) -> dict:
    work_root = Path(work_root)
    diagnostics = []
    root_reports = []
    extension_counts: dict[str, int] = {}
    chunk_fourcc_counts: dict[str, int] = {}
    member_type_counts: dict[str, int] = {}
    binary_signature_counts: dict[str, int] = {}
    total_files = 0
    total_bytes = 0
    binary_chunk_count = 0
    json_chunk_count = 0
    script_file_count = 0
    movie_file_count = 0
    binary_chunks: dict[tuple[str, str], dict] = {}

    for alias, root in dump_roots:
        if not _is_safe_symbol(alias):
            diagnostics.append(
                {
                    "code": "TSUI_PROJECTORRAYS_FULL_DUMP_ALIAS_INVALID",
                    "alias": alias or "unknown",
                    "message": "ProjectorRays full dump root alias must be a safe symbol",
                }
            )
            continue
        if not root.is_dir():
            diagnostics.append(
                {
                    "code": "TSUI_PROJECTORRAYS_FULL_DUMP_ROOT_MISSING",
                    "alias": alias,
                    "message": "ProjectorRays full dump root is missing or inaccessible",
                }
            )
            continue
        root_files = 0
        root_bytes = 0
        root_extensions: dict[str, int] = {}
        root_fourcc_counts: dict[str, int] = {}
        for path in sorted(p for p in root.rglob("*") if p.is_file()):
            size = path.stat().st_size
            ext = path.suffix.lower() or "<none>"
            root_files += 1
            total_files += 1
            root_bytes += size
            total_bytes += size
            root_extensions[ext] = root_extensions.get(ext, 0) + 1
            extension_counts[ext] = extension_counts.get(ext, 0) + 1
            if ext == ".bin":
                binary_chunk_count += 1
                chunk_fourcc = _projectorrays_chunk_fourcc(path)
                relative_path = _rel(path, root)
                binary_chunks[(alias, relative_path)] = {
                    "source_alias": alias,
                    "source_relative_path": relative_path,
                    "chunk_fourcc": chunk_fourcc,
                    "source_sha256": _sha256(path),
                    "byte_size": size,
                }
                chunk_fourcc_counts[chunk_fourcc] = chunk_fourcc_counts.get(chunk_fourcc, 0) + 1
                root_fourcc_counts[chunk_fourcc] = root_fourcc_counts.get(chunk_fourcc, 0) + 1
                with path.open("rb") as handle:
                    signature = handle.read(4).hex() or "<empty>"
                binary_signature_counts[signature] = binary_signature_counts.get(signature, 0) + 1
            elif ext == ".json":
                json_chunk_count += 1
                try:
                    value = loads_projectorrays_json(path.read_text(encoding="utf-8"))
                except (json.JSONDecodeError, UnicodeDecodeError):
                    diagnostics.append(
                        {
                            "code": "TSUI_PROJECTORRAYS_FULL_DUMP_JSON_INVALID",
                            "alias": alias,
                            "message": "ProjectorRays JSON chunk could not be parsed",
                        }
                    )
                    continue
                if isinstance(value, dict) and "type" in value and "member" in value:
                    member_type = str(value.get("type"))
                    member_type_counts[member_type] = member_type_counts.get(member_type, 0) + 1
            elif ext in {".ls", ".lasm"}:
                script_file_count += 1
            elif ext == ".dir":
                movie_file_count += 1
        root_reports.append(
            {
                "alias": alias,
                "file_count": root_files,
                "byte_size": root_bytes,
                "extensions": dict(sorted(root_extensions.items())),
                "chunk_fourcc_counts": dict(sorted(root_fourcc_counts.items())),
            }
        )

    if not root_reports:
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_FULL_DUMP_EMPTY",
                "message": "ProjectorRays full dump report requires at least one readable dump root",
            }
        )
    converted_resources, converted_counts, converted_diagnostics = _projectorrays_converted_resource_evidence(
        work_root,
        binary_chunks,
    )
    diagnostics.extend(converted_diagnostics)
    converted_resource_count = len(converted_resources)
    resource_coverage_status = (
        "pass" if binary_chunk_count == converted_resource_count and not converted_diagnostics else "blocked"
    )
    if resource_coverage_status != "pass":
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_FULL_RESOURCE_CONVERSION_REQUIRED",
                "binary_chunk_count": binary_chunk_count,
                "converted_resource_count": converted_resource_count,
                "message": "full TsuiNoSora playable acceptance requires converted evidence for every ProjectorRays binary chunk",
            }
        )
    conversion_plan = [
        {
            "chunk_fourcc": fourcc,
            "role": PROJECTORRAYS_REQUIRED_CHUNK_ROLES.get(fourcc, "director_chunk"),
            "required": count,
            "converted": converted_counts.get(fourcc, 0),
            "status": "converted" if converted_counts.get(fourcc, 0) == count else "pending_converter",
        }
        for fourcc, count in sorted(chunk_fourcc_counts.items())
    ]
    report = {
        "schema": "tsuinosora.projectorrays_full_dump_report.v1",
        "status": "blocked" if diagnostics else "pass",
        "roots": root_reports,
        "counts": {
            "file_count": total_files,
            "byte_size": total_bytes,
            "binary_chunk_count": binary_chunk_count,
            "json_chunk_count": json_chunk_count,
            "script_file_count": script_file_count,
            "movie_file_count": movie_file_count,
            "converted_resource_count": converted_resource_count,
        },
        "extension_counts": dict(sorted(extension_counts.items())),
        "chunk_fourcc_counts": dict(sorted(chunk_fourcc_counts.items())),
        "member_type_counts": dict(sorted(member_type_counts.items())),
        "binary_signature_counts": dict(sorted(binary_signature_counts.items())),
        "converted_resources": converted_resources,
        "resource_coverage": {
            "status": resource_coverage_status,
            "required": binary_chunk_count,
            "converted": converted_resource_count,
        },
        "conversion_plan": conversion_plan,
        "diagnostics": _dedupe_diagnostics(diagnostics),
        "redaction": {
            "paths": "alias_only",
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
                "code": "TSUI_PROJECTORRAYS_FULL_DUMP_REPORT_PATH_LEAK",
                "message": "ProjectorRays full dump report contains a local path-like value",
            }
        )
        report["diagnostics"] = _dedupe_diagnostics(report["diagnostics"])
    _write_json(work_root / "reports" / "projectorrays_full_dump_report.json", report)
    return report
