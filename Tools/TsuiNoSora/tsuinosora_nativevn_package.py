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
from native_story_ir import convert_native_story_ir
from tsuinosora_diagnostics import _is_safe_report_relative_path, _is_safe_symbol, _write_json
from tsuinosora_nativevn_font import _copy_tsuinosora_ui_template, _nativevn_package_input_files, _write_asset_sidecar
from tsuinosora_nativevn_ui_derive import _copy_classic_ui_assets, _derive_director_background_transparent_sprite, _derive_director_character_sprite, _derive_director_dialogue_frame, _derive_director_solid_black, _director_runtime_bindings
from tsuinosora_rendering import _read_json, _render_nativevn_project, _report_has_path_leak, _write_nativevn_section_inputs

__all__ = ['write_nativevn_package_input', '_copy_native_assets_to_nativevn']


def write_nativevn_package_input(work_root: Path | str, routes: list[dict] | None = None) -> dict:
    work_root = Path(work_root)
    reports_root = work_root / "reports"
    nativevn_root = work_root / "nativevn"
    diagnostics = []
    if routes is not None:
        diagnostics.append(
            {
                "code": "TSUI_NATIVEVN_EXPLICIT_ROUTE_INPUT_RETIRED",
                "message": "NativeVN story and route coverage must come from the typed private story IR",
            }
        )

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
    for generated_dir in ("Scripts", "Localization", "Automation"):
        path = nativevn_root / generated_dir
        if path.exists():
            shutil.rmtree(path)
    story_report = convert_native_story_ir(work_root / "private" / "native_story_ir.json", nativevn_root)
    _write_json(reports_root / "full_conversion_coverage_report.json", story_report)
    if story_report.get("status") != "pass":
        diagnostics.append(
            {
                "code": "TSUI_NATIVEVN_FULL_STORY_CONVERSION_BLOCKED",
                "message": "NativeVN package input requires complete typed story conversion coverage",
            }
        )

    section_root = nativevn_root / "PackageSections"
    section_root.mkdir(parents=True, exist_ok=True)

    section_specs = _write_nativevn_section_inputs(reports_root, section_root)
    scenario_refs = sorted(
        str(item["relative_path"])
        for item in story_report.get("generated_files", [])
        if isinstance(item, dict)
        and str(item.get("relative_path", "")).startswith("Automation/")
    )
    wrote_story_inputs = not diagnostics
    if wrote_story_inputs:
        derivation_report = _copy_native_assets_to_nativevn(
            work_root, nativevn_root, conversion_report
        )
        _write_json(reports_root / "runtime_asset_derivation_report.json", derivation_report)
        _copy_tsuinosora_ui_template(work_root, nativevn_root)
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
        "story_source_count": len([item for item in story_report.get("generated_files", []) if str(item.get("relative_path", "")).startswith("Scripts/")]),
        "section_count": len(section_specs),
        "physical_input_sequence_count": len([item for item in story_report.get("generated_files", []) if str(item.get("relative_path", "")).startswith("Automation/")]),
        "route_count": story_report.get("counts", {}).get("routes", 0),
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


def _copy_native_assets_to_nativevn(
    work_root: Path, nativevn_root: Path, conversion_report: dict
) -> dict:
    source_root = work_root / "native-assets"
    target_root = nativevn_root / "native-assets"
    if target_root.exists():
        shutil.rmtree(target_root)
    binding_path = work_root / "private" / "director_asset_bindings.json"
    if not binding_path.is_file():
        coverage = _read_json(work_root / "reports" / "full_conversion_coverage_report.json")
        if coverage.get("counts", {}).get("media_commands") == 0:
            return {
                "schema": "tsuinosora.runtime_asset_derivation_report.v1",
                "status": "pass",
                "derived_asset_count": 0,
                "assets": [],
                "diagnostics": [],
            }
        raise FileNotFoundError(
            "runtime media commands require the validated Director asset binding IR"
        )
    binding_ir = _read_json(binding_path)
    if binding_ir.get("schema") != "tsuinosora.director_asset_binding_ir.v1":
        raise ValueError("runtime asset closure requires the validated Director asset binding IR")
    runtime_assets: dict[str, dict[str, set[str]]] = {}

    def register_runtime_binding(binding: object, role: str | None = None) -> None:
        if not isinstance(binding, dict) or "asset_id" not in binding:
            return
        native_path = str(binding.get("native_path", ""))
        asset_id = str(binding.get("asset_id", ""))
        if (
            not _is_safe_report_relative_path(native_path)
            or not native_path.startswith("native-assets/")
            or not _is_safe_symbol(asset_id)
        ):
            raise ValueError("Director runtime asset binding is unsafe")
        record = runtime_assets.setdefault(asset_id, {"paths": set(), "roles": set()})
        record["paths"].add(native_path)
        if role:
            record["roles"].add(role)

    for binding, role in _director_runtime_bindings(binding_ir):
        register_runtime_binding(binding, role)

    resources = {
        str(resource.get("native_path", "")): resource
        for resource in conversion_report.get("resources", [])
        if isinstance(resource, dict)
    }
    referenced_paths = {
        native_path
        for record in runtime_assets.values()
        for native_path in record["paths"]
    }
    missing = sorted(referenced_paths - set(resources))
    if missing:
        raise ValueError("Director runtime asset closure contains unconverted resources")
    derivations = []
    for asset_id, runtime_record in sorted(runtime_assets.items()):
        candidate_paths = runtime_record["paths"]
        hashes = {
            str(resources[native_path].get("converted_hash", ""))
            for native_path in candidate_paths
        }
        if len(hashes) != 1 or not next(iter(hashes)).startswith("sha256:"):
            raise ValueError("Director semantic asset id maps to conflicting converted payloads")
        native_path = min(candidate_paths)
        resource = resources[native_path]
        source = work_root / native_path
        if not source.is_file():
            raise FileNotFoundError("converted Director runtime asset is missing")
        target = nativevn_root / native_path
        target.parent.mkdir(parents=True, exist_ok=True)
        derived = False
        if "solid_black" in runtime_record["roles"]:
            derived = _derive_director_solid_black(source, target)
        transform = "director_solid_black_palette_v1"
        if not derived and "character" in runtime_record["roles"]:
            derived = _derive_director_character_sprite(source, target)
            transform = "director_white_matte_crop_v1"
        if not derived and "eye" in runtime_record["roles"]:
            derived = _derive_director_background_transparent_sprite(source, target)
            transform = "director_background_transparent_ink_v1"
        if not derived and "dialogue_frame" in runtime_record["roles"]:
            derived = _derive_director_dialogue_frame(source, target)
            transform = "director_dialogue_translucency_v1"
        if not derived:
            shutil.copy2(source, target)
        runtime_resource = dict(resource)
        runtime_resource["converted_hash"] = _sha256(target)
        _write_asset_sidecar(target, native_path, runtime_resource, asset_id)
        if derived:
            derivations.append(
                {
                    "asset_id": asset_id,
                    "native_path": native_path,
                    "source_hash": str(resource.get("converted_hash", "")),
                    "runtime_hash": runtime_resource["converted_hash"],
                    "transform": transform,
                }
            )
    _copy_classic_ui_assets(work_root, nativevn_root, resources)
    report = {
        "schema": "tsuinosora.runtime_asset_derivation_report.v1",
        "status": "pass",
        "derived_asset_count": len(derivations),
        "assets": derivations,
        "diagnostics": [],
        "redaction": {
            "paths": "report_relative_only",
            "payload": "omitted",
            "commercial_text": "omitted",
        },
    }
    if _report_has_path_leak(report):
        raise ValueError("runtime asset derivation report contains a local path-like value")
    return report
