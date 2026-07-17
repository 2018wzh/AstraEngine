#!/usr/bin/env python3
"""Validate same-commit Windows/Web Migration 8 evidence without recording local paths."""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
from pathlib import Path


CAPABILITY_SCHEMA = "astra.platform_capability_report.v2"
CONFORMANCE_SCHEMA = "astra.platform_host_conformance_report.v1"
PLAYER_SCHEMA = "astra.player_automation_report.v1"
HEADLESS_RUN_SCHEMA = "astra.headless_run_report.v2"
HEADLESS_REVIEW_BUNDLE_SCHEMA = "astra.headless_review_bundle.v2"
HEADLESS_REVIEW_SCHEMA = "astra.headless_review.v2"
PLATFORM_RUN_IDENTITY_SCHEMA = "astra.platform_run_identity.v1"
PREFLIGHT_LINK_SCHEMA = "astra.headless_preflight_link.v2"
REQUIRED = {
    "windows": {
        "host.lifecycle", "window.create_destroy", "surface.present_readback",
        "input.native_consumption", "audio.output_meter", "decode.platform",
        "save.atomic_reopen", "package.hash_range", "resource.zero_leaks",
    },
    "web": {
        "host.lifecycle", "window.canvas", "surface.webgpu_present_readback",
        "input.dom_consumption", "audio.webaudio_meter", "decode.webcodecs",
        "save.opfs_atomic_reopen", "package.hash_range", "resource.zero_leaks",
    },
}


def run(root: Path, command: list[str]) -> None:
    completed = subprocess.run(command, cwd=root, check=False)
    if completed.returncode:
        raise RuntimeError(f"acceptance command failed: {command[0]} {command[1]}")


def git(root: Path, *args: str) -> str:
    return subprocess.check_output(["git", *args], cwd=root, text=True).strip()


def load(path: Path) -> dict:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return f"sha256:{digest.hexdigest()}"


def evidence(check: dict, key: str) -> str | None:
    for entry in check.get("evidence", []):
        if entry.get("key") == key:
            return entry.get("value")
    return None


def validate_headless_review(run_path: Path, bundle_path: Path,
                             review_path: Path) -> dict:
    run = load(run_path)
    bundle = load(bundle_path)
    review = load(review_path)
    run_hash = sha256(run_path)
    errors: list[str] = []
    if run.get("schema") != HEADLESS_RUN_SCHEMA or run.get("status") != "passed":
        errors.append("headless.run")
    if any(not item.get("passed") for item in run.get("checkpoint_results", [])):
        errors.append("headless.automatic_checkpoint")
    if bundle.get("schema") != HEADLESS_REVIEW_BUNDLE_SCHEMA:
        errors.append("headless.bundle.schema")
    if bundle.get("run_report_hash") != run_hash:
        errors.append("headless.bundle.run_hash")
    if bundle.get("manifest_hash") != run.get("manifest_hash"):
        errors.append("headless.bundle.manifest_hash")
    if bundle.get("automatic_passed") is not True:
        errors.append("headless.bundle.automatic")
    if not bundle.get("selected_frames") or not bundle.get("selected_audio"):
        errors.append("headless.bundle.artifacts")
    if review.get("schema") != HEADLESS_REVIEW_SCHEMA:
        errors.append("headless.review.schema")
    if review.get("run_report_hash") != run_hash:
        errors.append("headless.review.run_hash")
    if review.get("reviewer_kind") not in {"model", "human"}:
        errors.append("headless.review.kind")
    if not review.get("reviewer_identity") or not is_sha256(review.get("tool_identity_hash")):
        errors.append("headless.review.identity")
    required = set(bundle.get("required_checkpoints", []))
    verdicts = review.get("checkpoints", [])
    reviewed = {item.get("checkpoint") for item in verdicts}
    if not required or reviewed != required or any(not item.get("passed") for item in verdicts):
        errors.append("headless.review.verdicts")
    if errors:
        raise RuntimeError("formal Headless review blocked: " + ",".join(errors))
    return run


def is_sha256(value: object) -> bool:
    if not isinstance(value, str) or not value.startswith("sha256:"):
        return False
    digest = value.removeprefix("sha256:")
    return len(digest) == 64 and all(character in "0123456789abcdef" for character in digest)


def validate_platform_preflight(platform: str, headless: dict, headless_run_path: Path,
                                identity_path: Path, link_path: Path,
                                player_path: Path, conformance: dict,
                                package_hash: str) -> dict:
    identity = load(identity_path)
    link = load(link_path)
    errors: list[str] = []
    if identity.get("schema") != PLATFORM_RUN_IDENTITY_SCHEMA:
        errors.append("identity.schema")
    if identity.get("run_report_hash") != sha256(player_path):
        errors.append("identity.run_report_hash")
    expected = {
        "build_fingerprint": headless.get("build_fingerprint"),
        "cooked_package_hash": package_hash,
        "input_sequence_hash": headless.get("input_sequence_hash"),
        "scenario": headless.get("scenario"),
        "target": headless.get("target"),
        "content_identity": headless.get("content_identity"),
        "profile_id": conformance.get("profile_hash"),
        "session_id": conformance.get("session_id"),
    }
    for field, value in expected.items():
        if identity.get(field) != value:
            errors.append(f"identity.{field}")
    if link.get("schema") != PREFLIGHT_LINK_SCHEMA:
        errors.append("link.schema")
    link_expected = {
        "headless_run_report_hash": sha256(headless_run_path),
        "platform_run_report_hash": sha256(identity_path),
        "build_fingerprint": headless.get("build_fingerprint"),
        "cooked_package_hash": package_hash,
        "input_sequence_hash": headless.get("input_sequence_hash"),
        "scenario": headless.get("scenario"),
        "target": headless.get("target"),
        "content_identity": headless.get("content_identity"),
        "headless_profile_id": headless.get("profile_id"),
        "headless_session_id": headless.get("session_id"),
        "platform_profile_id": identity.get("profile_id"),
        "platform_session_id": identity.get("session_id"),
    }
    for field, value in link_expected.items():
        if link.get(field) != value:
            errors.append(f"link.{field}")
    if errors:
        raise RuntimeError(
            f"{platform} Headless preflight blocked: " + ",".join(errors)
        )
    return {
        "platform": platform,
        "status": "pass",
        "identity_hash": sha256(identity_path),
        "link_hash": sha256(link_path),
        "profile_id": identity["profile_id"],
        "session_id": identity["session_id"],
    }


def validate_platform(platform: str, capability: dict, conformance: dict, player: dict,
                      package_hash: str) -> dict:
    errors: list[str] = []
    if capability.get("schema") != CAPABILITY_SCHEMA:
        errors.append("capability.schema")
    if conformance.get("schema") != CONFORMANCE_SCHEMA:
        errors.append("conformance.schema")
    if player.get("schema") != PLAYER_SCHEMA:
        errors.append("player.schema")
    if capability.get("platform") != platform or conformance.get("platform") != platform:
        errors.append("platform.identity")
    if conformance.get("package_hash") != package_hash or player.get("package_hash") != package_hash:
        errors.append("package.identity")
    if capability.get("profile_hash") != conformance.get("profile_hash"):
        errors.append("profile.identity")
    if capability.get("build_fingerprint") != conformance.get("build_fingerprint"):
        errors.append("build.identity")
    if conformance.get("status") != "pass" or player.get("status") != "pass":
        errors.append("report.status")
    checks = {item.get("id"): item for item in conformance.get("checks", [])}
    for check_id in sorted(REQUIRED[platform]):
        check = checks.get(check_id)
        if not check or check.get("status") != "pass" or not check.get("evidence"):
            errors.append(f"conformance.{check_id}")
    player_checks = player.get("checks", [])
    full = next((item for item in player_checks if item.get("id") == "player.full_playable"), None)
    if not full or full.get("status") != "pass":
        errors.append("player.full_playable")
    else:
        for key, expected in (
            ("profile_hash", conformance.get("profile_hash")),
            ("build_fingerprint", conformance.get("build_fingerprint")),
            ("session_id", conformance.get("session_id")),
        ):
            if evidence(full, key) != expected:
                errors.append(f"player.{key}")
    for domain in ("renderer", "decode", "audio", "save"):
        selection = capability.get(domain, {})
        selected = selection.get("selected")
        if not selected or selected not in selection.get("declared", []) or selected not in selection.get("available", []):
            errors.append(f"provider.{domain}")
    return {
        "platform": platform,
        "status": "pass" if not errors else "blocked",
        "profile_hash": conformance.get("profile_hash", ""),
        "build_fingerprint": conformance.get("build_fingerprint", ""),
        "session_id": conformance.get("session_id", ""),
        "selected": {
            domain: capability.get(domain, {}).get("selected")
            for domain in ("renderer", "decode", "audio", "save")
        },
        "check_count": len(checks),
        "diagnostics": errors,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--windows-capability", type=Path, required=True)
    parser.add_argument("--windows-conformance", type=Path, required=True)
    parser.add_argument("--windows-player", type=Path, required=True)
    parser.add_argument("--web-capability", type=Path, required=True)
    parser.add_argument("--web-conformance", type=Path, required=True)
    parser.add_argument("--web-player", type=Path, required=True)
    parser.add_argument("--headless-run-report", type=Path, required=True)
    parser.add_argument("--headless-review-bundle", type=Path, required=True)
    parser.add_argument("--headless-review", type=Path, required=True)
    parser.add_argument("--windows-platform-run-identity", type=Path, required=True)
    parser.add_argument("--windows-preflight-link", type=Path, required=True)
    parser.add_argument("--web-platform-run-identity", type=Path, required=True)
    parser.add_argument("--web-preflight-link", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--skip-host-runs", action="store_true")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    if git(root, "status", "--porcelain"):
        raise RuntimeError("formal platform evidence requires a clean worktree")
    commit = git(root, "rev-parse", "HEAD")
    package_hash = sha256(args.package)
    headless = validate_headless_review(
        args.headless_run_report,
        args.headless_review_bundle,
        args.headless_review,
    )
    windows_conformance = load(args.windows_conformance)
    web_conformance = load(args.web_conformance)
    preflights = [
        validate_platform_preflight(
            "windows", headless, args.headless_run_report,
            args.windows_platform_run_identity, args.windows_preflight_link,
            args.windows_player, windows_conformance, package_hash,
        ),
        validate_platform_preflight(
            "web", headless, args.headless_run_report,
            args.web_platform_run_identity, args.web_preflight_link,
            args.web_player, web_conformance, package_hash,
        ),
    ]
    if not args.skip_host_runs:
        run(root, ["cargo", "test", "-p", "astra-platform-windows", "--features", "platform-test-driver", "--", "--test-threads=1"])
        run(root, ["wasm-pack", "test", "--headless", "--chrome", "Engine/Source/Platform/astra-platform-web"])
    reports = [
        validate_platform("windows", load(args.windows_capability), windows_conformance, load(args.windows_player), package_hash),
        validate_platform("web", load(args.web_capability), web_conformance, load(args.web_player), package_hash),
    ]
    same_identity = len({(item["profile_hash"], item["build_fingerprint"]) for item in reports}) == 1
    status = "pass" if same_identity and all(item["status"] == "pass" for item in reports) else "blocked"
    manifest = {
        "schema": "astra.platform_acceptance_manifest.v1",
        "status": status,
        "commit": commit,
        "package_hash": package_hash,
        "same_identity": same_identity,
        "headless_run_report_hash": sha256(args.headless_run_report),
        "headless_review_bundle_hash": sha256(args.headless_review_bundle),
        "headless_review_hash": sha256(args.headless_review),
        "preflights": preflights,
        "platforms": reports,
    }
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return 0 if status == "pass" else 1


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError, subprocess.CalledProcessError, json.JSONDecodeError) as error:
        print(f"platform acceptance blocked: {error}", file=sys.stderr)
        raise SystemExit(1)
