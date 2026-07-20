import importlib.util
import hashlib
import sys
import unittest
from pathlib import Path


TOOLS_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(TOOLS_ROOT))
SPEC = importlib.util.spec_from_file_location(
    "classic_visual_acceptance",
    TOOLS_ROOT / "classic_visual_acceptance.py",
)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(MODULE)


def story_index():
    def state(state_id, kinds):
        return {
            "state_id": state_id,
            "scenes": [
                {
                    "scene_id": f"scene.{state_id}",
                    "commands": [
                        {"command_id": f"fixture.{state_id}.{kind}.{index}", "kind": kind}
                        for index, kind in enumerate(kinds)
                    ],
                }
            ],
        }

    return MODULE.StoryIndex(
        {
            "schema": "tsuinosora.native_story_ir.v1",
            "stories": [
                {
                    "states": [
                        state("tsui.init", ["system_page"]),
                        state("director.y.0010.score.0013", ["timeline"]),
                        state("director.y.0010.score.0015", ["timeline"]),
                        state("director.y.0020", ["wait"]),
                        state("director.y.0026", ["text"] * 45),
                        state("director.k.0015", ["wait"]),
                        state("director.k.0021", ["text"] * 12),
                        state("director.k.0027", ["text"]),
                        state("director.k.0033", ["wait"]),
                        state("director.y.0032.choice", ["choice"]),
                        state("director.y.0038", ["text"]),
                        state("director.y.0072", ["wait", "text"]),
                        state("director.y.0084", ["text"] * 14),
                    ]
                }
            ],
        }
    )


class ClassicVisualAcceptanceTests(unittest.TestCase):
    def test_text_locator_selects_a_declared_candidate_without_exposing_text(self) -> None:
        story = story_index()
        command_id = story.command("director.y.0026", "text", 0)
        story.commands[command_id][1]["text"] = "fixture-private-text"
        content_hash = "sha256:" + hashlib.sha256(b"fixture-private-text").hexdigest()
        node_map = {
            "schema": "tsuinosora.classic_visual_node_map.v3",
            "entries": [{
                "reference_id": "TSUI1999-UI-005",
                "comparison_class": "same_node",
                "identity": {
                    "typed_state": "director.y.0026",
                    "wait_command": command_id,
                    "locator": {
                        "method": "story_text",
                        "content_sha256": content_hash,
                        "candidate_commands": [command_id],
                    },
                },
            }],
        }
        self.assertTrue(story.validate_text_locators(node_map).startswith("sha256:"))

    def test_text_locator_rejects_incomplete_duplicate_candidate_closure(self) -> None:
        story = story_index()
        first = story.command("director.y.0026", "text", 0)
        second = story.command("director.y.0026", "text", 1)
        for command_id in (first, second):
            story.commands[command_id][1]["text"] = "reused-private-text"
        content_hash = "sha256:" + hashlib.sha256(b"reused-private-text").hexdigest()
        node_map = {
            "schema": "tsuinosora.classic_visual_node_map.v3",
            "entries": [{
                "reference_id": "TSUI1999-UI-005",
                "comparison_class": "same_node",
                "identity": {
                    "typed_state": "director.y.0026",
                    "wait_command": first,
                    "locator": {
                        "method": "story_text",
                        "content_sha256": content_hash,
                        "candidate_commands": [first],
                    },
                },
            }],
        }
        with self.assertRaisesRegex(MODULE.AcceptanceError, "candidate closure"):
            story.validate_text_locators(node_map)

    def test_sequences_cover_every_authored_classic_surface(self) -> None:
        sequences = [
            MODULE.build_system_sequence(story_index()),
            MODULE.build_k_sequence(story_index()),
            MODULE.build_two_character_sequence(story_index()),
        ]
        checkpoints = {checkpoint for sequence in sequences for checkpoint in sequence.checkpoints}
        expected = {
                "classic.title",
                "classic.title_load",
                "classic.exit_focused",
                "classic.dialogue.background_only",
                "classic.dialogue.character_overflow",
                "classic.popup",
                "classic.save",
                "classic.load",
                "classic.load_restored",
                "classic.config",
                "classic.test_menu",
                "classic.opening.staggered",
                "classic.stage.opening_sphere",
                "classic.monologue",
                "classic.choice",
                "classic.opening.centered",
                "classic.dialogue.legacy_game",
                "classic.dialogue.two_character_overflow",
                "classic.opening.viewpoint",
                "classic.dialogue.first",
                "classic.dialogue.background_only.next",
                "classic.choice.predecessor",
                "classic.choice.successor",
            }
        stable = {f"{checkpoint}.__stable" for checkpoint in expected if checkpoint in {
            "classic.title", "classic.title_load", "classic.dialogue.background_only",
            "classic.dialogue.character_overflow", "classic.popup", "classic.save",
            "classic.load", "classic.config", "classic.opening.staggered",
            "classic.monologue", "classic.choice", "classic.opening.centered",
            "classic.stage.opening_sphere", "classic.dialogue.legacy_game",
            "classic.dialogue.two_character_overflow",
            "classic.opening.viewpoint", "classic.dialogue.first",
            "classic.dialogue.background_only.next", "classic.choice.predecessor",
            "classic.choice.successor",
        }}
        self.assertEqual(checkpoints, expected | stable)

    def test_sequences_only_use_serialized_physical_input_and_observation_events(self) -> None:
        allowed = {
            "resume",
            "focus",
            "keyboard",
            "pointer_move",
            "pointer_button",
            "await",
            "checkpoint",
            "shutdown",
        }
        for sequence in (
            MODULE.build_system_sequence(story_index()),
            MODULE.build_k_sequence(story_index()),
            MODULE.build_two_character_sequence(story_index()),
        ):
            self.assertTrue(sequence.rows)
            self.assertTrue(all(row["event"]["type"] in allowed for row in sequence.rows))
            self.assertEqual(
                [row["sequence"] for row in sequence.rows],
                list(range(1, len(sequence.rows) + 1)),
            )

    def test_hidden_menu_entry_uses_the_original_bounded_hotspot(self) -> None:
        sequence = MODULE.Sequence("fixture")
        MODULE.open_hidden_test(sequence)
        move = next(row["event"] for row in sequence.rows if row["event"]["type"] == "pointer_move")
        self.assertLess(move["x"], round(64 * 65535 / 799))
        self.assertLess(move["y"], round(64 * 65535 / 599))


if __name__ == "__main__":
    unittest.main()
