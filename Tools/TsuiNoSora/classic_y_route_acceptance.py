#!/usr/bin/env python3
"""Prove the complete Classic Y movie through serialized physical Headless input."""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path

from classic_visual_acceptance import (
    AcceptanceError,
    CLASSIC_CONFIG_FAST_FORWARD_POINT,
    Sequence,
    json_hash,
    run_sequence,
)
from director_native_story import (
    DirectorNativeStoryError,
    _resolve_pending_wait_events,
    trace_route_choice_witness,
)
from headless_gpu_acceptance import GpuAcceptanceError


REPORT_SCHEMA = "tsuinosora.classic_y_route_acceptance_report.v1"
STORY_SCHEMA = "tsuinosora.native_story_ir.v1"
ROUTE_ID = "route.coverage.001"
BOUNDARY_MOVIE = "K"
CHOICE_ID_PATTERN = re.compile(r"^choice\.[a-z]\.\d{4}\.([1-5])$")


class YRouteAcceptanceError(RuntimeError):
    pass


def _await_event(key: str, value: object, timeout_ticks: int = 3_600) -> dict:
    return {
        "type": "await",
        "observation": {"kind": "equals", "key": key, "value_hash": json_hash(value)},
        "timeout_ticks": timeout_ticks,
        "continue_at_match": True,
    }


def _lower_transition_events(
    transitions: list[dict], stable_wait_hashes: set[str], auto_fence_hashes: set[str]
) -> list[dict]:
    raw: list[dict] = []
    for transition in transitions:
        events = list(transition["events"])
        if any(
            event.get("type") == "_pending_wait" and event.get("command_id") == "page.title"
            for event in events
        ):
            # The Classic preamble below replaces the route planner's Modern
            # title/Quick Panel interaction as one profile-owned physical path.
            continue
        choice_id = transition.get("choice_id")
        if choice_id is not None:
            match = CHOICE_ID_PATTERN.fullmatch(choice_id)
            pending = [
                (index, event)
                for index, event in enumerate(events)
                if event.get("type") == "_pending_wait"
            ]
            if match is None or len(pending) != 1:
                raise YRouteAcceptanceError("Classic route choice evidence is malformed")
            marker_index, marker = pending[0]
            raw.extend(events[:marker_index])
            raw.append(_await_event("vn.pending_wait_command", marker["command_id"], 14_400))
            authored_option_index = int(match.group(1))
            focus_index = transition.get("choice_focus_index")
            first_option_id = transition.get("choice_first_option_id")
            first_match = CHOICE_ID_PATTERN.fullmatch(first_option_id or "")
            if not isinstance(focus_index, int) or not (1 <= focus_index <= 5):
                raise YRouteAcceptanceError("Classic route choice focus index is invalid")
            if authored_option_index < 1 or first_match is None:
                raise YRouteAcceptanceError("Classic route authored option index is invalid")
            raw.append(
                _await_event(
                    "vn.focused_semantic_id",
                    f"root/choices/options/option/{first_option_id}",
                )
            )
            first_authored_index = int(first_match.group(1))
            if authored_option_index < first_authored_index:
                raise YRouteAcceptanceError("Classic route choice order is not monotonic")
            # Classic retains authored rows in keyboard navigation even when an
            # option is disabled.  The simulator's enabled focus index is still
            # validated above, but physical ArrowDown steps follow authored rows.
            for _ in range(authored_option_index - first_authored_index):
                raw.extend(
                    [
                        {
                            "type": "keyboard",
                            "physical_key": "ArrowDown",
                            "logical_key": "ArrowDown",
                            "state": "pressed",
                            "repeat": False,
                        },
                        {
                            "type": "keyboard",
                            "physical_key": "ArrowDown",
                            "logical_key": "ArrowDown",
                            "state": "released",
                            "repeat": False,
                        },
                    ]
                )
            raw.append(
                _await_event(
                    "vn.focused_semantic_id",
                    f"root/choices/options/option/{choice_id}",
                )
            )
            raw.extend(
                [
                    {
                        "type": "keyboard",
                        "physical_key": "Enter",
                        "logical_key": "Enter",
                        "state": "pressed",
                        "repeat": False,
                    },
                    {
                        "type": "keyboard",
                        "physical_key": "Enter",
                        "logical_key": "Enter",
                        "state": "released",
                        "repeat": False,
                    },
                ]
            )
            continue

        for index, event in enumerate(events):
            if event.get("type") != "_pending_wait":
                raw.append(event)
                continue
            has_physical_input = any(
                later.get("type") in {"keyboard", "pointer_button"}
                for later in events[index + 1 :]
            )
            if not has_physical_input:
                raw.append(event)
                continue
            command_id = event["command_id"]
            if command_id.startswith("page."):
                raw.append(_await_event("vn.system_page", command_id.removeprefix("page.")))
            else:
                raw.append(_await_event("vn.pending_wait_command", command_id, 14_400))
    resolved = _resolve_pending_wait_events(raw)
    filtered = []
    for event in resolved:
        if event.get("type") != "await" or event.get("observation", {}).get("key") != "vn.pending_wait_command":
            filtered.append(event)
            continue
        value_hash = event["observation"].get("value_hash")
        if value_hash in auto_fence_hashes:
            filtered.append({"type": "advance_ticks", "count": 120})
        elif value_hash in stable_wait_hashes:
            filtered.append(event)
    return filtered


def _append_event(sequence: Sequence, event: dict) -> None:
    tick_advance = 1
    if event.get("type") == "await":
        tick_advance = event.get("timeout_ticks", 1)
    elif event.get("type") == "advance_ticks":
        tick_advance = event.get("count", 1)
    sequence.add(event, tick_advance=tick_advance)


def build_sequence(story: dict) -> tuple[Sequence, dict]:
    if story.get("schema") != STORY_SCHEMA:
        raise YRouteAcceptanceError("Classic Y acceptance story schema is invalid")
    stories = story.get("stories")
    routes = story.get("routes")
    if not isinstance(stories, list) or len(stories) != 1 or not isinstance(routes, list):
        raise YRouteAcceptanceError("Classic Y acceptance story closure is invalid")
    matches = [route for route in routes if route.get("route_id") == ROUTE_ID]
    if len(matches) != 1:
        raise YRouteAcceptanceError("Classic Y acceptance route witness is missing or duplicated")
    route = matches[0]
    stable_wait_hashes = {
        event["observation"]["value_hash"]
        for row in route.get("input_events", [])
        for event in [row.get("event", {})]
        if event.get("type") == "await"
        and event.get("observation", {}).get("key") == "vn.pending_wait_command"
    }
    if not stable_wait_hashes:
        raise YRouteAcceptanceError("Classic Y acceptance route has no stable wait evidence")
    auto_fence_hashes = {
        json_hash(command["command_id"])
        for state in stories[0]["states"]
        for command in state["scenes"][0]["commands"]
        if command.get("kind") == "wait"
        and command.get("fence") in {"tsui.audio.bgm.end", "tsui.audio.se.end"}
    }
    stable_wait_hashes.update(
        json_hash(command["command_id"])
        for state in stories[0]["states"]
        for command in state["scenes"][0]["commands"]
        if command.get("kind") == "text"
    )
    trace = trace_route_choice_witness(
        stories[0]["states"], route.get("choice_sequence"), BOUNDARY_MOVIE
    )
    sequence = Sequence("tsui.classic.route.y.complete")
    sequence.start()
    sequence.await_value("vn.system_page", "title", 7_200)
    sequence.await_value("vn.focused_semantic_id", "root/start")
    sequence.key("Enter")
    sequence.secondary()
    sequence.await_value("vn.system_page", "quick_panel")
    sequence.await_value("vn.focused_semantic_id", "root/window/tabs/save")
    sequence.key("ArrowLeft")
    sequence.await_value("vn.focused_semantic_id", "root/window/tabs/config")
    sequence.key("Enter")
    sequence.await_value("vn.system_page", "config")
    sequence.await_value("vn.focused_semantic_id", "root/window/body/reading_modes/hidden")
    sequence.pointer_click(*CLASSIC_CONFIG_FAST_FORWARD_POINT)
    sequence.await_value("vn.reading_mode", "fast_forward")
    sequence.key("Escape")
    for event in _lower_transition_events(
        trace["transitions"], stable_wait_hashes, auto_fence_hashes
    ):
        _append_event(sequence, event)
    sequence.await_value("vn.pending_wait_command", trace["boundary_wait_command"], 18_000)
    sequence.checkpoint("classic.route.y.complete")
    sequence.finish()
    return sequence, trace


def run(arguments: argparse.Namespace) -> dict:
    story = json.loads(arguments.story_ir.read_text(encoding="utf-8"))
    sequence, trace = build_sequence(story)
    result = run_sequence(arguments, arguments.artifact_root, sequence)
    profile = json.loads(arguments.profile.read_text(encoding="utf-8"))
    consumed_choices = trace["consumed_choice_sequence"]
    return {
        "schema": REPORT_SCHEMA,
        "status": "passed",
        "route_id": ROUTE_ID,
        "guaranteed_movie": "Y",
        "boundary_movie": BOUNDARY_MOVIE,
        "boundary_state": trace["boundary_state"],
        "boundary_wait_command": trace["boundary_wait_command"],
        "choice_selection_count": len(consumed_choices),
        "choice_sequence_hash": json_hash(consumed_choices),
        "build_fingerprint": profile.get("build_fingerprint"),
        "package_hash": profile.get("package_hash"),
        "input_message_count": result["input_message_count"],
        "input_file_hash": result["input_file_hash"],
        "input_sequence_hash": result["input_sequence_hash"],
        "run_report_hash": result["run_report_hash"],
        "renderer_identity_hash": result["renderer_identity_hash"],
        "renderer_identity": result["renderer_identity"],
        "checkpoint_id": "classic.route.y.complete",
        "diagnostics": [],
        "redaction": {
            "commercial_text": "omitted",
            "payload": "omitted",
            "local_paths": "omitted",
        },
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--profile", required=True, type=Path)
    parser.add_argument("--package", required=True, type=Path)
    parser.add_argument("--build-identity", required=True, type=Path)
    parser.add_argument("--story-ir", required=True, type=Path)
    parser.add_argument("--artifact-root", required=True, type=Path)
    parser.add_argument("--report", required=True, type=Path)
    parser.add_argument("--source-profile", type=Path)
    parser.add_argument("--source-root", type=Path)
    return parser.parse_args()


def main() -> int:
    arguments = parse_args()
    try:
        report = run(arguments)
    except (
        AcceptanceError,
        DirectorNativeStoryError,
        GpuAcceptanceError,
        YRouteAcceptanceError,
        OSError,
        ValueError,
        json.JSONDecodeError,
    ) as error:
        raise SystemExit(f"Classic Y route acceptance blocked: {type(error).__name__}") from None
    arguments.report.parent.mkdir(parents=True, exist_ok=True)
    arguments.report.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({"schema": REPORT_SCHEMA, "status": report["status"]}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
