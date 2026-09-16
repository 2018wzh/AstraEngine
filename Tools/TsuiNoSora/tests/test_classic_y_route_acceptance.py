import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from classic_y_route_acceptance import _lower_transition_events, json_hash


class ReadingInputTests(unittest.TestCase):
    def test_dialogue_wait_requires_visible_text_before_physical_advance(self):
        press = {"type": "keyboard", "physical_key": "Enter", "logical_key": "Enter",
                 "state": "pressed", "repeat": False}
        release = dict(press, state="released")
        transitions = [{"events": [
            {"type": "_pending_wait", "command_id": "line.first"}, press, release,
            {"type": "_pending_wait", "command_id": "input.wait"}, press, release,
        ]}]
        result = _lower_transition_events(
            transitions, {json_hash("line.first"), json_hash("input.wait")},
            set(), {json_hash("line.first")},
        )
        self.assertEqual([row.get("observation", {}).get("key", row["type"]) for row in result], [
            "vn.pending_wait_command", "vn.text_reveal_complete", "keyboard", "keyboard",
            "vn.pending_wait_command", "keyboard", "keyboard",
        ])
        self.assertEqual(result[1]["observation"]["value_hash"], json_hash(True))
        self.assertEqual(result[2:4], [press, release])


if __name__ == "__main__":
    unittest.main()
