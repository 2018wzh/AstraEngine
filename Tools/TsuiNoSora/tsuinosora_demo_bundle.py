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
from tsuinosora_diagnostics import _rel, _is_safe_symbol, _is_safe_report_relative_path, _write_json, _dedupe_diagnostics
from tsuinosora_rendering import _read_json, _report_has_path_leak, _render_nativevn_project, _safe_identifier, _write_nativevn_section_inputs, _sanitize_tsuinosora_package_section, _is_forbidden_tsuinosora_package_section_key
from tsuinosora_stage3_gate import run_local_gate, run_demo_slice_gate
from tsuinosora_nativevn_package import write_nativevn_package_input
from tsuinosora_projectorrays_reader import _read_demo_slice_config
from tsuinosora_projectorrays_report import build_projectorrays_full_dump_report
from tsuinosora_visual_screenshot import build_visual_screenshot_capture_report
from tsuinosora_visual_comparison import build_visual_comparison_report
from tsuinosora_visual_automation import run_visual_capture_automation
from tsuinosora_rendering import _non_negative_int


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

__all__ = ['run_internal_demo_bundle', '_astra_command', '_run_bundle_command', '_bundle_file_record', '_bundle_manifest_matches', '_release_report_has_full_playable']


def run_internal_demo_bundle(
    config_path: Path | str,
    repo_root: Path | str = Path("."),
    astra_bin: Path | str | None = None,
    player_automation_report: Path | str | None = None,
    command_runner=None,
    visual_automation_runner=None,
) -> dict:
    config_path = Path(config_path)
    repo_root = Path(repo_root)
    config, config_diagnostics = _read_demo_slice_config(config_path)
    work_root = Path(str(config.get("local_work_root", ""))) if isinstance(config, dict) and config.get("local_work_root") else None
    diagnostics = list(config_diagnostics)
    demo_report = None
    full_dump_report = None
    visual_capture_report = None
    visual_comparison_report = None
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
                repo_root,
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
                repo_root,
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

        if not diagnostics and bool(config.get("require_visual_screenshot_acceptance", True)):
            visual_config = config.get("visual_capture")
            if not isinstance(visual_config, dict):
                diagnostics.append(
                    {
                        "code": "TSUI_INTERNAL_DEMO_VISUAL_CAPTURE_REQUIRED",
                        "message": "internal playable demo requires visual screenshot capture config",
                    }
                )
            else:
                visual_capture_report = build_visual_screenshot_capture_report(
                    work_root,
                    visual_config,
                    automation_runner=visual_automation_runner or run_visual_capture_automation,
                )
                visual_comparison_report = build_visual_comparison_report(
                    work_root,
                    visual_capture_report,
                    visual_config.get("visual_reviews", []),
                )
                if visual_capture_report.get("status") != "pass":
                    diagnostics.append(
                        {
                            "code": "TSUI_INTERNAL_DEMO_VISUAL_CAPTURE_BLOCKED",
                            "message": "internal playable demo requires passing visual screenshot capture evidence",
                        }
                    )
                    diagnostics.extend(visual_capture_report.get("diagnostics", []))
                if visual_comparison_report.get("status") != "pass":
                    diagnostics.append(
                        {
                            "code": "TSUI_INTERNAL_DEMO_VISUAL_COMPARISON_BLOCKED",
                            "message": "internal playable demo requires passing visual comparison evidence",
                        }
                    )
                    diagnostics.extend(visual_comparison_report.get("diagnostics", []))
                if not visual_capture_report.get("automation", {}).get("configured"):
                    diagnostics.append(
                        {
                            "code": "TSUI_INTERNAL_DEMO_VISUAL_AUTOMATION_REQUIRED",
                            "message": "internal playable demo requires automated original/demo screenshot capture intent",
                        }
                    )
                elif visual_capture_report.get("automation", {}).get("execution_status") != "pass":
                    diagnostics.append(
                        {
                            "code": "TSUI_INTERNAL_DEMO_VISUAL_AUTOMATION_BLOCKED",
                            "message": "internal playable demo requires passing automated original/demo screenshot capture execution",
                        }
                    )
                for path, role in [
                    (work_root / "reports" / "visual_screenshot_capture_report.json", "visual_screenshot_capture_report"),
                    (work_root / "reports" / "visual_comparison_report.json", "visual_comparison_report"),
                ]:
                    record = _bundle_file_record(work_root, path, role)
                    if record:
                        files.append(record)

        player_report_path = Path(str(player_automation_report)) if player_automation_report else None
        if not player_report_path and isinstance(config, dict) and config.get("player_automation_report"):
            player_report_path = Path(str(config["player_automation_report"]))
        player_script_path = work_root / "reports" / "live_player_script.json"
        player_transcript_path = work_root / "reports" / "live_player_transcript.json"
        player_trace_path = work_root / "reports" / "live_player_trace.log"
        player_automation_config = config.get("player_automation") if isinstance(config, dict) else None
        if not diagnostics and isinstance(player_automation_config, dict):
            if not player_report_path:
                diagnostics.append(
                    {
                        "code": "TSUI_INTERNAL_DEMO_PLAYER_EVIDENCE_REQUIRED",
                        "message": "player automation requires player_automation_report output path",
                    }
                )
            else:
                timeout_ms = _non_negative_int(player_automation_config.get("timeout_ms", 60000)) or 60000
                result = _run_bundle_command(
                    "player.windows_live_automation",
                    [
                        "cargo",
                        "run",
                        "-p",
                        "astra-player",
                        "--",
                        "--windows-bundle",
                        str(windows_bundle),
                        "--visual-comparison-report",
                        str(work_root / "reports" / "visual_comparison_report.json"),
                        "--output-report",
                        str(player_report_path),
                        "--output-script",
                        str(player_script_path),
                        "--output-transcript",
                        str(player_transcript_path),
                        "--output-trace-log",
                        str(player_trace_path),
                        "--timeout-ms",
                        str(timeout_ms),
                    ],
                    repo_root,
                    command_runner,
                )
                command_reports.append(result["record"])
                if result["status"] != "pass":
                    diagnostics.append(result["diagnostic"])
                for path, role in [
                    (player_report_path, "player_automation_report"),
                    (player_script_path, "player_automation_script"),
                    (player_transcript_path, "player_input_transcript"),
                    (player_trace_path, "player_trace_log"),
                ]:
                    record = _bundle_file_record(work_root, path, role)
                    if record:
                        files.append(record)
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
        "visual_capture": "reports/visual_screenshot_capture_report.json" if visual_capture_report else "",
        "visual_comparison": "reports/visual_comparison_report.json" if visual_comparison_report else "",
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
            encoding="utf-8",
            errors="replace",
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
