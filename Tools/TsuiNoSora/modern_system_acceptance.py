#!/usr/bin/env python3
"""Exercise the TsuiNoSora Modern system UI through physical Headless input."""

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


REPORT_SCHEMA = "tsuinosora.modern_system_acceptance_report.v1"
INPUT_SCHEMA = "astra.user_input_sequence.v1"


class AcceptanceError(RuntimeError):
    pass


def json_hash(value: object) -> str:
    encoded = json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
    return "sha256:" + hashlib.sha256(encoded).hexdigest()


class Sequence:
    def __init__(self, session: str) -> None:
        self.session = session
        self.rows: list[dict] = []
        self.tick = 0

    def add(self, event: dict, *, tick_advance: int = 1) -> None:
        self.rows.append(
            {
                "schema": INPUT_SCHEMA,
                "session": self.session,
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
    sequence = Sequence("tsui.modern.system.acceptance")
    sequence.add({"type": "resume"})
    sequence.add({"type": "focus", "focused": True})
    sequence.await_value("vn.system_page", "title", 7200)
    sequence.await_value("vn.focused_semantic_id", "root/gold/menu/continue")
    sequence.checkpoint("modern.title")
    sequence.key("Enter")
    sequence.await_value("vn.pending_wait_command", "tsui.command.014951", 7200)

    sequence.add({"type": "pointer_move", "x": 32768, "y": 32768})
    sequence.add({"type": "pointer_button", "button": "secondary", "state": "pressed"})
    sequence.add({"type": "pointer_button", "button": "secondary", "state": "released"})
    sequence.await_value("vn.system_page", "quick_panel")
    sequence.await_value("vn.focused_semantic_id", "root/gold/commands/skip_all")
    sequence.checkpoint("modern.quick_panel")

    sequence.key("Enter")
    sequence.await_value("vn.skip_mode", "all")

    sequence.key("Tab")
    sequence.await_value("vn.focused_semantic_id", "root/gold/commands/config")
    sequence.key("Enter")
    sequence.await_value("vn.system_page", "config")
    sequence.await_value("vn.focused_semantic_id", "root/gold/content/master")
    sequence.checkpoint("modern.config")
    sequence.key("ArrowLeft")
    sequence.await_value("vn.system_config", {"audio.master": "99"})
    for _ in range(4):
        sequence.key("Tab")
    sequence.await_value("vn.focused_semantic_id", "root/gold/content/locale")
    sequence.key("Enter")
    sequence.await_value("vn.locale", "en")
    sequence.checkpoint("modern.config.en")
    sequence.key("Enter")
    sequence.await_value("vn.locale", "zh-Hans")
    sequence.checkpoint("modern.config.zh-Hans")
    sequence.key("Enter")
    sequence.await_value("vn.locale", "ja")
    sequence.checkpoint("modern.config.ja")

    sequence.key("Escape")
    sequence.await_value("vn.system_page", "quick_panel")
    sequence.await_value("vn.focused_semantic_id", "root/gold/commands/skip_all")
    sequence.key("Tab")
    sequence.key("Tab")
    sequence.key("Tab")
    sequence.await_value("vn.focused_semantic_id", "root/gold/commands/save")
    sequence.key("Enter")
    sequence.await_value("vn.system_page", "save")
    sequence.checkpoint("modern.save")
    sequence.key("Enter")
    sequence.await_value("vn.occupied_save_slot_count", 1)

    sequence.key("Escape")
    sequence.await_value("vn.system_page", "quick_panel")
    sequence.await_value("vn.focused_semantic_id", "root/gold/commands/skip_all")
    sequence.key("Tab")
    sequence.key("Tab")
    sequence.key("Tab")
    sequence.key("Tab")
    sequence.await_value("vn.focused_semantic_id", "root/gold/commands/load")
    sequence.key("Enter")
    sequence.await_value("vn.system_page", "load")
    sequence.checkpoint("modern.load")
    sequence.key("Enter")
    sequence.await_value("vn.system_page", "save")
    sequence.await_value("vn.occupied_save_slot_count", 1)
    sequence.checkpoint("modern.load_restored")

    sequence.key("Escape")
    sequence.await_value("vn.system_page", "quick_panel")
    sequence.await_value("vn.focused_semantic_id", "root/gold/commands/skip_all")
    sequence.key("Tab")
    sequence.key("Tab")
    sequence.key("Tab")
    sequence.key("Tab")
    sequence.key("Tab")
    sequence.await_value("vn.focused_semantic_id", "root/gold/commands/backlog")
    sequence.key("Enter")
    sequence.await_value("vn.system_page", "backlog")
    sequence.await_value("vn.backlog_count", 1)
    sequence.checkpoint("modern.backlog")

    sequence.key("Escape")
    sequence.await_value("vn.system_page", "quick_panel")
    sequence.await_value("vn.focused_semantic_id", "root/gold/commands/skip_all")
    sequence.key("Tab")
    sequence.key("Tab")
    sequence.key("Tab")
    sequence.key("Tab")
    sequence.key("Tab")
    sequence.key("Tab")
    sequence.await_value("vn.focused_semantic_id", "root/gold/commands/auto")
    sequence.key("Enter")
    sequence.await_value("vn.auto_enabled", True)
    sequence.checkpoint("modern.auto_skip")
    sequence.add({"type": "shutdown"})
    return sequence


def run(arguments: argparse.Namespace) -> dict:
    profile = json.loads(arguments.profile.read_text(encoding="utf-8"))
    identity = json.loads(arguments.build_identity.read_text(encoding="utf-8"))
    if profile.get("product_profile") != "modern":
        raise AcceptanceError("Modern acceptance requires product_profile=modern")
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
    input_path = arguments.artifact_root / "modern-system-input.jsonl"
    input_path.write_text(
        "".join(json.dumps(row, ensure_ascii=False, separators=(",", ":")) + "\n" for row in sequence.rows),
        encoding="utf-8",
    )
    stdout_path = arguments.artifact_root / "runner.stdout.log"
    stderr_path = arguments.artifact_root / "runner.stderr.log"
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
    with stdout_path.open("wb") as stdout, stderr_path.open("wb") as stderr:
        completed = subprocess.run(command, stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr)
    if completed.returncode != 0:
        raise AcceptanceError(f"Headless Modern system run exited with {completed.returncode}")
    expected_checkpoints = [
        "modern.title",
        "modern.quick_panel",
        "modern.config",
        "modern.config.en",
        "modern.config.zh-Hans",
        "modern.config.ja",
        "modern.save",
        "modern.load",
        "modern.load_restored",
        "modern.backlog",
        "modern.auto_skip",
    ]
    run_report, manifest = validate_gpu_artifacts(
        arguments.artifact_root,
        build_fingerprint=profile["build_fingerprint"],
        package_hash=profile["package_hash"],
        completed_sequence=len(sequence.rows),
        checkpoint_ids=expected_checkpoints,
    )
    checkpoint_artifacts = [
        arguments.artifact_root / "checkpoints" / f"{checkpoint_id}.png"
        for checkpoint_id in expected_checkpoints
    ]
    missing_artifacts = [path.name for path in checkpoint_artifacts if not path.is_file()]
    if missing_artifacts:
        raise AcceptanceError(
            "Headless Modern system checkpoint images are missing: "
            + ", ".join(missing_artifacts)
        )
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
        "checkpoint_ids": expected_checkpoints,
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
