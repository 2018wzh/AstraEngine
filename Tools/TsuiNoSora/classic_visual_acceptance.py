#!/usr/bin/env python3
"""Capture TsuiNoSora Classic visual checkpoints through physical Headless input."""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
from pathlib import Path

from headless_gpu_acceptance import (
    GpuAcceptanceError,
    file_hash,
    prepare_gpu_profile,
    validate_gpu_artifacts,
)


REPORT_SCHEMA = "tsuinosora.classic_visual_acceptance_report.v1"
INPUT_SCHEMA = "astra.user_input_sequence.v1"
CHECKPOINTS = [
    "classic.title",
    "classic.message",
    "classic.save",
    "classic.load",
    "classic.load_restored",
]


class AcceptanceError(RuntimeError):
    pass


def json_hash(value: object) -> str:
    encoded = json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
    return "sha256:" + hashlib.sha256(encoded).hexdigest()


class Sequence:
    def __init__(self) -> None:
        self.rows: list[dict] = []
        self.tick = 0

    def add(self, event: dict, *, tick_advance: int = 1) -> None:
        self.rows.append(
            {
                "schema": INPUT_SCHEMA,
                "session": "tsui.classic.visual.acceptance",
                "sequence": len(self.rows) + 1,
                "tick": self.tick,
                "event": event,
            }
        )
        self.tick += tick_advance

    def key(self, key: str) -> None:
        for state in ("pressed", "released"):
            self.add(
                {
                    "type": "keyboard",
                    "physical_key": key,
                    "logical_key": key,
                    "state": state,
                    "repeat": False,
                }
            )

    def await_value(self, key: str, value: object, timeout: int = 3600) -> None:
        self.add(
            {
                "type": "await",
                "observation": {"kind": "equals", "key": key, "value_hash": json_hash(value)},
                "timeout_ticks": timeout,
                "continue_at_match": True,
            },
            tick_advance=timeout,
        )

    def checkpoint(self, checkpoint_id: str) -> None:
        self.add({"type": "checkpoint", "id": checkpoint_id})


def build_sequence() -> Sequence:
    sequence = Sequence()
    sequence.add({"type": "resume"})
    sequence.add({"type": "focus", "focused": True})
    sequence.await_value("vn.system_page", "title", 7200)
    sequence.await_value("vn.focused_semantic_id", "root/start")
    sequence.checkpoint("classic.title")
    sequence.key("Enter")
    sequence.await_value("vn.pending_wait_command", "tsui.command.014951", 7200)
    sequence.checkpoint("classic.message")

    sequence.add({"type": "pointer_move", "x": 32768, "y": 32768})
    sequence.add({"type": "pointer_button", "button": "secondary", "state": "pressed"})
    sequence.add({"type": "pointer_button", "button": "secondary", "state": "released"})
    sequence.await_value("vn.system_page", "save")
    sequence.await_value(
        "vn.focused_semantic_id",
        "root/gold/paper/slots/slot/slot.01/write/slot.01",
    )
    sequence.checkpoint("classic.save")
    sequence.key("Enter")
    sequence.await_value("vn.occupied_save_slot_count", 1)

    sequence.key("ArrowUp")
    sequence.await_value("vn.focused_semantic_id", "root/gold/paper/header/mode_load")
    sequence.key("Enter")
    sequence.await_value("vn.system_page", "load")
    sequence.await_value(
        "vn.focused_semantic_id",
        "root/gold/paper/slots/slot/slot.01/read/slot.01",
    )
    sequence.checkpoint("classic.load")
    sequence.key("Enter")
    sequence.await_value("vn.system_page", "save")
    sequence.await_value("vn.occupied_save_slot_count", 1)
    sequence.checkpoint("classic.load_restored")
    sequence.add({"type": "shutdown"})
    return sequence


def run(arguments: argparse.Namespace) -> dict:
    profile = json.loads(arguments.profile.read_text(encoding="utf-8"))
    identity = json.loads(arguments.build_identity.read_text(encoding="utf-8"))
    if profile.get("product_profile") != "classic":
        raise AcceptanceError("Classic acceptance requires product_profile=classic")
    if profile.get("build_fingerprint") != identity.get("identity_hash"):
        raise AcceptanceError("profile and build identity do not match")
    if not str(profile.get("package_hash", "")).startswith("sha256:"):
        raise AcceptanceError("profile package identity is invalid")
    if arguments.artifact_root.exists():
        raise AcceptanceError("artifact root already exists")
    arguments.artifact_root.mkdir(parents=True)
    gpu_profile_path = arguments.artifact_root / "headless-gpu-profile.json"
    gpu_profile = prepare_gpu_profile(arguments.profile, gpu_profile_path)

    sequence = build_sequence()
    input_path = arguments.artifact_root / "classic-visual-input.jsonl"
    input_path.write_text(
        "".join(json.dumps(row, ensure_ascii=False, separators=(",", ":")) + "\n" for row in sequence.rows),
        encoding="utf-8",
    )
    command = [
        str(arguments.binary),
        "run",
        "--gpu",
        "--profile",
        str(gpu_profile_path),
        "--package",
        str(arguments.package),
        "--input",
        str(input_path),
        "--artifact-root",
        str(arguments.artifact_root),
        "--build-identity",
        str(arguments.build_identity),
    ]
    with (arguments.artifact_root / "runner.stdout.log").open("wb") as stdout, (
        arguments.artifact_root / "runner.stderr.log"
    ).open("wb") as stderr:
        completed = subprocess.run(command, stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr)
    if completed.returncode != 0:
        raise AcceptanceError(f"Headless Classic visual run exited with {completed.returncode}")

    run_report, manifest = validate_gpu_artifacts(
        arguments.artifact_root,
        build_fingerprint=profile["build_fingerprint"],
        package_hash=profile["package_hash"],
        completed_sequence=len(sequence.rows),
        checkpoint_ids=CHECKPOINTS,
    )
    checkpoint_artifacts = [
        arguments.artifact_root / "checkpoints" / f"{checkpoint_id}.png"
        for checkpoint_id in CHECKPOINTS
    ]
    missing = [path.name for path in checkpoint_artifacts if not path.is_file()]
    if missing:
        raise AcceptanceError("Headless Classic checkpoint images are missing: " + ", ".join(missing))
    return {
        "schema": REPORT_SCHEMA,
        "status": "passed",
        "profile_id": gpu_profile.get("id"),
        "build_fingerprint": profile.get("build_fingerprint"),
        "package_hash": profile.get("package_hash"),
        "headless_gpu_profile_hash": file_hash(gpu_profile_path),
        "renderer_identity_hash": manifest.get("renderer_identity_hash"),
        "renderer_identity": manifest.get("renderer_identity"),
        "input_sequence_hash": run_report.get("input_sequence_hash"),
        "input_file_hash": file_hash(input_path),
        "input_message_count": len(sequence.rows),
        "checkpoint_ids": CHECKPOINTS,
        "checkpoint_artifact_hashes": [file_hash(path) for path in checkpoint_artifacts],
        "run_report_hash": file_hash(arguments.artifact_root / "run-report.json"),
        "diagnostics": [],
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--profile", required=True, type=Path)
    parser.add_argument("--package", required=True, type=Path)
    parser.add_argument("--build-identity", required=True, type=Path)
    parser.add_argument("--artifact-root", required=True, type=Path)
    parser.add_argument("--report", required=True, type=Path)
    arguments = parser.parse_args()
    try:
        report = run(arguments)
    except (AcceptanceError, GpuAcceptanceError, OSError, json.JSONDecodeError) as error:
        parser.error(str(error))
    arguments.report.parent.mkdir(parents=True, exist_ok=True)
    arguments.report.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({"schema": REPORT_SCHEMA, "status": "passed"}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
