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
from tsuinosora_diagnostics import _dedupe_diagnostics, _is_safe_report_relative_path, _is_safe_symbol, _is_sanitized_sha256, _looks_like_local_path
from tsuinosora_rendering import _read_json, _report_has_path_leak

__all__ = ['rearrange_native_assets', 'build_conversion_report', '_conversion_route_record', 'build_route_scenarios', '_route_mount_assets', '_routes_with_native_mount_assets', 'build_mount_policy']


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
