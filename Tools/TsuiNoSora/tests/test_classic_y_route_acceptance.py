import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from classic_y_route_acceptance import _lower_transition_events, _sample_checkpoints, json_hash


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


class SamplingTests(unittest.TestCase):
    def test_samples_cover_visible_dialogue_and_choices_across_the_route(self):
        events, states = [], {}
        for i in range(60):
            command = json_hash(f"text.{i}")
            states[command] = f"scene.{i // 3}"
            events.extend([
                {"type": "await", "observation": {"key": "vn.pending_wait_command", "value_hash": command}},
                {"type": "await", "observation": {"key": "vn.text_reveal_complete", "value_hash": json_hash(True)}},
                {"type": "keyboard", "physical_key": "Enter", "state": "pressed"},
            ])
            if i % 5 == 0:
                events.extend([
                    {"type": "await", "observation": {"key": "vn.focused_semantic_id", "value_hash": json_hash(f"choice.{i}")}},
                    {"type": "keyboard", "physical_key": "Enter", "state": "pressed"},
                ])
        original = json_hash(events)
        samples = _sample_checkpoints(events, states, "classic.test")
        self.assertLessEqual(len(samples), 28)
        self.assertEqual(len(samples), len(set(samples.values())))
        self.assertEqual(sum("dialogue" in value for value in samples.values()), 12)
        self.assertEqual(sum("choice" in value for value in samples.values()), 8)
        self.assertEqual(sum("scene" in value for value in samples.values()), 8)
        dialogue = [i for i in samples if "dialogue" in samples[i]]
        self.assertEqual(dialogue[0], 1)
        self.assertEqual(dialogue[-1], len(events) - 2)
        self.assertEqual(json_hash(events), original)
        for i in samples:
            self.assertEqual(events[i]["type"], "await")
            self.assertEqual(events[i+1]["type"], "keyboard")

    def test_empty_and_single_visible_point(self):
        self.assertEqual(_sample_checkpoints([], {}, "route"), {})
        events = [{"type": "await", "observation": {"key": "vn.text_reveal_complete"}}]
        self.assertEqual(_sample_checkpoints(events, {}, "route"), {0: "route.sample.001.dialogue"})

if __name__ == "__main__":
    unittest.main()
