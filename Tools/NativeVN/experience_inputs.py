#!/usr/bin/env python3
"""Write physical input for the NativeVN sample's short reading experience."""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path


def build_sequence() -> list[dict]:
    rows = []
    tick = 0

    def add(event, advance=1):
        nonlocal tick
        rows.append({"schema": "astra.user_input_sequence.v1", "session": "nativevn.experience",
                     "sequence": len(rows) + 1, "tick": tick, "event": event})
        tick += advance

    def await_value(key, value, timeout=600):
        digest = hashlib.sha256(json.dumps(value, ensure_ascii=False, sort_keys=True,
                                          separators=(",", ":")).encode()).hexdigest()
        add({"type": "await", "observation": {"kind": "equals", "key": key,
             "value_hash": "sha256:" + digest}, "timeout_ticks": timeout,
             "continue_at_match": True}, timeout)

    def key(name):
        for state in ("pressed", "released"):
            add({"type": "keyboard", "physical_key": name, "logical_key": name,
                 "state": state, "repeat": False})

    def dialogue(command):
        await_value("vn.pending_wait_command", command)
        await_value("vn.text_reveal_complete", True)
        key("Enter")

    add({"type": "resume"})
    add({"type": "focus", "focused": True})
    await_value("vn.pending_wait_command", "experience.read.first")
    await_value("vn.text_reveal_complete", True)
    add({"type": "checkpoint", "id": "experience.reading"})
    key("Enter")
    dialogue("experience.read.second")
    await_value("vn.pending_wait_command", "experience.stage.first")
    # Fixed physical time lets the walk and camera progress together.
    add({"type": "checkpoint", "id": "experience.arrival"}, 240)
    await_value("vn.text_reveal_complete", True)
    add({"type": "checkpoint", "id": "experience.approach"})
    key("Enter")
    dialogue("experience.stage.second")
    dialogue("experience.stage.third")
    await_value("vn.pending_wait_command", "experience.save.before")
    await_value("vn.text_reveal_complete", True)
    add({"type": "checkpoint", "id": "experience.keepsake"})
    # Saving/restoring and full video need separate input cases.
    add({"type": "shutdown"})
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    rows = build_sequence()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    # Existing sequences may be evidence from another build; do not overwrite them.
    with args.output.open("x", encoding="utf-8") as stream:
        for row in rows:
            stream.write(json.dumps(row, ensure_ascii=False, separators=(",", ":")) + "\n")


if __name__ == "__main__":
    main()
