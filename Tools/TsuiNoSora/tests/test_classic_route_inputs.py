import copy
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from classic_route_inputs import generate_inputs, matrix_rows
from classic_visual_acceptance import Sequence


class ClassicMatrixInputTests(unittest.TestCase):
    def sequence(self, route_id):
        sequence = Sequence(f"tsui.classic.{route_id}.complete")
        sequence.start()
        sequence.key("Enter")
        sequence.await_value("vn.terminal_routes", ["state.ending"], 18000)
        sequence.checkpoint(f"classic.{route_id}.complete")
        sequence.finish()
        return sequence

    def route(self, route_id):
        return dict(route_id=route_id, terminal_id="ending",
                    terminal_route_node_id="state.ending", choice_ids=[], choice_sequence=[])

    def test_matrix_changes_only_session_and_checkpoint_names(self):
        sequence = self.sequence("route.coverage.001")
        original = copy.deepcopy(sequence.rows)
        rows = matrix_rows(sequence, "route.coverage.001")
        self.assertEqual(sequence.rows, original)
        for before, after in zip(original, rows):
            expected = dict(before, session="tsui.route.coverage.001")
            if before["event"]["type"] == "checkpoint":
                expected["event"] = dict(type="checkpoint", id="checkpoint.route.coverage.001")
            self.assertEqual(after, expected)
        sequence.checkpoints = ["classic.route.y.complete"]
        with self.assertRaises(ValueError):
            matrix_rows(sequence, "route.coverage.001")

    def test_generation_validates_all_routes_and_cleans_failed_staging(self):
        routes = [self.route(f"route.coverage.{index:03}") for index in (1, 2)]
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "inputs"
            with patch("classic_route_inputs.build_sequence", side_effect=[
                (self.sequence(routes[0]["route_id"]), {}), ValueError("broken witness")
            ]):
                with self.assertRaises(ValueError):
                    generate_inputs({"routes": routes}, output)
            self.assertEqual(list(Path(temporary).iterdir()), [])
            with patch("classic_route_inputs.build_sequence", side_effect=lambda story, route_id, **kw:
                       (self.sequence(route_id), {})):
                result = generate_inputs({"routes": routes}, output)
            self.assertEqual(result["route_count"], 2)
            self.assertEqual(len(list(output.glob("*.jsonl"))), 2)
            with self.assertRaises(ValueError):
                generate_inputs({"routes": routes}, output)

    def test_invalid_route_names_never_create_output(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "inputs"
            for route_id in ["../escape", "route.coverage.001/../../escape", "route.coverage."]:
                with self.assertRaises(ValueError):
                    generate_inputs({"routes": [self.route(route_id)]}, output)
                self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
