#!/usr/bin/env python3
"""TsuiNoSora Stage 3 local-only conversion helpers.

Reports produced by this module are intentionally sanitized: they use caller
provided aliases and relative paths, and never embed payload bytes or local
absolute paths.
"""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import re
import shutil
import struct
import subprocess
import sys
import zlib
from collections import deque
from pathlib import Path


IMAGE_EXTS = {".png"}
AUDIO_EXTS = {".wav", ".ogg", ".flac", ".mp3"}
VOICE_HINTS = {"voice", "voices", "seiyuu"}
MOVIE_EXTS = {".mp4", ".webm", ".avi", ".mpg", ".mpeg"}
FONT_EXTS = {".ttf", ".otf", ".ttc"}
TEXT_EXTS = {".astra", ".txt", ".ini", ".json", ".csv", ".xml", ".html", ".js", ".cfg", ".scr", ".ls"}
BACKGROUND_HINTS = {"bg", "back", "background", "haikei"}
CHARACTER_HINTS = {"char", "character", "sprite", "tachie", "stand", "face", "pose", "chara"}
TEXT_WINDOW_HINTS = {"text_window", "textbox", "message", "msgwindow", "nameplate", "name_plate"}
BUTTON_HINTS = {"button", "btn", "menuitem", "selected"}
UI_HINTS = {"ui", "window", "frame", "border", "menu", "title"}
DIRECTOR_CONTAINER_EXTS = {".dxr", ".cxt", ".dir", ".dcr", ".cst", ".cct"}
READABLE_EXTRACT_EXTS = IMAGE_EXTS | AUDIO_EXTS | MOVIE_EXTS | FONT_EXTS | TEXT_EXTS
READABLE_RIFF_SIGNATURES = {b"RIFF", b"RIFX"}
SCRIPT_TEXT_CHUNK_IDS = {"Lscr", "scrp", "TEXT", "STXT", "LctX", "STR "}
DIRECTOR_LINGO_CHUNK_IDS = {"Lctx", "Lnam", "Lscr"}
METADATA_JSON_SCHEMAS = {
    "tsuinosora.cast_map.v1",
    "tsuinosora.route_graph.v1",
    "tsuinosora.script_source_map.v1",
    "tsuinosora.projectorrays_dump_manifest.v1",
}
DEFAULT_STAGE3_TARGETS = [
    {
        "target": "tsuinosora-internal-game",
        "profiles": ["classic", "modern"],
        "platforms": ["headless", "windows", "web"],
    },
    {
        "target": "tsuinosora-patch-game",
        "profiles": ["classic", "modern"],
        "platforms": ["headless", "windows", "web"],
    },
]
INTERNAL_DEMO_STAGE3_TARGETS = [
    {
        "target": "tsuinosora-internal-game",
        "profiles": ["classic"],
        "platforms": ["headless", "windows", "web"],
    },
]
DEMO_SLICE_CONFIG_TEMPLATE = {
    "schema": "tsuinosora.demo_slice_config.v1",
    "original_install_root": "Examples/TsuiNoSora/.local/original",
    "local_work_root": "Examples/TsuiNoSora/.local/work",
    "title_png": "Examples/TsuiNoSora/Docs/Title.png",
    "game_png": "Examples/TsuiNoSora/Docs/Game.png",
    "projectorrays_tool": "Examples/TsuiNoSora/.local/tools/ProjectorRays",
    "projectorrays_dump_root": "Examples/TsuiNoSora/.local/projectorrays-dump",
    "projectorrays_full_dump_roots": [
        {"alias": "root", "path": "Examples/TsuiNoSora/.local/projectorrays-full-root"},
        {"alias": "data", "path": "Examples/TsuiNoSora/.local/projectorrays-full-data"},
        {"alias": "casts", "path": "Examples/TsuiNoSora/.local/projectorrays-full-casts"},
    ],
    "player_automation_report": "Examples/TsuiNoSora/.local/work/reports/live_player_report.json",
    "require_full_resource_conversion": True,
}
DIRECTOR_CAST_MEMBER_METADATA_SCHEMA = "tsuinosora.director_cast_member_metadata.v1"
CAST_MEMBER_KINDS = {
    "background",
    "character_sprite",
    "character_atlas",
    "cg",
    "ui",
    "text_window",
    "button",
    "audio",
    "voice",
    "movie",
    "font",
    "script",
    "unknown",
}
MOUNT_ASSET_ROLES = CAST_MEMBER_KINDS - {"script", "unknown"}
NATIVE_ASSET_BUCKETS = {
    "background": "backgrounds",
    "character_sprite": "characters/sprites",
    "character_atlas": "characters/atlases",
    "cg": "cg",
    "ui": "ui",
    "text_window": "ui/text_windows",
    "button": "ui/buttons",
    "audio": "audio",
    "voice": "voice",
    "movie": "movies",
    "font": "fonts",
}
SCRIPT_ROUTE_RE = re.compile(
    r"^\s*(?:#|--|//)?\s*(?:astra[\s._-]*)?route\s*[:\s]\s*"
    r"(?P<route>[A-Za-z0-9_.-]+)"
    r"(?:\s*(?:->|terminal\s*[:=]?)\s*(?P<terminal>[A-Za-z0-9_.-]+))?"
    r"(?:.*?\bchoices?\s*[:=]?\s*(?P<choices>[A-Za-z0-9_., -]+))?",
    re.IGNORECASE,
)
PROJECTORRAYS_REQUIRED_CHUNK_ROLES = {
    "BITD": "bitmap_or_palette_backed_image",
    "CASt": "cast_member_metadata",
    "STXT": "text_or_field_member",
    "Lscr": "lingo_script_bytecode",
    "SCRF": "script_context_reference",
    "snd ": "sound_media",
    "sndH": "sound_header",
    "sndS": "sound_sample_data",
    "ediM": "embedded_media",
    "XMED": "xtra_media_metadata",
    "CAS_": "cast_member_binding",
    "KEY_": "resource_key_table",
    "Lctx": "lingo_context_table",
    "Lnam": "lingo_name_table",
    "mmap": "resource_map",
    "imap": "initial_map",
    "Cinf": "cast_info_table",
    "DRCF": "director_config",
    "Fmap": "font_map",
    "FCOL": "color_palette",
    "FXmp": "font_xtra_map",
    "MCsL": "movie_cast_list",
    "Sord": "score_order",
    "VERS": "director_version",
    "VWFI": "view_frame_info",
    "VWLB": "view_label_table",
    "VWSC": "view_score",
    "XTRl": "xtra_list",
    "cupt": "cue_point_table",
}
SCRIPT_SOURCE_MAP_FORBIDDEN_KEYS = {
    "body",
    "bytecode",
    "bytes",
    "commercial_text",
    "content",
    "lingo_source",
    "payload",
    "payload_bytes",
    "script_text",
    "source_text",
    "text",
}
TSUINOSORA_REFERENCE_HASHES = {
    "title": "sha256:3799183a831bdbdc144e1bc9e06dffd831417d436338a1daf04b45bc35624bca",
    "game": "sha256:1c4ddf68fa15fd6a76db259b155366456198bd551c49de8a9ede9ca0f2be9d84",
}
TSUINOSORA_REFERENCE_DIMENSIONS = {
    "title": {"width": 1386, "height": 1040},
    "game": {"width": 1403, "height": 1053},
}


def build_source_inventory(root: Path | str, root_alias: str) -> dict:
    root = Path(root)
    files = []
    for path in sorted(p for p in root.rglob("*") if p.is_file()):
        rel = _rel(path, root)
        files.append(
            {
                "relative_path": rel,
                "size": path.stat().st_size,
                "sha256": _sha256(path),
                "extension": path.suffix.lower(),
                "format_probe": _format_probe(path),
            }
        )
    edition = _edition_fingerprint(files)
    return {
        "schema": "tsuinosora.source_inventory.v1",
        "root_alias": root_alias,
        "file_count": len(files),
        "format_counts": _format_counts(files),
        "edition_fingerprint": edition,
        "files": files,
    }


def build_visual_reference_report(
    title_png: Path | str,
    game_png: Path | str,
    expected_hashes: dict[str, str] | None = None,
    expected_dimensions: dict[str, dict[str, int]] | None = None,
) -> dict:
    refs = []
    diagnostics = []
    expected_hashes = expected_hashes or {}
    expected_dimensions = expected_dimensions or {}
    for logical_id, path, regions in [
        (
            "title",
            Path(title_png),
            ["title_background", "title_menu_buttons", "title_selected_state"],
        ),
        (
            "game",
            Path(game_png),
            ["background_viewport", "text_window", "speaker_name", "message_text"],
        ),
    ]:
        entry = {
            "logical_id": logical_id,
            "file_name": path.name,
            "dimensions": {"width": 0, "height": 0},
            "hash": "",
            "allowed_regions": regions,
            "report_only": [
                "hash",
                "dimensions",
                "region_id",
                "coverage",
                "diagnostic",
                "layout_metric",
            ],
        }
        if not path.is_file():
            diagnostics.append(
                {
                    "code": "TSUI_REFERENCE_MISSING",
                    "logical_id": logical_id,
                    "file_name": path.name,
                    "message": "authoritative visual reference file is missing",
                }
            )
            refs.append(entry)
            continue
        try:
            image = read_png(path)
        except (OSError, ValueError, zlib.error, struct.error):
            diagnostics.append(
                {
                    "code": "TSUI_REFERENCE_PNG_INVALID",
                    "logical_id": logical_id,
                    "file_name": path.name,
                    "message": "visual reference must be a readable PNG with supported encoding",
                }
            )
            refs.append(entry)
            continue
        dimensions = {"width": image["width"], "height": image["height"]}
        digest = _sha256(path)
        entry["dimensions"] = dimensions
        entry["hash"] = digest
        expected_digest = expected_hashes.get(logical_id, "")
        if expected_digest and digest != expected_digest:
            diagnostics.append(
                {
                    "code": "TSUI_REFERENCE_HASH_MISMATCH",
                    "logical_id": logical_id,
                    "file_name": path.name,
                    "expected_hash": expected_digest,
                    "actual_hash": digest,
                    "message": "visual reference hash does not match the authoritative evidence manifest",
                }
            )
        expected_size = expected_dimensions.get(logical_id)
        if expected_size and dimensions != expected_size:
            diagnostics.append(
                {
                    "code": "TSUI_REFERENCE_DIMENSION_MISMATCH",
                    "logical_id": logical_id,
                    "file_name": path.name,
                    "expected_dimensions": expected_size,
                    "actual_dimensions": dimensions,
                    "message": "visual reference dimensions do not match the authoritative evidence manifest",
                }
            )
        refs.append(entry)
    return {
        "schema": "tsuinosora.visual_reference_report.v1",
        "status": "blocked" if diagnostics else "pass",
        "references": refs,
        "diagnostics": diagnostics,
        "prohibited_outputs": [
            "new_commercial_screenshot",
            "commercial_text",
            "commercial_audio",
            "commercial_movie",
        ],
    }


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


def analyze_assets(root: Path | str, reference_report: dict | None) -> dict:
    root = Path(root)
    assets = []
    quarantine = []
    diagnostics = []
    for path in sorted(p for p in root.rglob("*") if p.is_file()):
        if _is_unpacked_metadata_file(path):
            continue
        rel = _rel(path, root)
        try:
            asset = analyze_asset(path, root)
        except Exception as exc:  # noqa: BLE001 - diagnostic must survive malformed local data.
            asset = {
                "relative_path": rel,
                "classification": "unknown",
                "confidence": 0.0,
                "diagnostics": [str(exc)],
            }
        assets.append(asset)
    usage_index = _asset_usage_index(root, [asset["relative_path"] for asset in assets])
    duplicate_groups = _duplicate_hash_groups(assets)
    duplicate_by_path = {
        rel: group for group in duplicate_groups for rel in group["relative_paths"]
    }
    for asset in assets:
        rel = asset["relative_path"]
        asset["container_source"] = _container_source(rel)
        asset["script_references"] = usage_index.get(rel, [])
        asset["use_timing"] = _use_timing(asset["script_references"])
        if rel in duplicate_by_path:
            asset["duplicate_hash_group"] = duplicate_by_path[rel]["duplicate_hash_group"]
            asset["duplicate_paths"] = duplicate_by_path[rel]["relative_paths"]
        asset["reference_matches"] = _reference_matches(asset, reference_report)

        asset_diagnostics = []
        if asset["classification"] == "unknown" or asset["confidence"] < 0.65:
            asset_diagnostics.append(
                {
                    "code": "TSUI_ASSET_LOW_CONFIDENCE",
                    "relative_path": rel,
                    "message": "asset classification is unknown or below confidence threshold",
                }
            )
        asset_diagnostics.extend(_classification_conflicts(asset))
        if asset_diagnostics:
            quarantine.append(
                {
                    "relative_path": rel,
                    "classification": asset["classification"],
                    "confidence": asset["confidence"],
                    "reason_codes": [diagnostic["code"] for diagnostic in asset_diagnostics],
                }
            )
            diagnostics.extend(asset_diagnostics)
    return {
        "schema": "tsuinosora.asset_analysis.v1",
        "status": "blocked" if quarantine else "pass",
        "reference_hashes": _reference_hashes(reference_report),
        "classification_counts": _classification_counts(assets),
        "duplicate_hashes": duplicate_groups,
        "assets": assets,
        "quarantine": quarantine,
        "diagnostics": diagnostics,
    }


def build_route_graph_report(root: Path | str) -> dict:
    root = Path(root)
    diagnostics = []
    routes = []
    sources = []
    for path in sorted(p for p in root.rglob("*.json") if p.is_file()):
        rel = _rel(path, root)
        try:
            value = _read_json(path)
        except json.JSONDecodeError:
            continue
        if not isinstance(value, dict) or value.get("schema") != "tsuinosora.route_graph.v1":
            continue
        payload_diagnostics = _forbidden_payload_key_diagnostics(
            value,
            rel,
            code="TSUI_ROUTE_GRAPH_PAYLOAD_FIELD",
            source_field="source",
            message="route graph sidecar must not contain script text, bytecode or payload fields",
        )
        diagnostics.extend(payload_diagnostics)
        extracted = []
        for route in value.get("routes", []):
            if not isinstance(route, dict):
                continue
            route_id = str(route.get("route_id", "")).strip()
            terminal = str(route.get("terminal", "")).strip()
            coverage = str(route.get("coverage", "unknown")).strip()
            choices = route.get("choices", [])
            route_diagnostics = []
            if not route_id or not terminal or coverage != "covered":
                route_diagnostics.append(
                    {
                        "code": "TSUI_ROUTE_GRAPH_INCOMPLETE_ROUTE",
                        "source": rel,
                        "route_id": route_id or "unknown",
                        "message": "route graph entries must include route_id, terminal and covered coverage",
                    }
                )
            if route_id and not _is_safe_symbol(route_id):
                route_diagnostics.append(
                    {
                        "code": "TSUI_ROUTE_GRAPH_ROUTE_ID_INVALID",
                        "source": rel,
                        "route_id": "invalid",
                        "message": "route graph route_id must be a safe symbol",
                    }
                )
            if terminal and not _is_safe_symbol(terminal):
                route_diagnostics.append(
                    {
                        "code": "TSUI_ROUTE_GRAPH_TERMINAL_INVALID",
                        "source": rel,
                        "route_id": route_id or "unknown",
                        "message": "route graph terminal must be a safe symbol",
                    }
                )
            if not isinstance(choices, list):
                route_diagnostics.append(
                    {
                        "code": "TSUI_ROUTE_GRAPH_CHOICES_INVALID",
                        "source": rel,
                        "route_id": route_id or "unknown",
                        "message": "route graph choices must be a list of safe symbols",
                    }
                )
                choices = []
            safe_choices = []
            for choice_index, choice in enumerate(choices):
                choice_id = str(choice).strip()
                if not choice_id:
                    continue
                if not _is_safe_symbol(choice_id):
                    route_diagnostics.append(
                        {
                            "code": "TSUI_ROUTE_GRAPH_CHOICE_INVALID",
                            "source": rel,
                            "route_id": route_id or "unknown",
                            "choice_index": choice_index,
                            "message": "route graph choice id must be a safe symbol",
                        }
                    )
                    continue
                safe_choices.append(choice_id)
            if payload_diagnostics:
                route_diagnostics.append(
                    {
                        "code": "TSUI_ROUTE_GRAPH_PAYLOAD_BLOCKED",
                        "source": rel,
                        "route_id": route_id or "unknown",
                        "message": "route graph with payload-like fields cannot prove sanitized route coverage",
                    }
                )
            if route_diagnostics:
                diagnostics.extend(route_diagnostics)
                continue
            extracted.append(
                {
                    "route_id": route_id,
                    "coverage": coverage,
                    "terminal": terminal,
                    "choices": safe_choices,
                    "source": rel,
                }
            )
        if extracted:
            sources.append(
                {
                    "source": rel,
                    "route_count": len(extracted),
                    "sha256": _sha256(path),
                }
            )
            routes.extend(extracted)
    diagnostics.extend(
        _duplicate_choice_diagnostics(
            routes,
            code="TSUI_ROUTE_GRAPH_DUPLICATE_CHOICE",
            message="route graph route choices must be unique for each route_id",
        )
    )
    diagnostics.extend(
        _duplicate_route_conflict_diagnostics(
            routes,
            code="TSUI_ROUTE_GRAPH_DUPLICATE_ROUTE_CONFLICT",
            message="route graph maps one route_id to multiple terminal or choice signatures",
        )
    )
    if not routes:
        diagnostics.append(
            {
                "code": "TSUI_ROUTE_GRAPH_MISSING",
                "message": "no covered route graph was found in unpacked assets",
            }
        )
    report = {
        "schema": "tsuinosora.route_graph_report.v1",
        "status": "blocked" if diagnostics else "pass",
        "source_count": len(sources),
        "route_count": len(routes),
        "sources": sources,
        "routes": routes,
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
                "code": "TSUI_ROUTE_GRAPH_REPORT_PATH_LEAK",
                "message": "route graph report contains a local path-like value",
            }
        )
    return report


def build_script_source_map_report(root: Path | str) -> dict:
    root = Path(root)
    diagnostics = []
    sources = []
    routes = []
    readers = []
    lingo_bytecode_requirements = []
    for path in sorted(p for p in root.rglob("*") if p.is_file() and p.suffix.lower() in TEXT_EXTS):
        if path.suffix.lower() == ".json" and _is_unpacked_metadata_file(path):
            continue
        rel = _rel(path, root)
        text = _read_text_lossless(path)
        line_count = len(text.splitlines())
        source_routes = []
        for line_no, line in enumerate(text.splitlines(), start=1):
            route = _script_route_marker(line)
            if not route:
                continue
            route["source"] = rel
            route["line"] = line_no
            source_routes.append(route)
            routes.append(route)
        sources.append(
            {
                "source": rel,
                "sha256": _sha256(path),
                "line_count": line_count,
                "route_marker_count": len(source_routes),
            }
        )
    for path in sorted(p for p in root.rglob("*.json") if p.is_file()):
        rel = _rel(path, root)
        try:
            value = _read_json(path)
        except json.JSONDecodeError:
            continue
        if not isinstance(value, dict):
            continue
        if value.get("schema") == "tsuinosora.script_source_map.v1":
            sidecar_sources, sidecar_routes, sidecar_readers, sidecar_diagnostics = _script_source_map_sidecar_routes(value, rel, root)
            sources.extend(sidecar_sources)
            routes.extend(sidecar_routes)
            readers.extend(sidecar_readers)
            diagnostics.extend(sidecar_diagnostics)
            continue
        if value.get("schema") != "tsuinosora.director_lingo_map.v1":
            continue
        unsupported_count = int(value.get("unsupported_script_count", 0))
        script_count = int(value.get("script_count", 0))
        if script_count:
            sources.append(
                {
                    "source": rel,
                    "sha256": _sha256(path),
                    "line_count": 0,
                    "route_marker_count": 0,
                    "lingo_script_count": script_count,
                    "unsupported_script_count": unsupported_count,
                }
            )
        if unsupported_count:
            required_scripts = []
            for resource in value.get("resources", []):
                if not isinstance(resource, dict):
                    continue
                if resource.get("tag") != "Lscr" or not resource.get("requires_bytecode_reader"):
                    continue
                try:
                    resource_id = int(resource.get("resource_id"))
                except (TypeError, ValueError):
                    continue
                required_scripts.append(
                    {
                        "resource_id": resource_id,
                        "entry_id": str(resource.get("entry_id", "")),
                        "payload_sha256": str(resource.get("payload_sha256", "")),
                    }
                )
            lingo_bytecode_requirements.append(
                {
                    "source": rel,
                    "source_hash": _sha256(path),
                    "script_count": script_count,
                    "unsupported_script_count": unsupported_count,
                    "required_scripts": required_scripts,
                }
            )
    covered_source_hashes = {
        (str(route.get("source", "")), str(route.get("source_hash", "")))
        for route in routes
        if route.get("source") and route.get("source_hash")
    }
    for requirement in lingo_bytecode_requirements:
        if (requirement["source"], requirement["source_hash"]) in covered_source_hashes:
            covered_scripts = {
                (
                    str(route.get("source", "")),
                    str(route.get("source_hash", "")),
                    int(route.get("script_resource_id")),
                    str(route.get("script_payload_sha256", "")),
                )
                for route in routes
                if route.get("source") == requirement["source"]
                and route.get("source_hash") == requirement["source_hash"]
                and route.get("script_resource_id") is not None
                and route.get("script_payload_sha256")
            }
            for script in requirement.get("required_scripts", []):
                expected = (
                    requirement["source"],
                    requirement["source_hash"],
                    int(script["resource_id"]),
                    str(script["payload_sha256"]),
                )
                if expected in covered_scripts:
                    continue
                diagnostics.append(
                    {
                        "code": "TSUI_SCRIPT_SOURCE_MAP_LINGO_BYTECODE_RESOURCE_UNCOVERED",
                        "source": requirement["source"],
                        "script_resource_id": int(script["resource_id"]),
                        "script_payload_sha256": str(script["payload_sha256"]),
                        "message": "Director Lingo bytecode route coverage must bind every unsupported Lscr resource id and payload hash",
                    }
                )
            continue
        diagnostics.append(
            {
                "code": "TSUI_SCRIPT_SOURCE_MAP_LINGO_BYTECODE_UNSUPPORTED",
                "source": requirement["source"],
                "script_count": requirement["script_count"],
                "unsupported_script_count": requirement["unsupported_script_count"],
                "message": "Director Lingo bytecode requires a complete Lctx/Lnam/Lscr source-map reader before route coverage can be proven",
            }
        )
    diagnostics.extend(
        _duplicate_route_conflict_diagnostics(
            routes,
            code="TSUI_SCRIPT_SOURCE_MAP_DUPLICATE_ROUTE_CONFLICT",
            message="script source map maps one route_id to multiple terminal or choice signatures",
        )
    )
    diagnostics.extend(
        _duplicate_choice_diagnostics(
            routes,
            code="TSUI_SCRIPT_SOURCE_MAP_DUPLICATE_CHOICE",
            message="script source map route choices must be unique for each route_id",
        )
    )
    routes = _dedupe_script_source_routes(routes)
    if not routes:
        diagnostics.append(
            {
                "code": "TSUI_SCRIPT_SOURCE_MAP_ROUTE_MISSING",
                "message": "no route markers were found in readable script text",
            }
        )
    report = {
        "schema": "tsuinosora.script_source_map_report.v1",
        "status": "blocked" if diagnostics else "pass",
        "source_count": len(sources),
        "route_count": len(routes),
        "reader_count": len(readers),
        "readers": readers,
        "sources": sources,
        "routes": routes,
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
                "code": "TSUI_SCRIPT_SOURCE_MAP_REPORT_PATH_LEAK",
                "message": "script source map report contains a local path-like value",
            }
        )
    return report


def _duplicate_choice_diagnostics(routes: list[dict], *, code: str, message: str) -> list[dict]:
    diagnostics = []
    for route in routes:
        seen: set[str] = set()
        reported: set[str] = set()
        for choice in [str(value).strip() for value in route.get("choices", []) or []]:
            if not choice:
                continue
            if choice not in seen:
                seen.add(choice)
                continue
            if choice in reported:
                continue
            reported.add(choice)
            diagnostics.append(
                {
                    "code": code,
                    "route_id": str(route.get("route_id", "unknown")).strip() or "unknown",
                    "source": str(route.get("source", "")).strip(),
                    "choice": choice,
                    "message": message,
                }
            )
    return diagnostics


def _duplicate_route_conflict_diagnostics(routes: list[dict], *, code: str, message: str) -> list[dict]:
    diagnostics = []
    first_by_route_id: dict[str, dict] = {}
    reported: set[str] = set()
    for route in routes:
        route_id = str(route.get("route_id", "")).strip()
        if not route_id:
            continue
        signature = {
            "terminal": str(route.get("terminal", "")).strip(),
            "choices": [str(choice).strip() for choice in route.get("choices", []) or []],
        }
        current = {
            "route_id": route_id,
            "source": str(route.get("source", "")).strip(),
            **signature,
        }
        first = first_by_route_id.get(route_id)
        if first is None:
            first_by_route_id[route_id] = current
            continue
        if first["terminal"] == signature["terminal"] and first["choices"] == signature["choices"]:
            continue
        if route_id in reported:
            continue
        reported.add(route_id)
        diagnostics.append(
            {
                "code": code,
                "route_id": route_id,
                "source": current["source"],
                "first_source": first["source"],
                "terminal": current["terminal"],
                "first_terminal": first["terminal"],
                "choice_count": len(signature["choices"]),
                "first_choice_count": len(first["choices"]),
                "message": message,
            }
        )
    return diagnostics


def _dedupe_script_source_routes(routes: list[dict]) -> list[dict]:
    by_key: dict[tuple[str, str, tuple[str, ...]], dict] = {}
    order: list[tuple[str, str, tuple[str, ...]]] = []
    for route in routes:
        key = (
            str(route.get("route_id", "")),
            str(route.get("terminal", "")),
            tuple(str(choice) for choice in route.get("choices", [])),
        )
        if key not in by_key:
            by_key[key] = route
            order.append(key)
            continue
        current = by_key[key]
        if route.get("source_map") and not current.get("source_map"):
            by_key[key] = route
    return [by_key[key] for key in order]


def _script_source_map_sidecar_routes(
    value: dict,
    map_source: str,
    root: Path,
) -> tuple[list[dict], list[dict], list[dict], list[dict]]:
    diagnostics = _script_source_map_payload_key_diagnostics(value, map_source)
    sources = []
    routes = []
    readers = []
    declared_source_hashes = {}
    declared_source_line_counts = {}
    lingo_bytecode_resources_by_source = {}

    reader = value.get("reader", {})
    if reader and not isinstance(reader, dict):
        diagnostics.append(
            {
                "code": "TSUI_SCRIPT_SOURCE_MAP_READER_INVALID",
                "source_map": map_source,
                "message": "script source map reader metadata must be an object",
            }
        )
    elif reader:
        reader_valid = True
        tool_id = str(reader.get("tool_id", ""))
        tool_hash = str(reader.get("tool_hash", ""))
        output_contract = str(reader.get("output_contract", ""))
        if not _is_safe_symbol(tool_id):
            diagnostics.append(
                {
                    "code": "TSUI_SCRIPT_SOURCE_MAP_READER_ID_INVALID",
                    "source_map": map_source,
                    "field": "reader.tool_id",
                    "message": "reader tool_id must be a sanitized tool identity",
                }
            )
            reader_valid = False
        if not tool_hash:
            diagnostics.append(
                {
                    "code": "TSUI_SCRIPT_SOURCE_MAP_READER_HASH_MISSING",
                    "source_map": map_source,
                    "field": "reader.tool_hash",
                    "message": "reader hash evidence is required when reader metadata is present",
                }
            )
            reader_valid = False
        elif not _is_sanitized_sha256(tool_hash):
            diagnostics.append(
                {
                    "code": "TSUI_SCRIPT_SOURCE_MAP_READER_HASH_INVALID",
                    "source_map": map_source,
                    "field": "reader.tool_hash",
                    "message": "reader hash evidence must be a sanitized sha256 value",
                }
            )
            reader_valid = False
        if output_contract and not _is_safe_symbol(output_contract):
            diagnostics.append(
                {
                    "code": "TSUI_SCRIPT_SOURCE_MAP_READER_CONTRACT_INVALID",
                    "source_map": map_source,
                    "field": "reader.output_contract",
                    "message": "reader output_contract must be a sanitized contract id",
                }
            )
            reader_valid = False
        if reader_valid:
            readers.append(
                {
                    "source_map": map_source,
                    "tool_id": tool_id,
                    "tool_hash": tool_hash,
                    "output_contract": output_contract,
                }
            )

    raw_sources = value.get("sources", [])
    if raw_sources and not isinstance(raw_sources, list):
        diagnostics.append(
            {
                "code": "TSUI_SCRIPT_SOURCE_MAP_SOURCES_INVALID",
                "source_map": map_source,
                "message": "script source map sources must be a list",
            }
        )
        raw_sources = []
    for index, raw_source in enumerate(raw_sources):
        if not isinstance(raw_source, dict):
            diagnostics.append(
                {
                    "code": "TSUI_SCRIPT_SOURCE_MAP_SOURCE_INVALID",
                    "source_map": map_source,
                    "index": index,
                    "message": "script source map source entry must be an object",
                }
            )
            continue
        source = str(raw_source.get("source", ""))
        source_invalid = False
        if not _is_safe_report_relative_path(source):
            diagnostics.append(
                {
                    "code": "TSUI_SCRIPT_SOURCE_MAP_SOURCE_INVALID",
                    "source_map": map_source,
                    "index": index,
                    "message": "script source map source must be report-relative",
                }
            )
            source_invalid = True
        digest = str(raw_source.get("sha256", ""))
        digest_invalid = False
        if digest and not _is_sanitized_sha256(digest):
            diagnostics.append(
                {
                    "code": "TSUI_SCRIPT_SOURCE_MAP_HASH_INVALID",
                    "source_map": map_source,
                    "index": index,
                    "field": "sha256",
                    "message": "script source map source hash must be a sanitized sha256 value",
                }
            )
            digest_invalid = True
        line_count = _nonnegative_int(raw_source.get("line_count", 0))
        if source_invalid or digest_invalid:
            continue
        if not digest:
            diagnostics.append(
                {
                    "code": "TSUI_SCRIPT_SOURCE_MAP_HASH_MISSING",
                    "source_map": map_source,
                    "index": index,
                    "field": "sha256",
                    "message": "script source map source hash is required to prove route coverage",
                }
            )
            continue
        source_path = root / source
        if source_path.is_file():
            actual_digest = _sha256(source_path)
            if digest != actual_digest:
                diagnostics.append(
                    {
                        "code": "TSUI_SCRIPT_SOURCE_MAP_SOURCE_HASH_MISMATCH",
                        "source_map": map_source,
                        "index": index,
                        "source": source,
                        "message": "script source map source hash does not match the report-relative source file",
                    }
                )
                continue
            lingo_bytecode_resources = _lingo_bytecode_resource_index(source_path)
            if lingo_bytecode_resources:
                lingo_bytecode_resources_by_source[source] = lingo_bytecode_resources
        existing_digest = declared_source_hashes.get(source)
        if existing_digest and existing_digest != digest:
            diagnostics.append(
                {
                    "code": "TSUI_SCRIPT_SOURCE_MAP_SOURCE_HASH_CONFLICT",
                    "source_map": map_source,
                    "index": index,
                    "source": source,
                    "message": "script source map source hash must be stable for each source",
                }
            )
            continue
        declared_source_hashes[source] = digest
        declared_source_line_counts[source] = line_count
        sources.append(
            {
                "source": source,
                "sha256": digest,
                "line_count": line_count,
                "route_marker_count": 0,
                "script_count": _nonnegative_int(raw_source.get("script_count", 0)),
                "source_map": map_source,
            }
        )

    raw_routes = value.get("routes", [])
    if raw_routes and not isinstance(raw_routes, list):
        diagnostics.append(
            {
                "code": "TSUI_SCRIPT_SOURCE_MAP_ROUTES_INVALID",
                "source_map": map_source,
                "message": "script source map routes must be a list",
            }
        )
        raw_routes = []
    for index, raw_route in enumerate(raw_routes):
        if not isinstance(raw_route, dict):
            diagnostics.append(
                {
                    "code": "TSUI_SCRIPT_SOURCE_MAP_ROUTE_INVALID",
                    "source_map": map_source,
                    "index": index,
                    "message": "script source map route entry must be an object",
                }
            )
            continue
        route, route_diagnostics = _script_source_map_route(raw_route, map_source, index)
        diagnostics.extend(route_diagnostics)
        if route_diagnostics:
            continue
        declared_hash = declared_source_hashes.get(route["source"])
        if not declared_hash:
            diagnostics.append(
                {
                    "code": "TSUI_SCRIPT_SOURCE_MAP_ROUTE_SOURCE_UNDECLARED",
                    "source_map": map_source,
                    "index": index,
                    "source": route["source"],
                    "message": "script source map route source must match a declared source entry",
                }
            )
            continue
        if route["source_hash"] != declared_hash:
            diagnostics.append(
                {
                    "code": "TSUI_SCRIPT_SOURCE_MAP_ROUTE_HASH_MISMATCH",
                    "source_map": map_source,
                    "index": index,
                    "source": route["source"],
                    "message": "script source map route hash must match the declared source hash",
                }
            )
            continue
        declared_line_count = declared_source_line_counts.get(route["source"], 0)
        if declared_line_count and route["line"] > declared_line_count:
            diagnostics.append(
                {
                    "code": "TSUI_SCRIPT_SOURCE_MAP_ROUTE_LINE_OUT_OF_RANGE",
                    "source_map": map_source,
                    "index": index,
                    "source": route["source"],
                    "line": route["line"],
                    "line_count": declared_line_count,
                    "message": "script source map route line must be inside the declared source line range",
                }
            )
            continue
        script_resources = lingo_bytecode_resources_by_source.get(route["source"], {})
        if script_resources:
            script_resource_id = route.get("script_resource_id")
            script_payload_sha256 = str(route.get("script_payload_sha256", ""))
            if script_resource_id is None or not script_payload_sha256:
                diagnostics.append(
                    {
                        "code": "TSUI_SCRIPT_SOURCE_MAP_SCRIPT_RESOURCE_REQUIRED",
                        "source_map": map_source,
                        "index": index,
                        "source": route["source"],
                        "message": "Lingo bytecode coverage requires script resource id and payload hash evidence",
                    }
                )
                continue
            script_resource = script_resources.get(int(script_resource_id))
            if not script_resource:
                diagnostics.append(
                    {
                        "code": "TSUI_SCRIPT_SOURCE_MAP_SCRIPT_RESOURCE_UNKNOWN",
                        "source_map": map_source,
                        "index": index,
                        "source": route["source"],
                        "script_resource_id": script_resource_id,
                        "message": "Lingo bytecode route references a script resource that is not present in the Director Lingo map",
                    }
                )
                continue
            if script_payload_sha256 != script_resource["payload_sha256"]:
                diagnostics.append(
                    {
                        "code": "TSUI_SCRIPT_SOURCE_MAP_SCRIPT_HASH_MISMATCH",
                        "source_map": map_source,
                        "index": index,
                        "source": route["source"],
                        "script_resource_id": script_resource_id,
                        "message": "Lingo bytecode route hash does not match the Director Lingo map script payload hash",
                    }
                )
                continue
            route["script_entry_id"] = script_resource.get("entry_id", "")
        routes.append(route)

    return sources, routes, readers, diagnostics


def _lingo_bytecode_resource_index(source_path: Path) -> dict[int, dict]:
    try:
        value = _read_json(source_path)
    except json.JSONDecodeError:
        return {}
    if not isinstance(value, dict) or value.get("schema") != "tsuinosora.director_lingo_map.v1":
        return {}
    resources = {}
    for raw_resource in value.get("resources", []):
        if not isinstance(raw_resource, dict):
            continue
        if raw_resource.get("tag") != "Lscr" or not raw_resource.get("requires_bytecode_reader"):
            continue
        try:
            resource_id = int(raw_resource.get("resource_id"))
        except (TypeError, ValueError):
            continue
        payload_sha256 = str(raw_resource.get("payload_sha256", ""))
        if not _is_sanitized_sha256(payload_sha256):
            continue
        resources[resource_id] = {
            "resource_id": resource_id,
            "entry_id": str(raw_resource.get("entry_id", "")).strip(),
            "payload_sha256": payload_sha256,
        }
    return resources


def _script_source_map_route(raw_route: dict, map_source: str, index: int) -> tuple[dict | None, list[dict]]:
    diagnostics = []
    route_id = str(raw_route.get("route_id", ""))
    terminal = str(raw_route.get("terminal", route_id))
    source = str(raw_route.get("source", ""))
    coverage = str(raw_route.get("coverage", "covered"))
    line = _positive_int(raw_route.get("line", 0))
    source_hash = str(raw_route.get("source_hash", ""))
    script_resource_id = None
    if "script_resource_id" in raw_route:
        try:
            parsed_script_resource_id = int(raw_route.get("script_resource_id"))
        except (TypeError, ValueError):
            parsed_script_resource_id = -1
        if parsed_script_resource_id < 0:
            diagnostics.append(
                {
                    "code": "TSUI_SCRIPT_SOURCE_MAP_SCRIPT_RESOURCE_INVALID",
                    "source_map": map_source,
                    "index": index,
                    "message": "script_resource_id must be a non-negative Director Lingo resource id",
                }
            )
        else:
            script_resource_id = parsed_script_resource_id
    script_payload_sha256 = str(raw_route.get("script_payload_sha256", ""))
    if script_payload_sha256 and not _is_sanitized_sha256(script_payload_sha256):
        diagnostics.append(
            {
                "code": "TSUI_SCRIPT_SOURCE_MAP_SCRIPT_HASH_INVALID",
                "source_map": map_source,
                "index": index,
                "field": "script_payload_sha256",
                "message": "script_payload_sha256 must be a sanitized sha256 value",
            }
        )

    if not _is_safe_symbol(route_id):
        diagnostics.append(
            {
                "code": "TSUI_SCRIPT_SOURCE_MAP_ROUTE_ID_INVALID",
                "source_map": map_source,
                "index": index,
                "message": "script source map route_id must be a safe symbol",
            }
        )
    if terminal and not _is_safe_symbol(terminal):
        diagnostics.append(
            {
                "code": "TSUI_SCRIPT_SOURCE_MAP_TERMINAL_INVALID",
                "source_map": map_source,
                "index": index,
                "message": "script source map terminal must be a safe symbol",
            }
        )
    if not _is_safe_report_relative_path(source):
        diagnostics.append(
            {
                "code": "TSUI_SCRIPT_SOURCE_MAP_SOURCE_INVALID",
                "source_map": map_source,
                "index": index,
                "message": "script source map route source must be report-relative",
            }
        )
    if line <= 0:
        diagnostics.append(
            {
                "code": "TSUI_SCRIPT_SOURCE_MAP_LINE_INVALID",
                "source_map": map_source,
                "index": index,
                "message": "script source map line must be a positive integer",
            }
        )
    if coverage != "covered":
        diagnostics.append(
            {
                "code": "TSUI_SCRIPT_SOURCE_MAP_COVERAGE_INVALID",
                "source_map": map_source,
                "index": index,
                "message": "script source map routes must prove covered coverage",
            }
        )
    if source_hash and not _is_sanitized_sha256(source_hash):
        diagnostics.append(
            {
                "code": "TSUI_SCRIPT_SOURCE_MAP_HASH_INVALID",
                "source_map": map_source,
                "index": index,
                "field": "source_hash",
                "message": "script source map route hash must be a sanitized sha256 value",
            }
        )

    raw_choices = raw_route.get("choices", [])
    if raw_choices is None:
        raw_choices = []
    if not isinstance(raw_choices, list):
        diagnostics.append(
            {
                "code": "TSUI_SCRIPT_SOURCE_MAP_CHOICES_INVALID",
                "source_map": map_source,
                "index": index,
                "message": "script source map choices must be a list of safe symbols",
            }
        )
        raw_choices = []
    choices = []
    for choice_index, choice in enumerate(raw_choices):
        choice_id = str(choice)
        if not _is_safe_symbol(choice_id):
            diagnostics.append(
                {
                    "code": "TSUI_SCRIPT_SOURCE_MAP_CHOICE_INVALID",
                    "source_map": map_source,
                    "index": index,
                    "choice_index": choice_index,
                    "message": "script source map choice id must be a safe symbol",
                }
            )
            continue
        choices.append(choice_id)

    if diagnostics:
        return None, diagnostics
    route = {
        "route_id": route_id,
        "coverage": "covered",
        "terminal": terminal or route_id,
        "choices": choices,
        "source": source,
        "line": line,
        "source_hash": source_hash,
        "source_map": map_source,
    }
    if script_resource_id is not None:
        route["script_resource_id"] = script_resource_id
    if script_payload_sha256:
        route["script_payload_sha256"] = script_payload_sha256
    return route, diagnostics


def _script_source_map_payload_key_diagnostics(value, map_source: str) -> list[dict]:
    return _forbidden_payload_key_diagnostics(
        value,
        map_source,
        code="TSUI_SCRIPT_SOURCE_MAP_PAYLOAD_FIELD",
        source_field="source_map",
        message="script source map sidecar must not contain script text, bytecode or payload fields",
    )


def _forbidden_payload_key_diagnostics(
    value,
    source: str,
    *,
    code: str,
    source_field: str,
    message: str,
) -> list[dict]:
    diagnostics = []

    def walk(node, path: str):
        if isinstance(node, dict):
            for key, child in node.items():
                key_name = str(key)
                field = f"{path}.{key_name}" if path else key_name
                if key_name.lower() in SCRIPT_SOURCE_MAP_FORBIDDEN_KEYS:
                    if path == "redaction" and child == "omitted":
                        continue
                    diagnostics.append(
                        {
                            "code": code,
                            source_field: source,
                            "field": field,
                            "message": message,
                        }
                    )
                    continue
                walk(child, field)
        elif isinstance(node, list):
            for index, child in enumerate(node):
                walk(child, f"{path}[{index}]")

    walk(value, "")
    return diagnostics


def build_cast_source_map_report(root: Path | str) -> dict:
    root = Path(root)
    diagnostics = []
    sources = []
    members = []
    asset_index = {
        _rel(path, root): path
        for path in sorted(p for p in root.rglob("*") if p.is_file() and not _is_unpacked_metadata_file(p))
    }

    for path in sorted(p for p in root.rglob("*.json") if p.is_file()):
        rel = _rel(path, root)
        try:
            value = _read_json(path)
        except json.JSONDecodeError:
            continue
        if not isinstance(value, dict):
            continue
        schema = value.get("schema")
        if schema == "tsuinosora.cast_map.v1":
            payload_diagnostics = _cast_source_map_payload_key_diagnostics(value, rel)
            diagnostics.extend(payload_diagnostics)
            source_members = []
            for raw_member in value.get("members", []):
                if not isinstance(raw_member, dict):
                    continue
                member, member_diagnostics = _cast_member_from_map(raw_member, rel, asset_index)
                if member:
                    source_members.append(member)
                    members.append(member)
                diagnostics.extend(member_diagnostics)
            sources.append(
                {
                    "source": rel,
                    "sha256": _sha256(path),
                    "member_count": len(source_members),
                }
            )
        elif schema == "tsuinosora.director_cast_map.v1":
            diagnostics.extend(_cast_source_map_payload_key_diagnostics(value, rel))
            source_members, member_diagnostics = _cast_members_from_director_cast_report(value, rel, root, asset_index)
            members.extend(source_members)
            diagnostics.extend(member_diagnostics)
            sources.append(
                {
                    "source": rel,
                    "sha256": _sha256(path),
                    "member_count": len(source_members),
                    "source_schema": schema,
                }
            )

    if not members:
        diagnostics.append(
            {
                "code": "TSUI_CAST_SOURCE_MAP_MISSING",
                "message": "no tsuinosora.cast_map.v1 or tsuinosora.director_cast_map.v1 members were found in unpacked assets",
            }
        )

    report = {
        "schema": "tsuinosora.cast_source_map_report.v1",
        "status": "blocked" if diagnostics else "pass",
        "source_count": len(sources),
        "member_count": len(members),
        "sources": sources,
        "members": members,
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
                "code": "TSUI_CAST_SOURCE_MAP_REPORT_PATH_LEAK",
                "message": "cast source map report contains a local path-like value",
            }
        )
    return report


def _cast_source_map_payload_key_diagnostics(value, map_source: str) -> list[dict]:
    return _forbidden_payload_key_diagnostics(
        value,
        map_source,
        code="TSUI_CAST_SOURCE_MAP_PAYLOAD_FIELD",
        source_field="source",
        message="cast source map sidecar must not contain commercial text, bytecode or payload fields",
    )


def _cast_members_from_director_cast_report(
    report: dict,
    map_source: str,
    root: Path,
    asset_index: dict[str, Path],
) -> tuple[list[dict], list[dict]]:
    diagnostics = [
        diagnostic
        for diagnostic in report.get("diagnostics", [])
        if isinstance(diagnostic, dict)
    ]
    members = []
    asset_hash_index = _asset_hash_index(asset_index)
    for container in report.get("containers", []):
        if not isinstance(container, dict):
            continue
        if container.get("status") != "pass":
            diagnostics.extend(
                diagnostic
                for diagnostic in container.get("diagnostics", [])
                if isinstance(diagnostic, dict)
            )
        source_container = str(container.get("relative_path", "")).strip()
        container_id = _safe_identifier(Path(source_container).with_suffix("").as_posix())
        for director_member in container.get("members", []):
            if not isinstance(director_member, dict):
                continue
            member_id = str(director_member.get("member_id", "")).strip()
            for child in director_member.get("child_resources", []):
                if not isinstance(child, dict):
                    continue
                child_resource_id = child.get("resource_id")
                try:
                    entry_id = f"{container_id}.{int(child_resource_id):04d}"
                except (TypeError, ValueError):
                    diagnostics.append(
                        {
                            "code": "TSUI_CAST_DIRECTOR_CHILD_RESOURCE_ID_INVALID",
                            "source": map_source,
                            "member_id": member_id or "unknown",
                            "message": "Director child resource id is not numeric",
                        }
                    )
                    continue
                source, source_diagnostics = _director_child_source_from_hash(
                    child,
                    container_id,
                    map_source,
                    member_id,
                    asset_hash_index,
                )
                diagnostics.extend(source_diagnostics)
                if not source:
                    continue
                metadata_kind = str(director_member.get("kind", "")).strip()
                if metadata_kind and metadata_kind not in CAST_MEMBER_KINDS:
                    diagnostics.append(
                        {
                            "code": "TSUI_CAST_DIRECTOR_MEMBER_KIND_INVALID",
                            "source": map_source,
                            "member_id": member_id or "unknown",
                            "message": "Director cast member kind is not part of the allowed classification set",
                        }
                    )
                route_ids = _safe_symbol_list(director_member.get("route_ids", []))
                if route_ids is None:
                    diagnostics.append(
                        {
                            "code": "TSUI_CAST_DIRECTOR_MEMBER_ROUTE_ID_INVALID",
                            "source": map_source,
                            "member_id": member_id or "unknown",
                            "message": "Director cast member route_ids must be safe symbols",
                        }
                    )
                    route_ids = []
                command_ids = _safe_symbol_list(director_member.get("command_ids", []))
                if command_ids is None:
                    diagnostics.append(
                        {
                            "code": "TSUI_CAST_DIRECTOR_MEMBER_COMMAND_ID_INVALID",
                            "source": map_source,
                            "member_id": member_id or "unknown",
                            "message": "Director cast member command_ids must be safe symbols",
                        }
                    )
                    command_ids = []
                parts = []
                if "parts" in director_member:
                    parts, part_diagnostics = _safe_atlas_parts(
                        director_member.get("parts"),
                        source=map_source,
                        owner_id=member_id or "unknown",
                        source_field="source",
                        code_prefix="TSUI_CAST_DIRECTOR_MEMBER",
                    )
                    diagnostics.extend(part_diagnostics)
                elif metadata_kind == "character_atlas":
                    diagnostics.append(
                        {
                            "code": "TSUI_CAST_DIRECTOR_MEMBER_ATLAS_PARTS_MISSING",
                            "source": map_source,
                            "member_id": member_id or "unknown",
                            "message": "character_atlas director member must include crop/part records",
                        }
                    )
                kind = metadata_kind if metadata_kind in CAST_MEMBER_KINDS else _director_child_kind(child, asset_index[source], root)
                raw_member = {
                    "member_id": member_id,
                    "kind": kind,
                    "source": source,
                    "container_entry_id": entry_id,
                    "director_child_resource_id": child_resource_id,
                    "director_child_tag": child.get("tag", ""),
                    "director_child_payload_sha256": child.get("payload_sha256", ""),
                    "route_ids": route_ids,
                    "command_ids": command_ids,
                }
                if parts:
                    raw_member["parts"] = parts
                member, member_diagnostics = _cast_member_from_map(raw_member, map_source, asset_index)
                if member:
                    members.append(member)
                diagnostics.extend(member_diagnostics)
    return members, diagnostics


def _asset_hash_index(asset_index: dict[str, Path]) -> dict[str, list[str]]:
    index: dict[str, list[str]] = {}
    for rel, path in asset_index.items():
        index.setdefault(_sha256(path), []).append(rel)
    return {digest: sorted(paths) for digest, paths in index.items()}


def _director_child_source_from_hash(
    child: dict,
    container_id: str,
    map_source: str,
    member_id: str,
    asset_hash_index: dict[str, list[str]],
) -> tuple[str, list[dict]]:
    diagnostics = []
    payload_hash = str(child.get("payload_sha256", "")).strip()
    if not payload_hash.startswith("sha256:"):
        return "", [
            {
                "code": "TSUI_CAST_DIRECTOR_CHILD_HASH_MISSING",
                "source": map_source,
                "member_id": member_id or "unknown",
                "resource_id": child.get("resource_id", "unknown"),
                "message": "Director child resource requires a sanitized payload hash",
            }
        ]
    candidates = asset_hash_index.get(payload_hash, [])
    container_prefix = f"containers/{container_id}/"
    scoped_candidates = [candidate for candidate in candidates if candidate.startswith(container_prefix)]
    if not scoped_candidates:
        return "", [
            {
                "code": "TSUI_CAST_DIRECTOR_CHILD_SOURCE_MISSING",
                "source": map_source,
                "member_id": member_id or "unknown",
                "resource_id": child.get("resource_id", "unknown"),
                "payload_sha256": payload_hash,
                "message": "Director child resource was not found among extracted readable assets",
            }
        ]
    if len(scoped_candidates) > 1:
        diagnostics.append(
            {
                "code": "TSUI_CAST_DIRECTOR_CHILD_SOURCE_AMBIGUOUS",
                "source": map_source,
                "member_id": member_id or "unknown",
                "resource_id": child.get("resource_id", "unknown"),
                "candidate_count": len(scoped_candidates),
                "message": "Director child resource hash matches multiple extracted assets in the same container",
            }
        )
    return scoped_candidates[0], diagnostics


def _director_child_kind(child: dict, source_path: Path, root: Path) -> str:
    tag = str(child.get("tag", "")).strip()
    if tag in SCRIPT_TEXT_CHUNK_IDS:
        return "script"
    if source_path.suffix.lower() in AUDIO_EXTS:
        return "audio"
    if source_path.suffix.lower() in MOVIE_EXTS:
        return "movie"
    if source_path.suffix.lower() in FONT_EXTS:
        return "font"
    if source_path.suffix.lower() in IMAGE_EXTS:
        classification = analyze_asset(source_path, root).get("classification", "unknown")
        if classification in CAST_MEMBER_KINDS:
            return classification
    return "unknown"


def rearrange_native_assets(unpacked_root: Path | str, work_root: Path | str, asset_analysis: dict) -> dict:
    unpacked_root = Path(unpacked_root)
    work_root = Path(work_root)
    diagnostics = []
    resources = []
    if asset_analysis.get("status") != "pass":
        diagnostics.append(
            {
                "code": "TSUI_NATIVE_ASSET_ANALYSIS_BLOCKED",
                "message": "native asset rearrange requires a passing asset analysis report",
            }
        )
    if not unpacked_root.is_dir():
        diagnostics.append(
            {
                "code": "TSUI_NATIVE_ASSET_UNPACKED_MISSING",
                "message": "native asset rearrange requires an unpacked asset root",
            }
        )

    if not diagnostics:
        for asset in asset_analysis.get("assets", []):
            source = str(asset.get("relative_path", "")).strip()
            classification = str(asset.get("classification", "unknown")).strip()
            bucket = NATIVE_ASSET_BUCKETS.get(classification)
            if not bucket:
                diagnostics.append(
                    {
                        "code": "TSUI_NATIVE_ASSET_CLASSIFICATION_UNSUPPORTED",
                        "source": source or "unknown",
                        "classification": classification or "unknown",
                        "message": "asset classification cannot be written into native-assets",
                    }
                )
                continue
            if not _is_safe_report_relative_path(source):
                diagnostics.append(
                    {
                        "code": "TSUI_NATIVE_ASSET_SOURCE_PATH_INVALID",
                        "source": source or "unknown",
                        "message": "native asset source must be report-relative",
                    }
                )
                continue
            source_path = unpacked_root / source
            if not source_path.is_file():
                diagnostics.append(
                    {
                        "code": "TSUI_NATIVE_ASSET_SOURCE_MISSING",
                        "source": source,
                        "message": "asset analysis source is missing from unpacked assets",
                    }
                )
                continue
            native_rel = f"native-assets/{bucket}/{source}"
            if not _is_safe_report_relative_path(native_rel):
                diagnostics.append(
                    {
                        "code": "TSUI_NATIVE_ASSET_OUTPUT_PATH_INVALID",
                        "source": source,
                        "message": "native asset output path is not report-relative",
                    }
                )
                continue
            target_path = work_root / native_rel
            target_path.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source_path, target_path)
            source_hash = asset.get("sha256", _sha256(source_path))
            converted_hash = _sha256(target_path)
            if source_hash != converted_hash:
                diagnostics.append(
                    {
                        "code": "TSUI_NATIVE_ASSET_HASH_MISMATCH",
                        "source": source,
                        "native_path": native_rel,
                        "message": "copied native asset hash does not match the analyzed source hash",
                    }
                )
            resources.append(
                {
                    "source": source,
                    "native_path": native_rel,
                    "classification": classification,
                    "source_hash": source_hash,
                    "converted_hash": converted_hash,
                    "byte_size": target_path.stat().st_size,
                    "coverage_status": "converted",
                }
            )

    report = {
        "schema": "tsuinosora.native_asset_rearrange_report.v1",
        "status": "blocked" if diagnostics else "pass",
        "output_root": "local_work_root/native-assets",
        "converted_assets": len(resources),
        "resources": resources,
        "diagnostics": _dedupe_diagnostics(diagnostics),
        "redaction": {
            "paths": "report_relative_only",
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
                "code": "TSUI_NATIVE_ASSET_REARRANGE_PATH_LEAK",
                "message": "native asset rearrange report contains a local path-like value",
            }
        )
    return report


def build_conversion_report(
    source_inventory: dict,
    asset_analysis: dict,
    routes: list[dict],
    native_asset_report: dict | None = None,
) -> dict:
    diagnostics = []
    if asset_analysis.get("status") == "blocked" or asset_analysis.get("quarantine"):
        diagnostics.append(
            {
                "code": "TSUI_CONVERSION_ASSET_QUARANTINE",
                "message": "asset analysis has quarantine entries; conversion is blocked",
            }
        )
    if not routes:
        diagnostics.append(
            {
                "code": "TSUI_CONVERSION_ROUTE_COVERAGE_MISSING",
                "message": "conversion report requires at least one covered route",
            }
        )
    for route in routes:
        if route.get("coverage") != "covered":
            diagnostics.append(
                {
                    "code": "TSUI_CONVERSION_ROUTE_COVERAGE_MISSING",
                    "route_id": route.get("route_id", "unknown"),
                    "message": "route coverage is not proven",
                }
            )

    native_resources = []
    if native_asset_report is not None:
        native_resources = list(native_asset_report.get("resources", []))
        if native_asset_report.get("status") != "pass":
            diagnostics.append(
                {
                    "code": "TSUI_CONVERSION_NATIVE_ASSET_REARRANGE_BLOCKED",
                    "message": "conversion requires native-assets rearrange evidence",
                }
            )
            diagnostics.extend(native_asset_report.get("diagnostics", []))

    alias = source_inventory.get("root_alias", "original_install_root")
    diagnostics = _dedupe_diagnostics(diagnostics)
    return {
        "schema": "tsuinosora.conversion_report.v1",
        "status": "blocked" if diagnostics else "pass",
        "inputs": {
            alias: alias,
        },
        "counts": {
            "source_files": source_inventory.get("file_count", len(source_inventory.get("files", []))),
            "asset_count": len(asset_analysis.get("assets", [])),
            "quarantine_count": len(asset_analysis.get("quarantine", [])),
            "route_count": len(routes),
            "converted_assets": len(native_resources),
            "missing_assets": max(len(asset_analysis.get("assets", [])) - len(native_resources), 0),
        },
        "routes": [
            _conversion_route_record(route)
            for route in routes
        ],
        "resources": native_resources,
        "diagnostics": diagnostics,
        "redaction": {
            "paths": "alias_only",
            "payload": "omitted",
            "commercial_text": "omitted",
            "screenshots": "omitted",
            "audio": "omitted",
            "movie": "omitted",
        },
    }


def _conversion_route_record(route: dict) -> dict:
    route_id = str(route.get("route_id", "unknown")).strip() or "unknown"
    record = {
        "route_id": route_id,
        "coverage": route.get("coverage", "unknown"),
        "terminal": route.get("terminal", ""),
    }
    choices = [
        str(choice).strip()
        for choice in route.get("choices", []) or []
        if str(choice).strip() and _is_safe_symbol(str(choice).strip())
    ]
    if choices:
        record["choices"] = choices
    mount_assets, _ = _route_mount_assets("tsuinosora-patch-game", "windows", route, route_id)
    if mount_assets:
        record["mount_assets"] = mount_assets
    return record


def build_modern_profile_report(conversion_report: dict, features: list[dict]) -> dict:
    diagnostics = []
    if conversion_report.get("status") != "pass":
        diagnostics.append(
            {
                "code": "TSUI_MODERN_BASE_CONVERSION_BLOCKED",
                "message": "modern profile requires a passing classic conversion report",
            }
        )
    if not features:
        diagnostics.append(
            {
                "code": "TSUI_MODERN_FEATURES_MISSING",
                "message": "modern profile requires at least one reversible feature with fallback evidence",
            }
        )

    sanitized_features = []
    for feature in features:
        feature_id = feature.get("feature_id", "unknown")
        fallback_hash = feature.get("fallback_hash", "")
        independent_switch = bool(feature.get("independent_switch", False))
        affects_core_state = bool(feature.get("affects_core_state", False))
        if affects_core_state:
            diagnostics.append(
                {
                    "code": "TSUI_MODERN_CORE_STATE_CHANGE",
                    "feature_id": feature_id,
                    "message": "modern feature must not change route, save/replay, backlog or read-state",
                }
            )
        if not independent_switch:
            diagnostics.append(
                {
                    "code": "TSUI_MODERN_SWITCH_MISSING",
                    "feature_id": feature_id,
                    "message": "modern feature requires an independent profile switch",
                }
            )
        if not fallback_hash:
            diagnostics.append(
                {
                    "code": "TSUI_MODERN_FALLBACK_MISSING",
                    "feature_id": feature_id,
                    "message": "modern feature requires fallback hash evidence",
                }
            )
        for hash_key in ["input_hash", "output_hash", "fallback_hash"]:
            value = feature.get(hash_key, "")
            if value and (_looks_like_local_path(str(value)) or not str(value).startswith("sha256:")):
                diagnostics.append(
                    {
                        "code": "TSUI_MODERN_HASH_EVIDENCE_INVALID",
                        "feature_id": feature_id,
                        "field": hash_key,
                        "message": "modern profile evidence must be sanitized sha256 hashes",
                    }
                )
        sanitized_features.append(
            {
                "feature_id": feature_id,
                "feature_kind": feature.get("feature_kind", "unknown"),
                "input_hash": feature.get("input_hash", ""),
                "output_hash": feature.get("output_hash", ""),
                "fallback_hash": fallback_hash,
                "independent_switch": independent_switch,
                "affects_core_state": affects_core_state,
            }
        )

    diagnostics = _dedupe_diagnostics(diagnostics)
    return {
        "schema": "tsuinosora.modern_profile_report.v1",
        "status": "blocked" if diagnostics else "pass",
        "base_conversion_status": conversion_report.get("status", "unknown"),
        "counts": {
            "feature_count": len(features),
            "route_count": len(conversion_report.get("routes", [])),
        },
        "features": sanitized_features,
        "diagnostics": diagnostics,
        "redaction": {
            "paths": "alias_or_hash_only",
            "payload": "omitted",
            "commercial_text": "omitted",
            "screenshots": "omitted",
            "audio": "omitted",
            "movie": "omitted",
        },
    }


def build_route_scenarios(target: str, profile: str, platform: str, routes: list[dict]) -> dict:
    scenarios = []
    diagnostics = []
    for route in routes:
        route_id = route.get("route_id", "classic.main")
        terminal = route.get("terminal", route_id)
        actions = [{"launch": {}}]
        for choice in route.get("choices", []):
            actions.append({"player_input": {"kind": "advance"}})
            actions.append({"player_input": {"kind": "choose", "value": choice}})
        actions.append({"player_input": {"kind": "advance"}})
        actions.append({"replay_from_start": {}})
        scenario = {
            "schema": "astra.scenario.v1",
            "stage": "stage3-astra-vn",
            "target": target,
            "profile": profile,
            "platform": platform,
            "generated_route_id": route_id,
            "seed": 42,
            "actions": actions,
            "assertions": [
                {"coverage": {"routes": [terminal]}},
                {"replay_hash_match": True},
                {"no_blocking_diagnostics": True},
            ],
        }
        mount_assets, asset_diagnostics = _route_mount_assets(target, platform, route, route_id)
        diagnostics.extend(asset_diagnostics)
        if mount_assets:
            scenario["mount_assets"] = mount_assets
        scenarios.append(scenario)
    return {
        "schema": "astra.scenario_refs.v1",
        "status": "blocked" if diagnostics else "pass",
        "target": target,
        "profile": profile,
        "platform": platform,
        "scenarios": scenarios,
        "diagnostics": _dedupe_diagnostics(diagnostics),
    }


def _route_mount_assets(target: str, platform: str, route: dict, route_id: str) -> tuple[list[dict], list[dict]]:
    if target != "tsuinosora-patch-game" or platform != "windows":
        return [], []
    diagnostics = []
    assets = []
    for index, raw in enumerate(route.get("mount_assets", [])):
        if not isinstance(raw, dict):
            diagnostics.append(
                {
                    "code": "TSUI_ROUTE_MOUNT_ASSET_INVALID",
                    "route_id": route_id,
                    "index": index,
                    "message": "mount asset entry must be an object",
                }
            )
            continue
        alias = str(raw.get("alias", "")).strip()
        rel_path = str(raw.get("path", "")).replace("\\", "/").strip()
        role = str(raw.get("role", "")).strip()
        asset_route_id = str(raw.get("route_id", route_id)).strip()
        digest = str(raw.get("sha256", "")).strip()
        if not role or role not in MOUNT_ASSET_ROLES:
            diagnostics.append(
                {
                    "code": "TSUI_ROUTE_MOUNT_ASSET_ROLE_INVALID",
                    "route_id": route_id,
                    "index": index,
                    "role": role or "unknown",
                    "message": "mount asset role must match an asset analysis classification",
                }
            )
            continue
        if (
            not _is_safe_symbol(alias)
            or not _is_safe_report_relative_path(rel_path)
            or asset_route_id != route_id
            or not _is_sanitized_sha256(digest)
        ):
            diagnostics.append(
                {
                    "code": "TSUI_ROUTE_MOUNT_ASSET_UNSAFE",
                    "route_id": route_id,
                    "index": index,
                    "message": "mount asset evidence must use safe alias/path/role, matching route id and sanitized sha256",
                }
            )
            continue
        assets.append(
            {
                "alias": alias,
                "path": rel_path,
                "role": role,
                "route_id": asset_route_id,
                "sha256": digest,
            }
        )
    return assets, diagnostics


def _routes_with_native_mount_assets(
    routes: list[dict],
    cast_source_map_report: dict | None,
    native_asset_report: dict | None,
) -> tuple[list[dict], list[dict]]:
    if not routes:
        return routes, []
    if not cast_source_map_report or cast_source_map_report.get("status") != "pass":
        return routes, []
    if not native_asset_report or native_asset_report.get("status") != "pass":
        return routes, []

    resources_by_source = {
        str(resource.get("source", "")): resource
        for resource in native_asset_report.get("resources", [])
        if isinstance(resource, dict) and str(resource.get("source", ""))
    }
    diagnostics = []
    enriched = []
    for route in routes:
        route_copy = dict(route)
        route_id = str(route_copy.get("route_id", "")).strip()
        command_ids = {str(value).strip() for value in route_copy.get("command_ids", []) if str(value).strip()}
        mount_assets = [
            dict(asset)
            for asset in route_copy.get("mount_assets", [])
            if isinstance(asset, dict)
        ]
        existing = {
            (
                str(asset.get("alias", "")),
                str(asset.get("path", "")),
                str(asset.get("role", "")),
                str(asset.get("sha256", "")),
            )
            for asset in mount_assets
        }
        for member in cast_source_map_report.get("members", []):
            if not isinstance(member, dict):
                continue
            member_routes = {str(value).strip() for value in member.get("route_ids", []) if str(value).strip()}
            member_commands = {str(value).strip() for value in member.get("command_ids", []) if str(value).strip()}
            if route_id not in member_routes and not (command_ids and member_commands & command_ids):
                continue
            role = str(member.get("kind", "")).strip()
            source = str(member.get("source", "")).strip()
            member_id = str(member.get("member_id", "")).strip() or source or "asset"
            if role not in MOUNT_ASSET_ROLES:
                diagnostics.append(
                    {
                        "code": "TSUI_ROUTE_MOUNT_ASSET_ROLE_INVALID",
                        "route_id": route_id or "unknown",
                        "member_id": member_id,
                        "role": role or "unknown",
                        "message": "route-bound cast member cannot be used as a patch mount asset",
                    }
                )
                continue
            resource = resources_by_source.get(source)
            if not resource:
                diagnostics.append(
                    {
                        "code": "TSUI_ROUTE_MOUNT_ASSET_SOURCE_UNCONVERTED",
                        "route_id": route_id or "unknown",
                        "member_id": member_id,
                        "source": source or "unknown",
                        "message": "route-bound cast member was not converted into native-assets",
                    }
                )
                continue
            member_hash = str(member.get("source_hash", "")).strip()
            resource_hash = str(resource.get("source_hash", "")).strip()
            converted_hash = str(resource.get("converted_hash", "")).strip()
            native_path = str(resource.get("native_path", "")).strip()
            if member_hash and resource_hash and member_hash != resource_hash:
                diagnostics.append(
                    {
                        "code": "TSUI_ROUTE_MOUNT_ASSET_HASH_MISMATCH",
                        "route_id": route_id or "unknown",
                        "member_id": member_id,
                        "source": source,
                        "message": "route-bound cast member hash does not match native asset source hash",
                    }
                )
                continue
            mount_asset = {
                "alias": "original",
                "path": native_path,
                "role": role,
                "route_id": route_id,
                "sha256": converted_hash,
            }
            validated, validation_diagnostics = _route_mount_assets(
                "tsuinosora-patch-game",
                "windows",
                {"mount_assets": [mount_asset]},
                route_id,
            )
            if validation_diagnostics:
                diagnostics.extend(validation_diagnostics)
                continue
            mount_asset = validated[0]
            key = (
                mount_asset["alias"],
                mount_asset["path"],
                mount_asset["role"],
                mount_asset["sha256"],
            )
            if key not in existing:
                mount_assets.append(mount_asset)
                existing.add(key)
        if mount_assets:
            route_copy["mount_assets"] = mount_assets
        enriched.append(route_copy)
    return enriched, _dedupe_diagnostics(diagnostics)


def build_mount_policy(target: str, aliases: dict[str, str]) -> dict:
    diagnostics = []
    entries = []
    for alias, value in sorted(aliases.items()):
        if _looks_like_local_path(value) or not _is_safe_symbol(value):
            diagnostics.append(
                {
                    "code": "TSUI_MOUNT_ALIAS_PATH_LEAK",
                    "alias": alias,
                    "message": "mount policy values must be sanitized aliases, not local paths or traversal values",
                }
            )
            continue
        entries.append(
            {
                "alias": alias,
                "value": value,
                "hash_policy": "manifest_required",
                "fallback": "blocking",
            }
        )
    diagnostics = _dedupe_diagnostics(diagnostics)
    return {
        "schema": "tsuinosora.mount_policy.v1",
        "target": target,
        "status": "blocked" if diagnostics else "pass",
        "aliases": entries,
        "diagnostics": diagnostics,
    }


def _normalize_stage3_targets(targets: list[dict] | None) -> list[dict]:
    normalized = []
    source = targets or DEFAULT_STAGE3_TARGETS
    for raw in source:
        if not isinstance(raw, dict):
            continue
        target = str(raw.get("target", "")).strip()
        if target not in {"tsuinosora-internal-game", "tsuinosora-patch-game"}:
            continue
        profiles = [
            str(profile).strip()
            for profile in raw.get("profiles", [])
            if str(profile).strip() in {"classic", "modern"}
        ]
        platforms = [
            str(platform).strip()
            for platform in raw.get("platforms", [])
            if str(platform).strip() in {"headless", "windows", "web"}
        ]
        if not profiles:
            profiles = ["classic"]
        if not platforms:
            platforms = ["headless"]
        normalized.append(
            {
                "target": target,
                "profiles": list(dict.fromkeys(profiles)),
                "platforms": list(dict.fromkeys(platforms)),
            }
        )
    if normalized:
        return normalized
    return [dict(spec) for spec in DEFAULT_STAGE3_TARGETS]


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


def build_stage3_gate_report(
    original_root: Path | str,
    work_root: Path | str,
    title_png: Path | str,
    game_png: Path | str,
    remake_root: Path | str | None = None,
    unpacked_root: Path | str | None = None,
    routes: list[dict] | None = None,
    modern_features: list[dict] | None = None,
    targets: list[dict] | None = None,
    external_reader_report: dict | None = None,
) -> dict:
    original_root = Path(original_root)
    work_root = Path(work_root)
    title_png = Path(title_png)
    game_png = Path(game_png)
    remake_root = Path(remake_root) if remake_root else None
    unpacked_root = Path(unpacked_root) if unpacked_root else None
    routes = routes or []
    modern_features = modern_features or []
    target_specs = _normalize_stage3_targets(targets)
    target_names = {spec["target"] for spec in target_specs}
    requires_modern = any("modern" in spec["profiles"] for spec in target_specs)
    reports_root = work_root / "reports"
    reports_root.mkdir(parents=True, exist_ok=True)

    diagnostics = []
    diagnostics.extend(_source_root_diagnostics(original_root, "original_install_root"))
    if remake_root:
        diagnostics.extend(_source_root_diagnostics(remake_root, "remake_install_root", require_director=False))

    expected_hashes, expected_dimensions = _authoritative_reference_expectations(title_png, game_png)
    reference_report = build_visual_reference_report(
        title_png,
        game_png,
        expected_hashes=expected_hashes,
        expected_dimensions=expected_dimensions,
    )
    if reference_report.get("status") != "pass":
        diagnostics.extend(reference_report.get("diagnostics", []))
    _write_json(reports_root / "reference_evidence.json", reference_report)

    if original_root.is_dir():
        original_inventory = build_source_inventory(original_root, "original_install_root")
    else:
        original_inventory = _empty_inventory("original_install_root")
    _write_json(reports_root / "source_inventory.original.json", original_inventory)

    remake_inventory = None
    if remake_root:
        remake_inventory = (
            build_source_inventory(remake_root, "remake_install_root")
            if remake_root.is_dir()
            else _empty_inventory("remake_install_root")
        )
        _write_json(reports_root / "source_inventory.remake.json", remake_inventory)

    extract_report = None
    if not unpacked_root and original_root.is_dir():
        extract_report = extract_readable_assets(original_root, work_root, "original_install_root")
        if extract_report.get("extracted_count", 0) > 0:
            unpacked_root = work_root / "unpacked"
        if extract_report.get("status") != "pass":
            diagnostics.extend(
                _extract_diagnostics_after_external_reader(
                    extract_report.get("diagnostics", []),
                    external_reader_report,
                )
            )

    if unpacked_root and unpacked_root.is_dir():
        asset_analysis = analyze_assets(unpacked_root, reference_report)
    else:
        asset_analysis = _blocked_asset_analysis(
            reference_report,
            "TSUI_UNPACKED_ROOT_MISSING",
            "unpacked assets are required before native-assets rearrange and conversion",
        )
    _write_json(reports_root / "asset_analysis.json", asset_analysis)

    if unpacked_root and unpacked_root.is_dir():
        native_asset_report = rearrange_native_assets(unpacked_root, work_root, asset_analysis)
    else:
        native_asset_report = rearrange_native_assets(work_root / "missing-unpacked", work_root, asset_analysis)
    _write_json(reports_root / "native_asset_rearrange_report.json", native_asset_report)

    cast_source_map_report = None
    if unpacked_root and unpacked_root.is_dir():
        cast_source_map_report = build_cast_source_map_report(unpacked_root)
        _write_json(reports_root / "cast_source_map_report.json", cast_source_map_report)
        if cast_source_map_report.get("status") != "pass":
            diagnostics.extend(cast_source_map_report.get("diagnostics", []))

    route_graph_report = None
    script_source_map_report = None
    if not routes and unpacked_root and unpacked_root.is_dir():
        route_graph_report = build_route_graph_report(unpacked_root)
        _write_json(reports_root / "route_graph_report.json", route_graph_report)
        if route_graph_report.get("status") == "pass":
            routes = route_graph_report.get("routes", [])
        else:
            route_graph_diagnostics = route_graph_report.get("diagnostics", [])
            route_graph_has_invalid_sidecar = any(
                diagnostic.get("code") != "TSUI_ROUTE_GRAPH_MISSING"
                for diagnostic in route_graph_diagnostics
            )
            if route_graph_has_invalid_sidecar:
                diagnostics.extend(route_graph_diagnostics)
            script_source_map_report = build_script_source_map_report(unpacked_root)
            _write_json(reports_root / "script_source_map_report.json", script_source_map_report)
            if script_source_map_report.get("status") == "pass":
                routes = script_source_map_report.get("routes", [])
            else:
                if not route_graph_has_invalid_sidecar:
                    diagnostics.extend(route_graph_diagnostics)
                diagnostics.extend(script_source_map_report.get("diagnostics", []))

    routes, route_asset_diagnostics = _routes_with_native_mount_assets(
        routes,
        cast_source_map_report,
        native_asset_report,
    )
    diagnostics.extend(route_asset_diagnostics)

    conversion_report = build_conversion_report(original_inventory, asset_analysis, routes, native_asset_report)
    if diagnostics:
        conversion_report["status"] = "blocked"
        conversion_report.setdefault("diagnostics", []).extend(diagnostics)
        conversion_report["diagnostics"] = _dedupe_diagnostics(conversion_report["diagnostics"])
    _write_json(reports_root / "conversion_report.json", conversion_report)

    modern_profile_path = reports_root / "modern_profile_report.json"
    if requires_modern:
        modern_profile_report = build_modern_profile_report(conversion_report, modern_features)
        _write_json(modern_profile_path, modern_profile_report)
    else:
        modern_profile_report = {"status": "skipped", "diagnostics": []}
        try:
            modern_profile_path.unlink()
        except OSError:
            pass

    for stale_policy in [
        reports_root / "mount_policy.tsuinosora-internal-game.json",
        reports_root / "mount_policy.tsuinosora-patch-game.json",
    ]:
        try:
            stale_policy.unlink()
        except OSError:
            pass

    mount_policies = []
    if "tsuinosora-internal-game" in target_names:
        mount_policies.append(
            build_mount_policy(
                "tsuinosora-internal-game",
                {
                    "original": "original_install_root",
                    "remake": "remake_install_root" if remake_root else "remake_install_root.optional",
                    "local_work": "local_work_root",
                },
            )
        )
    if "tsuinosora-patch-game" in target_names:
        mount_policies.append(
            build_mount_policy(
                "tsuinosora-patch-game",
                {
                    "original": "original_install_root",
                    "remake": "remake_install_root" if remake_root else "remake_install_root.optional",
                },
            )
        )
    for policy in mount_policies:
        _write_json(reports_root / f"mount_policy.{policy['target']}.json", policy)

    scenario_ref_reports = []
    for target_spec in target_specs:
        target = target_spec["target"]
        for profile in target_spec["profiles"]:
            for platform in target_spec["platforms"]:
                scenarios = build_route_scenarios(target, profile, platform, routes)
                name = f"scenario_refs.{target}.{profile}.{platform}.json"
                _write_json(reports_root / name, scenarios)
                scenario_ref_reports.append(
                    {
                        "target": target,
                        "profile": profile,
                        "platform": platform,
                        "report": f"reports/{name}",
                        "route_count": len(routes),
                    }
                )

    report_diagnostics = _dedupe_diagnostics(
        diagnostics
        + asset_analysis.get("diagnostics", [])
        + native_asset_report.get("diagnostics", [])
        + conversion_report.get("diagnostics", [])
        + modern_profile_report.get("diagnostics", [])
        + [diag for policy in mount_policies for diag in policy.get("diagnostics", [])]
    )

    report = {
        "schema": "tsuinosora.stage3_gate_report.v1",
        "status": "pass",
        "input_aliases": {
            "original": "original_install_root",
            "remake": "remake_install_root" if remake_root else "remake_install_root.optional",
            "local_work": "local_work_root",
            "unpacked": "local_work_root/unpacked",
        },
        "reports": {
            "reference_evidence": "reports/reference_evidence.json",
            "source_inventory_original": "reports/source_inventory.original.json",
            "source_inventory_remake": "reports/source_inventory.remake.json" if remake_inventory else "",
            "extract_report": "reports/extract_report.json" if extract_report else "",
            "external_reader_report": "reports/projectorrays_reader_report.json" if external_reader_report else "",
            "cast_source_map_report": "reports/cast_source_map_report.json" if cast_source_map_report else "",
            "route_graph_report": "reports/route_graph_report.json" if route_graph_report else "",
            "script_source_map_report": "reports/script_source_map_report.json" if script_source_map_report else "",
            "asset_analysis": "reports/asset_analysis.json",
            "native_asset_rearrange": "reports/native_asset_rearrange_report.json",
            "conversion_report": "reports/conversion_report.json",
            "modern_profile_report": "reports/modern_profile_report.json" if requires_modern else "",
        },
        "targets": target_specs,
        "scenario_refs": scenario_ref_reports,
        "diagnostics": report_diagnostics,
        "redaction": {
            "paths": "alias_or_report_relative_only",
            "payload": "omitted",
            "commercial_text": "omitted",
            "screenshots": "omitted",
            "audio": "omitted",
            "movie": "omitted",
        },
    }
    if (
        diagnostics
        or native_asset_report.get("status") != "pass"
        or conversion_report.get("status") != "pass"
        or (requires_modern and modern_profile_report.get("status") != "pass")
        or any(policy.get("status") != "pass" for policy in mount_policies)
        or _report_has_path_leak(report)
    ):
        report["status"] = "blocked"
    if _report_has_path_leak(report):
        report.setdefault("diagnostics", []).append(
            {
                "code": "TSUI_REPORT_PATH_LEAK",
                "message": "stage3 gate report contains a local path-like value",
            }
        )
        report["diagnostics"] = _dedupe_diagnostics(report["diagnostics"])
    _write_json(reports_root / "stage3_gate_report.json", report)
    return report


def _authoritative_reference_expectations(
    title_png: Path,
    game_png: Path,
) -> tuple[dict[str, str], dict[str, dict[str, int]]]:
    hashes = {}
    dimensions = {}
    if _is_authoritative_reference_path(title_png, "Title.png"):
        hashes["title"] = TSUINOSORA_REFERENCE_HASHES["title"]
        dimensions["title"] = TSUINOSORA_REFERENCE_DIMENSIONS["title"]
    if _is_authoritative_reference_path(game_png, "Game.png"):
        hashes["game"] = TSUINOSORA_REFERENCE_HASHES["game"]
        dimensions["game"] = TSUINOSORA_REFERENCE_DIMENSIONS["game"]
    return hashes, dimensions


def _is_authoritative_reference_path(path: Path, file_name: str) -> bool:
    normalized = path.as_posix().replace("\\", "/")
    return normalized.endswith(f"Examples/TsuiNoSora/Docs/{file_name}")


def run_local_gate(
    original_root: Path | str,
    work_root: Path | str,
    title_png: Path | str,
    game_png: Path | str,
    remake_root: Path | str | None = None,
    unpacked_root: Path | str | None = None,
    routes: list[dict] | None = None,
    modern_features: list[dict] | None = None,
    targets: list[dict] | None = None,
    external_reader_report: dict | None = None,
) -> dict:
    work_root = Path(work_root)
    reports_root = work_root / "reports"
    explicit_routes = list(routes or [])
    route_evidence_diagnostics = []
    if explicit_routes:
        route_evidence_diagnostics.append(
            {
                "code": "TSUI_LOCAL_GATE_ROUTE_EVIDENCE_REQUIRED",
                "message": "local gate requires route graph or script source-map report evidence; explicit routes cannot substitute commercial route coverage",
            }
        )
    stage3_report = build_stage3_gate_report(
        original_root=original_root,
        work_root=work_root,
        title_png=title_png,
        game_png=game_png,
        remake_root=remake_root,
        unpacked_root=unpacked_root,
        routes=[],
        modern_features=modern_features,
        targets=targets,
        external_reader_report=external_reader_report,
    )
    diagnostics = route_evidence_diagnostics + list(stage3_report.get("diagnostics", []))
    nativevn_report = None
    route_count = 0
    if not route_evidence_diagnostics and stage3_report.get("status") == "pass":
        nativevn_report = write_nativevn_package_input(work_root)
        diagnostics.extend(nativevn_report.get("diagnostics", []))
        route_count = int(nativevn_report.get("route_count", route_count))
    elif stage3_report.get("status") != "pass":
        diagnostics.append(
            {
                "code": "TSUI_LOCAL_GATE_STAGE3_BLOCKED",
                "message": "local gate cannot write NativeVN package input until stage3 gate passes",
            }
        )

    report = {
        "schema": "tsuinosora.local_gate_report.v1",
        "status": "pass",
        "reports": {
            "stage3_gate": "reports/stage3_gate_report.json",
            "nativevn_package_input": "reports/nativevn_package_input_report.json" if nativevn_report else "",
        },
        "targets": stage3_report.get("targets", []),
        "route_count": route_count,
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
    if (
        route_evidence_diagnostics
        or stage3_report.get("status") != "pass"
        or (nativevn_report and nativevn_report.get("status") != "pass")
    ):
        report["status"] = "blocked"
    if _report_has_path_leak(report):
        report["status"] = "blocked"
        report["diagnostics"].append(
            {
                "code": "TSUI_LOCAL_GATE_REPORT_PATH_LEAK",
                "message": "local gate report contains a local path-like value",
            }
        )
    report["diagnostics"] = _dedupe_diagnostics(report["diagnostics"])
    _write_json(reports_root / "local_gate_report.json", report)
    return report


def run_demo_slice_gate(config_path: Path | str) -> dict:
    config_path = Path(config_path)
    config, config_diagnostics = _read_demo_slice_config(config_path)
    work_root_value = str(config.get("local_work_root", "")).strip() if isinstance(config, dict) else ""
    work_root = Path(work_root_value) if work_root_value else None

    local_report = None
    projectorrays_report = None
    diagnostics = list(config_diagnostics)
    if not diagnostics:
        projectorrays_report = _run_projectorrays_from_demo_config(config)
        if projectorrays_report:
            diagnostics.extend(projectorrays_report.get("diagnostics", []))
            if projectorrays_report.get("status") != "pass":
                diagnostics.append(
                    {
                        "code": "TSUI_DEMO_SLICE_PROJECTORRAYS_BLOCKED",
                        "message": "ProjectorRays reader evidence is configured but did not pass",
                    }
                )
    if not diagnostics:
        local_report = run_local_gate(
            original_root=Path(str(config["original_install_root"])),
            work_root=Path(str(config["local_work_root"])),
            title_png=Path(str(config.get("title_png", "Examples/TsuiNoSora/Docs/Title.png"))),
            game_png=Path(str(config.get("game_png", "Examples/TsuiNoSora/Docs/Game.png"))),
            remake_root=Path(str(config["remake_install_root"])) if config.get("remake_install_root") else None,
            unpacked_root=Path(str(config["unpacked_root"])) if config.get("unpacked_root") else None,
            routes=[],
            modern_features=list(config.get("modern_features", [])),
            targets=INTERNAL_DEMO_STAGE3_TARGETS,
            external_reader_report=projectorrays_report,
        )
        diagnostics.extend(local_report.get("diagnostics", []))

    nativevn_package_input = ""
    route_count = 0
    targets = []
    if local_report:
        nativevn_package_input = local_report.get("reports", {}).get("nativevn_package_input", "")
        route_count = int(local_report.get("route_count", 0))
        targets = local_report.get("targets", [])

    report = {
        "schema": "tsuinosora.demo_slice_report.v1",
        "mode": "demo-slice",
        "status": "pass",
        "input_aliases": {
            "original": "original_install_root",
            "remake": "remake_install_root" if isinstance(config, dict) and config.get("remake_install_root") else "remake_install_root.optional",
            "local_work": "local_work_root",
            "unpacked": "local_work_root/unpacked",
        },
        "reports": {
            "projectorrays_reader": "reports/projectorrays_reader_report.json" if projectorrays_report else "",
            "local_gate": "reports/local_gate_report.json" if local_report else "",
            "stage3_gate": "reports/stage3_gate_report.json" if local_report else "",
            "nativevn_package_input": nativevn_package_input,
        },
        "targets": targets,
        "route_count": route_count,
        "automation_targets": _normalize_stage3_targets(INTERNAL_DEMO_STAGE3_TARGETS),
        "diagnostics": _dedupe_diagnostics(diagnostics),
        "redaction": {
            "paths": "alias_or_report_relative_only",
            "payload": "omitted",
            "commercial_text": "omitted",
            "screenshots": "omitted",
            "audio": "omitted",
            "movie": "omitted",
        },
    }
    if diagnostics or not local_report or local_report.get("status") != "pass" or not nativevn_package_input:
        report["status"] = "blocked"
    if _report_has_path_leak(report):
        report["status"] = "blocked"
        report["diagnostics"].append(
            {
                "code": "TSUI_DEMO_SLICE_REPORT_PATH_LEAK",
                "message": "demo-slice report contains a local path-like value",
            }
        )
        report["diagnostics"] = _dedupe_diagnostics(report["diagnostics"])
    if work_root:
        _write_json(work_root / "reports" / "demo_slice_report.json", report)
    return report


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
                chunk_fourcc_counts[chunk_fourcc] = chunk_fourcc_counts.get(chunk_fourcc, 0) + 1
                root_fourcc_counts[chunk_fourcc] = root_fourcc_counts.get(chunk_fourcc, 0) + 1
                with path.open("rb") as handle:
                    signature = handle.read(4).hex() or "<empty>"
                binary_signature_counts[signature] = binary_signature_counts.get(signature, 0) + 1
            elif ext == ".json":
                json_chunk_count += 1
                try:
                    value = _read_json(path)
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
    converted_resource_count = 0
    resource_coverage_status = "pass" if binary_chunk_count == converted_resource_count else "blocked"
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
            "converted": 0,
            "status": "pending_converter",
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


def _projectorrays_chunk_fourcc(path: Path) -> str:
    name = path.stem
    if "-" not in name:
        return "unknown"
    fourcc = name.rsplit("-", 1)[0]
    return fourcc if _is_safe_projectorrays_fourcc(fourcc) else "unknown"


def _is_safe_projectorrays_fourcc(value: str) -> bool:
    if not value or len(value) > 8:
        return False
    return all(32 <= ord(char) <= 126 and char not in "\\/:*" for char in value)


def _run_projectorrays_full_dump_from_demo_config(config: dict) -> dict | None:
    roots = config.get("projectorrays_full_dump_roots")
    if not roots:
        return None
    dump_roots = []
    for item in roots:
        if not isinstance(item, dict):
            continue
        dump_roots.append((str(item.get("alias", "")), Path(str(item.get("path", "")))))
    return build_projectorrays_full_dump_report(Path(str(config["local_work_root"])), dump_roots)


def run_internal_demo_bundle(
    config_path: Path | str,
    repo_root: Path | str = Path("."),
    astra_bin: Path | str | None = None,
    player_automation_report: Path | str | None = None,
    command_runner=None,
) -> dict:
    config_path = Path(config_path)
    repo_root = Path(repo_root)
    config, config_diagnostics = _read_demo_slice_config(config_path)
    work_root = Path(str(config.get("local_work_root", ""))) if isinstance(config, dict) and config.get("local_work_root") else None
    diagnostics = list(config_diagnostics)
    demo_report = None
    full_dump_report = None
    files: list[dict] = []
    command_reports: list[dict] = []
    bundle_manifests: dict[str, str] = {}
    release_report_rel = ""
    package_rel = "bundles/internal-classic/tsuinosora-internal-game.classic.astrapkg"

    if not diagnostics and bool(config.get("require_full_resource_conversion")):
        full_dump_report = _run_projectorrays_full_dump_from_demo_config(config)
        if not full_dump_report:
            diagnostics.append(
                {
                    "code": "TSUI_INTERNAL_DEMO_FULL_DUMP_REQUIRED",
                    "message": "full playable TsuiNoSora acceptance requires ProjectorRays full dump roots",
                }
            )
        elif full_dump_report.get("resource_coverage", {}).get("status") != "pass":
            diagnostics.append(
                {
                    "code": "TSUI_INTERNAL_DEMO_FULL_RESOURCE_CONVERSION_BLOCKED",
                    "required": full_dump_report.get("resource_coverage", {}).get("required", 0),
                    "converted": full_dump_report.get("resource_coverage", {}).get("converted", 0),
                    "message": "internal demo bundle cannot be built until every ProjectorRays binary chunk has converted resource evidence",
                }
            )

    if not diagnostics:
        demo_report = run_demo_slice_gate(config_path)
        diagnostics.extend(demo_report.get("diagnostics", []))
        if demo_report.get("status") != "pass":
            diagnostics.append(
                {
                    "code": "TSUI_INTERNAL_DEMO_SLICE_BLOCKED",
                    "message": "demo slice gate must pass before building the internal bundle",
                }
            )

    if not diagnostics and work_root:
        target = "tsuinosora-internal-game"
        profile = "classic"
        nativevn_root = work_root / "nativevn"
        project = nativevn_root / "project.yaml"
        cooked = work_root / "bundles" / "internal-classic" / "cooked"
        package = work_root / package_rel
        windows_bundle = work_root / "bundles" / "internal-classic" / "windows"
        web_bundle = work_root / "bundles" / "internal-classic" / "web"
        astra = _astra_command(astra_bin)

        for phase, command, cwd in [
            (
                "cook",
                astra
                + [
                    "cook",
                    str(project),
                    "--profile",
                    profile,
                    "--target",
                    target,
                    "--out",
                    str(cooked),
                ],
                repo_root,
            ),
            (
                "package",
                astra
                + [
                    "package",
                    "build",
                    str(cooked),
                    "--target",
                    target,
                    "--out",
                    str(package),
                ],
                repo_root,
            ),
            (
                "bundle.windows",
                astra
                + [
                    "package",
                    "bundle",
                    str(package),
                    "--target",
                    target,
                    "--profile",
                    profile,
                    "--platform",
                    "windows",
                    "--out",
                    str(windows_bundle),
                    "--format",
                    "json",
                ],
                nativevn_root,
            ),
            (
                "bundle.web",
                astra
                + [
                    "package",
                    "bundle",
                    str(package),
                    "--target",
                    target,
                    "--profile",
                    profile,
                    "--platform",
                    "web",
                    "--out",
                    str(web_bundle),
                    "--format",
                    "json",
                ],
                nativevn_root,
            ),
        ]:
            result = _run_bundle_command(phase, command, cwd, command_runner)
            command_reports.append(result["record"])
            if result["status"] != "pass":
                diagnostics.append(result["diagnostic"])
                break

        if not diagnostics:
            for path, role in [
                (package, "package"),
                (windows_bundle / "bundle_manifest.json", "windows_bundle_manifest"),
                (web_bundle / "bundle_manifest.json", "web_bundle_manifest"),
            ]:
                record = _bundle_file_record(work_root, path, role)
                if record:
                    files.append(record)
                else:
                    diagnostics.append(
                        {
                            "code": "TSUI_INTERNAL_DEMO_ARTIFACT_MISSING",
                            "message": "internal demo bundle artifact is missing",
                            "phase": role,
                        }
                    )
            for platform, manifest_path in [
                ("windows", windows_bundle / "bundle_manifest.json"),
                ("web", web_bundle / "bundle_manifest.json"),
            ]:
                if manifest_path.is_file():
                    manifest = _read_json(manifest_path)
                    if _bundle_manifest_matches(manifest, target, profile, platform):
                        bundle_manifests[platform] = _rel(manifest_path, work_root)
                    else:
                        diagnostics.append(
                            {
                                "code": "TSUI_INTERNAL_DEMO_BUNDLE_MANIFEST",
                                "message": "bundle manifest does not match internal demo target/profile/platform",
                                "platform": platform,
                            }
                        )

        player_report_path = Path(str(player_automation_report)) if player_automation_report else None
        if not player_report_path and isinstance(config, dict) and config.get("player_automation_report"):
            player_report_path = Path(str(config["player_automation_report"]))
        if not diagnostics and player_report_path:
            if not player_report_path.is_file():
                diagnostics.append(
                    {
                        "code": "TSUI_INTERNAL_DEMO_PLAYER_EVIDENCE_MISSING",
                        "message": "live player automation report is missing or inaccessible",
                    }
                )
            else:
                release_report_path = work_root / "reports" / "internal_demo_release_report.json"
                result = _run_bundle_command(
                    "validate.player_full_playable",
                    astra
                    + [
                        "package",
                        "validate",
                        str(package),
                        "--profile",
                        profile,
                        "--target",
                        target,
                        "--player-automation-report",
                        str(player_report_path),
                        "--format",
                        "json",
                        "--report",
                        str(release_report_path),
                    ],
                    repo_root,
                    command_runner,
                )
                command_reports.append(result["record"])
                if result["status"] != "pass":
                    diagnostics.append(result["diagnostic"])
                elif _release_report_has_full_playable(release_report_path, package):
                    release_report_rel = _rel(release_report_path, work_root)
                    files.append(_bundle_file_record(work_root, release_report_path, "release_report"))
                else:
                    diagnostics.append(
                        {
                            "code": "TSUI_INTERNAL_DEMO_PLAYER_FULL_PLAYABLE_BLOCKED",
                            "message": "package validate did not prove player.full_playable for this package",
                        }
                    )
        elif not diagnostics:
            diagnostics.append(
                {
                    "code": "TSUI_INTERNAL_DEMO_PLAYER_EVIDENCE_REQUIRED",
                    "message": "internal playable demo requires a live player automation report",
                }
            )

    report = {
        "schema": "tsuinosora.internal_demo_bundle_report.v1",
        "target": "tsuinosora-internal-game",
        "profile": "classic",
        "status": "pass" if not diagnostics else "blocked",
        "full_dump": "reports/projectorrays_full_dump_report.json" if full_dump_report else "",
        "demo_slice": "reports/demo_slice_report.json" if demo_report else "",
        "package": package_rel if work_root else "",
        "bundles": bundle_manifests,
        "release_report": release_report_rel,
        "files": [record for record in files if record],
        "commands": command_reports,
        "diagnostics": _dedupe_diagnostics(diagnostics),
        "redaction": {
            "paths": "work_root_relative_only",
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
                "code": "TSUI_INTERNAL_DEMO_REPORT_PATH_LEAK",
                "message": "internal demo bundle report contains a local path-like value",
            }
        )
        report["diagnostics"] = _dedupe_diagnostics(report["diagnostics"])
    if work_root:
        _write_json(work_root / "reports" / "internal_demo_bundle_report.json", report)
    return report


def _astra_command(astra_bin: Path | str | None) -> list[str]:
    if astra_bin:
        return [str(astra_bin)]
    return ["cargo", "run", "-p", "astra-cli", "--"]


def _run_bundle_command(phase: str, command: list[str], cwd: Path, command_runner=None) -> dict:
    if command_runner:
        completed = command_runner(phase, command, cwd)
    else:
        completed = subprocess.run(
            command,
            cwd=cwd,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            check=False,
        )
    returncode = int(getattr(completed, "returncode", 1))
    record = {
        "phase": phase,
        "status": "pass" if returncode == 0 else "blocked",
    }
    if returncode == 0:
        return {"status": "pass", "record": record, "diagnostic": None}
    return {
        "status": "blocked",
        "record": record,
        "diagnostic": {
            "code": "TSUI_INTERNAL_DEMO_COMMAND_FAILED",
            "message": "internal demo bundle command failed",
            "phase": phase,
            "exit_code": returncode,
        },
    }


def _bundle_file_record(work_root: Path, path: Path, role: str) -> dict | None:
    if not path.is_file():
        return None
    return {
        "path": _rel(path, work_root),
        "role": role,
        "sha256": _sha256(path),
        "byte_size": path.stat().st_size,
    }


def _bundle_manifest_matches(manifest: dict, target: str, profile: str, platform: str) -> bool:
    return (
        manifest.get("schema") == "astra.standalone_bundle_manifest.v1"
        and manifest.get("target") == target
        and manifest.get("profile") == profile
        and manifest.get("platform") == platform
        and manifest.get("package") == "package/nativevn.astrapkg"
    )


def _release_report_has_full_playable(path: Path, package: Path) -> bool:
    if not path.is_file():
        return False
    value = _read_json(path)
    if value.get("schema") != "astra.release_report.v1":
        return False
    if value.get("package_hash") != _sha256(package):
        return False
    return any(
        check.get("id") == "player.full_playable" and check.get("status") == "pass"
        for check in value.get("checks", [])
        if isinstance(check, dict)
    )


def import_projectorrays_reader(config_path: Path | str) -> dict:
    config_path = Path(config_path)
    config, diagnostics = _read_projectorrays_reader_config(config_path)
    work_root = Path(str(config.get("local_work_root", ""))) if isinstance(config, dict) and config.get("local_work_root") else None
    routes = []
    sources = []
    tool_hash = ""

    if not diagnostics:
        tool_path = Path(str(config["projectorrays_tool"]))
        dump_root = Path(str(config["dump_root"]))
        work_root = Path(str(config["local_work_root"]))
        tool_hash = _sha256(tool_path)
        unpacked_root = work_root / "unpacked"
        manifest_rel = "projectorrays/script_dump_manifest.json"
        manifest_path = unpacked_root / manifest_rel
        source_records = []

        for path in sorted(p for p in dump_root.rglob("*") if p.is_file() and p.suffix.lower() in {".ls", ".lingo", ".txt"}):
            rel = _rel(path, dump_root)
            lines = _read_text_lossless(path).splitlines()
            source_hash = _sha256(path)
            source_routes = []
            for line_no, line in enumerate(lines, start=1):
                route = _script_route_marker(line)
                if not route:
                    continue
                route["source"] = manifest_rel
                route["line"] = line_no
                route["source_hash"] = ""
                source_routes.append(route)
            source_records.append(
                {
                    "dump_source": rel,
                    "sha256": source_hash,
                    "line_count": len(lines),
                    "route_count": len(source_routes),
                }
            )
            routes.extend(source_routes)

        manifest = {
            "schema": "tsuinosora.projectorrays_dump_manifest.v1",
            "source_count": len(source_records),
            "sources": source_records,
            "redaction": {
                "paths": "dump_relative_only",
                "payload": "omitted",
                "commercial_text": "omitted",
                "bytecode": "omitted",
            },
        }
        _write_json(manifest_path, manifest)
        manifest_hash = _sha256(manifest_path)
        for route in routes:
            route["source_hash"] = manifest_hash
        sources = [
            {
                "source": manifest_rel,
                "sha256": manifest_hash,
                "line_count": 0,
                "script_count": len(source_records),
            }
        ]
        sidecar = {
            "schema": "tsuinosora.script_source_map.v1",
            "reader": {
                "tool_id": "projectorrays",
                "tool_hash": tool_hash,
                "output_contract": "route_source_map",
            },
            "sources": sources,
            "routes": routes,
        }
        _write_json(unpacked_root / "projectorrays_script_source_map.json", sidecar)

        if not routes:
            diagnostics.append(
                {
                    "code": "TSUI_PROJECTORRAYS_ROUTES_MISSING",
                    "message": "ProjectorRays dump did not contain sanitized route markers",
                }
            )

    report = {
        "schema": "tsuinosora.projectorrays_reader_report.v1",
        "status": "blocked" if diagnostics else "pass",
        "tool": {
            "id": "projectorrays",
            "hash": tool_hash,
        },
        "source_count": len(sources),
        "route_count": len(routes),
        "source_map": "unpacked/projectorrays_script_source_map.json" if routes else "",
        "diagnostics": _dedupe_diagnostics(diagnostics),
        "redaction": {
            "paths": "alias_or_report_relative_only",
            "payload": "omitted",
            "commercial_text": "omitted",
            "bytecode": "omitted",
        },
    }
    if _report_has_path_leak(report):
        report["status"] = "blocked"
        report["diagnostics"].append(
            {
                "code": "TSUI_PROJECTORRAYS_READER_REPORT_PATH_LEAK",
                "message": "ProjectorRays reader report contains a local path-like value",
            }
        )
        report["diagnostics"] = _dedupe_diagnostics(report["diagnostics"])
    if work_root:
        _write_json(work_root / "reports" / "projectorrays_reader_report.json", report)
    return report


def _read_projectorrays_reader_config(config_path: Path) -> tuple[dict, list[dict]]:
    try:
        config = _read_json(config_path)
    except (OSError, json.JSONDecodeError):
        return {}, [
            {
                "code": "TSUI_PROJECTORRAYS_CONFIG_UNREADABLE",
                "message": "ProjectorRays reader config is missing, inaccessible or not valid JSON",
            }
        ]
    diagnostics = []
    if not isinstance(config, dict):
        return {}, [
            {
                "code": "TSUI_PROJECTORRAYS_CONFIG_INVALID",
                "message": "ProjectorRays reader config must be a JSON object",
            }
        ]
    if config.get("schema") != "tsuinosora.projectorrays_reader_config.v1":
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_CONFIG_SCHEMA_INVALID",
                "message": "ProjectorRays reader config schema must be tsuinosora.projectorrays_reader_config.v1",
            }
        )
    for key in ["projectorrays_tool", "dump_root", "local_work_root"]:
        value = config.get(key)
        if not isinstance(value, str) or not value.strip():
            diagnostics.append(
                {
                    "code": "TSUI_PROJECTORRAYS_CONFIG_PATH_MISSING",
                    "field": key,
                    "message": "ProjectorRays reader config requires tool, dump root and local work root path fields",
                }
            )
            continue
        path = Path(value)
        if key == "projectorrays_tool" and not path.is_file():
            diagnostics.append(
                {
                    "code": "TSUI_PROJECTORRAYS_TOOL_MISSING",
                    "message": "ProjectorRays tool path is missing or inaccessible",
                }
            )
        if key == "dump_root" and not path.is_dir():
            diagnostics.append(
                {
                    "code": "TSUI_PROJECTORRAYS_DUMP_ROOT_MISSING",
                    "message": "ProjectorRays dump root is missing or inaccessible",
                }
            )
    if config.get("routes"):
        diagnostics.append(
            {
                "code": "TSUI_PROJECTORRAYS_ROUTE_EVIDENCE_REQUIRED",
                "message": "ProjectorRays reader routes must be derived from dump evidence, not config",
            }
        )
    return config, _dedupe_diagnostics(diagnostics)


def _read_demo_slice_config(config_path: Path) -> tuple[dict, list[dict]]:
    try:
        config = _read_json(config_path)
    except (OSError, json.JSONDecodeError):
        return {}, [
            {
                "code": "TSUI_DEMO_SLICE_CONFIG_UNREADABLE",
                "message": "demo-slice config is missing, inaccessible or not valid JSON",
            }
        ]
    diagnostics = _demo_slice_config_diagnostics(config)
    return config if isinstance(config, dict) else {}, diagnostics


def demo_slice_config_template() -> dict:
    return json.loads(json.dumps(DEMO_SLICE_CONFIG_TEMPLATE))


def write_demo_slice_config_template(out_path: Path | str | None = None, force: bool = False) -> dict:
    template = demo_slice_config_template()
    diagnostics = []
    files = []
    output_alias = ""
    if _report_has_path_leak(template):
        diagnostics.append(
            {
                "code": "TSUI_DEMO_CONFIG_TEMPLATE_PATH_LEAK",
                "message": "demo config template must use repo-relative placeholder paths only",
            }
        )
    if out_path is not None:
        output = Path(out_path)
        output_alias = "requested_output"
        if output.exists() and not force:
            diagnostics.append(
                {
                    "code": "TSUI_DEMO_CONFIG_TEMPLATE_EXISTS",
                    "message": "demo config template output already exists; pass --force to replace it",
                }
            )
        if not diagnostics:
            _write_json(output, template)
            files.append(
                {
                    "role": "demo_config",
                    "path_alias": output_alias,
                    "sha256": _sha256(output),
                    "byte_size": output.stat().st_size,
                }
            )
    report = {
        "schema": "tsuinosora.demo_slice_config_template_report.v1",
        "status": "blocked" if diagnostics else "pass",
        "output": output_alias,
        "files": files,
        "template": template,
        "diagnostics": diagnostics,
        "redaction": {
            "paths": "repo_relative_or_alias_only",
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
                "code": "TSUI_DEMO_CONFIG_TEMPLATE_REPORT_PATH_LEAK",
                "message": "demo config template report contains a local path-like value",
            }
        )
    report["diagnostics"] = _dedupe_diagnostics(report["diagnostics"])
    return report


def _demo_slice_config_diagnostics(config: dict | list) -> list[dict]:
    diagnostics = []
    if not isinstance(config, dict):
        return [
            {
                "code": "TSUI_DEMO_SLICE_CONFIG_INVALID",
                "message": "demo-slice config must be a JSON object",
            }
        ]
    if config.get("schema") != "tsuinosora.demo_slice_config.v1":
        diagnostics.append(
            {
                "code": "TSUI_DEMO_SLICE_CONFIG_SCHEMA_INVALID",
                "message": "demo-slice config schema must be tsuinosora.demo_slice_config.v1",
            }
        )
    for key, alias in [
        ("original_install_root", "original_install_root"),
        ("local_work_root", "local_work_root"),
    ]:
        value = config.get(key)
        if not isinstance(value, str) or not value.strip():
            diagnostics.append(
                {
                    "code": "TSUI_DEMO_SLICE_CONFIG_ROOT_MISSING",
                    "root_alias": alias,
                    "message": "demo-slice config requires private root values supplied by config or CLI",
                }
            )
    for key in [
        "remake_install_root",
        "unpacked_root",
        "title_png",
        "game_png",
        "projectorrays_tool",
        "projectorrays_dump_root",
        "player_automation_report",
    ]:
        value = config.get(key)
        if value is not None and not isinstance(value, str):
            diagnostics.append(
                {
                    "code": "TSUI_DEMO_SLICE_CONFIG_PATH_INVALID",
                    "field": key,
                    "message": "demo-slice path fields must be strings and are never copied into reports",
                }
            )
    projectorrays_configured = bool(config.get("projectorrays_tool") or config.get("projectorrays_dump_root"))
    if projectorrays_configured:
        tool = config.get("projectorrays_tool")
        dump = config.get("projectorrays_dump_root")
        if not isinstance(tool, str) or not tool.strip() or not Path(tool).is_file():
            diagnostics.append(
                {
                    "code": "TSUI_DEMO_SLICE_PROJECTORRAYS_TOOL_MISSING",
                    "message": "configured ProjectorRays tool is missing or inaccessible",
                }
            )
        if not isinstance(dump, str) or not dump.strip() or not Path(dump).is_dir():
            diagnostics.append(
                {
                    "code": "TSUI_DEMO_SLICE_PROJECTORRAYS_DUMP_ROOT_MISSING",
                    "message": "configured ProjectorRays dump root is missing or inaccessible",
                }
            )
    if "require_full_resource_conversion" in config and not isinstance(config.get("require_full_resource_conversion"), bool):
        diagnostics.append(
            {
                "code": "TSUI_DEMO_SLICE_CONFIG_FULL_CONVERSION_INVALID",
                "message": "require_full_resource_conversion must be a boolean",
            }
        )
    if "projectorrays_full_dump_roots" in config:
        roots = config.get("projectorrays_full_dump_roots")
        if not isinstance(roots, list):
            diagnostics.append(
                {
                    "code": "TSUI_DEMO_SLICE_CONFIG_FULL_DUMP_ROOTS_INVALID",
                    "message": "projectorrays_full_dump_roots must be a list of alias/path objects",
                }
            )
        else:
            for index, item in enumerate(roots):
                if not isinstance(item, dict):
                    diagnostics.append(
                        {
                            "code": "TSUI_DEMO_SLICE_CONFIG_FULL_DUMP_ROOT_INVALID",
                            "index": index,
                            "message": "projectorrays_full_dump_roots entries must be objects",
                        }
                    )
                    continue
                alias = str(item.get("alias", ""))
                path = item.get("path")
                if not _is_safe_symbol(alias):
                    diagnostics.append(
                        {
                            "code": "TSUI_DEMO_SLICE_CONFIG_FULL_DUMP_ALIAS_INVALID",
                            "index": index,
                            "message": "ProjectorRays full dump root alias must be a safe symbol",
                        }
                    )
                if not isinstance(path, str) or not path.strip():
                    diagnostics.append(
                        {
                            "code": "TSUI_DEMO_SLICE_CONFIG_FULL_DUMP_PATH_INVALID",
                            "index": index,
                            "message": "ProjectorRays full dump root path must be a string",
                        }
                    )
                elif not Path(path).is_dir():
                    diagnostics.append(
                        {
                            "code": "TSUI_DEMO_SLICE_CONFIG_FULL_DUMP_ROOT_MISSING",
                            "index": index,
                            "alias": alias or "unknown",
                            "message": "configured ProjectorRays full dump root is missing or inaccessible",
                        }
                    )
    if "modern_features" in config and not isinstance(config.get("modern_features"), list):
        diagnostics.append(
            {
                "code": "TSUI_DEMO_SLICE_CONFIG_FEATURES_INVALID",
                "message": "modern_features must be a list of sanitized feature evidence entries",
            }
        )
    if config.get("routes"):
        diagnostics.append(
            {
                "code": "TSUI_DEMO_SLICE_ROUTE_EVIDENCE_REQUIRED",
                "message": "demo-slice routes must be derived from route graph or script source-map evidence, not from config",
            }
        )
    return _dedupe_diagnostics(diagnostics)


def write_nativevn_package_input(work_root: Path | str, routes: list[dict] | None = None) -> dict:
    work_root = Path(work_root)
    reports_root = work_root / "reports"
    nativevn_root = work_root / "nativevn"
    routes = routes or _routes_from_conversion_report(reports_root / "conversion_report.json")
    diagnostics = []
    route_diagnostics = _nativevn_route_diagnostics(routes) if routes else []

    conversion_report = _read_json(reports_root / "conversion_report.json")
    asset_analysis = _read_json(reports_root / "asset_analysis.json")
    if conversion_report.get("status") != "pass":
        diagnostics.append(
            {
                "code": "TSUI_NATIVEVN_CONVERSION_BLOCKED",
                "message": "NativeVN package input requires a passing conversion report",
            }
        )
    if asset_analysis.get("status") != "pass":
        diagnostics.append(
            {
                "code": "TSUI_NATIVEVN_ASSET_ANALYSIS_BLOCKED",
                "message": "NativeVN package input requires a passing asset analysis report",
            }
        )
    if not routes:
        diagnostics.append(
            {
                "code": "TSUI_NATIVEVN_ROUTES_MISSING",
                "message": "NativeVN package input requires at least one covered route",
            }
        )
    diagnostics.extend(route_diagnostics)

    scripts_root = nativevn_root / "Scripts"
    section_root = nativevn_root / "PackageSections"
    scenario_root = nativevn_root / "scenarios"
    section_root.mkdir(parents=True, exist_ok=True)

    section_specs = _write_nativevn_section_inputs(reports_root, section_root)
    scenario_refs = []
    wrote_story_inputs = not diagnostics
    if wrote_story_inputs:
        scripts_root.mkdir(parents=True, exist_ok=True)
        scenario_root.mkdir(parents=True, exist_ok=True)
        _copy_native_assets_to_nativevn(work_root, nativevn_root, conversion_report)
        scenario_refs = _write_nativevn_scenarios(scenario_root, routes)
        (scripts_root / "main.astra").write_text(_render_nativevn_story(routes), encoding="utf-8")
        (nativevn_root / "project.yaml").write_text(
            _render_nativevn_project(section_specs, scenario_refs),
            encoding="utf-8",
        )
    files = _nativevn_package_input_files(nativevn_root, section_specs, scenario_refs)

    report = {
        "schema": "tsuinosora.nativevn_package_input_report.v1",
        "status": "blocked" if diagnostics or _report_has_path_leak(section_specs) or _report_has_path_leak(files) else "pass",
        "project_root": "local_work_root/nativevn",
        "project": "nativevn/project.yaml" if wrote_story_inputs else "",
        "story": "nativevn/Scripts/main.astra" if wrote_story_inputs else "",
        "section_count": len(section_specs),
        "scenario_count": len(scenario_refs),
        "route_count": len(routes),
        "files": files,
        "diagnostics": diagnostics,
        "redaction": {
            "paths": "report_relative_or_alias_only",
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
                "code": "TSUI_NATIVEVN_REPORT_PATH_LEAK",
                "message": "NativeVN package input report contains a local path-like value",
            }
        )
    _write_json(reports_root / "nativevn_package_input_report.json", report)
    return report


def _copy_native_assets_to_nativevn(work_root: Path, nativevn_root: Path, conversion_report: dict) -> None:
    source_root = work_root / "native-assets"
    target_root = nativevn_root / "native-assets"
    if target_root.exists():
        shutil.rmtree(target_root)
    resources = [
        resource
        for resource in conversion_report.get("resources", [])
        if isinstance(resource, dict)
        and _is_safe_report_relative_path(str(resource.get("native_path", "")))
        and str(resource.get("native_path", "")).startswith("native-assets/")
    ]
    for resource in resources:
        native_path = str(resource["native_path"])
        source = work_root / native_path
        if not source.is_file():
            continue
        target = nativevn_root / native_path
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, target)
        _write_asset_sidecar(target, native_path, resource)


def _write_asset_sidecar(asset_path: Path, native_path: str, resource: dict) -> None:
    asset_id = "asset:/tsuinosora/" + _asset_id_path(native_path)
    asset_type = _asset_type_for_native_path(native_path, str(resource.get("classification", "")))
    processor = _cook_processor_for_asset_type(asset_type)
    source_hash = str(resource.get("converted_hash") or _sha256(asset_path))
    sidecar = [
        "schema: astra.asset.v1",
        f"id: {asset_id}",
        f"source: {native_path}",
        f"source_hash: {source_hash}",
        f"type: {asset_type}",
        "license: local-user-owned",
        "importer: astra.tsui.native_asset",
        "cook:",
        f"  processor: {processor}",
        "  target_profiles:",
        "    - classic",
        "    - modern",
        "review: accepted",
        "",
    ]
    asset_path.with_name(asset_path.name + ".astra-asset.yaml").write_text("\n".join(sidecar), encoding="utf-8")


def _asset_id_path(native_path: str) -> str:
    path = native_path.removeprefix("native-assets/")
    parts = []
    for part in path.split("/"):
        stem = "".join(ch if ch.isalnum() or ch in {"_", "-"} else "_" for ch in part)
        parts.append(stem.strip("_") or "asset")
    return "native-assets/" + "/".join(parts)


def _asset_type_for_native_path(native_path: str, classification: str) -> str:
    suffix = Path(native_path).suffix.lower()
    if suffix in IMAGE_EXTS:
        return "image.rgba"
    if suffix in AUDIO_EXTS:
        return "audio.stream"
    if suffix in MOVIE_EXTS:
        return "movie.stream"
    if suffix in FONT_EXTS:
        return "font"
    return f"binary.{_safe_identifier(classification or 'asset')}"


def _cook_processor_for_asset_type(asset_type: str) -> str:
    if asset_type.startswith("image."):
        return "astra.cook.texture2d"
    if asset_type.startswith("audio."):
        return "astra.cook.audio"
    if asset_type.startswith("movie."):
        return "astra.cook.movie"
    if asset_type == "font":
        return "astra.cook.font"
    return "astra.cook.binary"


def _nativevn_route_diagnostics(routes: list[dict]) -> list[dict]:
    diagnostics = []
    for route_index, route in enumerate(routes):
        route_id = str(route.get("route_id", "")).strip()
        terminal = str(route.get("terminal", "")).strip()
        coverage = str(route.get("coverage", "")).strip()
        if not _is_safe_symbol(route_id):
            diagnostics.append(
                {
                    "code": "TSUI_NATIVEVN_ROUTE_ID_INVALID",
                    "route_index": route_index,
                    "message": "NativeVN route_id must be a safe symbol before story/scenario generation",
                }
            )
        if terminal and not _is_safe_symbol(terminal):
            diagnostics.append(
                {
                    "code": "TSUI_NATIVEVN_ROUTE_TERMINAL_INVALID",
                    "route_id": route_id or "unknown",
                    "route_index": route_index,
                    "message": "NativeVN route terminal must be a safe symbol before story/scenario generation",
                }
            )
        if coverage != "covered":
            diagnostics.append(
                {
                    "code": "TSUI_NATIVEVN_ROUTE_COVERAGE_INVALID",
                    "route_id": route_id or "unknown",
                    "route_index": route_index,
                    "message": "NativeVN route must carry covered coverage before story/scenario generation",
                }
            )
        raw_choices = route.get("choices", [])
        if raw_choices is None:
            continue
        if not isinstance(raw_choices, list):
            diagnostics.append(
                {
                    "code": "TSUI_NATIVEVN_ROUTE_CHOICES_INVALID",
                    "route_id": route_id or "unknown",
                    "route_index": route_index,
                    "message": "NativeVN route choices must be a list of safe symbols",
                }
            )
            continue
        for choice_index, choice in enumerate(raw_choices):
            choice_id = str(choice).strip()
            if not _is_safe_symbol(choice_id):
                diagnostics.append(
                    {
                        "code": "TSUI_NATIVEVN_ROUTE_CHOICE_INVALID",
                        "route_id": route_id or "unknown",
                        "route_index": route_index,
                        "choice_index": choice_index,
                        "message": "NativeVN route choice id must be a safe symbol",
                    }
                )
    diagnostics.extend(
        _duplicate_choice_diagnostics(
            routes,
            code="TSUI_NATIVEVN_ROUTE_DUPLICATE_CHOICE",
            message="NativeVN explicit route choices must not be deduped silently before story/scenario generation",
        )
    )
    diagnostics.extend(
        _duplicate_route_conflict_diagnostics(
            routes,
            code="TSUI_NATIVEVN_ROUTE_CONFLICT",
            message="NativeVN explicit routes must not reuse a route_id with conflicting terminal or choice evidence",
        )
    )
    return diagnostics


def _nativevn_package_input_files(nativevn_root: Path, section_specs: list[dict], scenario_refs: list[str]) -> list[dict]:
    records = []
    project_path = nativevn_root / "project.yaml"
    story_path = nativevn_root / "Scripts" / "main.astra"
    if project_path.exists():
        records.append(_nativevn_file_record(project_path, "project", "nativevn/project.yaml"))
    if story_path.exists():
        records.append(_nativevn_file_record(story_path, "story", "nativevn/Scripts/main.astra"))
    for spec in section_specs:
        record = _nativevn_file_record(nativevn_root / spec["path"], "package_section", f"nativevn/{spec['path']}")
        record["section_id"] = spec["id"]
        record["section_schema"] = spec["schema"]
        records.append(record)
    for ref in scenario_refs:
        records.append(_nativevn_file_record(nativevn_root / ref, "scenario_ref", f"nativevn/{ref}"))
    asset_root = nativevn_root / "native-assets"
    if asset_root.exists():
        for path in sorted(p for p in asset_root.rglob("*") if p.is_file()):
            rel = str(path.relative_to(nativevn_root)).replace("\\", "/")
            role = "asset_sidecar" if path.name.endswith(".astra-asset.yaml") else "asset"
            records.append(_nativevn_file_record(path, role, f"nativevn/{rel}"))
    return records


def _nativevn_file_record(path: Path, role: str, report_path: str) -> dict:
    return {
        "role": role,
        "path": report_path,
        "sha256": _sha256(path),
        "byte_size": path.stat().st_size,
    }


def analyze_asset(path: Path, root: Path) -> dict:
    rel = _rel(path, root)
    ext = path.suffix.lower()
    if ext in IMAGE_EXTS:
        return analyze_png_asset(path, rel)
    if ext in AUDIO_EXTS:
        parts = {part.lower() for part in path.parts}
        classification = "voice" if parts & VOICE_HINTS else "audio"
    elif ext in MOVIE_EXTS:
        classification = "movie"
    elif ext in FONT_EXTS:
        classification = "font"
    else:
        classification = "unknown"
    return {
        "relative_path": rel,
        "classification": classification,
        "confidence": 0.9 if classification != "unknown" else 0.0,
        "sha256": _sha256(path),
    }


def analyze_png_asset(path: Path, rel: str) -> dict:
    image = read_png(path)
    mask = image["alpha_mask"]
    visible = [(x, y) for y, row in enumerate(mask) for x, value in enumerate(row) if value]
    base = {
        "relative_path": rel,
        "sha256": _sha256(path),
        "dimensions": {"width": image["width"], "height": image["height"]},
        "has_alpha": image["has_alpha"],
        "color_distribution": image["color_distribution"],
    }
    if not visible:
        return {
            **base,
            "classification": "unknown",
            "confidence": 0.0,
            "visible_bbox": None,
            "parts": [],
        }

    bbox = _bbox(visible)
    components = _components(mask)
    total_area = image["width"] * image["height"]
    bbox_area = (bbox[2] - bbox[0] + 1) * (bbox[3] - bbox[1] + 1)
    visible_ratio = len(visible) / total_area
    hints = _path_hints(rel)

    if hints["text_window"]:
        classification = "text_window"
        confidence = 0.9
        parts = []
    elif hints["button"]:
        classification = "button"
        confidence = 0.88
        parts = []
    elif hints["ui"]:
        classification = "ui"
        confidence = 0.84
        parts = []
    elif image["has_alpha"] and len(components) >= 2:
        classification = "character_atlas"
        confidence = 0.92
        parts = [
            {
                "part_id": f"part.{index:03d}",
                "pose_id": f"pose.{index:03d}",
                "expression_id": "neutral",
                "anchor": {"x": (part[0] + part[2]) // 2, "y": part[3]},
                "crop": _bbox_dict(part),
                "layer": "character",
                "mouth_eye_state_compatible": True,
                "fallback": "nearest_pose",
            }
            for index, part in enumerate(components, start=1)
        ]
    elif image["has_alpha"] and bbox_area / total_area < 0.85:
        classification = "character_sprite"
        confidence = 0.86
        parts = []
    elif not image["has_alpha"] and visible_ratio > 0.95:
        classification = "background"
        confidence = 0.88
        parts = []
    else:
        classification = "cg"
        confidence = 0.7
        parts = []

    return {
        **base,
        "classification": classification,
        "confidence": confidence,
        "visible_bbox": _bbox_dict(bbox),
        "edge_padding": _edge_padding_dict(bbox, image["width"], image["height"]),
        "component_count": len(components),
        "parts": parts,
    }


def read_png(path: Path) -> dict:
    data = path.read_bytes()
    if not data.startswith(b"\x89PNG\r\n\x1a\n"):
        raise ValueError("not a PNG file")
    offset = 8
    width = height = color_type = None
    idat = bytearray()
    while offset < len(data):
        length = struct.unpack(">I", data[offset : offset + 4])[0]
        kind = data[offset + 4 : offset + 8]
        payload = data[offset + 8 : offset + 8 + length]
        offset += 12 + length
        if kind == b"IHDR":
            width, height, bit_depth, color_type, compression, filter_method, interlace = struct.unpack(
                ">IIBBBBB", payload
            )
            if bit_depth != 8 or compression != 0 or filter_method != 0 or interlace != 0:
                raise ValueError("unsupported PNG encoding")
            if color_type not in (2, 6):
                raise ValueError("unsupported PNG color type")
        elif kind == b"IDAT":
            idat.extend(payload)
        elif kind == b"IEND":
            break
    if width is None or height is None or color_type is None:
        raise ValueError("missing PNG IHDR")
    channels = 4 if color_type == 6 else 3
    raw = zlib.decompress(bytes(idat))
    stride = width * channels
    rows = []
    previous = [0] * stride
    cursor = 0
    for _ in range(height):
        filter_type = raw[cursor]
        cursor += 1
        encoded = list(raw[cursor : cursor + stride])
        cursor += stride
        row = _unfilter(encoded, previous, channels, filter_type)
        rows.append(row)
        previous = row
    alpha_mask = []
    has_alpha = False
    histogram = {}
    visible_count = 0
    for row in rows:
        mask_row = []
        for x in range(width):
            alpha = row[x * channels + 3] if channels == 4 else 255
            visible = alpha > 0
            mask_row.append(visible)
            has_alpha = has_alpha or alpha < 255
            if visible:
                r = row[x * channels]
                g = row[x * channels + 1]
                b = row[x * channels + 2]
                key = _quantized_rgb(r, g, b)
                histogram[key] = histogram.get(key, 0) + 1
                visible_count += 1
        alpha_mask.append(mask_row)
    return {
        "width": width,
        "height": height,
        "has_alpha": has_alpha,
        "alpha_mask": alpha_mask,
        "color_distribution": _color_distribution(histogram, visible_count),
    }


def _unfilter(row: list[int], previous: list[int], bpp: int, filter_type: int) -> list[int]:
    out = row[:]
    for i, value in enumerate(row):
        left = out[i - bpp] if i >= bpp else 0
        up = previous[i]
        up_left = previous[i - bpp] if i >= bpp else 0
        if filter_type == 0:
            predicted = 0
        elif filter_type == 1:
            predicted = left
        elif filter_type == 2:
            predicted = up
        elif filter_type == 3:
            predicted = (left + up) // 2
        elif filter_type == 4:
            predicted = _paeth(left, up, up_left)
        else:
            raise ValueError("unsupported PNG filter")
        out[i] = (value + predicted) & 0xFF
    return out


def _paeth(left: int, up: int, up_left: int) -> int:
    prediction = left + up - up_left
    pa = abs(prediction - left)
    pb = abs(prediction - up)
    pc = abs(prediction - up_left)
    if pa <= pb and pa <= pc:
        return left
    if pb <= pc:
        return up
    return up_left


def _components(mask: list[list[bool]]) -> list[tuple[int, int, int, int]]:
    height = len(mask)
    width = len(mask[0]) if height else 0
    seen = [[False for _ in range(width)] for _ in range(height)]
    components = []
    for y in range(height):
        for x in range(width):
            if not mask[y][x] or seen[y][x]:
                continue
            queue = deque([(x, y)])
            seen[y][x] = True
            pixels = []
            while queue:
                cx, cy = queue.popleft()
                pixels.append((cx, cy))
                for nx, ny in ((cx + 1, cy), (cx - 1, cy), (cx, cy + 1), (cx, cy - 1)):
                    if 0 <= nx < width and 0 <= ny < height and mask[ny][nx] and not seen[ny][nx]:
                        seen[ny][nx] = True
                        queue.append((nx, ny))
            components.append(_bbox(pixels))
    components.sort(key=lambda box: (box[0], box[1], box[2], box[3]))
    return components


def _bbox(pixels: list[tuple[int, int]]) -> tuple[int, int, int, int]:
    xs = [pixel[0] for pixel in pixels]
    ys = [pixel[1] for pixel in pixels]
    return min(xs), min(ys), max(xs), max(ys)


def _bbox_dict(box: tuple[int, int, int, int]) -> dict:
    return {"x": box[0], "y": box[1], "width": box[2] - box[0] + 1, "height": box[3] - box[1] + 1}


def _edge_padding_dict(box: tuple[int, int, int, int], width: int, height: int) -> dict:
    return {
        "left": box[0],
        "top": box[1],
        "right": width - box[2] - 1,
        "bottom": height - box[3] - 1,
    }


def _quantized_rgb(red: int, green: int, blue: int) -> str:
    return "#{:02x}{:02x}{:02x}".format(
        (red // 64) * 64,
        (green // 64) * 64,
        (blue // 64) * 64,
    )


def _color_distribution(histogram: dict[str, int], visible_count: int) -> list[dict]:
    if visible_count == 0:
        return []
    top = sorted(histogram.items(), key=lambda item: (-item[1], item[0]))[:5]
    return [
        {
            "rgb_bin": key,
            "coverage": round(count / visible_count, 6),
        }
        for key, count in top
    ]


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return "sha256:" + digest.hexdigest()


def _sha256_bytes(value: bytes) -> str:
    return "sha256:" + hashlib.sha256(value).hexdigest()


def _fourcc(value: bytes) -> str:
    if len(value) != 4:
        return value.hex()
    try:
        decoded = value.decode("ascii")
    except UnicodeDecodeError:
        return value.hex()
    if all(32 <= ord(ch) <= 126 for ch in decoded):
        return decoded
    return value.hex()


def _slice_embedded_payload(payload: bytes) -> tuple[bytes, str, str, int] | None:
    png_offset = payload.find(b"\x89PNG\r\n\x1a\n")
    if png_offset >= 0:
        end = _png_payload_end(payload, png_offset)
        if end:
            return payload[png_offset:end], "png", "image_png", png_offset

    riff_offset = payload.find(b"RIFF")
    if riff_offset >= 0 and len(payload) >= riff_offset + 12 and payload[riff_offset + 8 : riff_offset + 12] == b"WAVE":
        size = struct.unpack("<I", payload[riff_offset + 4 : riff_offset + 8])[0] + 8
        end = min(len(payload), riff_offset + size)
        return payload[riff_offset:end], "wav", "audio", riff_offset

    signatures = [
        (b"OggS", "ogg", "audio"),
        (b"fLaC", "flac", "audio"),
        (b"ID3", "mp3", "audio"),
        (b"\xff\xfb", "mp3", "audio"),
        (b"\x00\x00\x00\x18ftyp", "mp4", "movie"),
    ]
    for signature, extension, probe in signatures:
        offset = payload.find(signature)
        if offset >= 0:
            return payload[offset:], extension, probe, offset
    return None


def _slice_metadata_json_payload(payload: bytes) -> tuple[str, str, int] | None:
    stripped = payload.strip(b"\x00\r\n\t ")
    if not stripped or b"{" not in stripped:
        return None
    offset = stripped.find(b"{")
    candidate = stripped[offset:]
    decoded = _decode_script_text(candidate)
    if not decoded:
        return None
    text, _encoding = decoded
    try:
        value = json.loads(text)
    except json.JSONDecodeError:
        return None
    if not isinstance(value, dict) or value.get("schema") not in METADATA_JSON_SCHEMAS:
        return None
    normalized = json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    original_offset = payload.find(stripped) + offset
    return normalized, value["schema"], max(original_offset, 0)


def _slice_script_text_payload(payload: bytes, chunk_id: str) -> tuple[str, str, int] | None:
    if chunk_id not in SCRIPT_TEXT_CHUNK_IDS:
        return None
    stripped = payload.strip(b"\x00\r\n\t ")
    if len(stripped) < 4:
        return None
    decoded = _decode_script_text(stripped)
    if decoded:
        text, encoding = decoded
        if _looks_like_script_text(text) and _script_text_starts_cleanly(text):
            offset = payload.find(stripped)
            return text, encoding, max(offset, 0)
    return _slice_embedded_script_text_payload(payload)


def _slice_embedded_script_text_payload(payload: bytes) -> tuple[str, str, int] | None:
    lower = payload.lower()
    offsets = []
    for marker in (b"astra route", b"astra_route", b"astra-route", b"route:"):
        search_from = 0
        while True:
            marker_offset = lower.find(marker, search_from)
            if marker_offset < 0:
                break
            start = _embedded_script_line_start(payload, marker_offset)
            if start not in offsets:
                offsets.append(start)
            search_from = marker_offset + len(marker)
    for start in sorted(offsets):
        candidate = payload[start:].strip(b"\x00\r\n\t ")
        if len(candidate) < 4:
            continue
        decoded = _decode_script_text(candidate)
        if not decoded:
            continue
        text, encoding = decoded
        if _looks_like_script_text(text):
            inner_offset = start + max(payload[start:].find(candidate), 0)
            return text, encoding, inner_offset
    return None


def _embedded_script_line_start(payload: bytes, marker_offset: int) -> int:
    line_start = payload.rfind(b"\n", 0, marker_offset) + 1
    prefix = payload[line_start:marker_offset]
    starts = [prefix.rfind(token) for token in (b"--", b"//", b"#")]
    best = max(starts)
    if best >= 0:
        return line_start + best
    return marker_offset


def _decode_script_text(payload: bytes) -> tuple[str, str] | None:
    for encoding in ("utf-8-sig", "cp932", "shift_jis"):
        try:
            text = payload.decode(encoding)
        except UnicodeDecodeError:
            continue
        return text.replace("\r\n", "\n").replace("\r", "\n"), encoding
    return None


def _looks_like_script_text(text: str) -> bool:
    if not text.strip():
        return False
    printable = 0
    controls = 0
    for ch in text:
        if ch in "\n\t" or ch.isprintable():
            printable += 1
        else:
            controls += 1
    return printable > 0 and controls / max(printable + controls, 1) < 0.05


def _script_text_starts_cleanly(text: str) -> bool:
    stripped = text.lstrip("\ufeff\r\n\t ")
    return bool(stripped) and (stripped[0].isprintable() or stripped[0] in "\n\t")


def _png_payload_end(payload: bytes, start: int) -> int | None:
    offset = start + 8
    while offset + 12 <= len(payload):
        length = struct.unpack(">I", payload[offset : offset + 4])[0]
        kind = payload[offset + 4 : offset + 8]
        next_offset = offset + 12 + length
        if next_offset > len(payload):
            return None
        offset = next_offset
        if kind == b"IEND":
            return offset
    return None


def _read_text_lossless(path: Path) -> str:
    data = path.read_bytes()
    decoded = _decode_script_text(data)
    if decoded:
        return decoded[0]
    return data.decode("utf-8", errors="ignore").replace("\r\n", "\n").replace("\r", "\n")


def _script_route_marker(line: str) -> dict | None:
    match = SCRIPT_ROUTE_RE.match(line)
    if not match:
        return None
    route_id = match.group("route")
    terminal = match.group("terminal") or f"ending.{_safe_identifier(route_id)}"
    choices = _parse_choice_list(match.group("choices") or "")
    return {
        "route_id": route_id,
        "coverage": "covered",
        "terminal": terminal,
        "choices": choices,
    }


def _parse_choice_list(value: str) -> list[str]:
    if not value:
        return []
    return [
        item.strip()
        for item in re.split(r"[, ]+", value)
        if item.strip() and re.match(r"^[A-Za-z0-9_.-]+$", item.strip())
    ]


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


def _rel(path: Path, root: Path) -> str:
    return path.relative_to(root).as_posix()


def _reference_hashes(reference_report: dict | None) -> list[str]:
    if not reference_report:
        return []
    return [entry["hash"] for entry in reference_report.get("references", []) if "hash" in entry]


def _format_probe(path: Path) -> str:
    ext = path.suffix.lower()
    if ext in DIRECTOR_CONTAINER_EXTS:
        return "director_container"
    if ext in IMAGE_EXTS:
        return "image_png"
    if ext in AUDIO_EXTS:
        return "audio"
    if ext in MOVIE_EXTS:
        return "movie"
    if ext in FONT_EXTS:
        return "font"
    if ext in TEXT_EXTS:
        return "text"
    return "unknown"


def _is_unpacked_metadata_file(path: Path) -> bool:
    ext = path.suffix.lower()
    if ext in TEXT_EXTS - FONT_EXTS:
        if ext == ".json":
            try:
                value = _read_json(path)
            except json.JSONDecodeError:
                return False
            return isinstance(value, dict) and value.get("schema") in {
                "tsuinosora.cast_map.v1",
                "tsuinosora.cast_source_map_report.v1",
                "tsuinosora.director_cast_map.v1",
                "tsuinosora.director_lingo_map.v1",
                "tsuinosora.director_resource_map.v1",
                "tsuinosora.projectorrays_dump_manifest.v1",
                "tsuinosora.route_graph.v1",
                "tsuinosora.route_graph_report.v1",
                "tsuinosora.script_source_map.v1",
                "tsuinosora.script_source_map_report.v1",
            }
        return True
    return False


def _format_counts(files: list[dict]) -> dict:
    counts = {}
    for entry in files:
        probe = entry.get("format_probe", "unknown")
        counts[probe] = counts.get(probe, 0) + 1
    return dict(sorted(counts.items()))


def _edition_fingerprint(files: list[dict]) -> dict:
    ready = next((entry for entry in files if Path(entry["relative_path"]).name.lower() == "ready.dxr"), None)
    return {
        "ready_dxr_present": ready is not None,
        "ready_dxr_hash": ready.get("sha256", "") if ready else "",
        "director_container_count": sum(1 for entry in files if entry.get("format_probe") == "director_container"),
    }


def _classification_counts(assets: list[dict]) -> dict:
    counts = {}
    for asset in assets:
        classification = asset.get("classification", "unknown")
        counts[classification] = counts.get(classification, 0) + 1
    return dict(sorted(counts.items()))


def _duplicate_hash_groups(assets: list[dict]) -> list[dict]:
    by_hash = {}
    for asset in assets:
        digest = asset.get("sha256", "")
        if digest:
            by_hash.setdefault(digest, []).append(asset["relative_path"])
    groups = []
    for index, (digest, paths) in enumerate(sorted(by_hash.items()), start=1):
        if len(paths) < 2:
            continue
        groups.append(
            {
                "duplicate_hash_group": f"dup.{index:03d}",
                "sha256": digest,
                "relative_paths": sorted(paths),
            }
        )
    return groups


def _asset_usage_index(root: Path, asset_paths: list[str]) -> dict[str, list[dict]]:
    tokens: dict[str, set[str]] = {}
    for rel in asset_paths:
        lower_rel = rel.lower()
        name = Path(rel).name.lower()
        stem = Path(rel).stem.lower()
        for token in {lower_rel, name, stem}:
            if token:
                tokens.setdefault(token, set()).add(rel)

    usage: dict[str, list[dict]] = {rel: [] for rel in asset_paths}
    for path in sorted(p for p in root.rglob("*") if p.is_file() and p.suffix.lower() in TEXT_EXTS):
        try:
            text = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            text = path.read_text(encoding="utf-8", errors="ignore")
        source = _rel(path, root)
        for line_no, line in enumerate(text.splitlines(), start=1):
            lower = line.lower()
            for token, rels in tokens.items():
                if token in lower:
                    for rel in rels:
                        entry = {
                            "source": source,
                            "line": line_no,
                            "reference_kind": _reference_kind(line),
                        }
                        if entry not in usage[rel]:
                            usage[rel].append(entry)
    return {rel: refs for rel, refs in usage.items() if refs}


def _reference_kind(line: str) -> str:
    lower = line.lower()
    if any(hint in lower for hint in BACKGROUND_HINTS):
        return "background"
    if any(hint in lower for hint in CHARACTER_HINTS):
        return "character"
    if any(hint in lower for hint in TEXT_WINDOW_HINTS):
        return "text_window"
    if any(hint in lower for hint in BUTTON_HINTS):
        return "button"
    if "voice" in lower:
        return "voice"
    if "movie" in lower:
        return "movie"
    return "unknown"


def _container_source(rel: str) -> str:
    parts = rel.split("/")
    return parts[0] if len(parts) > 1 else "root"


def _use_timing(references: list[dict]) -> str:
    if not references:
        return "unreferenced"
    sources = " ".join(ref["source"].lower() for ref in references)
    if any(key in sources for key in ["title", "menu", "system"]):
        return "system_ui"
    if any(key in sources for key in ["route", "scenario", "scene", "main"]):
        return "story_route"
    return "script_referenced"


def _reference_matches(asset: dict, reference_report: dict | None) -> list[dict]:
    if not reference_report or "dimensions" not in asset:
        return []
    matches = []
    dims = asset["dimensions"]
    for reference in reference_report.get("references", []):
        ref_dims = reference.get("dimensions", {})
        same_size = dims == ref_dims
        if same_size:
            matches.append(
                {
                    "logical_id": reference.get("logical_id", "unknown"),
                    "region_id": "full_frame",
                    "metric": "dimensions",
                    "status": "match",
                }
            )
    return matches


def _path_hints(rel: str) -> dict[str, bool]:
    normalized = rel.lower().replace("\\", "/")
    tokens = set(_path_tokens(rel))
    return {
        "background": _has_hint(normalized, tokens, BACKGROUND_HINTS),
        "character": _has_hint(normalized, tokens, CHARACTER_HINTS),
        "text_window": _has_hint(normalized, tokens, TEXT_WINDOW_HINTS),
        "button": _has_hint(normalized, tokens, BUTTON_HINTS),
        "ui": _has_hint(normalized, tokens, UI_HINTS),
    }


def _has_hint(normalized: str, tokens: set[str], hints: set[str]) -> bool:
    return bool(tokens & hints) or any(hint in normalized for hint in hints)


def _path_tokens(rel: str) -> list[str]:
    normalized = rel.lower().replace("\\", "/")
    raw = []
    for part in normalized.split("/"):
        raw.append(part)
        raw.extend(part.replace(".", "_").replace("-", "_").split("_"))
    return [token for token in raw if token]


def _classification_conflicts(asset: dict) -> list[dict]:
    rel = asset["relative_path"]
    hints = _path_hints(rel)
    classification = asset.get("classification", "unknown")
    diagnostics = []
    references = {ref.get("reference_kind") for ref in asset.get("script_references", [])}

    if classification == "background" and (hints["character"] or "character" in references):
        diagnostics.append(
            {
                "code": "TSUI_ASSET_CHARACTER_AS_BACKGROUND",
                "relative_path": rel,
                "message": "character evidence conflicts with background classification",
            }
        )
    if classification in {"character_sprite", "character_atlas"} and (
        hints["background"] or "background" in references
    ):
        diagnostics.append(
            {
                "code": "TSUI_ASSET_BACKGROUND_AS_CHARACTER",
                "relative_path": rel,
                "message": "background evidence conflicts with character classification",
            }
        )
    if classification in {"background", "cg"} and (
        hints["ui"] or hints["text_window"] or hints["button"] or references & {"text_window", "button"}
    ):
        diagnostics.append(
            {
                "code": "TSUI_ASSET_UI_AS_BACKGROUND",
                "relative_path": rel,
                "message": "UI evidence conflicts with background/cg classification",
            }
        )
    if asset.get("has_alpha") and classification == "background":
        diagnostics.append(
            {
                "code": "TSUI_ASSET_TRANSPARENT_BACKGROUND",
                "relative_path": rel,
                "message": "transparent image cannot be flattened into a background",
            }
        )
    if classification == "character_atlas" and not asset.get("parts"):
        diagnostics.append(
            {
                "code": "TSUI_ASSET_ATLAS_WITHOUT_PARTS",
                "relative_path": rel,
                "message": "character_atlas must include crop/part metadata",
            }
        )
    if classification == "character_sprite" and asset.get("component_count", 0) >= 2:
        diagnostics.append(
            {
                "code": "TSUI_ASSET_ATLAS_NOT_SLICED",
                "relative_path": rel,
                "message": "multi-component character image must be treated as character_atlas",
            }
        )
    return diagnostics


def _looks_like_local_path(value: str) -> bool:
    return (
        value.startswith("/")
        or value.startswith("\\\\")
        or any(left.isalpha() and right == ":" for left, right in zip(value, value[1:]))
    )


def _is_safe_report_relative_path(value: str) -> bool:
    if not value or _looks_like_local_path(value):
        return False
    parts = value.replace("\\", "/").split("/")
    return all(part and part not in {".", ".."} for part in parts)


def _is_safe_symbol(value: str) -> bool:
    return bool(value) and re.match(r"^[A-Za-z0-9_.-]+$", value) is not None


def _is_sanitized_sha256(value: str) -> bool:
    return re.match(r"^sha256:[0-9a-fA-F]{64}$", value) is not None


def _positive_int(value) -> int:
    try:
        parsed = int(value)
    except (TypeError, ValueError):
        return 0
    return parsed if parsed > 0 else 0


def _nonnegative_int(value) -> int:
    try:
        parsed = int(value)
    except (TypeError, ValueError):
        return 0
    return parsed if parsed >= 0 else 0


def _dedupe_diagnostics(diagnostics: list[dict]) -> list[dict]:
    seen = set()
    result = []
    for diagnostic in diagnostics:
        key = json.dumps(diagnostic, sort_keys=True, separators=(",", ":"), default=str)
        if key in seen:
            continue
        seen.add(key)
        result.append(diagnostic)
    return result


def _source_root_diagnostics(root: Path, alias: str, require_director: bool = True) -> list[dict]:
    diagnostics = []
    if not root.exists():
        return [
            {
                "code": "TSUI_SOURCE_ROOT_MISSING",
                "root_alias": alias,
                "message": "source root does not exist or is not accessible",
            }
        ]
    if not root.is_dir():
        return [
            {
                "code": "TSUI_SOURCE_ROOT_NOT_DIRECTORY",
                "root_alias": alias,
                "message": "source root must be a directory",
            }
        ]
    files = [path for path in root.rglob("*") if path.is_file()]
    if not files:
        diagnostics.append(
            {
                "code": "TSUI_SOURCE_EMPTY",
                "root_alias": alias,
                "message": "source root contains no files",
            }
        )
    if require_director:
        extensions = {path.suffix.lower() for path in files}
        names = {path.name.upper() for path in files}
        if not ({".dxr", ".cxt"} & extensions):
            diagnostics.append(
                {
                    "code": "TSUI_SOURCE_CONTAINER_MISSING",
                    "root_alias": alias,
                    "message": "original source must expose legal readable Director/Shockwave containers",
                }
            )
        if "READY.DXR" not in names:
            diagnostics.append(
                {
                    "code": "TSUI_SOURCE_EDITION_FINGERPRINT_INCOMPLETE",
                    "root_alias": alias,
                    "message": "edition fingerprint is missing READY.dxr",
                }
            )
    return diagnostics


def _empty_inventory(alias: str) -> dict:
    return {
        "schema": "tsuinosora.source_inventory.v1",
        "root_alias": alias,
        "file_count": 0,
        "files": [],
    }


def _blocked_extract_report(source_alias: str, code: str, message: str) -> dict:
    return {
        "schema": "tsuinosora.extract_report.v1",
        "status": "blocked",
        "source_alias": source_alias,
        "output_alias": "local_work_root/unpacked",
        "input_file_count": 0,
        "extracted_count": 0,
        "skipped_count": 0,
        "protected_container_count": 0,
        "format_counts": {},
        "files": [],
        "skipped": [],
        "diagnostics": [
            {
                "code": code,
                "source_alias": source_alias,
                "message": message,
            }
        ],
        "redaction": {
            "paths": "alias_or_report_relative_only",
            "payload": "omitted",
            "commercial_text": "omitted",
            "screenshots": "omitted",
            "audio": "omitted",
            "movie": "omitted",
        },
    }


def _blocked_asset_analysis(reference_report: dict | None, code: str, message: str) -> dict:
    return {
        "schema": "tsuinosora.asset_analysis.v1",
        "status": "blocked",
        "reference_hashes": _reference_hashes(reference_report),
        "assets": [],
        "quarantine": [],
        "diagnostics": [
            {
                "code": code,
                "message": message,
            }
        ],
    }


def _write_json(path: Path, value: dict | list) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def _routes_from_conversion_report(path: Path) -> list[dict]:
    if not path.exists():
        return []
    report = _read_json(path)
    return [
        route
        for route in report.get("routes", [])
        if isinstance(route, dict) and route.get("coverage") == "covered"
    ]


def _write_nativevn_section_inputs(reports_root: Path, section_root: Path) -> list[dict]:
    entries = [
        (
            "tsuinosora.reference_evidence",
            "tsuinosora.visual_reference_report.v1",
            "reference_evidence.json",
            "reference_evidence.json",
            [],
            [],
        ),
        (
            "tsuinosora.asset_analysis",
            "tsuinosora.asset_analysis.v1",
            "asset_analysis.json",
            "asset_analysis.json",
            [],
            [],
        ),
        (
            "tsuinosora.conversion_manifest",
            "tsuinosora.conversion_report.v1",
            "conversion_report.json",
            "conversion_report.json",
            [],
            [],
        ),
        (
            "tsuinosora.mount_policy",
            "tsuinosora.mount_policy.v1",
            "mount_policy.tsuinosora-internal-game.json",
            "mount_policy.internal.json",
            ["tsuinosora-internal-game"],
            [],
        ),
        (
            "tsuinosora.mount_policy",
            "tsuinosora.mount_policy.v1",
            "mount_policy.tsuinosora-patch-game.json",
            "mount_policy.patch.json",
            ["tsuinosora-patch-game"],
            [],
        ),
        (
            "tsuinosora.modern_profile_report",
            "tsuinosora.modern_profile_report.v1",
            "modern_profile_report.json",
            "modern_profile_report.json",
            [],
            ["modern"],
        ),
        (
            "tsuinosora.manual_signoff",
            "tsuinosora.manual_signoff.v1",
            "manual_signoff.json",
            "manual_signoff.json",
            [],
            ["desktop-release", "web-release"],
        ),
    ]
    specs = []
    for section_id, schema, source_name, output_name, targets, profiles in entries:
        source = reports_root / source_name
        if not source.exists():
            continue
        target = section_root / output_name
        data = _sanitize_tsuinosora_package_section(_read_json(source))
        _write_json(target, data)
        spec = {
            "id": section_id,
            "schema": schema,
            "path": f"PackageSections/{output_name}",
            "codec": "raw",
        }
        if targets:
            spec["targets"] = targets
        if profiles:
            spec["profiles"] = profiles
        specs.append(spec)
    return specs


def _sanitize_tsuinosora_package_section(value, path: list[str] | None = None):
    path = path or []
    if isinstance(value, dict):
        sanitized = {}
        for key, child in value.items():
            child_path = path + [str(key)]
            if _is_forbidden_tsuinosora_package_section_key(str(key), child_path):
                continue
            sanitized[key] = _sanitize_tsuinosora_package_section(child, child_path)
        return sanitized
    if isinstance(value, list):
        return [_sanitize_tsuinosora_package_section(item, path) for item in value]
    return value


def _is_forbidden_tsuinosora_package_section_key(key: str, path: list[str]) -> bool:
    if key == "payload":
        return not (path == ["redaction", "payload"])
    return key in {
        "text",
        "script_text",
        "source_text",
        "content",
        "payload_bytes",
        "bytecode",
        "bytes",
        "commercial_text",
        "lingo_source",
        "raw_payload",
        "source_payload",
    }


def _write_nativevn_scenarios(scenario_root: Path, routes: list[dict]) -> list[str]:
    refs = []
    for target, platforms in [
        ("tsuinosora-internal-game", ["headless", "windows", "web"]),
        ("tsuinosora-patch-game", ["headless", "windows", "web"]),
    ]:
        for profile in ["classic", "modern"]:
            for platform in platforms:
                for route in routes:
                    route_id = _safe_identifier(route.get("route_id", "classic.main"))
                    file_name = f"{target}.{profile}.{platform}.{route_id}.json"
                    scenario = _nativevn_scenario(target, profile, platform, route)
                    _write_json(scenario_root / file_name, scenario)
                    refs.append(f"scenarios/{file_name}")
    return refs


def _nativevn_scenario(target: str, profile: str, platform: str, route: dict) -> dict:
    route_id = route.get("route_id", "classic.main")
    terminal = route.get("terminal", f"ending.{_safe_identifier(route_id)}")
    choices = _route_choice_ids(route, route_id)
    actions = [
        {"launch": {}},
        {"player_input": {"kind": "complete_wait", "value": "movie.opening.end"}},
        {"player_input": {"kind": "complete_wait", "value": "voice.opening.end"}},
        {"player_input": {"kind": "advance"}},
    ]
    for index, choice in enumerate(choices):
        actions.append({"player_input": {"kind": "choose", "value": choice}})
        if index + 1 < len(choices):
            actions.append({"player_input": {"kind": "advance"}})
    actions.extend(
        [
            {"player_input": {"kind": "complete_wait", "value": "voice.opening.end"}},
            {"player_input": {"kind": "advance"}},
            {"player_input": {"kind": "replay_voice", "value": "voice.narrator.0001"}},
            {"player_input": {"kind": "open_system", "value": "route_chart"}},
            {"player_input": {"kind": "save", "slot": "slot.demo"}},
            {"player_input": {"kind": "load", "slot": "slot.demo"}},
            {"replay_from_start": {}},
        ]
    )
    scenario = {
        "schema": "astra.scenario.v1",
        "stage": "stage3-astra-vn",
        "target": target,
        "profile": profile,
        "platform": platform,
        "generated_route_id": route_id,
        "seed": 42,
        "mount_aliases": {
            "original": "original_install_root",
            "remake": "remake_install_root.optional",
        },
        "actions": actions,
        "assertions": [
            {
                "coverage": {
                    "routes": [terminal],
                    "backlog_keys": ["tsui.prologue", f"tsui.{_safe_identifier(route_id)}.line"],
                    "read_state": ["line.tsui.prologue", f"line.{_safe_identifier(route_id)}.line"],
                    "voice_replay": ["voice.narrator.0001"],
                }
            },
            {"replay_hash_match": True},
            {"no_blocking_diagnostics": True},
        ],
    }
    mount_assets, _ = _route_mount_assets(target, platform, route, route_id)
    if mount_assets:
        scenario["mount_assets"] = mount_assets
    return scenario


def _render_nativevn_story(routes: list[dict]) -> str:
    covered_routes = routes or [{"route_id": "classic.main", "terminal": "ending.classic_main"}]
    lines = [
        "story main #@id story.main",
        "",
        "state prologue #@id state.prologue",
        "  scene opening #@id scene.opening",
        "    movie layer:video.opening asset:native-assets/movies/opening.webm end:wait fallback:native-assets/backgrounds/opening_fallback.png #@id movie.opening",
        "    voice asset:native-assets/voice/narrator0001.ogg sync:text #@id voice.opening",
        "    text key:tsui.prologue speaker:narrator #@id line.tsui.prologue",
        "    choice key:tsui.route #@id choice.tsui.route",
    ]
    for route in covered_routes:
        route_id = _safe_identifier(route.get("route_id", "classic.main"))
        choices = _route_choice_ids(route, str(route.get("route_id", "classic.main")))
        first_target = _route_choice_state(route_id, 1) if len(choices) > 1 else f"route_{route_id}"
        lines.append(f"      option key:{choices[0]} -> {first_target} #@id choice.{route_id}.001")
    for route in covered_routes:
        route_id = _safe_identifier(route.get("route_id", "classic.main"))
        choices = _route_choice_ids(route, str(route.get("route_id", "classic.main")))
        for index, choice in enumerate(choices[1:], start=1):
            state = _route_choice_state(route_id, index)
            next_state = (
                _route_choice_state(route_id, index + 1)
                if index + 1 < len(choices)
                else f"route_{route_id}"
            )
            choice_id = _safe_identifier(choice)
            lines.extend(
                [
                    "",
                    f"state {state} #@id state.{state}",
                    f"  scene {state} #@id scene.{state}",
                    f"    text key:tsui.{route_id}.choice_{index:03d} speaker:narrator #@id line.{route_id}.choice_{index:03d}",
                    f"    choice key:tsui.{route_id}.choice_{index:03d} #@id choice.{route_id}.{index + 1:03d}",
                    f"      option key:{choice} -> {next_state} #@id choice.{route_id}.{index + 1:03d}.{choice_id}",
                ]
            )
        state = f"route_{route_id}"
        terminal = route.get("terminal", f"ending.{route_id}")
        lines.extend(
            [
                "",
                f"state {state} #@id state.{state}",
                f"  scene {state} #@id scene.{state}",
                f"    bgm asset:native-assets/bgm/{route_id}.ogg loop:true #@id bgm.{route_id}",
                f"    se asset:native-assets/se/page.ogg #@id se.{route_id}",
                "    wait fence:voice.opening.end #@id wait.route_pause",
                f"    text key:tsui.{route_id}.line speaker:narrator voice:voice.narrator.0001 #@id line.{route_id}.line",
                f"    jump {terminal} #@id jump.{route_id}",
            ]
        )
    lines.extend(_system_story_lines())
    return "\n".join(lines) + "\n"


def _route_choice_ids(route: dict, route_id: str) -> list[str]:
    choices = []
    for raw_choice in route.get("choices", []) or []:
        choice = str(raw_choice).strip()
        if choice and _is_safe_symbol(choice) and choice not in choices:
            choices.append(choice)
    return choices or [f"choice.{_safe_identifier(route_id)}"]


def _route_choice_state(route_id: str, index: int) -> str:
    return f"route_{route_id}_choice_{index:03d}"


def _system_story_lines() -> list[str]:
    lines = ["", "story system #@id story.system"]
    for page in [
        "title",
        "save",
        "load",
        "config",
        "gallery",
        "replay",
        "voice_replay",
        "route_chart",
        "backlog",
        "localization_preview",
    ]:
        lines.extend(
            [
                "",
                f"state {page} #@id state.system.{page}",
                f"  scene {page} #@id scene.system.{page}",
                f"    system_page kind:{page} policy:astra.policy.standard #@id page.{page}",
            ]
        )
    return lines


def _render_nativevn_project(section_specs: list[dict], scenario_refs: list[str]) -> str:
    lines = [
        "schema: astra.project.v1",
        "id: com.example.tsuinosora.stage3",
        "runtime: astra-vn",
        "targets:",
        "  - id: tsuinosora-internal-game",
        "    kind: game",
        "    crate: astra-vn",
        "    default_profile: classic",
        "    platforms: [headless, windows, web]",
        "    packaged: true",
        "  - id: tsuinosora-patch-game",
        "    kind: game",
        "    crate: astra-vn",
        "    default_profile: classic",
        "    platforms: [headless, windows, web]",
        "    packaged: true",
        "nativevn:",
        "  sources:",
        "    - Scripts",
        "  profiles: [classic, modern]",
        "  asset_roots:",
        "    - native-assets",
        "  scenario_refs:",
    ]
    if scenario_refs:
        lines.extend(f"    - {ref}" for ref in scenario_refs)
    else:
        lines.append("    []")
    if section_specs:
        lines.append("package_sections:")
        for spec in section_specs:
            lines.append(f"  - id: {spec['id']}")
            lines.append(f"    schema: {spec['schema']}")
            lines.append(f"    path: {spec['path']}")
            lines.append(f"    codec: {spec['codec']}")
            if spec.get("targets"):
                lines.append("    targets: [" + ", ".join(spec["targets"]) + "]")
            if spec.get("profiles"):
                lines.append("    profiles: [" + ", ".join(spec["profiles"]) + "]")
    return "\n".join(lines) + "\n"


def _safe_identifier(value: str) -> str:
    cleaned = "".join(ch if ch.isalnum() else "_" for ch in value.lower()).strip("_")
    return cleaned or "route"


def _report_has_path_leak(value) -> bool:
    if isinstance(value, str):
        return _looks_like_local_path(value)
    if isinstance(value, list):
        return any(_report_has_path_leak(item) for item in value)
    if isinstance(value, dict):
        return any(_report_has_path_leak(item) for item in value.values())
    return False


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="TsuiNoSora local-only conversion report helpers")
    sub = parser.add_subparsers(dest="command", required=True)
    inventory = sub.add_parser("inventory")
    inventory.add_argument("--root", required=True)
    inventory.add_argument("--alias", required=True)
    extract = sub.add_parser("extract-readable")
    extract.add_argument("--source-root", required=True)
    extract.add_argument("--work-root", required=True)
    extract.add_argument("--alias", default="original_install_root")
    analyze = sub.add_parser("analyze-assets")
    analyze.add_argument("--root", required=True)
    director_resource_map = sub.add_parser("director-resource-map")
    director_resource_map.add_argument("--root", required=True)
    director_cast_map = sub.add_parser("director-cast-map")
    director_cast_map.add_argument("--root", required=True)
    director_lingo_map = sub.add_parser("director-lingo-map")
    director_lingo_map.add_argument("--root", required=True)
    route_graph = sub.add_parser("route-graph")
    route_graph.add_argument("--root", required=True)
    script_source_map = sub.add_parser("script-source-map")
    script_source_map.add_argument("--root", required=True)
    cast_source_map = sub.add_parser("cast-source-map")
    cast_source_map.add_argument("--root", required=True)
    refs = sub.add_parser("reference-report")
    refs.add_argument("--title", required=True)
    refs.add_argument("--game", required=True)
    conversion = sub.add_parser("conversion-report")
    conversion.add_argument("--inventory", required=True)
    conversion.add_argument("--asset-analysis", required=True)
    conversion.add_argument("--routes", required=True)
    modern_profile = sub.add_parser("modern-profile-report")
    modern_profile.add_argument("--conversion", required=True)
    modern_profile.add_argument("--features", required=True)
    route_scenarios = sub.add_parser("route-scenarios")
    route_scenarios.add_argument("--target", required=True)
    route_scenarios.add_argument("--profile", required=True)
    route_scenarios.add_argument("--platform", required=True)
    route_scenarios.add_argument("--routes", required=True)
    nativevn_project = sub.add_parser("nativevn-project")
    nativevn_project.add_argument("--work-root", required=True)
    nativevn_project.add_argument("--routes")
    mount_policy = sub.add_parser("mount-policy")
    mount_policy.add_argument("--target", required=True)
    mount_policy.add_argument("--alias", action="append", default=[])
    stage3_gate = sub.add_parser("stage3-gate")
    stage3_gate.add_argument("--original-root", required=True)
    stage3_gate.add_argument("--work-root", required=True)
    stage3_gate.add_argument("--title", default="Examples/TsuiNoSora/Docs/Title.png")
    stage3_gate.add_argument("--game", default="Examples/TsuiNoSora/Docs/Game.png")
    stage3_gate.add_argument("--remake-root")
    stage3_gate.add_argument("--unpacked-root")
    stage3_gate.add_argument("--routes")
    stage3_gate.add_argument("--features")
    local_gate = sub.add_parser("local-gate")
    local_gate.add_argument("--original-root", required=True)
    local_gate.add_argument("--work-root", required=True)
    local_gate.add_argument("--title", default="Examples/TsuiNoSora/Docs/Title.png")
    local_gate.add_argument("--game", default="Examples/TsuiNoSora/Docs/Game.png")
    local_gate.add_argument("--remake-root")
    local_gate.add_argument("--unpacked-root")
    local_gate.add_argument("--routes")
    local_gate.add_argument("--features")
    demo_slice = sub.add_parser("demo-slice")
    demo_slice.add_argument("--config", required=True)
    demo_config_template = sub.add_parser("demo-config-template")
    demo_config_template.add_argument("--out")
    demo_config_template.add_argument("--force", action="store_true")
    projectorrays_full = sub.add_parser("projectorrays-full-dump")
    projectorrays_full.add_argument("--work-root", required=True)
    projectorrays_full.add_argument("--dump-root", action="append", default=[])
    internal_bundle = sub.add_parser("internal-demo-bundle")
    internal_bundle.add_argument("--config", required=True)
    internal_bundle.add_argument("--repo-root", default=".")
    internal_bundle.add_argument("--astra-bin")
    internal_bundle.add_argument("--player-automation-report")
    args = parser.parse_args(argv)
    if args.command == "inventory":
        report = build_source_inventory(Path(args.root), args.alias)
    elif args.command == "extract-readable":
        report = extract_readable_assets(
            source_root=Path(args.source_root),
            work_root=Path(args.work_root),
            source_alias=args.alias,
        )
    elif args.command == "analyze-assets":
        report = analyze_assets(Path(args.root), reference_report=None)
    elif args.command == "director-resource-map":
        report = build_director_resource_map_report(Path(args.root))
    elif args.command == "director-cast-map":
        report = build_director_cast_map_report(Path(args.root))
    elif args.command == "director-lingo-map":
        report = build_director_lingo_map_report(Path(args.root))
    elif args.command == "route-graph":
        report = build_route_graph_report(Path(args.root))
    elif args.command == "script-source-map":
        report = build_script_source_map_report(Path(args.root))
    elif args.command == "cast-source-map":
        report = build_cast_source_map_report(Path(args.root))
    elif args.command == "reference-report":
        expected_hashes, expected_dimensions = _authoritative_reference_expectations(
            Path(args.title),
            Path(args.game),
        )
        report = build_visual_reference_report(
            Path(args.title),
            Path(args.game),
            expected_hashes=expected_hashes,
            expected_dimensions=expected_dimensions,
        )
    elif args.command == "conversion-report":
        report = build_conversion_report(
            _read_json(Path(args.inventory)),
            _read_json(Path(args.asset_analysis)),
            _read_json(Path(args.routes)),
        )
    elif args.command == "modern-profile-report":
        report = build_modern_profile_report(
            _read_json(Path(args.conversion)),
            _read_json(Path(args.features)),
        )
    elif args.command == "route-scenarios":
        report = build_route_scenarios(
            target=args.target,
            profile=args.profile,
            platform=args.platform,
            routes=_read_json(Path(args.routes)),
        )
    elif args.command == "nativevn-project":
        report = write_nativevn_package_input(
            work_root=Path(args.work_root),
            routes=_read_json(Path(args.routes)) if args.routes else None,
        )
    elif args.command == "mount-policy":
        report = build_mount_policy(
            target=args.target,
            aliases=dict(_split_alias(item) for item in args.alias),
        )
    elif args.command == "stage3-gate":
        report = build_stage3_gate_report(
            original_root=Path(args.original_root),
            work_root=Path(args.work_root),
            title_png=Path(args.title),
            game_png=Path(args.game),
            remake_root=Path(args.remake_root) if args.remake_root else None,
            unpacked_root=Path(args.unpacked_root) if args.unpacked_root else None,
            routes=_read_json(Path(args.routes)) if args.routes else [],
            modern_features=_read_json(Path(args.features)) if args.features else [],
        )
    elif args.command == "local-gate":
        report = run_local_gate(
            original_root=Path(args.original_root),
            work_root=Path(args.work_root),
            title_png=Path(args.title),
            game_png=Path(args.game),
            remake_root=Path(args.remake_root) if args.remake_root else None,
            unpacked_root=Path(args.unpacked_root) if args.unpacked_root else None,
            routes=_read_json(Path(args.routes)) if args.routes else [],
            modern_features=_read_json(Path(args.features)) if args.features else [],
        )
    elif args.command == "demo-slice":
        report = run_demo_slice_gate(Path(args.config))
    elif args.command == "demo-config-template":
        report = write_demo_slice_config_template(
            out_path=Path(args.out) if args.out else None,
            force=bool(args.force),
        )
    elif args.command == "projectorrays-full-dump":
        report = build_projectorrays_full_dump_report(
            work_root=Path(args.work_root),
            dump_roots=[_split_alias_path(item) for item in args.dump_root],
        )
    else:
        report = run_internal_demo_bundle(
            config_path=Path(args.config),
            repo_root=Path(args.repo_root),
            astra_bin=Path(args.astra_bin) if args.astra_bin else None,
            player_automation_report=Path(args.player_automation_report) if args.player_automation_report else None,
        )
    json.dump(report, sys.stdout, ensure_ascii=False, indent=2)
    sys.stdout.write("\n")
    return 0


def _read_json(path: Path) -> dict | list:
    return json.loads(path.read_text(encoding="utf-8"))


def _split_alias(value: str) -> tuple[str, str]:
    if "=" not in value:
        raise SystemExit(f"alias must use name=value: {value}")
    name, alias = value.split("=", 1)
    return name, alias


def _split_alias_path(value: str) -> tuple[str, Path]:
    name, path = _split_alias(value)
    return name, Path(path)


if __name__ == "__main__":
    raise SystemExit(main())
