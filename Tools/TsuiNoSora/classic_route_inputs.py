#!/usr/bin/env python3
"""Generate Classic physical inputs for the existing native route matrix runner."""

from __future__ import annotations

import argparse
import copy
import json
import tempfile
from pathlib import Path

from classic_y_route_acceptance import build_sequence
from headless_route_matrix import _validate_route_input


def matrix_rows(sequence, route_id: str) -> list[dict]:
    """Change only matrix-owned session/checkpoint names, preserving input timing."""
    checkpoint = f"classic.{route_id}.complete"
    if sequence.checkpoints != [checkpoint]:
        raise ValueError("Classic matrix input requires a complete route")
    rows = copy.deepcopy(sequence.rows)
    if len(rows) < 2 or rows[-2]["event"] != {"type": "checkpoint", "id": checkpoint}:
        raise ValueError("Classic matrix input has no final checkpoint")
    if rows[-1]["event"] != {"type": "shutdown"}:
        raise ValueError("Classic matrix input has no shutdown")
    for row in rows:
        row["session"] = f"tsui.{route_id}"
    rows[-2]["event"]["id"] = f"checkpoint.{route_id}"
    return rows


def generate_inputs(story: dict, output: Path) -> dict:
    routes = story.get("routes")
    if not isinstance(routes, list) or not routes:
        raise ValueError("Classic matrix story contains no routes")
    route_ids = [route.get("route_id") for route in routes if isinstance(route, dict)]
    if len(route_ids) != len(routes) or any(
        not isinstance(route_id, str)
        or not route_id.startswith("route.coverage.")
        or not route_id.removeprefix("route.coverage.").isascii()
        or not route_id.removeprefix("route.coverage.").isdigit()
        for route_id in route_ids
    ):
        raise ValueError("Classic matrix route identity is invalid")
    if len(set(route_ids)) != len(route_ids):
        raise ValueError("Classic matrix contains duplicate routes")
    if output.exists():
        raise ValueError("Classic matrix output already exists")
    # Keep one route in memory and publish the directory only after all routes
    # validate. Failed generation removes only its own staging directory.
    output.parent.mkdir(parents=True, exist_ok=True)
    max_messages = max_tick = 0
    with tempfile.TemporaryDirectory(prefix=".classic-inputs-", dir=output.parent) as temporary:
        stage = Path(temporary) / "inputs"
        stage.mkdir()
        for route in routes:
            sequence, _ = build_sequence(story, route["route_id"], complete_route=True)
            rows = matrix_rows(sequence, route["route_id"])
            path = stage / f"{route['route_id']}.jsonl"
            with path.open("x", encoding="utf-8", newline="\n") as handle:
                for row in rows:
                    handle.write(json.dumps(row, ensure_ascii=False, separators=(",", ":")) + "\n")
            _validate_route_input(path, route)
            max_messages = max(max_messages, len(rows))
            max_tick = max(max_tick, max(row["tick"] for row in rows))
        if output.exists():
            raise ValueError("Classic matrix output appeared during generation")
        stage.rename(output)
    return {
        "route_count": len(routes),
        "required_max_messages": max_messages,
        "required_max_tick": max_tick,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--story-ir", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    try:
        story = json.loads(args.story_ir.read_text(encoding="utf-8"))
        if not isinstance(story, dict):
            raise ValueError("Classic story must be an object")
        result = generate_inputs(story, args.output)
    except (OSError, ValueError, RuntimeError) as error:
        # Exception payloads can contain commercial script data or private paths.
        raise SystemExit(f"Classic matrix input generation failed: {type(error).__name__}") from None
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
