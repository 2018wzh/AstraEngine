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
from tsuinosora_diagnostics import _dedupe_diagnostics, _is_safe_symbol, _write_json
from tsuinosora_rendering import _report_has_path_leak

__all__ = ['demo_slice_config_template', 'write_demo_slice_config_template', '_demo_slice_config_diagnostics']


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
    if "require_visual_screenshot_acceptance" in config and not isinstance(
        config.get("require_visual_screenshot_acceptance"), bool
    ):
        diagnostics.append(
            {
                "code": "TSUI_DEMO_SLICE_CONFIG_VISUAL_ACCEPTANCE_INVALID",
                "message": "require_visual_screenshot_acceptance must be a boolean",
            }
        )
    if "visual_capture" in config and not isinstance(config.get("visual_capture"), dict):
        diagnostics.append(
            {
                "code": "TSUI_DEMO_SLICE_CONFIG_VISUAL_CAPTURE_INVALID",
                "message": "visual_capture must be a sanitized object",
            }
        )
    if "player_automation" in config:
        player_automation = config.get("player_automation")
        if not isinstance(player_automation, dict):
            diagnostics.append(
                {
                    "code": "TSUI_DEMO_SLICE_CONFIG_PLAYER_AUTOMATION_INVALID",
                    "message": "player_automation must be a sanitized object",
                }
            )
        else:
            if player_automation.get("schema") != "astra.player_live_automation_config.v1":
                diagnostics.append(
                    {
                        "code": "TSUI_DEMO_SLICE_CONFIG_PLAYER_AUTOMATION_SCHEMA_INVALID",
                        "message": "player_automation must use schema astra.player_live_automation_config.v1",
                    }
                )
            if player_automation.get("backend") != "windows_sendinput":
                diagnostics.append(
                    {
                        "code": "TSUI_DEMO_SLICE_CONFIG_PLAYER_AUTOMATION_BACKEND_INVALID",
                        "message": "player_automation backend must be windows_sendinput for this milestone",
                    }
                )
            if "timeout_ms" in player_automation and not isinstance(player_automation.get("timeout_ms"), int):
                diagnostics.append(
                    {
                        "code": "TSUI_DEMO_SLICE_CONFIG_PLAYER_AUTOMATION_TIMEOUT_INVALID",
                        "message": "player_automation timeout_ms must be an integer",
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
