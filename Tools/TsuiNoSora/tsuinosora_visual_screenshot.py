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

import ctypes
import ctypes.wintypes

from tsuinosora_constants import *
from tsuinosora_visual_image_utils import _normalize_visual_capture_image
from tsuinosora_diagnostics import _dedupe_diagnostics, _is_safe_symbol, _looks_like_local_path, _write_json
from tsuinosora_rendering import _float_threshold, _is_sha256, _non_negative_int, _report_has_path_leak
from tsuinosora_visual_comparison import _visual_capture_checkpoint

__all__ = ['build_visual_screenshot_capture_report', '_execute_visual_capture_automation', '_sanitize_visual_capture_automation_execution', '_visual_capture_execution_captures', '_visual_capture_execution_role_summary', '_visual_capture_execution_coverage_diagnostics', '_sanitize_visual_capture_automation_diagnostic', '_visual_capture_automation_record', '_visual_capture_automation_sessions', '_visual_capture_automation_scripts', '_string_list', '_visual_capture_launch_environment', '_resolve_visual_capture_launch_command']


def build_visual_screenshot_capture_report(
    work_root: Path | str,
    visual_capture: dict,
    automation_runner=None,
) -> dict:
    work_root = Path(work_root)
    diagnostics = []
    checkpoints = []
    thresholds = visual_capture.get("thresholds", {}) if isinstance(visual_capture, dict) else {}
    if not isinstance(visual_capture, dict) or visual_capture.get("schema") != "tsuinosora.visual_capture_config.v1":
        diagnostics.append(
            {
                "code": "TSUI_VISUAL_CAPTURE_CONFIG_INVALID",
                "message": "visual_capture must use schema tsuinosora.visual_capture_config.v1",
            }
        )
        visual_capture = {}
    raw_checkpoints = visual_capture.get("checkpoints", []) if isinstance(visual_capture, dict) else []
    if not isinstance(raw_checkpoints, list) or not raw_checkpoints:
        diagnostics.append(
            {
                "code": "TSUI_VISUAL_CAPTURE_CHECKPOINTS_MISSING",
                "message": "visual screenshot acceptance requires at least one checkpoint",
            }
        )
        raw_checkpoints = []
    automation_execution = _execute_visual_capture_automation(
        work_root,
        visual_capture,
        automation_runner,
        diagnostics,
    )
    for raw in raw_checkpoints:
        checkpoint, checkpoint_diagnostics = _visual_capture_checkpoint(work_root, raw)
        diagnostics.extend(checkpoint_diagnostics)
        checkpoints.append(checkpoint)
    automation = _visual_capture_automation_record(
        visual_capture.get("capture_automation") if isinstance(visual_capture, dict) else None,
        checkpoints,
        diagnostics,
        automation_execution,
    )
    report = {
        "schema": "tsuinosora.visual_screenshot_capture_report.v1",
        "status": "blocked" if diagnostics else "pass",
        "thresholds": {
            "max_mean_delta": _float_threshold(thresholds, "max_mean_delta", 4.0),
            "max_changed_ratio": _float_threshold(thresholds, "max_changed_ratio", 0.05),
        },
        "automation": automation,
        "checkpoints": checkpoints,
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
                "code": "TSUI_VISUAL_CAPTURE_REPORT_PATH_LEAK",
                "message": "visual screenshot capture report contains a local path-like value",
            }
        )
        report["diagnostics"] = _dedupe_diagnostics(report["diagnostics"])
    _write_json(work_root / "reports" / "visual_screenshot_capture_report.json", report)
    return report


def _execute_visual_capture_automation(
    work_root: Path,
    visual_capture: dict,
    automation_runner,
    diagnostics: list[dict],
) -> dict:
    if not isinstance(visual_capture, dict) or not isinstance(visual_capture.get("capture_automation"), dict):
        return {
            "status": "not_configured",
            "captured_checkpoint_count": 0,
            "screenshot_count": 0,
            "transcript_hash": "",
        }
    if automation_runner is None:
        return {
            "status": "not_run",
            "captured_checkpoint_count": 0,
            "screenshot_count": 0,
            "transcript_hash": "",
        }
    try:
        result = automation_runner(work_root, visual_capture)
    except Exception:
        diagnostics.append(
            {
                "code": "TSUI_VISUAL_CAPTURE_AUTOMATION_EXECUTION_FAILED",
                "message": "visual capture automation runner failed before producing sanitized evidence",
            }
        )
        return {
            "status": "blocked",
            "captured_checkpoint_count": 0,
            "screenshot_count": 0,
            "transcript_hash": "",
        }
    return _sanitize_visual_capture_automation_execution(result, visual_capture, diagnostics)


def _sanitize_visual_capture_automation_execution(
    result: object,
    visual_capture: dict,
    diagnostics: list[dict],
) -> dict:
    if not isinstance(result, dict) or result.get("schema") != "tsuinosora.visual_capture_automation_execution.v1":
        diagnostics.append(
            {
                "code": "TSUI_VISUAL_CAPTURE_AUTOMATION_EXECUTION_INVALID",
                "message": "visual capture automation runner must return execution evidence schema v1",
            }
        )
        return {
            "status": "blocked",
            "captured_checkpoint_count": 0,
            "screenshot_count": 0,
            "transcript_hash": "",
            "capture_roles": [],
        }
    status = str(result.get("status", "blocked"))
    if status not in {"pass", "blocked"}:
        diagnostics.append(
            {
                "code": "TSUI_VISUAL_CAPTURE_AUTOMATION_EXECUTION_STATUS_INVALID",
                "message": "visual capture automation execution status must be pass or blocked",
            }
        )
        status = "blocked"
    captured_checkpoint_count = _non_negative_int(result.get("captured_checkpoint_count", 0))
    screenshot_count = _non_negative_int(result.get("screenshot_count", 0))
    captures = _visual_capture_execution_captures(result.get("captures"), diagnostics)
    capture_roles = _visual_capture_execution_role_summary(captures)
    if status == "pass":
        coverage_diagnostics = _visual_capture_execution_coverage_diagnostics(visual_capture, capture_roles)
        if coverage_diagnostics:
            diagnostics.extend(coverage_diagnostics)
            status = "blocked"
    transcript_hash = str(result.get("transcript_hash", ""))
    if status == "pass" and not _is_sha256(transcript_hash):
        diagnostics.append(
            {
                "code": "TSUI_VISUAL_CAPTURE_AUTOMATION_TRANSCRIPT_HASH_INVALID",
                "message": "passing visual capture automation requires a transcript sha256",
            }
        )
        status = "blocked"
        transcript_hash = ""
    for diagnostic in result.get("diagnostics", []):
        sanitized = _sanitize_visual_capture_automation_diagnostic(diagnostic)
        if sanitized:
            diagnostics.append(sanitized)
    if status != "pass":
        diagnostics.append(
            {
                "code": "TSUI_VISUAL_CAPTURE_AUTOMATION_EXECUTION_BLOCKED",
                "message": "visual capture automation did not produce passing live capture evidence",
            }
        )
    return {
        "status": status,
        "captured_checkpoint_count": captured_checkpoint_count,
        "screenshot_count": screenshot_count,
        "transcript_hash": transcript_hash if status == "pass" else "",
        "capture_roles": capture_roles,
    }


def _visual_capture_execution_captures(raw: object, diagnostics: list[dict]) -> list[dict]:
    if not isinstance(raw, list) or not raw:
        diagnostics.append(
            {
                "code": "TSUI_VISUAL_CAPTURE_AUTOMATION_CAPTURES_MISSING",
                "message": "passing visual capture automation requires per-role capture evidence",
            }
        )
        return []
    captures = []
    for item in raw:
        if not isinstance(item, dict):
            diagnostics.append(
                {
                    "code": "TSUI_VISUAL_CAPTURE_AUTOMATION_CAPTURE_INVALID",
                    "message": "visual capture automation capture evidence must be an object",
                }
            )
            continue
        checkpoint_id = str(item.get("checkpoint_id", ""))
        role = str(item.get("role", ""))
        digest = str(item.get("hash", ""))
        if not _is_safe_symbol(checkpoint_id) or role not in {"original", "demo"} or not _is_sha256(digest):
            diagnostics.append(
                {
                    "code": "TSUI_VISUAL_CAPTURE_AUTOMATION_CAPTURE_INVALID",
                    "checkpoint_id": checkpoint_id if _is_safe_symbol(checkpoint_id) else "unknown",
                    "role": role if role in {"original", "demo"} else "unknown",
                    "message": "visual capture automation capture evidence must use safe checkpoint id, role and hash",
                }
            )
            continue
        captures.append({"checkpoint_id": checkpoint_id, "role": role, "hash": digest})
    return captures


def _visual_capture_execution_role_summary(captures: list[dict]) -> list[dict]:
    roles_by_checkpoint: dict[str, set[str]] = {}
    for capture in captures:
        roles_by_checkpoint.setdefault(capture["checkpoint_id"], set()).add(capture["role"])
    return [
        {"checkpoint_id": checkpoint_id, "roles": sorted(roles)}
        for checkpoint_id, roles in sorted(roles_by_checkpoint.items())
    ]


def _visual_capture_execution_coverage_diagnostics(visual_capture: dict, capture_roles: list[dict]) -> list[dict]:
    roles_by_checkpoint = {
        str(item.get("checkpoint_id", "")): set(item.get("roles", []))
        for item in capture_roles
        if isinstance(item, dict)
    }
    diagnostics = []
    raw_checkpoints = visual_capture.get("checkpoints", []) if isinstance(visual_capture, dict) else []
    if not isinstance(raw_checkpoints, list):
        return diagnostics
    for checkpoint in raw_checkpoints:
        if not isinstance(checkpoint, dict) or not bool(checkpoint.get("required", True)):
            continue
        checkpoint_id = str(checkpoint.get("checkpoint_id", ""))
        if not _is_safe_symbol(checkpoint_id):
            continue
        missing_roles = {"original", "demo"} - roles_by_checkpoint.get(checkpoint_id, set())
        for role in sorted(missing_roles):
            diagnostics.append(
                {
                    "code": "TSUI_VISUAL_CAPTURE_AUTOMATION_ROLE_CAPTURE_MISSING",
                    "checkpoint_id": checkpoint_id,
                    "role": role,
                    "message": "required visual checkpoint is missing same-run capture evidence for a role",
                }
            )
    return diagnostics


def _sanitize_visual_capture_automation_diagnostic(raw: object) -> dict | None:
    if not isinstance(raw, dict):
        return None
    code = str(raw.get("code", "TSUI_VISUAL_CAPTURE_AUTOMATION_DIAGNOSTIC"))
    if not _is_safe_symbol(code):
        code = "TSUI_VISUAL_CAPTURE_AUTOMATION_DIAGNOSTIC"
    out = {"code": code}
    for key in ("checkpoint_id", "route_id", "region_id", "role", "backend", "step_kind", "phase"):
        value = raw.get(key)
        if isinstance(value, str) and _is_safe_symbol(value):
            out[key] = value
    for key in ("exit_code", "count", "duration_ms"):
        if key in raw:
            out[key] = _non_negative_int(raw.get(key))
    message = str(raw.get("message", "visual capture automation diagnostic"))
    if _looks_like_local_path(message):
        message = "visual capture automation reported a blocked condition"
    out["message"] = message
    return out


def _visual_capture_automation_record(
    raw: object,
    checkpoints: list[dict],
    diagnostics: list[dict],
    execution: dict | None = None,
) -> dict:
    execution = execution or {
        "status": "not_run",
        "captured_checkpoint_count": 0,
        "screenshot_count": 0,
        "transcript_hash": "",
        "capture_roles": [],
    }
    if raw is None:
        return {
            "schema": "tsuinosora.visual_capture_automation_report.v1",
            "configured": False,
            "backend": "",
            "session_roles": [],
            "checkpoint_scripts": [],
            "automation_hash": "",
            "execution_status": "not_configured",
            "captured_checkpoint_count": 0,
            "screenshot_count": 0,
            "transcript_hash": "",
            "capture_roles": [],
        }
    if not isinstance(raw, dict) or raw.get("schema") != "tsuinosora.visual_capture_automation.v1":
        diagnostics.append(
            {
                "code": "TSUI_VISUAL_CAPTURE_AUTOMATION_CONFIG_INVALID",
                "message": "capture_automation must use schema tsuinosora.visual_capture_automation.v1",
            }
        )
        raw = {}
    backend = str(raw.get("backend", ""))
    if backend != "windows_sendinput":
        diagnostics.append(
            {
                "code": "TSUI_VISUAL_CAPTURE_AUTOMATION_BACKEND_INVALID",
                "message": "capture automation backend must be windows_sendinput for this milestone",
            }
        )
        backend = "unknown"
    sessions = _visual_capture_automation_sessions(raw.get("sessions"), diagnostics)
    checkpoint_scripts = _visual_capture_automation_scripts(
        raw.get("input_scripts"),
        {str(checkpoint.get("checkpoint_id", "")) for checkpoint in checkpoints},
        diagnostics,
    )
    automation_hash = _sha256_bytes(
        json.dumps(raw, sort_keys=True, separators=(",", ":"), default=str).encode("utf-8")
    )
    return {
        "schema": "tsuinosora.visual_capture_automation_report.v1",
        "configured": True,
        "backend": backend,
        "session_roles": [session["role"] for session in sessions],
        "checkpoint_scripts": checkpoint_scripts,
        "automation_hash": automation_hash,
        "execution_status": str(execution.get("status", "not_run")),
        "captured_checkpoint_count": _non_negative_int(execution.get("captured_checkpoint_count", 0)),
        "screenshot_count": _non_negative_int(execution.get("screenshot_count", 0)),
        "transcript_hash": str(execution.get("transcript_hash", "")),
        "capture_roles": execution.get("capture_roles", []),
    }


def _visual_capture_automation_sessions(raw: object, diagnostics: list[dict]) -> list[dict]:
    if not isinstance(raw, list) or not raw:
        diagnostics.append(
            {
                "code": "TSUI_VISUAL_CAPTURE_AUTOMATION_SESSIONS_MISSING",
                "message": "capture automation requires original and demo sessions",
            }
        )
        return []
    sessions = []
    seen = set()
    for item in raw:
        if not isinstance(item, dict):
            diagnostics.append(
                {
                    "code": "TSUI_VISUAL_CAPTURE_AUTOMATION_SESSION_INVALID",
                    "message": "capture automation session must be an object",
                }
            )
            continue
        role = str(item.get("role", ""))
        if role not in {"original", "demo"}:
            diagnostics.append(
                {
                    "code": "TSUI_VISUAL_CAPTURE_AUTOMATION_SESSION_ROLE_INVALID",
                    "role": role or "unknown",
                    "message": "capture automation session role must be original or demo",
                }
            )
            continue
        if role in seen:
            diagnostics.append(
                {
                    "code": "TSUI_VISUAL_CAPTURE_AUTOMATION_SESSION_DUPLICATE",
                    "role": role,
                    "message": "capture automation session role is duplicated",
                }
            )
            continue
        seen.add(role)
        launch = item.get("launch")
        if not isinstance(launch, dict):
            diagnostics.append(
                {
                    "code": "TSUI_VISUAL_CAPTURE_AUTOMATION_LAUNCH_INVALID",
                    "role": role,
                    "message": "capture automation launch command must be a non-empty string list",
                }
            )
            launch = {}
        elif not _string_list(launch.get("command")):
            diagnostics.append(
                {
                    "code": "TSUI_VISUAL_CAPTURE_AUTOMATION_LAUNCH_INVALID",
                    "role": role,
                    "message": "capture automation launch command must be a non-empty string list",
                }
            )
        if launch.get("working_directory") is not None and not isinstance(launch.get("working_directory"), str):
            diagnostics.append(
                {
                    "code": "TSUI_VISUAL_CAPTURE_AUTOMATION_LAUNCH_INVALID",
                    "role": role,
                    "message": "capture automation working_directory must be a string when present",
                }
            )
        environment = launch.get("environment") if isinstance(launch, dict) else None
        if environment is not None and (
            not isinstance(environment, dict)
            or any(not isinstance(key, str) or not isinstance(value, str) for key, value in environment.items())
        ):
            diagnostics.append(
                {
                    "code": "TSUI_VISUAL_CAPTURE_AUTOMATION_LAUNCH_ENVIRONMENT_INVALID",
                    "role": role,
                    "message": "capture automation launch environment must be a string map when present",
                }
            )
        window_match = item.get("window_match")
        if not isinstance(window_match, dict) or not any(
            isinstance(window_match.get(key), str)
            for key in ("title_contains", "process_name", "class_name")
        ):
            diagnostics.append(
                {
                    "code": "TSUI_VISUAL_CAPTURE_AUTOMATION_WINDOW_MATCH_INVALID",
                    "role": role,
                    "message": "capture automation window_match must identify a title, process or class",
                }
            )
        sessions.append({"role": role})
    missing_roles = {"original", "demo"} - seen
    for role in sorted(missing_roles):
        diagnostics.append(
            {
                "code": "TSUI_VISUAL_CAPTURE_AUTOMATION_SESSION_MISSING",
                "role": role,
                "message": "capture automation requires both original and demo sessions",
            }
        )
    return sessions


def _visual_capture_automation_scripts(
    raw: object,
    checkpoint_ids: set[str],
    diagnostics: list[dict],
) -> list[dict]:
    if not isinstance(raw, list) or not raw:
        diagnostics.append(
            {
                "code": "TSUI_VISUAL_CAPTURE_AUTOMATION_INPUT_SCRIPTS_MISSING",
                "message": "capture automation requires checkpoint input scripts",
            }
        )
        return []
    scripts = []
    seen = set()
    for item in raw:
        if not isinstance(item, dict):
            diagnostics.append(
                {
                    "code": "TSUI_VISUAL_CAPTURE_AUTOMATION_INPUT_SCRIPT_INVALID",
                    "message": "capture automation input script must be an object",
                }
            )
            continue
        checkpoint_id = str(item.get("checkpoint_id", ""))
        if not _is_safe_symbol(checkpoint_id) or checkpoint_id not in checkpoint_ids:
            diagnostics.append(
                {
                    "code": "TSUI_VISUAL_CAPTURE_AUTOMATION_CHECKPOINT_INVALID",
                    "checkpoint_id": checkpoint_id or "unknown",
                    "message": "capture automation input script must target a declared checkpoint",
                }
            )
            continue
        if checkpoint_id in seen:
            diagnostics.append(
                {
                    "code": "TSUI_VISUAL_CAPTURE_AUTOMATION_CHECKPOINT_DUPLICATE",
                    "checkpoint_id": checkpoint_id,
                    "message": "capture automation input script checkpoint is duplicated",
                }
            )
            continue
        seen.add(checkpoint_id)
        steps = item.get("steps")
        if not isinstance(steps, list) or not steps:
            diagnostics.append(
                {
                    "code": "TSUI_VISUAL_CAPTURE_AUTOMATION_STEPS_MISSING",
                    "checkpoint_id": checkpoint_id,
                    "message": "capture automation input script requires at least one step",
                }
            )
            steps = []
        step_kinds = []
        for step in steps:
            if not isinstance(step, dict):
                diagnostics.append(
                    {
                        "code": "TSUI_VISUAL_CAPTURE_AUTOMATION_STEP_INVALID",
                        "checkpoint_id": checkpoint_id,
                        "message": "capture automation step must be an object",
                    }
                )
                continue
            kind = str(step.get("kind", ""))
            if kind not in {"wait", "focus", "key", "mouse", "capture"}:
                diagnostics.append(
                    {
                        "code": "TSUI_VISUAL_CAPTURE_AUTOMATION_STEP_KIND_INVALID",
                        "checkpoint_id": checkpoint_id,
                        "message": "capture automation step kind is not allowed",
                    }
                )
                continue
            step_kinds.append(kind)
        scripts.append(
            {
                "checkpoint_id": checkpoint_id,
                "step_count": len(step_kinds),
                "step_kinds": step_kinds,
            }
        )
    for checkpoint_id in sorted(checkpoint_ids - seen):
        diagnostics.append(
            {
                "code": "TSUI_VISUAL_CAPTURE_AUTOMATION_CHECKPOINT_MISSING",
                "checkpoint_id": checkpoint_id,
                "message": "capture automation must provide an input script for every visual checkpoint",
            }
        )
    return scripts


def _string_list(value: object) -> list[str]:
    if not isinstance(value, list):
        return []
    strings = []
    for item in value:
        if not isinstance(item, str) or not item:
            return []
        strings.append(item)
    return strings


def _visual_capture_launch_environment(base_env: dict[str, str], launch: object) -> dict[str, str]:
    merged = dict(base_env)
    if not isinstance(launch, dict):
        return merged
    raw = launch.get("environment")
    if not isinstance(raw, dict):
        return merged
    for key, value in raw.items():
        if isinstance(key, str) and key and isinstance(value, str):
            merged[key] = value
    return merged


def _resolve_visual_capture_launch_command(command: list[str], cwd_arg: str | None) -> list[str]:
    resolved = list(command)
    if not resolved:
        return resolved
    executable = Path(resolved[0])
    if executable.is_absolute():
        return resolved
    candidates: list[Path] = []
    if cwd_arg:
        candidates.append(Path(cwd_arg) / executable)
    candidates.append(executable)
    for candidate in candidates:
        try:
            if candidate.is_file():
                resolved[0] = str(candidate.resolve())
                return resolved
        except OSError:
            continue
    return resolved
