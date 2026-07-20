#!/usr/bin/env python3
"""Assemble a private TsuiNoSora RC delivery after every release gate passes."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import sys
import tempfile
from datetime import datetime, timedelta, timezone
from pathlib import Path, PurePosixPath
from typing import Any


GATE_SCHEMA = "tsuinosora.private_rc_release_gate.v1"
BUNDLE_SCHEMA = "astra.standalone_bundle_manifest.v2"
DELIVERY_SCHEMA = "tsuinosora.private_rc_delivery_manifest.v1"
NOTICE_NAME = "PRIVATE_RESEARCH_NOTICE.txt"
MANIFEST_NAME = "private-rc-delivery-manifest.json"


class DeliveryError(RuntimeError):
    pass


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return "sha256:" + digest.hexdigest()


def load_json(path: Path, schema: str) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict) or value.get("schema") != schema:
        raise DeliveryError(f"TSUI_PRIVATE_RC_DELIVERY_SCHEMA: expected {schema}")
    return value


def safe_relative(value: object) -> PurePosixPath:
    if not isinstance(value, str) or not value:
        raise DeliveryError("TSUI_PRIVATE_RC_DELIVERY_PATH: an artifact path is missing")
    path = PurePosixPath(value)
    if path.is_absolute() or ".." in path.parts or any(not part for part in path.parts):
        raise DeliveryError("TSUI_PRIVATE_RC_DELIVERY_PATH: an artifact path is unsafe")
    return path


def parse_timestamp(value: str) -> datetime:
    normalized = value[:-1] + "+00:00" if value.endswith("Z") else value
    try:
        timestamp = datetime.fromisoformat(normalized)
    except ValueError as error:
        raise DeliveryError("TSUI_PRIVATE_RC_DELIVERY_TIME: generated-at is invalid") from error
    if timestamp.tzinfo is None or timestamp.utcoffset() != timedelta(0):
        raise DeliveryError("TSUI_PRIVATE_RC_DELIVERY_TIME: generated-at must use UTC")
    return timestamp.astimezone(timezone.utc)


def timestamp_text(value: datetime) -> str:
    return value.isoformat(timespec="seconds").replace("+00:00", "Z")


def validate_gate(gate: dict[str, Any]) -> None:
    if gate.get("status") != "passed" or gate.get("diagnostics"):
        raise DeliveryError("TSUI_PRIVATE_RC_DELIVERY_GATE: release gate is not passed")
    checks = gate.get("checks")
    if not isinstance(checks, list) or not checks or any(check.get("status") != "pass" for check in checks):
        raise DeliveryError("TSUI_PRIVATE_RC_DELIVERY_GATE: a release check is not passed")
    scope = gate.get("scope", {})
    if (
        scope.get("profile") != "classic"
        or scope.get("locale") != "ja"
        or scope.get("guaranteed_routes") != ["Y"]
        or scope.get("present_unvalidated_route_count") != 36
        or scope.get("windows_e3") != "deferred"
        or scope.get("distribution") != "private_research_preview"
    ):
        raise DeliveryError("TSUI_PRIVATE_RC_DELIVERY_SCOPE: release scope is invalid")
    counts = gate.get("counts", {})
    if counts.get("visual_checks_required") != 13 or counts.get("visual_checks_passed") != 13:
        raise DeliveryError("TSUI_PRIVATE_RC_DELIVERY_VISUALS: 13 visual checks are required")
    if not isinstance(gate.get("identity", {}).get("manual_signoff"), str):
        raise DeliveryError("TSUI_PRIVATE_RC_DELIVERY_SIGNOFF: manual signoff is missing")


def validate_bundle(bundle: Path, manifest: dict[str, Any], gate: dict[str, Any]) -> list[dict[str, Any]]:
    if (
        manifest.get("target") != "tsuinosora-internal-game"
        or manifest.get("profile") != "classic"
        or manifest.get("platform") != "windows"
        or manifest.get("package_hash") != gate.get("identity", {}).get("package")
    ):
        raise DeliveryError("TSUI_PRIVATE_RC_DELIVERY_BUNDLE: bundle identity is invalid")
    files = manifest.get("files")
    if not isinstance(files, list) or not files:
        raise DeliveryError("TSUI_PRIVATE_RC_DELIVERY_BUNDLE: bundle file list is missing")
    seen: set[PurePosixPath] = set()
    verified: list[dict[str, Any]] = []
    for entry in files:
        if not isinstance(entry, dict):
            raise DeliveryError("TSUI_PRIVATE_RC_DELIVERY_BUNDLE: bundle file entry is invalid")
        relative = safe_relative(entry.get("path"))
        if relative in seen:
            raise DeliveryError("TSUI_PRIVATE_RC_DELIVERY_BUNDLE: bundle file path is duplicated")
        seen.add(relative)
        source = bundle.joinpath(*relative.parts)
        if source.is_symlink() or not source.is_file():
            raise DeliveryError("TSUI_PRIVATE_RC_DELIVERY_BUNDLE: bundle file is missing or linked")
        actual_hash = sha256_file(source)
        actual_size = source.stat().st_size
        if entry.get("hash") != actual_hash or entry.get("byte_size") != actual_size:
            raise DeliveryError("TSUI_PRIVATE_RC_DELIVERY_BUNDLE: bundle file integrity failed")
        verified.append(
            {
                "path": relative.as_posix(),
                "role": entry.get("role"),
                "sha256": actual_hash,
                "byte_size": actual_size,
            }
        )
    return verified


def notice() -> str:
    return """AstraEngine TsuiNoSora Classic - Private Research Preview

This is an unofficial, non-commercial, private research preview.
It is not an official KeroQ / Makura product and is not evidence of copyright permission.

The recipient must supply an exact supported copy of the 1999 Japanese release.
The package does not prove legal ownership and must not be forwarded, mirrored, uploaded,
sold, publicly indexed, or redistributed. The encrypted package and any local decrypted
content remain non-redistributable. Do not publish screenshots, recordings, extracted
assets, dialogue, audio, video, caches, saves, reports, or crash artifacts.

Guaranteed scope: Classic profile, Japanese locale, Title, Classic system pages, and the
Y route only. The other 36 converted routes are present as unvalidated research content
and carry no playability guarantee. Windows E3 is deferred; this preview is accepted only
against the bound hardware Headless E2 evidence and formal human visual signoff.

Access is intended for a fixed private group for no more than seven days. The shared link
may be revoked for the entire group at any time. Stop use and delete all copies when the
link expires, is revoked, or the project maintainer requests removal.

『終ノ空』 and all original content remain the property of their respective rights holders.
© ケロＱ
"""


def build_delivery(
    bundle: Path,
    release_gate: Path,
    output: Path,
    generated_at: datetime,
    retention_days: int,
) -> dict[str, Any]:
    if retention_days != 7:
        raise DeliveryError("TSUI_PRIVATE_RC_DELIVERY_RETENTION: retention must be seven days")
    if output.exists():
        raise DeliveryError("TSUI_PRIVATE_RC_DELIVERY_OUTPUT: output already exists")
    gate = load_json(release_gate, GATE_SCHEMA)
    validate_gate(gate)
    manifest_path = bundle / "bundle_manifest.json"
    manifest = load_json(manifest_path, BUNDLE_SCHEMA)
    verified = validate_bundle(bundle, manifest, gate)
    parent = output.parent.resolve()
    parent.mkdir(parents=True, exist_ok=True)
    staging = Path(tempfile.mkdtemp(prefix=f".{output.name}.", dir=parent))
    try:
        payload = staging / "TsuiNoSora-Classic-Private-RC"
        payload.mkdir()
        for entry in verified:
            relative = PurePosixPath(entry["path"])
            source = bundle.joinpath(*relative.parts)
            target = payload.joinpath(*relative.parts)
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, target)
        shutil.copy2(manifest_path, payload / "bundle_manifest.json")
        notice_path = payload / NOTICE_NAME
        notice_path.write_text(notice(), encoding="utf-8", newline="\n")
        expires_at = generated_at + timedelta(days=retention_days)
        delivery_files = [
            {
                "path": entry["path"],
                "role": entry["role"],
                "sha256": entry["sha256"],
                "byte_size": entry["byte_size"],
            }
            for entry in verified
        ]
        delivery_files.extend(
            [
                {
                    "path": "bundle_manifest.json",
                    "role": "bundle_manifest",
                    "sha256": sha256_file(payload / "bundle_manifest.json"),
                    "byte_size": (payload / "bundle_manifest.json").stat().st_size,
                },
                {
                    "path": NOTICE_NAME,
                    "role": "private_research_notice",
                    "sha256": sha256_file(notice_path),
                    "byte_size": notice_path.stat().st_size,
                },
            ]
        )
        report = {
            "schema": DELIVERY_SCHEMA,
            "status": "ready_for_private_distribution",
            "generated_at": timestamp_text(generated_at),
            "expires_at": timestamp_text(expires_at),
            "retention_days": retention_days,
            "scope": gate["scope"],
            "identity": {
                **gate["identity"],
                "release_gate": sha256_file(release_gate),
                "bundle_manifest": sha256_file(manifest_path),
            },
            "files": sorted(delivery_files, key=lambda entry: entry["path"]),
            "distribution": {
                "audience": "fixed_private_group",
                "public_indexing": "forbidden",
                "forwarding": "forbidden",
                "shared_link_retention_days": 7,
                "revocation": "whole_link",
            },
            "redaction": {
                "commercial_text": "omitted",
                "payload": "encrypted_or_omitted",
                "local_paths": "omitted",
                "key_material": "omitted",
            },
        }
        (payload / MANIFEST_NAME).write_text(
            json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
            newline="\n",
        )
        os.replace(staging, output)
        return report
    except BaseException:
        shutil.rmtree(staging, ignore_errors=True)
        raise


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", type=Path, required=True)
    parser.add_argument("--release-gate", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--generated-at", required=True)
    parser.add_argument("--retention-days", type=int, default=7)
    args = parser.parse_args()
    try:
        report = build_delivery(
            args.bundle.resolve(strict=True),
            args.release_gate.resolve(strict=True),
            args.out,
            parse_timestamp(args.generated_at),
            args.retention_days,
        )
    except (DeliveryError, FileNotFoundError):
        print("TSUI_PRIVATE_RC_DELIVERY_BLOCKED", file=sys.stderr)
        return 1
    print(json.dumps({"schema": report["schema"], "status": report["status"]}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
