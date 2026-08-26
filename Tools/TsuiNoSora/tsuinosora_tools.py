#!/usr/bin/env python3
"""TsuiNoSora Stage 3 local-only conversion helpers.

Thin facade that re-exports all domain modules and provides the CLI entry point.
"""  # noqa: D100

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from tsuinosora_constants import *  # noqa: F401,F403
from tsuinosora_diagnostics import *  # noqa: F401,F403
from tsuinosora_rendering import *  # noqa: F401,F403
from tsuinosora_visual_analysis import *  # noqa: F401,F403
from tsuinosora_visual_image_utils import *  # noqa: F401,F403
from tsuinosora_visual_screenshot import *  # noqa: F401,F403
from tsuinosora_visual_comparison import *  # noqa: F401,F403
from tsuinosora_visual_automation import *  # noqa: F401,F403
from tsuinosora_director_core import *  # noqa: F401,F403
from tsuinosora_route_graph import *  # noqa: F401,F403
from tsuinosora_script_source_map import *  # noqa: F401,F403
from tsuinosora_script_source_routes import *  # noqa: F401,F403
from tsuinosora_cast_source_map import *  # noqa: F401,F403
from tsuinosora_native_conversion import *  # noqa: F401,F403
from tsuinosora_native_modern_profile import *  # noqa: F401,F403
from tsuinosora_projectorrays_convert import *  # noqa: F401,F403
from tsuinosora_projectorrays_convert_metadata import *  # noqa: F401,F403
from tsuinosora_projectorrays_convert_bitmap import *  # noqa: F401,F403
from tsuinosora_projectorrays_media import *  # noqa: F401,F403
from tsuinosora_projectorrays_media_audio import *  # noqa: F401,F403
from tsuinosora_projectorrays_media_video import *  # noqa: F401,F403
from tsuinosora_projectorrays_lscr import *  # noqa: F401,F403
from tsuinosora_projectorrays_report import *  # noqa: F401,F403
from tsuinosora_projectorrays_reader import *  # noqa: F401,F403
from tsuinosora_projectorrays_validate import *  # noqa: F401,F403
from tsuinosora_stage3 import *  # noqa: F401,F403
from tsuinosora_stage3_gate import *  # noqa: F401,F403
from tsuinosora_stage3_story_source import *  # noqa: F401,F403
from tsuinosora_stage3_demo_slice import *  # noqa: F401,F403
from tsuinosora_nativevn_package import *  # noqa: F401,F403
from tsuinosora_nativevn_ui_derive import *  # noqa: F401,F403
from tsuinosora_nativevn_font import *  # noqa: F401,F403
from tsuinosora_demo_bundle import *  # noqa: F401,F403


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
    visual_capture = sub.add_parser("visual-capture")
    visual_capture.add_argument("--work-root", required=True)
    visual_capture.add_argument("--config", required=True)
    visual_comparison = sub.add_parser("visual-comparison")
    visual_comparison.add_argument("--work-root", required=True)
    visual_comparison.add_argument("--capture-report", required=True)
    visual_comparison.add_argument("--visual-reviews", required=True)
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
    projectorrays_convert = sub.add_parser("projectorrays-convert-resources")
    projectorrays_convert.add_argument("--work-root", required=True)
    projectorrays_convert.add_argument("--dump-root", action="append", default=[])
    projectorrays_convert.add_argument("--palette-sidecar", action="append", default=[])
    projectorrays_convert.add_argument("--summary", action="store_true")
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
    elif args.command == "visual-capture":
        report = build_visual_screenshot_capture_report(
            Path(args.work_root),
            _read_json(Path(args.config)),
            automation_runner=run_visual_capture_automation,
        )
    elif args.command == "visual-comparison":
        report = build_visual_comparison_report(
            Path(args.work_root),
            _read_json(Path(args.capture_report)),
            _read_json(Path(args.visual_reviews)),
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
    elif args.command == "projectorrays-convert-resources":
        report = convert_projectorrays_resources(
            work_root=Path(args.work_root),
            dump_roots=[_split_alias_path(item) for item in args.dump_root],
            palette_sidecars=[Path(item) for item in args.palette_sidecar],
        )
        if args.summary:
            report = _projectorrays_conversion_summary(report)
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


if __name__ == "__main__":
    sys.exit(main())
