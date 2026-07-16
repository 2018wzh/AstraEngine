#!/usr/bin/env python3
"""Run cross with a checkout-local, ignored osxcross installation."""

from __future__ import annotations

import hashlib
import json
import os
import pathlib
import shutil
import subprocess
import sys


TARGETS = {"aarch64-apple-darwin": "oa64-clang", "x86_64-apple-darwin": "o64-clang"}


def sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return "sha256:" + digest.hexdigest()


def fail(code: str, items: list[str]) -> int:
    print(f"{code}: " + ", ".join(items), file=sys.stderr)
    return 2


def main() -> int:
    if len(sys.argv) < 3 or sys.argv[1] not in TARGETS:
        print("usage: run_macos_cross.py <target> <check|clippy> [cargo args...]", file=sys.stderr)
        return 2
    root = pathlib.Path(__file__).resolve().parent.parent
    target = sys.argv[1]
    toolchain = root / ".tmp" / "osxcross"
    compiler = toolchain / "bin" / TARGETS[target]
    sdk = toolchain / "SDK" / "MacOSX13.3.sdk"
    missing = []
    if not compiler.is_file():
        missing.append(str(compiler.relative_to(root)))
    if not sdk.is_dir():
        missing.append(".tmp/osxcross/SDK/MacOSX13.3.sdk")
    elif not (sdk / "SDKSettings.json").is_file():
        missing.append(".tmp/osxcross/SDK/MacOSX13.3.sdk/SDKSettings.json")
    for executable in ("cross", "docker"):
        if shutil.which(executable) is None:
            missing.append(executable)
    if missing:
        return fail("ASTRA_MACOS_CROSS_DEPENDENCY_MISSING", missing)

    identity = {
        "schema": "astra.macos_cross_identity.v1",
        "target": target,
        "deployment_target": "13.0",
        "compiler_sha256": sha256(compiler),
        "sdk": sdk.name,
        "sdk_settings_sha256": sha256(sdk / "SDKSettings.json"),
        "cargo_lock_sha256": sha256(root / "Cargo.lock"),
    }
    identity_bytes = json.dumps(identity, sort_keys=True, separators=(",", ":")).encode()
    identity["identity_hash"] = "sha256:" + hashlib.sha256(identity_bytes).hexdigest()
    report = root / ".tmp" / "macos-cross" / target / "identity.json"
    report.parent.mkdir(parents=True, exist_ok=True)
    report.write_text(json.dumps(identity, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    environment = os.environ.copy()
    environment["MACOSX_DEPLOYMENT_TARGET"] = "13.0"
    environment[f"CARGO_TARGET_{target.upper().replace('-', '_')}_LINKER"] = f"/opt/osxcross/bin/{TARGETS[target]}"
    mount = f"{toolchain.resolve()}:/opt/osxcross:ro"
    existing = environment.get("CROSS_CONTAINER_OPTS", "").strip()
    environment["CROSS_CONTAINER_OPTS"] = f"{existing} --volume {mount}".strip()
    environment["CARGO_TARGET_DIR"] = str(
        root / "target" / "macos-cross" / identity["identity_hash"].removeprefix("sha256:")[:16]
    )
    command = ["cross", sys.argv[2], "--locked", "--target", target, *sys.argv[3:]]
    return subprocess.run(command, cwd=root, env=environment, check=False).returncode


if __name__ == "__main__":
    raise SystemExit(main())
