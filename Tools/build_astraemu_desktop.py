#!/usr/bin/env python3
"""Build an install-relative, signed AstraEMU desktop distribution.

The family private key is accepted only through the process environment. A
development-only ephemeral signer may be requested explicitly for local E3
runs; its private key is never written to disk or included in evidence.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import re
import secrets
import shutil
import subprocess
import sys
import tempfile
from typing import Any


SUPPORTED_TARGETS = {
    "x86_64-pc-windows-msvc": ("astra-emu-manager.exe", "astra-emu-cli.exe", "astra_emu_fvp.dll"),
    "x86_64-unknown-linux-gnu": ("astra-emu-manager", "astra-emu-cli", "libastra_emu_fvp.so"),
    "aarch64-unknown-linux-gnu": ("astra-emu-manager", "astra-emu-cli", "libastra_emu_fvp.so"),
    "x86_64-apple-darwin": ("astra-emu-manager", "astra-emu-cli", "libastra_emu_fvp.dylib"),
    "aarch64-apple-darwin": ("astra-emu-manager", "astra-emu-cli", "libastra_emu_fvp.dylib"),
}
MAX_REPORT_BYTES = 1024 * 1024


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", required=True, type=pathlib.Path)
    parser.add_argument("--target")
    parser.add_argument("--development-ephemeral-signer", action="store_true")
    parser.add_argument("--signer-identity")
    args = parser.parse_args()

    root = pathlib.Path(__file__).resolve().parents[1]
    output = args.output.resolve()
    if output.exists():
        fail("ASTRA_EMU_DESKTOP_OUTPUT_EXISTS")
    target = args.target or rust_host(root)
    if target not in SUPPORTED_TARGETS:
        fail("ASTRA_EMU_DESKTOP_TARGET_UNSUPPORTED")
    if target != rust_host(root):
        fail("ASTRA_EMU_DESKTOP_NATIVE_BUILD_REQUIRED")

    environment = os.environ.copy()
    configure_signer(environment, args.development_ephemeral_signer, args.signer_identity)
    identity = build_identity(root, target)
    target_root = root / ".tmp" / "astraemu-desktop-target" / identity["identity_id"]
    target_root.mkdir(parents=True, exist_ok=True)
    environment["CARGO_TARGET_DIR"] = str(target_root)

    if not environment.get("ASTRA_EMU_FAMILY_PUBLIC_KEY_HEX"):
        environment["ASTRA_EMU_FAMILY_PUBLIC_KEY_HEX"] = derive_public_key(root, environment)
    validate_hex(environment["ASTRA_EMU_FAMILY_PUBLIC_KEY_HEX"], 64, "ASTRA_EMU_FAMILY_PUBLIC_KEY_ENCODING")

    descriptor = cargo_build(root, target, target_root, environment)
    profile = target_root / target / "release"
    manager_name, cli_name, family_name = SUPPORTED_TARGETS[target]
    manager = profile / manager_name
    cli = profile / cli_name
    family = profile / family_name
    require_file(manager, "ASTRA_EMU_DESKTOP_MANAGER_MISSING")
    require_file(cli, "ASTRA_EMU_DESKTOP_CLI_MISSING")
    require_file(family, "ASTRA_EMU_DESKTOP_FAMILY_MISSING")

    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = pathlib.Path(tempfile.mkdtemp(prefix=f".{output.name}.", dir=output.parent))
    try:
        family_root = temporary / "families" / "fvp"
        family_root.mkdir(parents=True)
        manifest = family_root / "manifest.json"
        native_sign(root, family, descriptor, manifest, target, environment)
        shutil.copy2(family, family_root / family_name)
        shutil.copy2(manager, temporary / manager_name)
        shutil.copy2(cli, temporary / cli_name)
        copy_notice(root / "Emulator" / "THIRD_PARTY_NOTICES.md", temporary)
        copy_notice(root / "Emulator" / "Source" / "Families" / "astra-emu-fvp" / "THIRD_PARTY_NOTICES.md", temporary)
        verify_distribution(temporary, manager_name, cli_name, family_name, manifest, target, environment)
        report = distribution_report(
            temporary,
            manager_name,
            cli_name,
            family_name,
            manifest,
            target,
            identity,
            args.development_ephemeral_signer,
            environment["ASTRA_EMU_FAMILY_SIGNER_ID"],
        )
        write_json_new(temporary / "astraemu-desktop-package-evidence.json", report)
        temporary.replace(output)
    except BaseException:
        shutil.rmtree(temporary, ignore_errors=True)
        raise

    print(json.dumps({"artifact": output.name, "status": "PASS"}, sort_keys=True))
    return 0


def configure_signer(environment: dict[str, str], ephemeral: bool, signer: str | None) -> None:
    if ephemeral:
        if environment.get("ASTRA_EMU_FAMILY_SIGNING_KEY_HEX"):
            fail("ASTRA_EMU_DESKTOP_EPHEMERAL_SIGNER_CONFLICT")
        environment["ASTRA_EMU_FAMILY_SIGNING_KEY_HEX"] = secrets.token_hex(32)
        environment["ASTRA_EMU_FAMILY_SIGNER_ID"] = signer or "astra.development.local"
        environment.pop("ASTRA_EMU_FAMILY_PUBLIC_KEY_HEX", None)
    else:
        required = ("ASTRA_EMU_FAMILY_SIGNING_KEY_HEX", "ASTRA_EMU_FAMILY_SIGNER_ID")
        missing = [name for name in required if not environment.get(name)]
        if missing:
            fail("ASTRA_EMU_DESKTOP_SIGNER_ENV_MISSING:" + ",".join(missing))
        if signer and signer != environment["ASTRA_EMU_FAMILY_SIGNER_ID"]:
            fail("ASTRA_EMU_DESKTOP_SIGNER_IDENTITY_CONFLICT")
    validate_hex(environment["ASTRA_EMU_FAMILY_SIGNING_KEY_HEX"], 64, "ASTRA_EMU_FAMILY_SIGNING_KEY_ENCODING")
    if not re.fullmatch(r"[a-z0-9][a-z0-9._-]{0,127}", environment["ASTRA_EMU_FAMILY_SIGNER_ID"]):
        fail("ASTRA_EMU_DESKTOP_SIGNER_IDENTITY")


def derive_public_key(root: pathlib.Path, environment: dict[str, str]) -> str:
    result = run_capture(
        ["cargo", "run", "--quiet", "--release", "-p", "astra-emu-family-package", "--", "public-key"],
        root,
        environment,
    ).strip()
    validate_hex(result, 64, "ASTRA_EMU_FAMILY_PUBLIC_KEY_ENCODING")
    return result


def cargo_build(
    root: pathlib.Path,
    target: str,
    target_root: pathlib.Path,
    environment: dict[str, str],
) -> pathlib.Path:
    command = [
        "cargo", "build", "--locked", "--release", "--target", target,
        "-p", "astra-emu-fvp", "-p", "astra-emu-manager", "-p", "astra-emu-cli",
        "--message-format=json-render-diagnostics",
    ]
    process = subprocess.Popen(
        command,
        cwd=root,
        env=environment,
        text=True,
        stdout=subprocess.PIPE,
        stderr=None,
    )
    descriptor: pathlib.Path | None = None
    assert process.stdout is not None
    for line in process.stdout:
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        if message.get("reason") == "build-script-executed" and "astra-emu-fvp" in message.get("package_id", ""):
            candidate = pathlib.Path(message["out_dir"]) / "astra-fvp-descriptor.json"
            descriptor = candidate
    if process.wait() != 0:
        fail("ASTRA_EMU_DESKTOP_CARGO_BUILD_FAILED")
    if descriptor is None:
        candidates = list(target_root.glob(f"{target}/release/build/astra-emu-fvp-*/out/astra-fvp-descriptor.json"))
        if len(candidates) != 1:
            fail("ASTRA_EMU_FVP_DESCRIPTOR_MISSING")
        descriptor = candidates[0]
    require_file(descriptor, "ASTRA_EMU_FVP_DESCRIPTOR_MISSING")
    return descriptor


def native_sign(
    root: pathlib.Path,
    family: pathlib.Path,
    descriptor: pathlib.Path,
    manifest: pathlib.Path,
    target: str,
    environment: dict[str, str],
) -> None:
    run(
        [
            "cargo", "run", "--quiet", "--locked", "--release",
            "-p", "astra-emu-family-package", "--", "native-sign",
            "--binary", str(family), "--descriptor", str(descriptor),
            "--output", str(manifest), "--target", target,
            "--signer-identity", environment["ASTRA_EMU_FAMILY_SIGNER_ID"],
        ],
        root,
        environment,
    )


def verify_distribution(
    root: pathlib.Path,
    manager_name: str,
    cli_name: str,
    family_name: str,
    manifest_path: pathlib.Path,
    target: str,
    environment: dict[str, str],
) -> None:
    require_file(root / manager_name, "ASTRA_EMU_DESKTOP_MANAGER_MISSING")
    require_file(root / cli_name, "ASTRA_EMU_DESKTOP_CLI_MISSING")
    family = root / "families" / "fvp" / family_name
    require_file(family, "ASTRA_EMU_DESKTOP_FAMILY_MISSING")
    manifest = load_json(manifest_path)
    required = {
        "schema", "family_id", "plugin_id", "provider_id", "engine_version",
        "rustc_fingerprint", "feature_fingerprint", "abi_fingerprint", "binary_hash",
        "signer_identity", "signature_algorithm", "signature_hex", "package_eligible",
        "supported_targets", "native_manifest_hash",
    }
    if set(manifest) != required:
        fail("ASTRA_EMU_DESKTOP_MANIFEST_FIELDS")
    if (
        manifest["schema"] != "astra.emu.native_plugin_manifest.v1"
        or manifest["family_id"] != "fvp"
        or manifest["signer_identity"] != environment["ASTRA_EMU_FAMILY_SIGNER_ID"]
        or manifest["signature_algorithm"] != "ed25519-v1"
        or manifest["package_eligible"] is not True
        or manifest["supported_targets"] != [target]
        or manifest["binary_hash"] != "sha256:" + sha256_file(family)
    ):
        fail("ASTRA_EMU_DESKTOP_MANIFEST_IDENTITY")
    validate_hex(manifest["signature_hex"], 128, "ASTRA_EMU_DESKTOP_MANIFEST_SIGNATURE")


def distribution_report(
    root: pathlib.Path,
    manager_name: str,
    cli_name: str,
    family_name: str,
    manifest: pathlib.Path,
    target: str,
    identity: dict[str, Any],
    ephemeral: bool,
    signer_identity: str,
) -> dict[str, Any]:
    manager = root / manager_name
    cli = root / cli_name
    family = root / "families" / "fvp" / family_name
    report = {
        "schema": "astra.emu.desktop_package_evidence.v1",
        "status": "PASS",
        "distribution_tier": "development" if ephemeral else "release-candidate",
        "target": target,
        "manager_file": manager_name,
        "manager_sha256": "sha256." + sha256_file(manager),
        "cli_file": cli_name,
        "cli_sha256": "sha256." + sha256_file(cli),
        "family_file": f"families/fvp/{family_name}",
        "family_sha256": "sha256." + sha256_file(family),
        "family_manifest_sha256": "sha256." + sha256_file(manifest),
        "signer_identity": signer_identity,
        "build_identity": identity,
        "commercial_payload": "omitted",
        "diagnostic_codes": [],
    }
    encoded = json.dumps(report, sort_keys=True).encode("utf-8")
    if len(encoded) > MAX_REPORT_BYTES or has_absolute_path(report):
        fail("ASTRA_EMU_DESKTOP_REPORT_REDACTION")
    return report


def build_identity(root: pathlib.Path, target: str) -> dict[str, str]:
    commit = run_capture(["git", "rev-parse", "HEAD"], root, None).strip()
    status = run_capture(["git", "status", "--porcelain=v1", "--untracked-files=all"], root, None)
    diff = run_bytes(["git", "diff", "--binary", "HEAD"], root)
    untracked = sorted(
        line[3:] for line in status.splitlines() if line.startswith("?? ")
    )
    digest = hashlib.sha256(diff)
    for relative in untracked:
        path = root / relative
        if path.is_file() and ".tmp" not in path.parts:
            payload = path.read_bytes()
            digest.update(relative.replace("\\", "/").encode("utf-8"))
            digest.update(len(payload).to_bytes(8, "big"))
            digest.update(payload)
    lock_hash = sha256_file(root / "Cargo.lock")
    toolchain_hash = sha256_file(root / "rust-toolchain.toml")
    state = "dirty" if status.strip() else "clean"
    identity_seed = f"{commit}\n{state}\n{digest.hexdigest()}\n{lock_hash}\n{toolchain_hash}\n{target}\n"
    identity_id = hashlib.sha256(identity_seed.encode("utf-8")).hexdigest()[:16]
    return {
        "schema": "astra.build_identity.v1",
        "identity_id": identity_id,
        "commit": commit,
        "worktree_state": state,
        "source_state_sha256": "sha256." + digest.hexdigest(),
        "cargo_lock_sha256": "sha256." + lock_hash,
        "rust_toolchain_sha256": "sha256." + toolchain_hash,
        "target": target,
        "profile": "release",
    }


def rust_host(root: pathlib.Path) -> str:
    output = run_capture(["rustc", "-vV"], root, None)
    match = re.search(r"^host:\s*(\S+)$", output, re.MULTILINE)
    if not match:
        fail("ASTRA_EMU_DESKTOP_RUST_HOST_MISSING")
    return match.group(1)


def copy_notice(source: pathlib.Path, output: pathlib.Path) -> None:
    require_file(source, "ASTRA_EMU_DESKTOP_NOTICE_MISSING")
    destination = output / source.name
    if destination.exists():
        destination = output / "FVP_THIRD_PARTY_NOTICES.md"
    shutil.copy2(source, destination)


def load_json(path: pathlib.Path) -> dict[str, Any]:
    if path.stat().st_size > MAX_REPORT_BYTES:
        fail("ASTRA_EMU_DESKTOP_MANIFEST_BOUNDS")
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        fail("ASTRA_EMU_DESKTOP_MANIFEST_TYPE")
    return value


def has_absolute_path(value: Any) -> bool:
    if isinstance(value, dict):
        return any(has_absolute_path(item) for item in value.values())
    if isinstance(value, list):
        return any(has_absolute_path(item) for item in value)
    if isinstance(value, str):
        return bool(re.match(r"^[A-Za-z]:[\\/]", value) or value.startswith(("/", "\\\\")))
    return False


def validate_hex(value: str, length: int, code: str) -> None:
    if not isinstance(value, str) or not re.fullmatch(rf"[0-9a-f]{{{length}}}", value):
        fail(code)


def require_file(path: pathlib.Path, code: str) -> None:
    if not path.is_file() or path.stat().st_size == 0:
        fail(code)


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def write_json_new(path: pathlib.Path, value: dict[str, Any]) -> None:
    with path.open("x", encoding="utf-8", newline="\n") as stream:
        json.dump(value, stream, indent=2, sort_keys=True)
        stream.write("\n")


def run(command: list[str], cwd: pathlib.Path, environment: dict[str, str]) -> None:
    result = subprocess.run(command, cwd=cwd, env=environment, check=False)
    if result.returncode != 0:
        fail("ASTRA_EMU_DESKTOP_EXTERNAL_COMMAND_FAILED")


def run_capture(
    command: list[str], cwd: pathlib.Path, environment: dict[str, str] | None
) -> str:
    result = subprocess.run(
        command, cwd=cwd, env=environment, text=True, capture_output=True, check=False
    )
    if result.returncode != 0:
        fail("ASTRA_EMU_DESKTOP_EXTERNAL_COMMAND_FAILED")
    return result.stdout


def run_bytes(command: list[str], cwd: pathlib.Path) -> bytes:
    result = subprocess.run(command, cwd=cwd, stdout=subprocess.PIPE, check=False)
    if result.returncode != 0:
        fail("ASTRA_EMU_DESKTOP_EXTERNAL_COMMAND_FAILED")
    return result.stdout


def fail(code: str) -> None:
    raise SystemExit(code)


if __name__ == "__main__":
    sys.exit(main())
