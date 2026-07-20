#!/usr/bin/env python3
"""Build a fail-closed, redacted release report for the private Classic RC."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from pathlib import Path, PurePosixPath
from typing import Any


REPORT_SCHEMA = "tsuinosora.private_rc_release_gate.v1"
BUILD_SCHEMA = "astra.build_identity.v1"
BUNDLE_SCHEMA = "astra.standalone_bundle_manifest.v2"
HEADLESS_SCHEMA = "tsuinosora.classic_visual_acceptance_report.v2"
COMPARISON_SCHEMA = "tsuinosora.classic_visual_comparison_report.v3"
NODE_MAP_SCHEMA = "tsuinosora.classic_visual_node_map.v3"
SOURCE_PROFILE_SCHEMA = "astra.source_verification_manifest.v1"
COVERAGE_SCHEMA = "tsuinosora.full_conversion_coverage_report.v1"
SIGNOFF_SCHEMA = "tsuinosora.private_rc_manual_signoff.v1"
Y_ROUTE_SCHEMA = "tsuinosora.classic_y_route_acceptance_report.v1"
HASH_PATTERN = re.compile(r"^sha256:[0-9a-f]{64}$")
RC_REFERENCE_IDS = frozenset(
    f"TSUI1999-UI-{index:03d}" for index in range(1, 15) if index != 4
)
PAYLOAD_SIGNATURES = {
    "png": b"\x89PNG\r\n\x1a\n",
    "ogg": b"OggS\x00\x02",
    "flac": b"fLaC\x00\x00\x00",
    "jpeg": b"\xff\xd8\xff\xe0\x00\x10JFIF",
    "wave": b"WAVEfmt ",
}
MAX_COMMERCIAL_TEXT_PROBES = 64


class GateError(RuntimeError):
    pass


def sha256_bytes(payload: bytes) -> str:
    return "sha256:" + hashlib.sha256(payload).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return "sha256:" + digest.hexdigest()


def load_json(path: Path, schema: str) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict) or value.get("schema") != schema:
        raise GateError(f"TSUI_PRIVATE_RC_SCHEMA: expected {schema}")
    return value


def safe_bundle_path(value: object) -> PurePosixPath:
    if not isinstance(value, str):
        raise GateError("TSUI_PRIVATE_RC_BUNDLE_PATH: path must be a string")
    path = PurePosixPath(value)
    if path.is_absolute() or any(part in {"", ".", ".."} for part in path.parts):
        raise GateError("TSUI_PRIVATE_RC_BUNDLE_PATH: unsafe bundle path")
    return path


def add_check(checks: list[dict[str, str]], check_id: str, passed: bool) -> None:
    checks.append({"id": check_id, "status": "pass" if passed else "blocking"})


def bundle_integrity(bundle_root: Path, manifest: dict[str, Any], package: Path) -> tuple[bool, str]:
    package_hash = sha256_file(package)
    if (
        manifest.get("target") != "tsuinosora-internal-game"
        or manifest.get("profile") != "classic"
        or manifest.get("platform") != "windows"
        or manifest.get("entrypoint") != "AstraPlayer.exe"
        or manifest.get("package_hash") != package_hash
    ):
        return False, package_hash
    declared: set[str] = set()
    for entry in manifest.get("files", []):
        if not isinstance(entry, dict):
            return False, package_hash
        relative = safe_bundle_path(entry.get("path"))
        key = relative.as_posix()
        if key in declared:
            return False, package_hash
        declared.add(key)
        file_path = bundle_root.joinpath(*relative.parts)
        if (
            not file_path.is_file()
            or file_path.stat().st_size != entry.get("byte_size")
            or sha256_file(file_path) != entry.get("hash")
        ):
            return False, package_hash
    bundled_package = bundle_root.joinpath(*safe_bundle_path(manifest.get("package")).parts)
    return bundled_package.is_file() and sha256_file(bundled_package) == package_hash, package_hash


def source_profile_binding(
    bundle_root: Path, manifest: dict[str, Any], source_profile_path: Path
) -> tuple[bool, str]:
    source_profile = load_json(source_profile_path, SOURCE_PROFILE_SCHEMA)
    source_hash = sha256_file(source_profile_path)
    entries = [entry for entry in manifest.get("files", []) if entry.get("role") == "source_verification_profile"]
    if len(entries) != 1:
        return False, source_hash
    relative = safe_bundle_path(entries[0].get("path"))
    bundled = bundle_root.joinpath(*relative.parts)
    valid_entries = source_profile.get("entries")
    safe_source_paths: set[str] = set()
    if isinstance(valid_entries, list):
        for item in valid_entries:
            if isinstance(item, dict):
                relative_path = item.get("relative_path")
                if isinstance(relative_path, str):
                    safe_source_paths.add(safe_bundle_path(relative_path).as_posix())
    valid = (
        bundled.is_file()
        and sha256_file(bundled) == source_hash
        and isinstance(valid_entries, list)
        and bool(valid_entries)
        and len(safe_source_paths) == len(valid_entries)
        and all(
            isinstance(item, dict)
            and isinstance(item.get("relative_path"), str)
            and HASH_PATTERN.fullmatch(str(item.get("sha256", ""))) is not None
            and isinstance(item.get("byte_length"), int)
            and item["byte_length"] >= 0
            for item in valid_entries
        )
    )
    return valid, source_hash


def headless_checks(
    build: dict[str, Any], headless: dict[str, Any], package_hash: str, node_map: dict[str, Any]
) -> dict[str, bool]:
    identity_hash = build.get("identity_hash")
    identity = (
        HASH_PATTERN.fullmatch(str(identity_hash or "")) is not None
        and headless.get("status") == "passed"
        and headless.get("build_fingerprint") == identity_hash
        and headless.get("package_hash") == package_hash
        and headless.get("checkpoint_count") == len(headless.get("checkpoint_ids", []))
        and headless.get("checkpoint_count", 0) > 0
        and not headless.get("diagnostics")
    )
    hardware = bool(headless.get("runs"))
    for run in headless.get("runs", []):
        renderer = run.get("renderer_identity", {})
        hardware = hardware and (
            renderer.get("provider") == "wgpu_offscreen"
            and renderer.get("backend") == "dx12"
            and renderer.get("device_type") in {"discrete_gpu", "integrated_gpu"}
            and run.get("renderer_identity_hash") == headless.get("renderer_identity_hash")
        )
    mapped = {entry.get("checkpoint") for entry in node_map.get("entries", [])}
    checkpoints = set(headless.get("checkpoint_ids", []))
    coverage = bool(mapped) and mapped <= checkpoints
    return {
        "headless_identity": identity,
        "headless_hardware": hardware,
        "headless_checkpoint_coverage": coverage,
    }


def authoritative_y_boundary(story_ir: dict[str, Any]) -> tuple[str, str] | None:
    if story_ir.get("schema") != "tsuinosora.native_story_ir.v1":
        return None
    stories = story_ir.get("stories")
    if not isinstance(stories, list) or len(stories) != 1:
        return None
    states = stories[0].get("states")
    if not isinstance(states, list):
        return None
    boundary_state = "director.k.0010.score.0010"
    matches = [state for state in states if state.get("state_id") == boundary_state]
    if len(matches) != 1:
        return None
    scenes = matches[0].get("scenes")
    if not isinstance(scenes, list) or len(scenes) != 1:
        return None
    commands = scenes[0].get("commands")
    if not isinstance(commands, list):
        return None
    waits = [
        command.get("command_id")
        for command in commands
        if command.get("kind") == "timeline"
        and isinstance(command.get("duration_ms"), int)
        and command["duration_ms"] > 0
    ]
    if len(waits) != 1 or not isinstance(waits[0], str):
        return None
    return boundary_state, waits[0]


def y_route_gate(
    y_route: dict[str, Any],
    build_hash: str,
    package_hash: str,
    expected_boundary: tuple[str, str] | None,
) -> bool:
    renderer = y_route.get("renderer_identity", {})
    if expected_boundary is None:
        return False
    boundary_state, boundary_wait_command = expected_boundary
    return (
        y_route.get("status") == "passed"
        and y_route.get("build_fingerprint") == build_hash
        and y_route.get("package_hash") == package_hash
        and y_route.get("route_id") == "route.coverage.001"
        and y_route.get("guaranteed_movie") == "Y"
        and y_route.get("boundary_movie") == "K"
        and y_route.get("boundary_state") == boundary_state
        and y_route.get("boundary_wait_command") == boundary_wait_command
        and y_route.get("checkpoint_id") == "classic.route.y.complete"
        and isinstance(y_route.get("choice_selection_count"), int)
        and y_route["choice_selection_count"] > 0
        and HASH_PATTERN.fullmatch(str(y_route.get("choice_sequence_hash", ""))) is not None
        and renderer.get("provider") == "wgpu_offscreen"
        and renderer.get("backend") == "dx12"
        and renderer.get("device_type") in {"discrete_gpu", "integrated_gpu"}
        and not y_route.get("diagnostics")
    )


def visual_gate(comparison: dict[str, Any], node_map: dict[str, Any]) -> bool:
    node_entries = node_map.get("entries", [])
    all_reference_ids = {entry.get("reference_id") for entry in node_entries}
    expected = all_reference_ids & RC_REFERENCE_IDS
    scoped_checkpoints = {
        entry.get("checkpoint")
        for entry in node_entries
        if entry.get("reference_id") in RC_REFERENCE_IDS
    }
    results = comparison.get("results", [])
    scoped_results = [
        entry for entry in results if entry.get("reference_id") in RC_REFERENCE_IDS
    ]
    actual = {
        entry.get("reference_id")
        for entry in scoped_results
        if entry.get("status") == "pass"
    }
    scoped_diagnostics = [
        diagnostic
        for diagnostic in comparison.get("diagnostics", [])
        if diagnostic.get("check_id") is None
        or diagnostic.get("check_id") in scoped_checkpoints
    ]
    validations_ok = True
    for entry in node_entries:
        if entry.get("reference_id") not in RC_REFERENCE_IDS:
            continue
        identity = entry.get("identity")
        validation = entry.get("reference_validation")
        if not isinstance(identity, dict) or not isinstance(validation, dict):
            validations_ok = False
            continue
        reference_hash = identity.get("reference_sha256")
        method = validation.get("method")
        if validation.get("status") != "verified" or not isinstance(reference_hash, str):
            validations_ok = False
        elif method == "byte_identical_stable_pair":
            validations_ok &= validation.get("capture_pair_sha256") == reference_hash
        elif method == "score_bitmap_resource_closure":
            locator = identity.get("locator")
            resources = identity.get("resource_hashes")
            validations_ok &= (
                entry.get("reference_id") == "TSUI1999-UI-002"
                and validation.get("capture_sha256") == reference_hash
                and isinstance(locator, dict)
                and locator.get("method") == "score_bitmap_text"
                and validation.get("resource_sha256") == locator.get("content_sha256")
                and isinstance(resources, list)
                and validation.get("resource_sha256") in resources
            )
        else:
            validations_ok = False
    return (
        len(all_reference_ids) == 15
        and len(expected) == 13
        and len(scoped_results) == 13
        and actual == expected
        and not scoped_diagnostics
        and validations_ok
    )


def collect_story_text(value: object, output: set[str]) -> None:
    if isinstance(value, dict):
        text = value.get("text")
        if isinstance(text, str) and len(text) >= 8:
            output.add(text)
        for child in value.values():
            collect_story_text(child, output)
    elif isinstance(value, list):
        for child in value:
            collect_story_text(child, output)


def plaintext_scan(package: Path, story_ir: Path) -> tuple[bool, dict[str, int | str]]:
    package_bytes = package.read_bytes()
    story_bytes = story_ir.read_bytes()
    story = json.loads(story_bytes.decode("utf-8"))
    texts: set[str] = set()
    collect_story_text(story, texts)
    candidate_probes: set[bytes] = set()
    for text in texts:
        for encoding in ("utf-8", "cp932"):
            try:
                encoded = text.encode(encoding, errors="strict")
            except UnicodeEncodeError:
                continue
            if len(encoded) >= 16:
                candidate_probes.add(encoded)
    probes = sorted(
        candidate_probes,
        key=lambda value: (-len(value), hashlib.sha256(value).digest()),
    )[:MAX_COMMERCIAL_TEXT_PROBES]
    text_matches = sum(probe in package_bytes for probe in probes)
    signature_matches = sum(signature in package_bytes for signature in PAYLOAD_SIGNATURES.values())
    return text_matches == 0 and signature_matches == 0, {
        "story_ir_sha256": sha256_bytes(story_bytes),
        "text_probe_count": len(probes),
        "text_match_count": text_matches,
        "signature_probe_count": len(PAYLOAD_SIGNATURES),
        "signature_match_count": signature_matches,
    }


def path_scan(package: Path, bundle_root: Path, probe_file: Path | None) -> tuple[bool, dict[str, int | str | None]]:
    if probe_file is None:
        return False, {"probe_manifest_sha256": None, "probe_count": 0, "match_count": 0}
    probes_value = json.loads(probe_file.read_text(encoding="utf-8"))
    if not isinstance(probes_value, list) or not probes_value or not all(isinstance(x, str) for x in probes_value):
        raise GateError("TSUI_PRIVATE_RC_PATH_PROBES: expected a non-empty string array")
    probes = {value.encode("utf-8") for value in probes_value if len(value.encode("utf-8")) >= 6}
    if len(probes) != len(probes_value):
        raise GateError("TSUI_PRIVATE_RC_PATH_PROBES: probes must be unique and at least six bytes")
    searchable = [package.read_bytes()]
    searchable.extend(path.read_bytes() for path in bundle_root.rglob("*.json"))
    matches = sum(any(probe in payload for payload in searchable) for probe in probes)
    return matches == 0, {
        "probe_manifest_sha256": sha256_file(probe_file),
        "probe_count": len(probes),
        "match_count": matches,
    }


def signoff_gate(
    path: Path | None,
    build_hash: str,
    package_hash: str,
    headless_hash: str,
    comparison_hash: str,
    y_route_hash: str,
) -> tuple[bool, str | None]:
    if path is None or not path.is_file():
        return False, None
    value = load_json(path, SIGNOFF_SCHEMA)
    valid = (
        value.get("status") == "approved"
        and value.get("build_identity") == build_hash
        and value.get("package_identity") == package_hash
        and value.get("headless_report_sha256") == headless_hash
        and value.get("comparison_report_sha256") == comparison_hash
        and value.get("y_route_report_sha256") == y_route_hash
        and isinstance(value.get("reviewer"), str)
        and bool(value["reviewer"].strip())
    )
    return valid, sha256_file(path)


def build_report(args: argparse.Namespace) -> dict[str, Any]:
    package = args.package.resolve()
    bundle_root = args.bundle.resolve()
    build = load_json(args.build_identity, BUILD_SCHEMA)
    headless = load_json(args.headless_report, HEADLESS_SCHEMA)
    y_route = load_json(args.y_route_report, Y_ROUTE_SCHEMA)
    comparison = load_json(args.comparison_report, COMPARISON_SCHEMA)
    node_map = load_json(args.node_map, NODE_MAP_SCHEMA)
    coverage = load_json(args.coverage_report, COVERAGE_SCHEMA)
    story_ir = json.loads(args.story_ir.read_text(encoding="utf-8"))
    manifest = load_json(bundle_root / "bundle_manifest.json", BUNDLE_SCHEMA)

    checks: list[dict[str, str]] = []
    diagnostics: list[dict[str, str]] = []
    bundle_ok, package_hash = bundle_integrity(bundle_root, manifest, package)
    add_check(checks, "bundle_integrity", bundle_ok)
    source_ok, source_hash = source_profile_binding(bundle_root, manifest, args.source_profile)
    add_check(checks, "source_profile_binding", source_ok)
    for check_id, passed in headless_checks(build, headless, package_hash, node_map).items():
        add_check(checks, check_id, passed)
    build_hash = str(build.get("identity_hash"))
    add_check(
        checks,
        "guaranteed_route_y",
        y_route_gate(y_route, build_hash, package_hash, authoritative_y_boundary(story_ir)),
    )
    coverage_ok = (
        coverage.get("status") == "pass"
        and coverage.get("counts", {}).get("routes") == 37
        and not coverage.get("diagnostics")
    )
    add_check(checks, "full_content_present", coverage_ok)
    visuals_ok = visual_gate(comparison, node_map)
    add_check(checks, "visual_reference_y_13_of_13", visuals_ok)
    plaintext_ok, plaintext = plaintext_scan(package, args.story_ir)
    add_check(checks, "commercial_plaintext_absent", plaintext_ok)
    path_ok, paths = path_scan(package, bundle_root, args.private_path_probes)
    add_check(checks, "private_path_absent", path_ok)
    headless_hash = sha256_file(args.headless_report)
    comparison_hash = sha256_file(args.comparison_report)
    y_route_hash = sha256_file(args.y_route_report)
    signoff_ok, signoff_hash = signoff_gate(
        args.manual_signoff,
        str(build.get("identity_hash")),
        package_hash,
        headless_hash,
        comparison_hash,
        y_route_hash,
    )
    add_check(checks, "formal_human_signoff", signoff_ok)

    for check in checks:
        if check["status"] != "pass":
            diagnostics.append({"code": "TSUI_PRIVATE_RC_GATE_BLOCKING", "check_id": check["id"]})
    return {
        "schema": REPORT_SCHEMA,
        "status": "passed" if not diagnostics else "blocked",
        "scope": {
            "profile": "classic",
            "locale": "ja",
            "guaranteed_routes": ["Y"],
            "present_unvalidated_route_count": 36,
            "windows_e3": "deferred",
            "distribution": "private_research_preview",
        },
        "identity": {
            "build": build.get("identity_hash"),
            "package": package_hash,
            "bundle_manifest": sha256_file(bundle_root / "bundle_manifest.json"),
            "source_profile": source_hash,
            "headless_report": headless_hash,
            "y_route_report": y_route_hash,
            "comparison_report": comparison_hash,
            "manual_signoff": signoff_hash,
        },
        "counts": {
            "converted_routes": coverage.get("counts", {}).get("routes"),
            "headless_checkpoints": headless.get("checkpoint_count"),
            "visual_checks_required": len(RC_REFERENCE_IDS),
            "visual_checks_present": sum(
                1
                for result in comparison.get("results", [])
                if result.get("reference_id") in RC_REFERENCE_IDS
            ),
            "visual_checks_passed": sum(
                1
                for result in comparison.get("results", [])
                if result.get("reference_id") in RC_REFERENCE_IDS
                and result.get("status") == "pass"
            ),
        },
        "security_scan": {"commercial_plaintext": plaintext, "private_paths": paths},
        "checks": checks,
        "diagnostics": diagnostics,
        "redaction": {
            "commercial_text": "omitted",
            "payload": "omitted",
            "local_paths": "omitted",
            "key_material": "omitted",
        },
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--bundle", type=Path, required=True)
    parser.add_argument("--build-identity", type=Path, required=True)
    parser.add_argument("--headless-report", type=Path, required=True)
    parser.add_argument("--y-route-report", type=Path, required=True)
    parser.add_argument("--comparison-report", type=Path, required=True)
    parser.add_argument("--node-map", type=Path, required=True)
    parser.add_argument("--coverage-report", type=Path, required=True)
    parser.add_argument("--source-profile", type=Path, required=True)
    parser.add_argument("--story-ir", type=Path, required=True)
    parser.add_argument("--private-path-probes", type=Path)
    parser.add_argument("--manual-signoff", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        report = build_report(args)
    except (GateError, OSError, ValueError, json.JSONDecodeError):
        report = {
            "schema": REPORT_SCHEMA,
            "status": "blocked",
            "checks": [],
            "diagnostics": [{"code": "TSUI_PRIVATE_RC_GATE_ERROR"}],
            "redaction": {
                "commercial_text": "omitted",
                "payload": "omitted",
                "local_paths": "omitted",
                "key_material": "omitted",
            },
        }
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"schema": REPORT_SCHEMA, "status": report["status"]}))
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
