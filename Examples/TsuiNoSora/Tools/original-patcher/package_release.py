#!/usr/bin/env python3
"""Assemble a hash-locked redistributable patcher bundle."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import tempfile
from pathlib import Path

SCHEMA = "tsuinosora.original_patcher_bundle.v1"
HELPER_NAME = "projectorrays-0.2.0.exe"
HELPER_SHA256 = "e9814428ee503cf129b6f5cff54524177b7bdd63201a9095d8d19433535c70db"
LICENSE_NAME = "LICENSE.ProjectorRays.txt"
LICENSE_SHA256 = "66a3107d5ad6a058aab753eaac2047ccb2ed0e39465dd0fe5844da3e300d5172"
PATCHER_NAME = "TsuiNoSoraOriginalPatcher.exe"
NOTICE_NAME = "THIRD_PARTY_NOTICES.md"
LOCALE_DIRECTORY = "LocaleEmulator-2.5.0.1"
LOCALE_FILES = {
    "LoaderDll.dll": "82fae0f44f4ca0c9c37907df74cef2415eeb5fae1cf8d4f36f34ffcaf7e3cc0c",
    "LocaleEmulator.dll": "c79c175fdad174aa46a72197d148316299a56f950aaab1b84930d09ee1084a88",
    "COPYING": "8ceb4b9ee5adedde47b31e975c1d90c73ad27b6b165a1dcd80c7c545eb65b903",
    "COPYING.LESSER": "da7eabb7bafdf7d3ae5e9f223aa5bdc1eece45ac569dc21b3b037520b4464768",
}


def digest(path: Path) -> str:
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(block)
    return value.hexdigest()


def require_file(path: Path, role: str, expected_hash: str | None = None) -> str:
    if not path.is_file() or path.is_symlink():
        raise ValueError(f"{role} must be a regular file")
    actual = digest(path)
    if expected_hash is not None and actual != expected_hash:
        raise ValueError(f"{role} SHA-256 mismatch")
    return actual


def package(patcher: Path, helper: Path, license_file: Path, locale_emulator: Path, output: Path) -> dict[str, object]:
    if output.exists():
        raise ValueError("output must not already exist")
    parent = output.resolve(strict=False).parent
    if not parent.is_dir():
        raise ValueError("output parent must exist")
    patcher_hash = require_file(patcher, "patcher")
    helper_hash = require_file(helper, "ProjectorRays helper", HELPER_SHA256)
    license_hash = require_file(license_file, "ProjectorRays license", LICENSE_SHA256)
    locale_hashes = {
        name: require_file(locale_emulator / name, f"Locale Emulator {name}", expected)
        for name, expected in LOCALE_FILES.items()
    }
    notice = Path(__file__).with_name(NOTICE_NAME)
    notice_hash = require_file(notice, "third-party notices")
    records = [
        {"relative_path": PATCHER_NAME, "sha256": patcher_hash},
        {"relative_path": HELPER_NAME, "sha256": helper_hash},
        {"relative_path": LICENSE_NAME, "sha256": license_hash},
        {"relative_path": NOTICE_NAME, "sha256": notice_hash},
    ]
    records.extend(
        {"relative_path": f"{LOCALE_DIRECTORY}/{name}", "sha256": value}
        for name, value in sorted(locale_hashes.items())
    )
    manifest = {
        "schema": SCHEMA,
        "patcher_version": "0.1.0",
        "projectorrays": {
            "version": "0.2.0",
            "revision": "8a3d3b4211575170276fc6be350b6b52e96d4750",
            "license": "MPL-2.0",
            "sha256": helper_hash,
        },
        "locale_emulator": {
            "version": "2.5.0.1",
            "revision": "db03abf6914beeca09ee975120ff5ce2091c8dca",
            "license": "LGPL-3.0",
            "ansi_code_page": 932,
        },
        "files": records,
    }
    with tempfile.TemporaryDirectory(prefix=".tsuinosora-patcher-", dir=parent) as temporary:
        stage = Path(temporary)
        shutil.copy2(patcher, stage / PATCHER_NAME)
        shutil.copy2(helper, stage / HELPER_NAME)
        shutil.copy2(license_file, stage / LICENSE_NAME)
        shutil.copy2(notice, stage / NOTICE_NAME)
        locale_stage = stage / LOCALE_DIRECTORY
        locale_stage.mkdir()
        for name in LOCALE_FILES:
            shutil.copy2(locale_emulator / name, locale_stage / name)
        (stage / "bundle-manifest.json").write_text(
            json.dumps(manifest, ensure_ascii=True, indent=2) + "\n", encoding="ascii"
        )
        for record in records:
            if digest(stage / str(record["relative_path"])) != record["sha256"]:
                raise RuntimeError("staged release bundle verification failed")
        os.replace(stage, output)
    return manifest


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--patcher", type=Path, required=True)
    parser.add_argument("--projectorrays", type=Path, required=True)
    parser.add_argument("--projectorrays-license", type=Path, required=True)
    parser.add_argument("--locale-emulator", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    manifest = package(
        args.patcher, args.projectorrays, args.projectorrays_license, args.locale_emulator, args.output
    )
    print(json.dumps(manifest, ensure_ascii=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
