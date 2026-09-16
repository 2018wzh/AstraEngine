import argparse
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from classic_visual_acceptance import InputBudgetError, Sequence, run_sequence, validate_input_budget


class InputBudgetTests(unittest.TestCase):
    def test_exact_limits_accept_the_last_envelope_tick(self):
        rows = [{"tick": 0}, {"tick": 20}]
        profile = {"input": {"max_messages": 2, "max_tick": 20}}
        original = json.dumps(profile)
        validate_input_budget(profile, rows)
        self.assertEqual(json.dumps(profile), original)

    def test_invalid_or_insufficient_limits_are_not_expanded(self):
        rows = [{"tick": 0}, {"tick": 20}]
        for field in ["max_messages", "max_tick"]:
            for value in [None, True, "100", -1, 0, 1]:
                with self.subTest(field=field, value=value):
                    profile = {"input": {"max_messages": 2, "max_tick": 20}}
                    profile["input"][field] = value
                    original = json.dumps(profile)
                    with self.assertRaises(InputBudgetError):
                        validate_input_budget(profile, rows)
                    self.assertEqual(json.dumps(profile), original)

    def test_rejection_precedes_gpu_process_and_artifact_creation(self):
        sequence = Sequence("fixture")
        sequence.add({"type": "advance_ticks", "ticks": 1}, tick_advance=20)
        sequence.add({"type": "close"})
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            profile = root / "profile.json"
            contents = json.dumps({"input": {"max_messages": 2, "max_tick": 19}})
            profile.write_text(contents, encoding="utf-8")
            output = root / "run"
            with patch("classic_visual_acceptance.subprocess.run") as process:
                with self.assertRaisesRegex(
                    InputBudgetError, "max_tick is insufficient: required=20, configured=19"
                ):
                    run_sequence(argparse.Namespace(profile=profile), output, sequence)
                process.assert_not_called()
            self.assertFalse(output.exists())
            self.assertEqual(profile.read_text(encoding="utf-8"), contents)


if __name__ == "__main__":
    unittest.main()
