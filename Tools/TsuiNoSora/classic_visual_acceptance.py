#!/usr/bin/env python3
"""Capture TsuiNoSora Classic visual checkpoints through physical Headless input."""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
from pathlib import Path


REPORT_SCHEMA = "tsuinosora.classic_visual_acceptance_report.v1"
INPUT_SCHEMA = "astra.user_input_sequence.v1"
RUN_REPORT_SCHEMA = "astra.headless_run_report.v1"
CHECKPOINTS = [
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


def file_hash(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return "sha256:" + digest.hexdigest()


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
    sequence.await_value("vn.pending_wait_command", "tsui.command.014950", 7200)
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
    if profile.get("package_hash") != file_hash(arguments.package):
        raise AcceptanceError("profile and package do not match")
    if arguments.artifact_root.exists():
        raise AcceptanceError("artifact root already exists")
    arguments.artifact_root.mkdir(parents=True)

    sequence = build_sequence()
    input_path = arguments.artifact_root / "classic-visual-input.jsonl"
    input_path.write_text(
        "".join(json.dumps(row, ensure_ascii=False, separators=(",", ":")) + "\n" for row in sequence.rows),
        encoding="utf-8",
    )
    command = [
        str(arguments.binary),
        "run",
        "--profile",
        str(arguments.profile),
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

    run_report = json.loads((arguments.artifact_root / "run-report.json").read_text(encoding="utf-8"))
    if run_report.get("schema") != RUN_REPORT_SCHEMA or run_report.get("status") != "passed":
        raise AcceptanceError("Headless Classic visual run did not pass")
    if run_report.get("completed_sequence") != len(sequence.rows):
        raise AcceptanceError("Headless Classic visual run did not consume every input")
    checkpoints = run_report.get("checkpoint_results")
    if not isinstance(checkpoints, list) or [item.get("id") for item in checkpoints] != CHECKPOINTS:
        raise AcceptanceError("Headless Classic visual run has an invalid checkpoint set")
    if not all(item.get("passed") is True for item in checkpoints):
        raise AcceptanceError("Headless Classic visual checkpoint failed")
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
        "profile_id": profile.get("id"),
        "build_fingerprint": profile.get("build_fingerprint"),
        "package_hash": profile.get("package_hash"),
        "input_sequence_hash": file_hash(input_path),
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
    except (AcceptanceError, OSError, json.JSONDecodeError) as error:
        parser.error(str(error))
    arguments.report.parent.mkdir(parents=True, exist_ok=True)
    arguments.report.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({"schema": REPORT_SCHEMA, "status": "passed"}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
