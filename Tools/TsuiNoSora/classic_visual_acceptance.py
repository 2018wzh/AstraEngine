#!/usr/bin/env python3
"""Capture TsuiNoSora Classic checkpoints through physical GPU Headless input."""

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


REPORT_SCHEMA = "tsuinosora.classic_visual_acceptance_report.v2"
INPUT_SCHEMA = "astra.user_input_sequence.v1"
NODE_MAP_SCHEMA = "tsuinosora.classic_visual_node_map.v3"


class AcceptanceError(RuntimeError):
    pass


CLASSIC_CONFIG_FAST_FORWARD_POINT = (490, 265)


class StoryIndex:
    def __init__(self, story_ir: dict) -> None:
        if story_ir.get("schema") != "tsuinosora.native_story_ir.v1":
            raise AcceptanceError("Classic acceptance story IR schema is invalid")
        stories = story_ir.get("stories")
        if not isinstance(stories, list) or len(stories) != 1:
            raise AcceptanceError("Classic acceptance requires exactly one private story")
        self.states = {}
        self.commands = {}
        for state in stories[0]["states"]:
            commands = state["scenes"][0]["commands"]
            self.states[state["state_id"]] = commands
            for command in commands:
                command_id = command["command_id"]
                if command_id in self.commands:
                    raise AcceptanceError("Classic acceptance command identity is duplicated")
                self.commands[command_id] = (state["state_id"], command)

    def command(self, state_id: str, kind: str, ordinal: int = 0) -> str:
        commands = [command for command in self.states.get(state_id, []) if command.get("kind") == kind]
        if ordinal < 0 or ordinal >= len(commands):
            raise AcceptanceError(
                f"Classic checkpoint command identity is missing: {state_id}/{kind}/{ordinal}"
            )
        return commands[ordinal]["command_id"]

    @staticmethod
    def _content(command: dict) -> str | None:
        if command.get("kind") == "text":
            return command.get("text")
        if command.get("kind") == "choice":
            options = command.get("options")
            if isinstance(options, list):
                return "\0".join(str(option.get("text", "")) for option in options)
        return None

    def validate_text_locators(self, node_map: dict) -> str:
        if node_map.get("schema") != NODE_MAP_SCHEMA:
            raise AcceptanceError("Classic visual node-map schema is invalid")
        evidence = []
        for entry in node_map.get("entries", []):
            if entry.get("comparison_class") != "same_node":
                continue
            identity = entry.get("identity")
            locator = identity.get("locator") if isinstance(identity, dict) else None
            if not isinstance(locator, dict):
                raise AcceptanceError("Classic visual node text locator is missing")
            method = locator.get("method")
            content_hash = locator.get("content_sha256")
            if method in {"story_text", "story_choice"}:
                expected_kind = "text" if method == "story_text" else "choice"
                matches = []
                for command_id, (state_id, command) in self.commands.items():
                    content = self._content(command)
                    if content is None:
                        continue
                    actual_hash = "sha256:" + hashlib.sha256(content.encode()).hexdigest()
                    if actual_hash == content_hash:
                        if command.get("kind") != expected_kind:
                            raise AcceptanceError("Classic visual locator matched the wrong command kind")
                        matches.append((command_id, state_id))
                candidate_commands = locator.get("candidate_commands")
                if not isinstance(candidate_commands, list) or sorted(candidate_commands) != sorted(
                    command_id for command_id, _ in matches
                ):
                    raise AcceptanceError("Classic visual text locator candidate closure is incomplete")
                selected = self.commands.get(identity.get("wait_command"))
                if selected is None or selected[0] != identity.get("typed_state"):
                    raise AcceptanceError("Classic visual text locator selected the wrong typed state")
            elif method == "score_bitmap_text":
                asset_id = locator.get("asset_id")
                selected_commands = self.states.get(identity.get("typed_state"), [])
                if not isinstance(asset_id, str) or not any(
                    command.get("asset_id") == asset_id for command in selected_commands
                ):
                    raise AcceptanceError("Classic score-bitmap text locator asset is not bound")
            elif method not in {"system_resource", "resource_sequence"}:
                raise AcceptanceError("Classic visual text locator method is unsupported")
            evidence.append(
                {
                    "reference_id": entry.get("reference_id"),
                    "typed_state": identity.get("typed_state"),
                    "wait_command": identity.get("wait_command"),
                    "locator": locator,
                }
            )
        return json_hash(evidence)


def json_hash(value: object) -> str:
    encoded = json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
    return "sha256:" + hashlib.sha256(encoded).hexdigest()


class Sequence:
    def __init__(self, session: str) -> None:
        self.session = session
        self.rows: list[dict] = []
        self.tick = 0
        self.checkpoints: list[str] = []
        self.checkpoint_nodes: dict[str, dict] = {}
        self.stability_probe = 0

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

    def key(self, key: str, count: int = 1) -> None:
        for _ in range(count):
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

    def pointer_click(self, x: int, y: int) -> None:
        if not (0 <= x < 800 and 0 <= y < 600):
            raise AcceptanceError("pointer checkpoint coordinate is outside the 800x600 stage")
        self.add(
            {
                "type": "pointer_move",
                "x": round(x * 65535 / 799),
                "y": round(y * 65535 / 599),
            }
        )
        for state in ("pressed", "released"):
            self.add({"type": "pointer_button", "button": "primary", "state": state})

    def pointer_move(self, x: int, y: int) -> None:
        if not (0 <= x < 800 and 0 <= y < 600):
            raise AcceptanceError("pointer checkpoint coordinate is outside the 800x600 stage")
        self.add(
            {
                "type": "pointer_move",
                "x": round(x * 65535 / 799),
                "y": round(y * 65535 / 599),
            }
        )

    def secondary(self) -> None:
        for state in ("pressed", "released"):
            self.add({"type": "pointer_button", "button": "secondary", "state": state})

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
        if checkpoint_id in self.checkpoints:
            raise AcceptanceError(f"duplicate checkpoint id {checkpoint_id}")
        self.checkpoints.append(checkpoint_id)
        self.add({"type": "checkpoint", "id": checkpoint_id})

    def node_checkpoint(
        self,
        checkpoint_id: str,
        *,
        reference_id: str,
        typed_state: str,
        wait_command: str,
    ) -> None:
        if checkpoint_id in self.checkpoint_nodes:
            raise AcceptanceError(f"duplicate node checkpoint id {checkpoint_id}")
        evidence = {
            "reference_id": reference_id,
            "typed_state": typed_state,
            "wait_command": wait_command,
        }
        self.checkpoint_nodes[checkpoint_id] = evidence
        self.checkpoint(checkpoint_id)
        self.stability_probe ^= 1
        self.pointer_move(799 - self.stability_probe, 599)
        stable_id = f"{checkpoint_id}.__stable"
        self.checkpoint_nodes[stable_id] = evidence
        self.checkpoint(stable_id)

    def start(self) -> None:
        self.add({"type": "resume"})
        self.add({"type": "focus", "focused": True})

    def finish(self) -> None:
        self.add({"type": "shutdown"})


def await_title(sequence: Sequence) -> None:
    sequence.await_value("vn.system_page", "title", 7200)
    sequence.await_value("vn.focused_semantic_id", "root/start")


def open_popup(sequence: Sequence) -> None:
    sequence.secondary()
    sequence.await_value("vn.system_page", "quick_panel")
    sequence.await_value("vn.focused_semantic_id", "root/window/tabs/save")


def switch_popup_to_config(sequence: Sequence) -> None:
    sequence.key("ArrowLeft")
    sequence.await_value("vn.focused_semantic_id", "root/window/tabs/config")
    sequence.key("Enter")
    sequence.await_value("vn.system_page", "config")
    sequence.await_value("vn.focused_semantic_id", "root/window/body/reading_modes/hidden")


def choose_classic_option(sequence: Sequence, option_index: int, next_focus_index: int) -> None:
    """Choose by the authored fixed rectangle and wait for the next UI occurrence.

    A selector may return to the same command id with a different enabled set.  The neutral
    pointer move forces that new occurrence to finish its controller/layout lifecycle before
    the next physical choice is sent; focus is evidence only and never selects the option.
    """
    if not (1 <= option_index <= 5 and 1 <= next_focus_index <= 5):
        raise AcceptanceError("Classic selector option indices must be between 1 and 5")
    # The fixed Classic choice geometry rasterizes option centers at 134 + 43*n.
    # Keep this synchronized with the semantic/hit rectangle asserted by the v2 visual gate.
    sequence.pointer_click(244, 134 + (option_index - 1) * 43)
    sequence.pointer_move(799, 599)
    sequence.await_value(
        "vn.focused_semantic_id",
        f"root/choices/options/option/choice.y.0105.{next_focus_index}",
    )


def build_system_sequence(story: StoryIndex) -> Sequence:
    sequence = Sequence("tsui.classic.visual.system")
    sequence.start()
    await_title(sequence)
    title_wait = story.command("tsui.init", "system_page")
    sequence.node_checkpoint("classic.title", reference_id="TSUI1999-UI-001", typed_state="tsui.init", wait_command=title_wait)
    sequence.key("ArrowDown")
    sequence.await_value("vn.focused_semantic_id", "root/load")
    sequence.key("Enter")
    sequence.await_value("vn.system_page", "load")
    sequence.node_checkpoint("classic.title_load", reference_id="TSUI1999-UI-010", typed_state="tsui.init", wait_command=title_wait)
    sequence.key("Escape")
    sequence.await_value("vn.system_page", "title")
    sequence.await_value("vn.focused_semantic_id", "root/start")
    sequence.key("ArrowDown", 2)
    sequence.await_value("vn.focused_semantic_id", "root/exit")
    sequence.checkpoint("classic.exit_focused")
    sequence.key("ArrowUp", 2)
    sequence.await_value("vn.focused_semantic_id", "root/start")
    sequence.key("Enter")
    opening_staggered = story.command("director.y.0010.score.0013", "timeline")
    sequence.await_value("vn.pending_wait_command", opening_staggered, 7200)
    sequence.node_checkpoint(
        "classic.opening.staggered",
        reference_id="TSUI1999-UI-002",
        typed_state="director.y.0010.score.0013",
        wait_command=opening_staggered,
    )
    opening_viewpoint = story.command("director.y.0010.score.0015", "timeline")
    sequence.await_value("vn.pending_wait_command", opening_viewpoint, 7200)
    sequence.node_checkpoint(
        "classic.opening.viewpoint",
        reference_id="TSUI1999-EVIDENCE-Y-VIEWPOINT",
        typed_state="director.y.0010.score.0015",
        wait_command=opening_viewpoint,
    )
    opening_centered = story.command("director.y.0020", "wait")
    sequence.await_value("vn.pending_wait_command", opening_centered, 7200)
    sequence.node_checkpoint(
        "classic.opening.centered",
        reference_id="TSUI1999-UI-003",
        typed_state="director.y.0020",
        wait_command=opening_centered,
    )
    first_dialogue = story.command("director.y.0026", "text", 0)
    sequence.await_value("vn.pending_wait_command", first_dialogue, 7200)
    sequence.node_checkpoint(
        "classic.dialogue.first",
        reference_id="TSUI1999-EVIDENCE-Y-DIALOGUE-FIRST",
        typed_state="director.y.0026",
        wait_command=first_dialogue,
    )
    sequence.key("Enter", 2)
    background_dialogue = story.command("director.y.0026", "text", 2)
    sequence.await_value("vn.pending_wait_command", background_dialogue)
    sequence.node_checkpoint(
        "classic.dialogue.background_only",
        reference_id="TSUI1999-UI-005",
        typed_state="director.y.0026",
        wait_command=background_dialogue,
    )
    sequence.key("Enter", 2)
    next_dialogue = story.command("director.y.0026", "text", 4)
    sequence.await_value("vn.pending_wait_command", next_dialogue)
    sequence.node_checkpoint(
        "classic.dialogue.background_only.next",
        reference_id="TSUI1999-EVIDENCE-Y-DIALOGUE-NEXT",
        typed_state="director.y.0026",
        wait_command=next_dialogue,
    )
    sequence.key("Enter", 26)
    character_dialogue = story.command("director.y.0026", "text", 30)
    sequence.await_value("vn.pending_wait_command", character_dialogue)
    sequence.node_checkpoint("classic.dialogue.character_overflow", reference_id="TSUI1999-UI-006", typed_state="director.y.0026", wait_command=character_dialogue)

    open_popup(sequence)
    sequence.node_checkpoint("classic.popup", reference_id="TSUI1999-UI-011", typed_state="director.y.0026", wait_command=character_dialogue)
    sequence.key("Enter")
    sequence.await_value("vn.system_page", "save")
    sequence.await_value("vn.focused_semantic_id", "root/window/body/slots/slot/slot.01")
    sequence.node_checkpoint("classic.save", reference_id="TSUI1999-UI-014", typed_state="director.y.0026", wait_command=character_dialogue)
    sequence.key("Enter")
    sequence.await_value("vn.occupied_save_slot_count", 1)
    sequence.await_value("vn.pending_wait_command", character_dialogue)

    open_popup(sequence)
    sequence.key("ArrowRight")
    sequence.await_value("vn.focused_semantic_id", "root/window/tabs/load")
    sequence.key("Enter")
    sequence.await_value("vn.system_page", "load")
    sequence.await_value("vn.focused_semantic_id", "root/window/body/slots/slot/slot.01")
    sequence.node_checkpoint("classic.load", reference_id="TSUI1999-UI-013", typed_state="director.y.0026", wait_command=character_dialogue)
    sequence.key("Enter")
    sequence.await_value("vn.system_page", "save")
    sequence.await_value("vn.occupied_save_slot_count", 1)
    sequence.checkpoint("classic.load_restored")
    sequence.key("Escape")
    sequence.await_value("vn.pending_wait_command", character_dialogue)

    open_popup(sequence)
    switch_popup_to_config(sequence)
    sequence.node_checkpoint("classic.config", reference_id="TSUI1999-UI-012", typed_state="director.y.0026", wait_command=character_dialogue)
    sequence.finish()
    return sequence


def open_hidden_test(sequence: Sequence) -> None:
    # The original hidden entry is deliberately restricted to the Title's 64x64 hotspot.
    sequence.pointer_click(32, 32)
    sequence.await_value("vn.system_page", "custom")
    sequence.await_value("vn.focused_semantic_id", "root/window/body/flags/mode_ayana")


def build_k_sequence(story: StoryIndex) -> Sequence:
    sequence = Sequence("tsui.classic.visual.k")
    sequence.start()
    await_title(sequence)
    open_hidden_test(sequence)
    sequence.checkpoint("classic.test_menu")
    sequence.key("Tab", 12)
    sequence.await_value("vn.focused_semantic_id", "root/window/body/routes/k/k12")
    sequence.key("Enter")
    sequence.await_value("vn.pending_wait_command", story.command("director.k.0015", "wait"), 7200)
    sequence.await_value(
        "vn.pending_wait_command", story.command("director.k.0021", "text", 0), 7200
    )
    sequence.key("Enter", 12)
    legacy_dialogue = story.command("director.k.0027", "text", 0)
    sequence.await_value("vn.pending_wait_command", legacy_dialogue, 7200)
    sequence.node_checkpoint("classic.dialogue.legacy_game", reference_id="TSUI1999-UI-015", typed_state="director.k.0027", wait_command=legacy_dialogue)
    sequence.key("Enter", 5)
    stage_wait = story.command("director.k.0033", "wait")
    sequence.await_value("vn.pending_wait_command", stage_wait, 7200)
    sequence.node_checkpoint("classic.stage.opening_sphere", reference_id="TSUI1999-UI-004", typed_state="director.k.0033", wait_command=stage_wait)
    sequence.finish()
    return sequence


def build_two_character_sequence(story: StoryIndex) -> Sequence:
    sequence = Sequence("tsui.classic.visual.two_character")
    sequence.start()
    await_title(sequence)
    # The same-node Y evidence must enter through the authored movie lifecycle.
    # A direct test-menu label jump bypasses the converted startMovie/tinit Score
    # snapshot and therefore cannot prove the natural first-route presentation.
    sequence.key("Enter")
    sequence.await_value("vn.pending_wait_command", story.command("director.y.0020", "wait"), 7200)
    sequence.await_value("vn.pending_wait_command", story.command("director.y.0026", "text", 0), 7200)

    sequence.key("Enter", 30)
    pre_choice = story.command("director.y.0026", "text", 30)
    sequence.await_value("vn.pending_wait_command", pre_choice, 14400)
    sequence.node_checkpoint(
        "classic.choice.predecessor",
        reference_id="TSUI1999-EVIDENCE-Y-CHOICE-PREDECESSOR",
        typed_state="director.y.0026",
        wait_command=pre_choice,
    )
    sequence.key("Enter", 15)
    choice_wait = story.command("director.y.0032.choice", "choice")
    sequence.await_value("vn.pending_wait_command", choice_wait, 14400)
    sequence.node_checkpoint("classic.choice", reference_id="TSUI1999-UI-009", typed_state="director.y.0032.choice", wait_command=choice_wait)
    sequence.key("Enter")
    post_choice = story.command("director.y.0038", "text", 0)
    sequence.await_value("vn.pending_wait_command", post_choice, 14400)
    sequence.node_checkpoint(
        "classic.choice.successor",
        reference_id="TSUI1999-EVIDENCE-Y-CHOICE-SUCCESSOR",
        typed_state="director.y.0038",
        wait_command=post_choice,
    )
    open_popup(sequence)
    switch_popup_to_config(sequence)
    sequence.pointer_click(*CLASSIC_CONFIG_FAST_FORWARD_POINT)
    sequence.await_value("vn.reading_mode", "fast_forward")
    sequence.key("Escape")
    sequence.await_value("vn.pending_wait_command", story.command("director.y.0072", "wait"), 14400)

    open_popup(sequence)
    switch_popup_to_config(sequence)
    sequence.pointer_click(390, 265)
    sequence.await_value("vn.reading_mode", "manual")
    sequence.key("Escape")
    first_y72 = story.command("director.y.0072", "text", 0)
    sequence.await_value("vn.pending_wait_command", first_y72, 28800)
    sequence.key("Enter", 24)
    monologue = story.command("director.y.0084", "text", 10)
    sequence.await_value("vn.pending_wait_command", monologue)
    sequence.node_checkpoint("classic.monologue", reference_id="TSUI1999-UI-008", typed_state="director.y.0084", wait_command=monologue)
    sequence.key("Enter", 3)
    two_character = story.command("director.y.0084", "text", 13)
    sequence.await_value("vn.pending_wait_command", two_character)
    sequence.node_checkpoint("classic.dialogue.two_character_overflow", reference_id="TSUI1999-UI-007", typed_state="director.y.0084", wait_command=two_character)
    sequence.finish()
    return sequence


def write_sequence(path: Path, sequence: Sequence) -> None:
    path.write_text(
        "".join(
            json.dumps(row, ensure_ascii=False, separators=(",", ":")) + "\n"
            for row in sequence.rows
        ),
        encoding="utf-8",
    )


def run_sequence(arguments: argparse.Namespace, root: Path, sequence: Sequence) -> dict:
    root.mkdir(parents=True)
    gpu_profile_path = root / "headless-gpu-profile.json"
    profile = prepare_gpu_profile(arguments.profile, gpu_profile_path)
    input_path = root / "classic-visual-input.jsonl"
    write_sequence(input_path, sequence)
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
        str(root),
        "--build-identity",
        str(arguments.build_identity),
    ]
    source_profile = getattr(arguments, "source_profile", None)
    source_root = getattr(arguments, "source_root", None)
    if (source_profile is None) != (source_root is None):
        raise AcceptanceError(
            "Classic source-locked acceptance requires both source profile and source root"
        )
    if source_profile is not None:
        command.extend(
            [
                "--source-profile",
                str(source_profile),
                "--source-root",
                str(source_root),
            ]
        )
    with (root / "runner.stdout.log").open("wb") as stdout, (root / "runner.stderr.log").open(
        "wb"
    ) as stderr:
        completed = subprocess.run(command, stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr)
    if completed.returncode != 0:
        raise AcceptanceError(
            f"Headless Classic visual run {sequence.session} exited with {completed.returncode}"
        )
    run_report, manifest = validate_gpu_artifacts(
        root,
        build_fingerprint=profile["build_fingerprint"],
        package_hash=profile["package_hash"],
        completed_sequence=len(sequence.rows),
        checkpoint_ids=sequence.checkpoints,
    )
    artifacts = [root / "checkpoints" / f"{checkpoint_id}.png" for checkpoint_id in sequence.checkpoints]
    missing = [path.name for path in artifacts if not path.is_file()]
    if missing:
        raise AcceptanceError("Headless Classic checkpoint images are missing: " + ", ".join(missing))
    return {
        "session": sequence.session,
        "input_message_count": len(sequence.rows),
        "input_file_hash": file_hash(input_path),
        "input_sequence_hash": run_report.get("input_sequence_hash"),
        "checkpoint_ids": sequence.checkpoints,
        "checkpoint_nodes": sequence.checkpoint_nodes,
        "checkpoint_artifact_hashes": [file_hash(path) for path in artifacts],
        "run_report_hash": file_hash(root / "run-report.json"),
        "renderer_identity_hash": manifest.get("renderer_identity_hash"),
        "renderer_identity": manifest.get("renderer_identity"),
    }


def run(arguments: argparse.Namespace) -> dict:
    profile = json.loads(arguments.profile.read_text(encoding="utf-8"))
    identity = json.loads(arguments.build_identity.read_text(encoding="utf-8"))
    if profile.get("product_profile") != "classic":
        raise AcceptanceError("Classic acceptance requires product_profile=classic")
    if profile.get("build_fingerprint") != identity.get("identity_hash"):
        raise AcceptanceError("profile and build identity do not match")
    if arguments.artifact_root.exists():
        raise AcceptanceError("artifact root already exists")
    arguments.artifact_root.mkdir(parents=True)
    story = StoryIndex(json.loads(arguments.story_ir.read_text(encoding="utf-8")))
    locator_evidence_hash = story.validate_text_locators(
        json.loads(arguments.node_map.read_text(encoding="utf-8"))
    )
    sequences = [
        build_system_sequence(story),
        build_k_sequence(story),
        build_two_character_sequence(story),
    ]
    runs = [
        run_sequence(arguments, arguments.artifact_root / f"run-{index:02d}", sequence)
        for index, sequence in enumerate(sequences, start=1)
    ]
    renderer_hashes = {run["renderer_identity_hash"] for run in runs}
    if len(renderer_hashes) != 1:
        raise AcceptanceError("Classic GPU sessions used different renderer identities")
    checkpoint_ids = [checkpoint for run in runs for checkpoint in run["checkpoint_ids"]]
    return {
        "schema": REPORT_SCHEMA,
        "status": "passed",
        "profile_id": profile.get("id"),
        "build_fingerprint": profile.get("build_fingerprint"),
        "package_hash": profile.get("package_hash"),
        "renderer_identity_hash": next(iter(renderer_hashes)),
        "text_locator_evidence_hash": locator_evidence_hash,
        "checkpoint_ids": checkpoint_ids,
        "checkpoint_count": len(checkpoint_ids),
        "runs": runs,
        "diagnostics": [],
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--profile", required=True, type=Path)
    parser.add_argument("--package", required=True, type=Path)
    parser.add_argument("--build-identity", required=True, type=Path)
    parser.add_argument("--story-ir", required=True, type=Path)
    parser.add_argument("--node-map", required=True, type=Path)
    parser.add_argument("--artifact-root", required=True, type=Path)
    parser.add_argument("--report", required=True, type=Path)
    parser.add_argument("--source-profile", type=Path)
    parser.add_argument("--source-root", type=Path)
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
