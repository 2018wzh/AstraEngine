import hashlib
import json
import unittest

from director_native_story import (
    _derive_route_automation,
    _choice_display_text,
    _director_blend_to_linear_opacity,
    _flatten_scene_operations,
    _native_center_y,
    _resolve_pending_wait_events,
    _route_from_path,
    _score_openings,
    _split_reading_text,
    DirectorNativeStoryError,
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
    def test_director_choice_visual_prefix_is_lowered_into_the_classic_view(self):
        self.assertEqual(_choice_display_text("\u3000\u3000◆fixture"), "fixture")
        with self.assertRaisesRegex(DirectorNativeStoryError, "visual prefix"):
            _choice_display_text("fixture")

    def test_director_gamma_space_shade_is_converted_to_linear_scene_alpha(self):
        self.assertEqual(_director_blend_to_linear_opacity(0), 0)
        self.assertEqual(_director_blend_to_linear_opacity(70), 92)
        self.assertEqual(_director_blend_to_linear_opacity(100), 100)
        with self.assertRaisesRegex(DirectorNativeStoryError, "outside zero"):
            _director_blend_to_linear_opacity(101)

    def test_native_asset_center_uses_bound_height_not_stage_height(self):
        self.assertEqual(
            _native_center_y(
                {"height": 209},
                {"x": 400, "y": 196, "width": 800, "height": 600},
            ),
            300,
        )
        with self.assertRaisesRegex(
            DirectorNativeStoryError,
            "positive native height",
        ):
            _native_center_y({}, {"x": 0, "y": 0, "width": 800, "height": 600})

    def test_score_opening_is_bound_to_the_director_entry_node(self):
        movies = [
            {"movie_id": movie, "entry_node": f"director.{movie.lower()}.0010"}
            for movie in ("K", "S", "T", "Y", "Z")
        ]
        binding = {
            "asset_id": "tsui.asset.fixture",
            "width": 800,
            "height": 600,
        }
        records = [
            {
                "movie_id": movie,
                "entry_frame": 10,
                "next_frame": 11,
                "frames": [
                    {
                        "frame": 10,
                        "delay_ms": 9_000,
                        "sprite": {
                            "x": 400,
                            "y": 300,
                            "width": 800,
                            "height": 600,
                            "binding": binding,
                        },
                    }
                ],
            }
            for movie in ("K", "S", "T", "Y", "Z")
        ]

        openings = _score_openings(records, movies)

        self.assertEqual(set(openings), {movie["entry_node"] for movie in movies})
        self.assertEqual(openings["director.y.0010"]["frames"][0]["delay_ms"], 9_000)

    def test_reading_blocks_preserve_typed_surface_identity(self):
        operations = [
            {
                "kind": "reading",
                "mode": "talk",
                "termination": "close",
                "events": [{"kind": "text", "text": "話者「台詞」"}],
            },
            {
                "kind": "reading",
                "mode": "mono",
                "termination": "close",
                "events": [{"kind": "text", "text": "叙述"}],
            },
        ]

        flattened = list(_flatten_scene_operations(operations))
        text_operations = [operation for operation in flattened if operation["kind"] == "text"]

        self.assertEqual(
            [operation["reading_mode"] for operation in text_operations], ["talk", "mono"]
        )
        self.assertEqual([operation["reading_group"] for operation in text_operations], [0, 1])
        self.assertEqual(
            [operation for operation in flattened if operation["kind"] == "set_shade"],
            [
                {"kind": "set_shade", "opacity": 70},
                {"kind": "set_shade", "opacity": 0},
            ],
        )

    def test_monologue_escape_preserves_shade_until_monoreturn_closes(self):
        operations = [
            {
                "kind": "reading",
                "mode": "mono",
                "termination": "escape",
                "events": [{"kind": "text", "text": "first"}],
            },
            {
                "kind": "reading",
                "mode": "monoreturn",
                "termination": "close",
                "events": [{"kind": "text", "text": "second"}],
            },
        ]

        flattened = list(_flatten_scene_operations(operations))

        self.assertEqual(
            [operation for operation in flattened if operation["kind"] == "set_shade"],
            [
                {"kind": "set_shade", "opacity": 70},
                {"kind": "set_shade", "opacity": 0},
            ],
        )

    def test_text_outside_reading_block_is_blocking(self):
        with self.assertRaisesRegex(
            DirectorNativeStoryError,
            "scene text is not owned by a typed reading block",
        ):
            list(_flatten_scene_operations([{"kind": "text", "text": "orphan"}]))

    def test_talk_speaker_is_split_without_changing_dialogue_quote(self):
        text, speaker_id, speaker_text = _split_reading_text(
            "話 者「台詞」", "talk"
        )

        self.assertEqual(text, "「台詞」")
        self.assertEqual(speaker_text, "話者")
        self.assertRegex(speaker_id, r"^tsui\.speaker\.[0-9a-f]{16}$")

    def test_monologue_never_infers_a_speaker(self):
        text, speaker_id, speaker_text = _split_reading_text(
            "話者「叙述として表示する」", "mono"
        )

        self.assertEqual(text, "話者「叙述として表示する」")
        self.assertIsNone(speaker_id)
        self.assertIsNone(speaker_text)

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
