#!/usr/bin/env python3
"""Run Cargo in a checkout-bound artifact directory and record its identity."""

from __future__ import annotations

import hashlib
import json
import os
import pathlib
import subprocess
import sys
from typing import Iterable


SCHEMA = "astra.build_identity.v1"
REPORT_NAME = "astra-build-identity.json"


def _sha256(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def _manifest_hash(root: pathlib.Path) -> str:
    digest = hashlib.sha256()
    manifests: list[pathlib.Path] = []
    ignored_directories = {".git", ".tmp", ".worktrees", "worktrees", "target"}
    for directory, children, files in os.walk(root):
        children[:] = sorted(child for child in children if child not in ignored_directories)
        if "Cargo.toml" in files:
            manifests.append(pathlib.Path(directory) / "Cargo.toml")
    manifests.sort(key=lambda path: path.as_posix())
    for manifest in manifests:
        relative = manifest.relative_to(root).as_posix().encode("utf-8")
        digest.update(len(relative).to_bytes(8, "big"))
        digest.update(relative)
        payload = manifest.read_bytes()
        digest.update(len(payload).to_bytes(8, "big"))
        digest.update(payload)
    return "sha256:" + digest.hexdigest()


def _feature_arguments(cargo_args: Iterable[str]) -> list[str]:
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


def build_identity(
    *,
    root: pathlib.Path,
    git_head: str,
    git_diff: bytes,
    rustc_version: str,
    cargo_args: Iterable[str],
    untracked_files: Iterable[tuple[str, bytes]] = (),
) -> dict[str, object]:
    root = root.resolve()
    lock_path = root / "Cargo.lock"
    checkout_digest = hashlib.sha256()
    checkout_digest.update(git_head.strip().encode("utf-8"))
    checkout_digest.update(b"\0")
    checkout_digest.update(git_diff)
    for relative, payload in sorted(untracked_files, key=lambda item: item[0]):
        encoded = relative.encode("utf-8")
        checkout_digest.update(len(encoded).to_bytes(8, "big"))
        checkout_digest.update(encoded)
        checkout_digest.update(len(payload).to_bytes(8, "big"))
        checkout_digest.update(payload)
    identity: dict[str, object] = {
        "schema": SCHEMA,
        "checkout_id": git_head.strip(),
        "checkout_state_hash": "sha256:" + checkout_digest.hexdigest(),
        "workspace_manifest_hash": _manifest_hash(root),
        "dependency_lock_hash": _sha256(lock_path.read_bytes()),
        "toolchain_fingerprint": _sha256(rustc_version.encode("utf-8")),
        "feature_fingerprint": _sha256(
            json.dumps(
                _feature_arguments(cargo_args),
                ensure_ascii=True,
                separators=(",", ":"),
            ).encode("utf-8")
        ),
        "artifact_path_role": "checkout_bound_cargo_target",
    }
    canonical = json.dumps(identity, sort_keys=True, separators=(",", ":")).encode("utf-8")
    identity["identity_hash"] = _sha256(canonical)
    return identity


def target_directory(root: pathlib.Path, identity: dict[str, object]) -> pathlib.Path:
    identity_hash = str(identity["identity_hash"])
    if not identity_hash.startswith("sha256:") or len(identity_hash) != 71:
        raise ValueError("identity_hash must be a sha256 value")
    return root / "target" / "identity" / identity_hash.removeprefix("sha256:")[:16]


def collect_artifacts(target: pathlib.Path) -> list[dict[str, object]]:
    suffix_roles = {
        ".exe": "executable",
        ".dll": "dynamic_library",
        ".so": "dynamic_library",
        ".dylib": "dynamic_library",
    }
    artifacts: list[dict[str, object]] = []
    if not target.exists():
        return artifacts
    for profile in sorted(path for path in target.iterdir() if path.is_dir()):
        for artifact in sorted(path for path in profile.iterdir() if path.is_file()):
            role = suffix_roles.get(artifact.suffix.lower())
            if role is None:
                continue
            payload = artifact.read_bytes()
            artifacts.append(
                {
                    "path": artifact.relative_to(target).as_posix(),
                    "role": role,
                    "sha256": _sha256(payload),
                    "byte_size": len(payload),
                }
            )
    return artifacts


def validate_existing_identity(
    report_path: pathlib.Path, identity: dict[str, object]
) -> None:
    if not report_path.exists():
        return
    try:
        existing = json.loads(report_path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ValueError(f"ASTRA_BUILD_IDENTITY_INVALID: {error}") from error
    if existing.get("identity_hash") != identity.get("identity_hash"):
        raise ValueError(
            "ASTRA_BUILD_IDENTITY_MISMATCH: target directory belongs to another build identity"
        )


def ensure_identity_unchanged(
    before: dict[str, object], after: dict[str, object]
) -> None:
    if before.get("identity_hash") != after.get("identity_hash"):
        raise ValueError(
            "ASTRA_BUILD_INPUT_CHANGED: checkout or build inputs changed while Cargo was running"
        )


def _run_output(command: list[str], root: pathlib.Path) -> bytes:
    return subprocess.run(
        command,
        cwd=root,
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    ).stdout


def _untracked_files(root: pathlib.Path) -> list[tuple[str, bytes]]:
    raw = _run_output(
        ["git", "ls-files", "--others", "--exclude-standard", "-z"], root
    )
    files: list[tuple[str, bytes]] = []
    for encoded in raw.split(b"\0"):
        if not encoded:
            continue
        relative = encoded.decode("utf-8")
        path = root / relative
        if path.is_file():
            files.append((pathlib.PurePath(relative).as_posix(), path.read_bytes()))
    return files


def _current_identity(root: pathlib.Path, cargo_args: Iterable[str]) -> dict[str, object]:
    git_head = _run_output(["git", "rev-parse", "HEAD"], root).decode("utf-8").strip()
    git_diff = _run_output(["git", "diff", "--binary", "HEAD"], root)
    rustc_version = _run_output(["rustc", "-Vv"], root).decode("utf-8")
    return build_identity(
        root=root,
        git_head=git_head,
        git_diff=git_diff,
        rustc_version=rustc_version,
        cargo_args=cargo_args,
        untracked_files=_untracked_files(root),
    )


def _write_report(path: pathlib.Path, report: dict[str, object]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(".partial")
    temporary.write_text(
        json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    temporary.replace(path)


def main(argv: list[str]) -> int:
    if not argv:
        print("usage: python Tools/run_cargo_isolated.py <cargo arguments>", file=sys.stderr)
        return 2
    root = pathlib.Path(__file__).resolve().parents[1]
    try:
        identity = _current_identity(root, argv)
    except (OSError, subprocess.CalledProcessError, UnicodeDecodeError, ValueError) as error:
        print(f"ASTRA_BUILD_IDENTITY_FAILED: {error}", file=sys.stderr)
        return 2

    target = target_directory(root, identity)
    report_path = target / REPORT_NAME
    try:
        validate_existing_identity(report_path, identity)
    except ValueError as error:
        print(str(error), file=sys.stderr)
        return 2
    report = dict(identity)
    report["status"] = "running"
    report["artifacts"] = []
    _write_report(report_path, report)

    environment = os.environ.copy()
    environment["CARGO_TARGET_DIR"] = str(target)
    environment["ASTRA_BUILD_IDENTITY_HASH"] = str(identity["identity_hash"])
    result = subprocess.run(["cargo", *argv], cwd=root, env=environment, check=False)

    report["artifacts"] = collect_artifacts(target)
    report["cargo_exit_code"] = result.returncode
    try:
        ensure_identity_unchanged(identity, _current_identity(root, argv))
    except (OSError, subprocess.CalledProcessError, UnicodeDecodeError, ValueError) as error:
        report["status"] = "blocked"
        report["diagnostics"] = ["ASTRA_BUILD_INPUT_CHANGED"]
        _write_report(report_path, report)
        print(str(error), file=sys.stderr)
        return 2
    report["status"] = "pass" if result.returncode == 0 else "failed"
    _write_report(report_path, report)
    return result.returncode


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
