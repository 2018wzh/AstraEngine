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

__all__ = ['_rel', '_reference_hashes', '_format_probe', '_is_unpacked_metadata_file', '_format_counts', '_edition_fingerprint', '_classification_counts', '_duplicate_hash_groups', '_asset_usage_index', '_reference_kind', '_container_source', '_use_timing', '_reference_matches', '_path_hints', '_has_hint', '_path_tokens', '_classification_conflicts', '_looks_like_local_path', '_is_safe_report_relative_path', '_is_safe_symbol', '_is_sanitized_sha256', '_positive_int', '_nonnegative_int', '_dedupe_diagnostics', '_source_root_diagnostics', '_empty_inventory', '_blocked_extract_report', '_blocked_asset_analysis', '_write_json', '_routes_from_conversion_report', '_read_json']


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


def _read_json(path: Path) -> dict | list:
    return json.loads(path.read_text(encoding="utf-8"))


def _routes_from_conversion_report(path: Path) -> list[dict]:
    if not path.exists():
        return []
    report = _read_json(path)
    return [
        route
        for route in report.get("routes", [])
        if isinstance(route, dict) and route.get("coverage") == "covered"
    ]
