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
from tsuinosora_diagnostics import *
from tsuinosora_rendering import *
from tsuinosora_stage3 import _normalize_stage3_targets
from tsuinosora_visual_analysis import build_visual_reference_report, build_source_inventory
from tsuinosora_director_core import build_director_resource_map_report, build_director_cast_map_report, build_director_lingo_map_report
from tsuinosora_director_core import extract_readable_assets
from tsuinosora_route_graph import build_route_graph_report, analyze_assets
from tsuinosora_script_source_map import build_script_source_map_report
from tsuinosora_cast_source_map import build_cast_source_map_report
from tsuinosora_native_conversion import build_conversion_report, build_route_scenarios, build_mount_policy, rearrange_native_assets
from tsuinosora_projectorrays_report import build_projectorrays_full_dump_report
from tsuinosora_projectorrays_reader import import_projectorrays_reader
from tsuinosora_diagnostics import _blocked_asset_analysis, _dedupe_diagnostics, _empty_inventory, _source_root_diagnostics, _write_json
from tsuinosora_native_conversion import _routes_with_native_mount_assets
from tsuinosora_native_modern_profile import _builtin_modern_ui_feature, build_modern_profile_report
from tsuinosora_nativevn_package import write_nativevn_package_input
from tsuinosora_projectorrays_reader import _read_demo_slice_config
from tsuinosora_projectorrays_report import _external_reader_satisfies_director_preflight, _extract_diagnostics_after_external_reader, _projectorrays_converted_asset_reports, _projectorrays_converted_resources_available, _run_projectorrays_from_demo_config
from tsuinosora_rendering import _report_has_path_leak
from tsuinosora_stage3_story_source import _run_director_story_source_from_demo_config

__all__ = ['build_stage3_gate_report', '_authoritative_reference_expectations', '_is_authoritative_reference_path', 'run_local_gate', 'run_demo_slice_gate']


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

    (
        projectorrays_asset_analysis,
        projectorrays_native_asset_report,
        projectorrays_cast_source_map_report,
        projectorrays_asset_diagnostics,
    ) = _projectorrays_converted_asset_reports(work_root, reference_report, external_reader_report)
    diagnostics.extend(projectorrays_asset_diagnostics)

    if projectorrays_asset_analysis:
        asset_analysis = projectorrays_asset_analysis
    elif unpacked_root and unpacked_root.is_dir():
        asset_analysis = analyze_assets(unpacked_root, reference_report)
    else:
        asset_analysis = _blocked_asset_analysis(
            reference_report,
            "TSUI_UNPACKED_ROOT_MISSING",
            "unpacked assets are required before native-assets rearrange and conversion",
        )
    _write_json(reports_root / "asset_analysis.json", asset_analysis)

    if projectorrays_native_asset_report:
        native_asset_report = projectorrays_native_asset_report
    elif unpacked_root and unpacked_root.is_dir():
        native_asset_report = rearrange_native_assets(unpacked_root, work_root, asset_analysis)
    else:
        native_asset_report = rearrange_native_assets(work_root / "missing-unpacked", work_root, asset_analysis)
    _write_json(reports_root / "native_asset_rearrange_report.json", native_asset_report)

    cast_source_map_report = None
    if projectorrays_cast_source_map_report:
        cast_source_map_report = projectorrays_cast_source_map_report
        _write_json(reports_root / "cast_source_map_report.json", cast_source_map_report)
    elif unpacked_root and unpacked_root.is_dir():
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
        builtin_modern_ui, modern_ui_diagnostics = _builtin_modern_ui_feature()
        diagnostics.extend(modern_ui_diagnostics)
        effective_modern_features = list(modern_features)
        if builtin_modern_ui is not None:
            effective_modern_features.append(builtin_modern_ui)
        modern_profile_report = build_modern_profile_report(
            conversion_report,
            effective_modern_features,
        )
        if modern_ui_diagnostics:
            modern_profile_report["status"] = "blocked"
            modern_profile_report["diagnostics"] = _dedupe_diagnostics(
                modern_profile_report.get("diagnostics", []) + modern_ui_diagnostics
            )
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
    story_source_report = None
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
        story_source_report = _run_director_story_source_from_demo_config(config)
        if story_source_report and story_source_report.get("status") != "pass":
            diagnostics.extend(story_source_report.get("diagnostics", []))
    if not diagnostics:
        configured_unpacked_root = Path(str(config["unpacked_root"])) if config.get("unpacked_root") else None
        if (
            configured_unpacked_root is None
            and work_root is not None
            and _external_reader_satisfies_director_preflight(projectorrays_report)
            and _projectorrays_converted_resources_available(work_root)
        ):
            configured_unpacked_root = work_root / "unpacked"
        local_report = run_local_gate(
            original_root=Path(str(config["original_install_root"])),
            work_root=Path(str(config["local_work_root"])),
            title_png=Path(str(config.get("title_png", "Examples/TsuiNoSora/Docs/Title.png"))),
            game_png=Path(str(config.get("game_png", "Examples/TsuiNoSora/Docs/Game.png"))),
            remake_root=Path(str(config["remake_install_root"])) if config.get("remake_install_root") else None,
            unpacked_root=configured_unpacked_root,
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
            "director_story_source": "reports/director_story_source_report.json" if story_source_report else "",
            "director_scene_dsl": "reports/director_scene_dsl_report.json" if story_source_report else "",
            "director_scene_semantics": "reports/director_scene_semantic_report.json" if story_source_report else "",
            "director_asset_bindings": "reports/director_asset_binding_report.json" if story_source_report else "",
            "director_story_program": "reports/director_story_program_report.json" if story_source_report else "",
            "director_lingo": "reports/director_lingo_report.json" if story_source_report else "",
            "director_story_graph": "reports/director_story_graph_report.json" if story_source_report else "",
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
