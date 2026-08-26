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
from tsuinosora_diagnostics import _is_safe_report_relative_path, _looks_like_local_path, _write_json, _read_json

__all__ = ['_write_nativevn_section_inputs', '_sanitize_tsuinosora_package_section', '_is_forbidden_tsuinosora_package_section_key', '_render_nativevn_project', '_safe_identifier', '_report_has_path_leak', '_read_json', '_split_alias', '_split_alias_path', '_float_threshold', '_non_negative_int', '_safe_work_relative_path', '_is_sha256']


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
            "tsuinosora.full_conversion_coverage",
            "tsuinosora.full_conversion_coverage_report.v1",
            "full_conversion_coverage_report.json",
            "full_conversion_coverage_report.json",
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


def _render_nativevn_project(section_specs: list[dict], scenario_refs: list[str]) -> str:
    lines = [
        "schema: astra.target_manifest.v2",
        "id: com.example.tsuinosora.stage3",
        "platform_profiles:",
        "  windows-internal-release:",
        "    schema: astra.platform_host_profile.v2",
        "    id: windows-internal-release",
        "    platform: windows",
        "    target: tsuinosora-internal-game",
        "    package_id: com.example.tsuinosora.stage3",
        "    renderer: { providers: [wgpu_hardware], allow_software: false }",
        "    decode: { providers: [wmf], allow_software: false }",
        "    audio: { providers: [wasapi], allow_software: false }",
        "    save: { providers: [saved_games], allow_software: false }",
        "    package_sources: [{ kind: bundled }]",
        "    limits: { command_queue_capacity: 256, event_queue_capacity: 1024, max_frame_bytes: 67108864, max_audio_frames: 192000, max_package_read_bytes: 8388608 }",
        "    package_cache: { max_entry_bytes: 17179869184, max_total_bytes: 68719476736 }",
        "  web-release-chrome:",
        "    schema: astra.platform_host_profile.v2",
        "    id: web-release-chrome",
        "    platform: web",
        "    target: tsuinosora-internal-game",
        "    package_id: com.example.tsuinosora.stage3",
        "    renderer: { providers: [webgpu], allow_software: false }",
        "    decode: { providers: [webcodecs], allow_software: false }",
        "    audio: { providers: [webaudio], allow_software: false }",
        "    save: { providers: [opfs], allow_software: false }",
        "    package_sources: [{ kind: bundled }]",
        "    limits: { command_queue_capacity: 256, event_queue_capacity: 1024, max_frame_bytes: 67108864, max_audio_frames: 192000, max_package_read_bytes: 8388608 }",
        "    package_cache: { max_entry_bytes: 17179869184, max_total_bytes: 68719476736 }",
        "targets:",
        "  - id: tsuinosora-internal-game",
        "    kind: game",
        "    crate: astra-vn",
        "    runtime_provider: native_vn",
        "    default_profile: modern",
        "    ui_provider: astra.ui.yakui",
        "    platforms: [headless, windows, web]",
        "    packaged: true",
        "  - id: tsuinosora-patch-game",
        "    kind: game",
        "    crate: astra-vn",
        "    runtime_provider: native_vn",
        "    default_profile: modern",
        "    ui_provider: astra.ui.yakui",
        "    platforms: [headless, windows, web]",
        "    packaged: true",
        "nativevn:",
        "  sources:",
        "    - Scripts",
        "  default_locale: ja",
        "  ui_sources:",
        "    - UI",
        "  ui_themes:",
        "    - Themes",
        "  ui_controllers:",
        "    - Controllers",
        "  profiles: [classic, modern]",
        "  display:",
        "    original_resolution:",
        "      width: 800",
        "      height: 600",
        "    scale_filter: linear",
        "    preview_layers:",
        "      - vfs_uri: package:/native-assets/ui/classic/frame.png",
        "        x: 0",
        "        y: 0",
        "      - vfs_uri: package:/native-assets/ui/classic/menu-save.png",
        "        x: 564",
        "        y: 344",
        "      - vfs_uri: package:/native-assets/ui/classic/menu-load.png",
        "        x: 564",
        "        y: 432",
        "      - vfs_uri: package:/native-assets/ui/classic/menu-exit.png",
        "        x: 564",
        "        y: 520",
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
    else:
        lines.append("package_sections:")
    for locale in ("ja", "zh-Hans", "en"):
        lines.extend(
            [
                f"  - id: vn.localization.{locale}",
                "    schema: astra.vn.localization_table.v1",
                f"    path: Localization/{locale}.json",
                "    codec: raw",
                "    targets: [tsuinosora-internal-game, tsuinosora-patch-game]",
                "    profiles: [classic, modern]",
            ]
        )
    lines.extend(
        [
            "  - id: tsuinosora.ui_profiles",
            "    schema: tsuinosora.ui_profile_manifest.v1",
            "    path: Profiles/ui_profiles.json",
            "    codec: raw",
            "    targets: [tsuinosora-internal-game, tsuinosora-patch-game]",
            "    profiles: [classic, modern]",
        ]
    )
    return "\n".join(lines) + "\n"


def _report_has_path_leak(value) -> bool:
    if isinstance(value, str):
        return _looks_like_local_path(value)
    if isinstance(value, list):
        return any(_report_has_path_leak(item) for item in value)
    if isinstance(value, dict):
        return any(_report_has_path_leak(item) for item in value.values())
    return False


def _split_alias(value: str) -> tuple[str, str]:
    if "=" not in value:
        raise SystemExit(f"alias must use name=value: {value}")
    name, alias = value.split("=", 1)
    return name, alias


def _split_alias_path(value: str) -> tuple[str, Path]:
    name, path = _split_alias(value)
    return name, Path(path)


def _float_threshold(value: dict, key: str, default: float) -> float:
    if not isinstance(value, dict):
        return default
    raw = value.get(key, default)
    try:
        result = float(raw)
    except (TypeError, ValueError):
        return default
    return result if result >= 0.0 else default


def _non_negative_int(value: object) -> int:
    try:
        result = int(value)
    except (TypeError, ValueError):
        return 0
    return max(result, 0)


def _safe_work_relative_path(value: object) -> str:
    if not isinstance(value, str):
        return ""
    value = value.strip()
    return value if _is_safe_report_relative_path(value) else ""


def _is_sha256(value: str) -> bool:
    return bool(re.fullmatch(r"sha256:[0-9a-f]{64}", value))
