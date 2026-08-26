from __future__ import annotations

import hashlib
import json
from pathlib import Path

from tsuinosora_diagnostics import _dedupe_diagnostics, _looks_like_local_path, _read_json

__all__ = ['build_modern_profile_report', '_builtin_modern_ui_feature']


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


def _builtin_modern_ui_feature() -> tuple[dict | None, list[dict]]:
    """Validate and hash the checked-in Classic/Modern UI project template.

    The hashes are evidence for a reversible presentation feature. They do not
    include generated story/localization payloads and are safe to publish in a
    redacted Stage 3 report.
    """

    repository_root = Path(__file__).resolve().parents[2]
    template_root = repository_root / "Examples" / "TsuiNoSora" / "ProjectTemplate"
    required_files = {
        "classic": [
            "UI/classic.astra",
            "Themes/classic.json",
        ],
        "modern": [
            "UI/modern.astra",
            "Themes/modern.json",
            "Controllers/tsui_ui.luau",
            "Scripts/system.astra",
            "Localization/ja.system.json",
            "Localization/zh-Hans.system.json",
            "Localization/en.system.json",
        ],
        "shared": ["Profiles/ui_profiles.json"],
    }
    diagnostics: list[dict] = []
    for relative_path in sorted(
        path for paths in required_files.values() for path in paths
    ):
        if not (template_root / relative_path).is_file():
            diagnostics.append(
                {
                    "code": "TSUI_MODERN_UI_TEMPLATE_FILE_MISSING",
                    "file_id": relative_path.replace("/", "."),
                    "message": "the checked-in modern UI template is incomplete",
                }
            )

    if diagnostics:
        return None, diagnostics

    try:
        profile_manifest = _read_json(template_root / "Profiles" / "ui_profiles.json")
        if profile_manifest.get("schema") != "tsuinosora.ui_profile_manifest.v1":
            raise ValueError("unexpected UI profile manifest schema")
        if profile_manifest.get("default_profile") != "modern":
            raise ValueError("modern must be the default UI profile")
        profiles = {
            str(profile.get("id", "")): profile
            for profile in profile_manifest.get("profiles", [])
            if isinstance(profile, dict)
        }
        if set(profiles) != {"classic", "modern"}:
            raise ValueError("the UI profile manifest must bind exactly classic and modern")
        for profile_id in ("classic", "modern"):
            profile = profiles[profile_id]
            if (
                profile.get("design_width") != 800
                or profile.get("design_height") != 600
                or profile.get("aspect_policy") != "strict_letterbox"
                or profile.get("core_state_authority") != "shared"
            ):
                raise ValueError(f"{profile_id} does not preserve the shared 800x600 authority contract")

        modern_source = (template_root / "UI" / "modern.astra").read_text(encoding="utf-8")
        for view_id in (
            "ui.tsui.modern.title",
            "ui.tsui.modern.quick_panel",
            "ui.tsui.modern.save",
            "ui.tsui.modern.load",
            "ui.tsui.modern.backlog",
            "ui.tsui.modern.config",
        ):
            if view_id not in modern_source:
                raise ValueError(f"required modern UI view is missing: {view_id}")
        classic_source = (template_root / "UI" / "classic.astra").read_text(encoding="utf-8")
        for view_id in (
            "ui.tsui.classic.message",
            "ui.tsui.classic.title",
            "ui.tsui.classic.save",
            "ui.tsui.classic.load",
        ):
            if view_id not in classic_source:
                raise ValueError(f"required classic UI view is missing: {view_id}")
        for locale in ("ja", "zh-Hans", "en"):
            locale_table = _read_json(template_root / "Localization" / f"{locale}.system.json")
            if not isinstance(locale_table.get("strings"), dict) or not locale_table["strings"]:
                raise ValueError(f"system localization is empty: {locale}")
    except (OSError, UnicodeError, ValueError, json.JSONDecodeError) as error:
        return None, [
            {
                "code": "TSUI_MODERN_UI_TEMPLATE_INVALID",
                "message": str(error),
            }
        ]

    def tree_hash(groups: tuple[str, ...]) -> str:
        digest = hashlib.sha256()
        for relative_path in sorted(
            path for group in groups for path in required_files[group]
        ):
            digest.update(relative_path.encode("utf-8"))
            digest.update(b"\0")
            digest.update((template_root / relative_path).read_bytes())
            digest.update(b"\0")
        return f"sha256:{digest.hexdigest()}"

    classic_hash = tree_hash(("classic", "shared"))
    return (
        {
            "feature_id": "tsui.modern.core_reading_suite",
            "feature_kind": "yakui_system_ui_profile",
            "input_hash": classic_hash,
            "output_hash": tree_hash(("modern", "shared")),
            "fallback_hash": classic_hash,
            "independent_switch": True,
            "affects_core_state": False,
        },
        [],
    )
