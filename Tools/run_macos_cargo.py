#!/usr/bin/env python3
"""Run host Cargo against a local osxcross toolchain without containers."""

from __future__ import annotations

import hashlib
import json
import os
import pathlib
import shutil
import subprocess
import sys


TARGETS = {
    "aarch64-apple-darwin": {
        "cc": "oa64-clang",
        "cxx": "oa64-clang++",
        "ar": "aarch64-apple-darwin22.4-ar",
    },
    "x86_64-apple-darwin": {
        "cc": "o64-clang",
        "cxx": "o64-clang++",
        "ar": "x86_64-apple-darwin22.4-ar",
    },
}


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
        print(
            "usage: run_macos_cargo.py <target> <check|clippy> [cargo args...]",
            file=sys.stderr,
        )
        return 2
    if sys.argv[2] not in {"check", "clippy"}:
        return fail("ASTRA_MACOS_CARGO_COMMAND_UNSUPPORTED", [sys.argv[2]])

    root = pathlib.Path(__file__).resolve().parent.parent
    target = sys.argv[1]
    names = TARGETS[target]
    toolchain = pathlib.Path(
        os.environ.get("ASTRA_OSXCROSS_ROOT", "/usr/local/osx-ndk-x86")
    ).resolve()
    bin_dir = toolchain / "bin"
    sdk = toolchain / "SDK" / "MacOSX13.3.sdk"
    tools = {name: bin_dir / executable for name, executable in names.items()}
    missing = [str(path) for path in tools.values() if not path.is_file()]
    sdk_settings = sdk / "SDKSettings.json"
    if not sdk_settings.is_file():
        missing.append(str(sdk_settings))
    for executable in ("cargo", "rustup"):
        if shutil.which(executable) is None:
            missing.append(executable)
    if missing:
        return fail("ASTRA_MACOS_NATIVE_DEPENDENCY_MISSING", missing)

    installed = subprocess.run(
        ["rustup", "target", "list", "--installed"],
        check=False,
        capture_output=True,
        text=True,
    )
    if installed.returncode != 0 or target not in installed.stdout.splitlines():
        return fail("ASTRA_MACOS_RUST_TARGET_MISSING", [target])

    identity = {
        "schema": "astra.macos_cargo_identity.v1",
        "target": target,
        "deployment_target": "13.0",
        "compiler_sha256": sha256(tools["cc"]),
        "sdk": sdk.name,
        "sdk_settings_sha256": sha256(sdk_settings),
        "cargo_lock_sha256": sha256(root / "Cargo.lock"),
    }
    encoded = json.dumps(identity, sort_keys=True, separators=(",", ":")).encode()
    identity["identity_hash"] = "sha256:" + hashlib.sha256(encoded).hexdigest()
    report = root / ".tmp" / "macos-cargo" / target / "identity.json"
    report.parent.mkdir(parents=True, exist_ok=True)
    report.write_text(
        json.dumps(identity, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )

    environment = os.environ.copy()
    environment["PATH"] = f"{bin_dir}{os.pathsep}{environment.get('PATH', '')}"
    environment["SDKROOT"] = str(sdk)
    environment["MACOSX_DEPLOYMENT_TARGET"] = "13.0"
    environment[f"CC_{target.replace('-', '_')}"] = str(tools["cc"])
    environment[f"CXX_{target.replace('-', '_')}"] = str(tools["cxx"])
    environment[f"AR_{target.replace('-', '_')}"] = str(tools["ar"])
    cargo_target = target.upper().replace("-", "_")
    environment[f"CARGO_TARGET_{cargo_target}_LINKER"] = str(tools["cc"])
    identity_hash = identity["identity_hash"].removeprefix("sha256:")[:16]
    environment["CARGO_TARGET_DIR"] = str(
        root / "target" / "macos-cargo" / identity_hash
    )
    command = [
        "cargo",
        sys.argv[2],
        "--locked",
        "--target",
        target,
        *sys.argv[3:],
    ]
    return subprocess.run(command, cwd=root, env=environment, check=False).returncode


if __name__ == "__main__":
    raise SystemExit(main())
