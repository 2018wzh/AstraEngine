import unittest

from native_story_ir import _command_payload_valid, _render_command


class NativeStoryReadingIrTests(unittest.TestCase):
    def test_text_requires_an_explicit_supported_reading_window(self):
        command = {
            "kind": "text",
            "text": "line",
            "speaker_id": None,
            "speaker_text": None,
        }

        self.assertFalse(_command_payload_valid(command))
        command["window"] = "tsui.surface.dialogue"
        self.assertTrue(_command_payload_valid(command))
        command["window"] = "tsui.surface.synthetic"
        self.assertFalse(_command_payload_valid(command))

    def test_speaker_identity_and_localization_are_emitted_together(self):
        command = {
            "command_id": "line.001",
            "kind": "text",
            "text": "dialogue",
            "speaker_id": "tsui.speaker.0123456789abcdef",
            "speaker_text": "speaker",
            "window": "tsui.surface.dialogue",
        }
        strings = {}

        rendered = _render_command(command, strings)

        self.assertEqual(strings["story.line.001"], "dialogue")
        self.assertEqual(
            strings["speaker.tsui.speaker.0123456789abcdef"], "speaker"
        )
        self.assertIn("speaker:tsui.speaker.0123456789abcdef", rendered[0])
        self.assertIn("window:tsui.surface.dialogue", rendered[0])

    def test_speaker_id_without_private_localization_value_is_rejected(self):
        command = {
            "kind": "text",
            "text": "dialogue",
            "speaker_id": "tsui.speaker.0123456789abcdef",
            "speaker_text": None,
            "window": "tsui.surface.dialogue",
        }

        self.assertFalse(_command_payload_valid(command))

    def test_blocking_score_timeline_has_a_typed_fence_and_fixed_duration(self):
        command = {
            "command_id": "timeline.opening.001",
            "kind": "timeline",
            "timeline_id": "tsui.opening.y.0010",
            "target": "tsui.layer.event",
            "property": "opacity",
            "value": 100,
            "duration_ms": 9_000,
            "fence": "tsui.opening.y.0010.complete",
        }

        self.assertTrue(_command_payload_valid(command))
        rendered = _render_command(command, {})[0]
        self.assertIn("keyframes:0=1,9000=1", rendered)
        self.assertIn("join:block", rendered)
        self.assertIn("fence:tsui.opening.y.0010.complete", rendered)


if __name__ == "__main__":
    unittest.main()
