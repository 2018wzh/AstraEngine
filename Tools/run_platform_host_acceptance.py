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
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--skip-host-runs", action="store_true")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    if git(root, "status", "--porcelain"):
        raise RuntimeError("formal platform evidence requires a clean worktree")
    commit = git(root, "rev-parse", "HEAD")
    if not args.skip_host_runs:
        run(root, ["cargo", "test", "-p", "astra-platform-windows", "--features", "platform-test-driver", "--", "--test-threads=1"])
        run(root, ["wasm-pack", "test", "--headless", "--chrome", "Engine/Source/Platform/astra-platform-web"])
    package_hash = sha256(args.package)
    reports = [
        validate_platform("windows", load(args.windows_capability), load(args.windows_conformance), load(args.windows_player), package_hash),
        validate_platform("web", load(args.web_capability), load(args.web_conformance), load(args.web_player), package_hash),
    ]
    same_identity = len({(item["profile_hash"], item["build_fingerprint"]) for item in reports}) == 1
    status = "pass" if same_identity and all(item["status"] == "pass" for item in reports) else "blocked"
    manifest = {
        "schema": "astra.platform_acceptance_manifest.v1",
        "status": status,
        "commit": commit,
        "package_hash": package_hash,
        "same_identity": same_identity,
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
