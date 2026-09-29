#!/usr/bin/env python3
"""Write physical input for the NativeVN sample's short reading experience."""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path


def build_sequence(*, save_restore: bool = False, media: bool = False,
                   cold_restore: bool = False, media_action: str = "full") -> list[dict]:
    if media_action not in ("full", "restore", "system", "skip", "close"):
        raise ValueError("unknown media action")
    if media_action != "full" and not media:
        raise ValueError("media actions require the media branch")
    if cold_restore and (save_restore or media):
        raise ValueError("cold restore is a separate process input sequence")
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
    if cold_restore:
        # The runner must supply the previous process's save directory unchanged.
        await_value("vn.occupied_save_slot_count", 1)
        await_value("vn.pending_wait_command", "experience.read.first")
        await_value("vn.text_reveal_complete", True)
        for _ in range(4):
            key("Tab")
        key("Enter")
        await_value("vn.system_page", "load")
        add({"type": "checkpoint", "id": "experience.cold.catalog"})
        key("Tab")
        key("Enter")
        await_value("vn.system_page", None)
        await_value("vn.pending_wait_command", "experience.save.before")
        add({"type": "checkpoint", "id": "experience.cold.restored"}, 120)
        add({"type": "checkpoint", "id": "experience.cold.continued"})
        add({"type": "shutdown"})
        return rows
    await_value("vn.pending_wait_command", "experience.read.first")
    await_value("vn.text_reveal_complete", False)
    add({"type": "checkpoint", "id": "experience.reading.revealing"})
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
    await_value("vn.pending_wait_command", "experience.stage.second")
    await_value("vn.text_reveal_complete", True)
    add({"type": "checkpoint", "id": "experience.signal.dim"})
    key("Enter")
    await_value("vn.pending_wait_command", "experience.stage.third")
    await_value("vn.text_reveal_complete", True)
    add({"type": "checkpoint", "id": "experience.signal.restored"})
    key("Enter")
    await_value("vn.pending_wait_command", "experience.save.before")
    await_value("vn.text_reveal_complete", True)
    add({"type": "checkpoint", "id": "experience.keepsake"})
    if save_restore:
        # All actions go through real focus navigation and the rendered system UI.
        # The full-screen advance panel is the first focusable control.
        key("Tab")
        key("Tab")
        key("Tab")
        key("Enter")
        await_value("vn.system_page", "save")
        add({"type": "checkpoint", "id": "experience.save_page"})
        key("Tab")
        key("Enter")
        await_value("vn.occupied_save_slot_count", 1)
        add({"type": "checkpoint", "id": "experience.saved"})
        key("Escape")
        await_value("vn.system_page", None)
        key("Tab")
        key("Tab")
        key("Tab")
        key("Tab")
        key("Enter")
        await_value("vn.system_page", "load")
        add({"type": "checkpoint", "id": "experience.load_page"})
        key("Tab")
        key("Enter")
        await_value("vn.system_page", None)
        await_value("vn.pending_wait_command", "experience.save.before")
        add({"type": "checkpoint", "id": "experience.restored"}, 120)
        add({"type": "checkpoint", "id": "experience.restored.continued"})
    if media:
        dialogue("experience.save.before")
        dialogue("experience.save.after")
        await_value("vn.pending_choices", ["experience.save.rain", "experience.save.back"])
        key("Tab")
        key("Enter")
        dialogue("experience.media.before")
        await_value("media.active_video", True)
        add({"type": "checkpoint", "id": "experience.video.start"},
            360 if media_action == "full" else 180)
        if media_action == "restore":
            key("F5")
            await_value("vn.occupied_save_slot_count", 1)
            add({"type": "checkpoint", "id": "experience.video.saved"}, 120)
            key("F9")
            await_value("media.active_video", True)
            add({"type": "checkpoint", "id": "experience.video.restored"}, 120)
            add({"type": "checkpoint", "id": "experience.video.continued"})
        elif media_action == "system":
            for state in ("pressed", "released"):
                add({"type": "pointer_button", "button": "secondary", "state": state})
            await_value("vn.system_page", "quick_panel")
            add({"type": "checkpoint", "id": "experience.video.system"}, 60)
            # Right-click opens the page without keyboard focus; Tab selects Back.
            key("Tab")
            key("Enter")
            await_value("vn.system_page", None)
            await_value("media.active_video", True)
            add({"type": "checkpoint", "id": "experience.video.returned"})
        elif media_action == "skip":
            key("Enter")
        elif media_action == "close":
            # Shutdown closes immediately; consume the elapsed ticks first.
            add({"type": "checkpoint", "id": "experience.video.before_close"})
            add({"type": "shutdown"})
            return rows
        else:
            add({"type": "checkpoint", "id": "experience.video.middle"})
        await_value("vn.pending_wait_command", "experience.media.after", timeout=1200)
        await_value("media.active_video", False)
        add({"type": "checkpoint", "id": "experience.video.end"})
    # Cold-process restore needs a separate input case.
    add({"type": "shutdown"})
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--save-restore", action="store_true")
    parser.add_argument("--media", action="store_true")
    parser.add_argument("--media-action", choices=("full", "restore", "system", "skip", "close"),
                        default="full", help="physical interaction during the media branch")
    parser.add_argument("--cold-restore", action="store_true")
    args = parser.parse_args()
    if args.cold_restore and (args.save_restore or args.media):
        parser.error("--cold-restore cannot be combined with --save-restore or --media")
    if args.media_action != "full" and not args.media:
        parser.error("--media-action requires --media")
    rows = build_sequence(save_restore=args.save_restore, media=args.media,
                          cold_restore=args.cold_restore, media_action=args.media_action)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    # Preserve input files from previous runs; callers choose a fresh private output.
    with args.output.open("x", encoding="utf-8") as stream:
        for row in rows:
            stream.write(json.dumps(row, ensure_ascii=False, separators=(",", ":")) + "\n")


if __name__ == "__main__":
    main()
