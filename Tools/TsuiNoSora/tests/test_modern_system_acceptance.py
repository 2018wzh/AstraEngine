import sys
import unittest
from pathlib import Path


TOOLS_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(TOOLS_ROOT))

from classic_visual_acceptance import StoryIndex  # noqa: E402
from modern_system_acceptance import build_sequence, json_hash  # noqa: E402


class ModernSystemAcceptanceTests(unittest.TestCase):
    def test_title_entry_wait_comes_from_the_current_private_story_ir(self) -> None:
        story = StoryIndex(
            {
                "schema": "tsuinosora.native_story_ir.v1",
                "stories": [
                    {
                        "states": [
                            {
                                "state_id": "director.y.0026",
                                "scenes": [
                                    {
                                        "commands": [
                                            {
                                                "kind": "text",
                                                "command_id": "fixture.current.title.entry",
                                            }
                                        ]
                                    }
                                ],
                            }
                        ]
                    }
                ],
            }
        )

        sequence = build_sequence(story)
        pending_waits = [
            row["event"]["observation"]["value_hash"]
            for row in sequence.rows
            if row["event"].get("type") == "await"
            and row["event"]["observation"].get("key") == "vn.pending_wait_command"
        ]
        self.assertEqual(pending_waits, [json_hash("fixture.current.title.entry")])


if __name__ == "__main__":
    unittest.main()
