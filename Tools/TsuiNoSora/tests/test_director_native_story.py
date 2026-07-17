import hashlib
import json
import unittest

from director_native_story import (
    _derive_route_automation,
    _resolve_pending_wait_events,
    _route_from_path,
)


def observation_hash(value):
    encoded = json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    return f"sha256:{hashlib.sha256(encoded).hexdigest()}"


def _keyboard_events(key):
    return [
        {"type": "keyboard", "physical_key": key, "state": "pressed"},
        {"type": "keyboard", "physical_key": key, "state": "released"},
    ]


class DirectorNativeStoryAutomationTests(unittest.TestCase):
    def test_async_wait_continues_at_the_next_stable_wait_command(self):
        events = _resolve_pending_wait_events(
            [
                {"type": "_pending_wait", "command_id": "wait.audio"},
                {"type": "_await_next_wait", "timeout_ticks": 3600},
                {"type": "_pending_wait", "command_id": "line.after"},
                {"type": "keyboard", "physical_key": "Enter"},
            ]
        )

        self.assertEqual(len(events), 2)
        self.assertEqual(events[0]["type"], "await")
        self.assertTrue(events[0]["continue_at_match"])
        self.assertEqual(
            events[0]["observation"],
            {
                "kind": "equals",
                "key": "vn.pending_wait_command",
                "value_hash": observation_hash("line.after"),
            },
        )

    def test_terminal_async_wait_targets_absent_pending_wait(self):
        events = _resolve_pending_wait_events(
            [
                {"type": "_pending_wait", "command_id": "wait.final"},
                {"type": "_await_next_wait", "timeout_ticks": 18000},
            ]
        )

        self.assertEqual(events[0]["observation"]["value_hash"], observation_hash(None))

    def test_skip_all_does_not_wait_for_dialogue_commands(self):
        events = _resolve_pending_wait_events(
            [
                {"type": "_dialogue_advance", "command_id": "line.first"},
                {"type": "_dialogue_advance", "command_id": "line.second"},
                {"type": "_pending_wait", "command_id": "input.name"},
                *_keyboard_events("Enter"),
            ]
        )
        keyboard = [event for event in events if event["type"] == "keyboard"]
        self.assertEqual(len(keyboard), 2)
        self.assertFalse(any(event["type"] == "_pending_wait" for event in events))

    def test_async_wait_ignores_dialogue_that_skip_all_will_not_surface(self):
        events = _resolve_pending_wait_events(
            [
                {"type": "_dialogue_advance", "command_id": "line.first"},
                {"type": "_await_next_wait", "timeout_ticks": 3600},
                {"type": "_dialogue_advance", "command_id": "line.skipped"},
                {"type": "_pending_wait", "command_id": "input.next"},
                *_keyboard_events("Enter"),
            ]
        )
        self.assertEqual(events[0]["type"], "await")
        self.assertEqual(events[0]["observation"]["value_hash"], observation_hash("input.next"))

    def test_repeated_bgm_fade_stop_skips_the_already_completed_fence(self):
        events = _resolve_pending_wait_events(
            [
                {"type": "_audio_start", "target": "tsui.audio.bgm"},
                {
                    "type": "_audio_control",
                    "action": "fade_stop",
                    "target": "tsui.audio.bgm",
                    "fence": "tsui.audio.bgm.end",
                },
                {
                    "type": "_pending_wait",
                    "command_id": "wait.first",
                    "fence": "tsui.audio.bgm.end",
                },
                {"type": "_await_next_wait", "timeout_ticks": 3600},
                {"type": "_pending_wait", "command_id": "line.after.first"},
                {
                    "type": "_audio_control",
                    "action": "fade_stop",
                    "target": "tsui.audio.bgm",
                    "fence": "tsui.audio.bgm.end",
                },
                {
                    "type": "_pending_wait",
                    "command_id": "wait.second",
                    "fence": "tsui.audio.bgm.end",
                },
                {"type": "_await_next_wait", "timeout_ticks": 3600},
                {"type": "_pending_wait", "command_id": "line.after.second"},
            ]
        )
        wait_hashes = [
            event["observation"]["value_hash"]
            for event in events
            if event["type"] == "await"
        ]
        self.assertEqual(wait_hashes, [observation_hash("line.after.first")])

    def test_route_does_not_inject_a_synthetic_quick_panel_preamble(self):
        route = _route_from_path(1, [])
        types = [item["event"]["type"] for item in route["input_events"]]
        self.assertEqual(
            types[:3],
            [
                "resume",
                "focus",
                "pointer_move",
            ],
        )
        self.assertNotIn("pointer_button", types[:-2])
        terminal_wait = route["input_events"][-3]["event"]
        self.assertEqual(terminal_wait["type"], "await")
        self.assertEqual(terminal_wait["observation"]["key"], "vn.terminal_routes")
        self.assertEqual(
            terminal_wait["observation"]["value_hash"], observation_hash(["state.tsui.ending"])
        )

    def test_authored_title_drives_start_and_skip_all_through_physical_ui(self):
        states = [
            {
                "state_id": "tsui.init",
                "scenes": [
                    {
                        "commands": [
                            {"command_id": "page.title", "kind": "system_page", "page": "title"},
                            {"command_id": "jump.ending", "kind": "jump", "target": "ending"},
                        ]
                    }
                ],
            },
            {
                "state_id": "ending",
                "scenes": [{"commands": [{"command_id": "wait.ending", "kind": "input_wait"}]}],
            },
        ]

        route = _derive_route_automation(states, {})[0]
        events = [item["event"] for item in route["input_events"]]

        title_start = next(index for index, event in enumerate(events) if event.get("physical_key") == "Enter")
        secondary = next(
            index
            for index, event in enumerate(events)
            if event.get("type") == "pointer_button" and event.get("button") == "secondary"
        )
        self.assertLess(title_start, secondary)
        self.assertTrue(any(event.get("physical_key") == "Escape" for event in events[secondary:]))

    def test_route_planner_preserves_terminal_state_input(self):
        states = [
            {
                "state_id": "tsui.init",
                "scenes": [
                    {
                        "commands": [
                            {"command_id": "jump.ending", "kind": "jump", "target": "ending"}
                        ]
                    }
                ],
            },
            {
                "state_id": "ending",
                "scenes": [
                    {
                        "commands": [
                            {"command_id": "wait.ending", "kind": "input_wait"}
                        ]
                    }
                ],
            },
        ]
        routes = _derive_route_automation(states, {})
        self.assertEqual(len(routes), 1)
        self.assertEqual(routes[0]["terminal_id"], "ending")
        self.assertEqual(routes[0]["terminal_route_node_id"], "state.ending")
        self.assertIn("wait.ending", routes[0]["command_ids"])
        self.assertTrue(
            any(
                item["event"].get("physical_key") == "Enter"
                for item in routes[0]["input_events"]
            )
        )


if __name__ == "__main__":
    unittest.main()
