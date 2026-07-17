#!/usr/bin/env python3
"""Differentially verify the sanitized FVP VM trace against pinned rfvp."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import shutil
import subprocess
import sys
import uuid


REVISION = "3b5ea6c96a925c12f95aef8554905e8fecbc77c3"
UPSTREAM = "https://github.com/xmoezzz/rfvp.git"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--reference", type=pathlib.Path)
    parser.add_argument("--refresh-golden", action="store_true")
    parser.add_argument("--evidence-output", type=pathlib.Path)
    args = parser.parse_args()

    root = pathlib.Path(__file__).resolve().parents[1]
    reference = (args.reference or root / ".tmp" / "rfvp-reference").resolve()
    golden = (
        root
        / "Emulator"
        / "Source"
        / "Families"
        / "astra-emu-fvp"
        / "tests"
        / "golden"
        / "rfvp-0.5.0-vm-trace.json"
    )
    trace_source = (
        root
        / "Emulator"
        / "Source"
        / "Families"
        / "astra-emu-fvp-rfvp-core"
        / "src"
        / "bin"
        / "astra_fvp_parity_trace.rs"
    )
    require_file(trace_source, "ASTRA_EMU_FVP_PARITY_TRACE_SOURCE_MISSING")
    prepare_reference(reference)

    worktree_root = root / ".tmp" / "fvp-parity-worktrees"
    worktree_root.mkdir(parents=True, exist_ok=True)
    worktree = worktree_root / uuid.uuid4().hex
    run(["git", "-C", str(reference), "worktree", "add", "--detach", str(worktree), REVISION])
    try:
        reference_bin = worktree / "crates" / "rfvp" / "src" / "bin"
        reference_bin.mkdir(parents=True, exist_ok=True)
        shutil.copy2(trace_source, reference_bin / "astra_fvp_parity_trace.rs")

        target_root = root / ".tmp" / "fvp-parity-target"
        derivative = run_trace(
            [
                "cargo",
                "run",
                "--quiet",
                "-p",
                "astra-emu-fvp-rfvp-core",
                "--bin",
                "astra-fvp-parity-trace",
            ],
            root,
            target_root / "derivative",
        )
        upstream = run_trace(
            [
                "cargo",
                "run",
                "--quiet",
                "--locked",
                "-p",
                "rfvp",
                "--bin",
                "astra_fvp_parity_trace",
                "--no-default-features",
                "--features",
                "native-video,zlib-flate2",
            ],
            worktree,
            target_root / "reference",
        )
    finally:
        run(
            ["git", "-C", str(reference), "worktree", "remove", "--force", str(worktree)],
            check=False,
        )

    if canonical(derivative) != canonical(upstream):
        fail("ASTRA_EMU_FVP_PARITY_DIVERGENCE")
    if args.refresh_golden:
        write_atomic(golden, canonical(upstream) + b"\n")
    require_file(golden, "ASTRA_EMU_FVP_PARITY_GOLDEN_MISSING")
    expected = parse_json(golden.read_text(encoding="utf-8"), "ASTRA_EMU_FVP_PARITY_GOLDEN_INVALID")
    if canonical(expected) != canonical(upstream):
        fail("ASTRA_EMU_FVP_PARITY_GOLDEN_DRIFT")

    digest = hashlib.sha256(canonical(upstream)).hexdigest()
    fixture_digest = sha256_file(trace_source)
    if args.evidence_output:
        evidence_path = (root / args.evidence_output).resolve()
        ensure_descendant(evidence_path, root)
        evidence = {
            "schema": "astra.frame_parity_report.v1",
            "reference_revision": REVISION,
            "reference_observer_patch_hash": f"sha256.{fixture_digest}",
            "build_identity": "local-private",
            "profile": "synthetic.vm",
            "game_identity_hash": f"sha256.{fixture_digest}",
            "input_sequence_hash": f"sha256.{hashlib.sha256(b'no-input').hexdigest()}",
            "fixture_id": "fvp.synthetic.vm.reference.v1",
            "fixture_hash": f"sha256.{fixture_digest}",
            "astra_trace_hash": f"sha256.{digest}",
            "reference_trace_hash": f"sha256.{digest}",
            "compared_event_count": count_leaves(upstream),
            "first_divergence_sequence": None,
            "frames": [{
                "frame_index": 0,
                "semantic_astra_hash": f"sha256.{digest}",
                "semantic_reference_hash": f"sha256.{digest}",
                "rgba_astra_hash": None,
                "rgba_reference_hash": None,
                "audio_astra_hash": None,
                "audio_reference_hash": None,
                "video_pts": None,
            }],
            "difference_window_before": 30,
            "difference_window_after": 60,
            "status": "pass",
            "diagnostic_codes": [],
        }
        write_new_atomic(evidence_path, canonical(evidence) + b"\n")
    print(
        json.dumps(
            {
                "schema": "astra.emu.fvp.differential_result.v1",
                "status": "PASS",
                "rfvp_revision": REVISION,
                "trace_sha256": f"sha256.{digest}",
                "opcode_count": len(upstream["opcode_table"]),
                "commercial_payload": "omitted",
            },
            sort_keys=True,
        )
    )
    return 0


def prepare_reference(path: pathlib.Path) -> None:
    if not path.exists():
        path.parent.mkdir(parents=True, exist_ok=True)
        run(["git", "clone", "--filter=blob:none", "--no-checkout", UPSTREAM, str(path)])
    if not (path / ".git").exists():
        fail("ASTRA_EMU_FVP_REFERENCE_NOT_GIT")
    remote = capture(["git", "-C", str(path), "remote", "get-url", "origin"]).strip()
    if normalize_remote(remote) != normalize_remote(UPSTREAM):
        fail("ASTRA_EMU_FVP_REFERENCE_REMOTE_MISMATCH")
    if subprocess.run(
        ["git", "-C", str(path), "cat-file", "-e", f"{REVISION}^{{commit}}"],
        check=False,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    ).returncode != 0:
        run(["git", "-C", str(path), "fetch", "--tags", "origin"])
    observed = capture(["git", "-C", str(path), "rev-parse", f"{REVISION}^{{commit}}"])
    if observed.strip() != REVISION:
        fail("ASTRA_EMU_FVP_REFERENCE_REVISION_MISSING")


def run_trace(command: list[str], cwd: pathlib.Path, target: pathlib.Path) -> object:
    target.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env["CARGO_TARGET_DIR"] = str(target)
    result = subprocess.run(
        command,
        cwd=cwd,
        env=env,
        text=True,
        encoding="utf-8",
        errors="replace",
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        sys.stderr.write(result.stderr)
        fail("ASTRA_EMU_FVP_PARITY_TRACE_EXECUTION_FAILED")
    return parse_json(result.stdout, "ASTRA_EMU_FVP_PARITY_TRACE_INVALID")


def parse_json(value: str, code: str) -> object:
    try:
        parsed = json.loads(value)
    except json.JSONDecodeError:
        fail(code)
    if not isinstance(parsed, dict) or parsed.get("schema") != "astra.emu.fvp.reference_trace.v1":
        fail(code)
    return parsed


def canonical(value: object) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def normalize_remote(value: str) -> str:
    return value.removesuffix(".git").rstrip("/").lower()


def require_file(path: pathlib.Path, code: str) -> None:
    if not path.is_file() or path.stat().st_size == 0:
        fail(code)


def ensure_descendant(path: pathlib.Path, parent: pathlib.Path) -> None:
    try:
        path.relative_to(parent.resolve())
    except ValueError:
        fail("ASTRA_EMU_FVP_PARITY_OUTPUT_OUTSIDE_WORKSPACE")


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def count_leaves(value: object) -> int:
    if isinstance(value, dict):
        return sum(count_leaves(item) for key, item in value.items() if key != "schema")
    if isinstance(value, list):
        return sum(count_leaves(item) for item in value)
    return 1


def write_atomic(path: pathlib.Path, content: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp")
    if temporary.exists():
        fail("ASTRA_EMU_FVP_PARITY_GOLDEN_TEMP_EXISTS")
    with temporary.open("xb") as stream:
        stream.write(content)
        stream.flush()
        os.fsync(stream.fileno())
    temporary.replace(path)


def write_new_atomic(path: pathlib.Path, content: bytes) -> None:
    if path.exists():
        fail("ASTRA_EMU_FVP_PARITY_OUTPUT_EXISTS")
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp")
    if temporary.exists():
        fail("ASTRA_EMU_FVP_PARITY_OUTPUT_TEMP_EXISTS")
    with temporary.open("xb") as stream:
        stream.write(content)
        stream.flush()
        os.fsync(stream.fileno())
    temporary.replace(path)


def capture(command: list[str]) -> str:
    result = subprocess.run(
        command,
        text=True,
        encoding="utf-8",
        errors="replace",
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        fail("ASTRA_EMU_FVP_PARITY_GIT_FAILED")
    return result.stdout


def run(command: list[str], check: bool = True) -> None:
    result = subprocess.run(command, check=False)
    if check and result.returncode != 0:
        fail("ASTRA_EMU_FVP_PARITY_EXTERNAL_COMMAND_FAILED")


def fail(code: str) -> None:
    raise SystemExit(code)


if __name__ == "__main__":
    sys.exit(main())
