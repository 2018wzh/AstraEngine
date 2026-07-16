#!/usr/bin/env python3
"""Write an Astra build identity for a direct Cargo invocation.

This command records the checked-out source and the feature-bearing Cargo
arguments. It does not run Cargo and does not select or mutate CARGO_TARGET_DIR.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import subprocess
from collections.abc import Iterable


SCHEMA = "astra.build_identity.v1"


def sha256(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def run_output(command: list[str], root: pathlib.Path) -> bytes:
    return subprocess.run(
        command,
        cwd=root,
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    ).stdout


def manifest_hash(root: pathlib.Path) -> str:
    digest = hashlib.sha256()
    manifests: list[pathlib.Path] = []
    ignored = {".git", ".tmp", ".worktrees", "worktrees", "target"}
    for directory, children, files in os.walk(root):
        children[:] = sorted(child for child in children if child not in ignored)
        if "Cargo.toml" in files:
            manifests.append(pathlib.Path(directory) / "Cargo.toml")
    for manifest in sorted(manifests, key=lambda path: path.as_posix()):
        relative = manifest.relative_to(root).as_posix().encode("utf-8")
        payload = manifest.read_bytes()
        digest.update(len(relative).to_bytes(8, "big"))
        digest.update(relative)
        digest.update(len(payload).to_bytes(8, "big"))
        digest.update(payload)
    return "sha256:" + digest.hexdigest()


def feature_arguments(cargo_args: Iterable[str]) -> list[str]:
    args = list(cargo_args)
    selected: list[str] = []
    index = 0
    value_options = {"--features", "-F", "--target", "--profile"}
    flags = {"--all-features", "--no-default-features", "--release"}
    while index < len(args):
        argument = args[index]
        if argument in value_options:
            selected.append(argument)
            if index + 1 < len(args):
                selected.append(args[index + 1])
                index += 1
        elif argument in flags or argument.startswith("--features="):
            selected.append(argument)
        index += 1
    return selected


def untracked_files(root: pathlib.Path) -> list[tuple[str, bytes]]:
    raw = run_output(["git", "ls-files", "--others", "--exclude-standard", "-z"], root)
    files: list[tuple[str, bytes]] = []
    for encoded in raw.split(b"\0"):
        if not encoded:
            continue
        relative = encoded.decode("utf-8")
        path = root / relative
        if path.is_file():
            files.append((pathlib.PurePath(relative).as_posix(), path.read_bytes()))
    return files


def ui_toolchain_identity(root: pathlib.Path) -> dict[str, object] | None:
    lock_path = root / "Tools" / "ui-toolchain-lock.json"
    if not lock_path.is_file():
        return None
    report_path = root / ".tmp" / "ui-toolchain" / "preflight.json"
    if not report_path.is_file():
        raise ValueError("ASTRA_UI_TOOLCHAIN_PREFLIGHT_MISSING")
    report = json.loads(report_path.read_text(encoding="utf-8"))
    if report.get("schema") != "astra.ui_toolchain_preflight.v1":
        raise ValueError("ASTRA_UI_TOOLCHAIN_PREFLIGHT_SCHEMA_INVALID")
    if report.get("lock_sha256") != sha256(lock_path.read_bytes()):
        raise ValueError("ASTRA_UI_TOOLCHAIN_PREFLIGHT_STALE")
    if report.get("controller_analysis") != "passed":
        raise ValueError("ASTRA_UI_TOOLCHAIN_LUAU_ANALYSIS_NOT_PASSED")
    tools = report.get("tools")
    if not isinstance(tools, dict) or set(tools) != {"node", "luau_analyze", "jco"}:
        raise ValueError("ASTRA_UI_TOOLCHAIN_PREFLIGHT_TOOLS_INVALID")
    claimed_hash = report.get("report_hash")
    canonical_report = dict(report)
    canonical_report.pop("report_hash", None)
    actual_hash = sha256(
        json.dumps(canonical_report, sort_keys=True, separators=(",", ":")).encode("utf-8")
    )
    if claimed_hash != actual_hash:
        raise ValueError("ASTRA_UI_TOOLCHAIN_PREFLIGHT_HASH_INVALID")
    return {
        "lock_sha256": report["lock_sha256"],
        "target": report.get("target"),
        "tools": tools,
        "supply_chain": report.get("supply_chain"),
        "preflight_hash": actual_hash,
    }


def build_identity(root: pathlib.Path, cargo_args: Iterable[str]) -> dict[str, object]:
    git_head = run_output(["git", "rev-parse", "HEAD"], root).decode("utf-8").strip()
    git_diff = run_output(["git", "diff", "--binary", "HEAD"], root)
    rustc_version = run_output(["rustc", "-Vv"], root).decode("utf-8")
    checkout_digest = hashlib.sha256()
    checkout_digest.update(git_head.encode("utf-8"))
    checkout_digest.update(b"\0")
    checkout_digest.update(git_diff)
    for relative, payload in sorted(untracked_files(root), key=lambda item: item[0]):
        encoded = relative.encode("utf-8")
        checkout_digest.update(len(encoded).to_bytes(8, "big"))
        checkout_digest.update(encoded)
        checkout_digest.update(len(payload).to_bytes(8, "big"))
        checkout_digest.update(payload)
    identity: dict[str, object] = {
        "schema": SCHEMA,
        "checkout_id": git_head,
        "checkout_state_hash": "sha256:" + checkout_digest.hexdigest(),
        "workspace_manifest_hash": manifest_hash(root),
        "dependency_lock_hash": sha256((root / "Cargo.lock").read_bytes()),
        "toolchain_fingerprint": sha256(rustc_version.encode("utf-8")),
        "ui_toolchain": ui_toolchain_identity(root) or {"status": "not_configured"},
        "feature_fingerprint": sha256(
            json.dumps(
                feature_arguments(cargo_args),
                ensure_ascii=True,
                separators=(",", ":"),
            ).encode("utf-8")
        ),
        "artifact_path_role": "checkout_bound_cargo_target",
    }
    canonical = json.dumps(identity, sort_keys=True, separators=(",", ":")).encode("utf-8")
    identity["identity_hash"] = sha256(canonical)
    return identity


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", required=True, type=pathlib.Path)
    parser.add_argument("cargo_args", nargs=argparse.REMAINDER)
    arguments = parser.parse_args()
    cargo_args = arguments.cargo_args
    if cargo_args[:1] == ["--"]:
        cargo_args = cargo_args[1:]
    if not cargo_args:
        parser.error("Cargo arguments are required after --")
    root = pathlib.Path(__file__).resolve().parents[1]
    report = build_identity(root, cargo_args)
    arguments.output.parent.mkdir(parents=True, exist_ok=True)
    temporary = arguments.output.with_suffix(arguments.output.suffix + ".partial")
    temporary.write_text(
        json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    temporary.replace(arguments.output)
    print(json.dumps({"schema": SCHEMA, "identity_hash": report["identity_hash"]}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
