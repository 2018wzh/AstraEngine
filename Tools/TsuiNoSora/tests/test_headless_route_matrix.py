import json
import tempfile
import unittest
from pathlib import Path

from headless_route_matrix import (
    RouteMatrixError,
    _json_hash,
    _validate_route_input,
)


class HeadlessRouteMatrixTests(unittest.TestCase):
    def test_route_input_requires_same_session_terminal_evidence_and_shutdown(self):
        route = {
            "route_id": "route.coverage.001",
            "terminal_id": "tsui.ending",
            "terminal_route_node_id": "state.tsui.ending",
            "choice_ids": ["choice.safe.1"],
            "choice_sequence": ["choice.safe.1"],
        }
        events = [
            {"type": "resume"},
            {
                "type": "await",
                "observation": {
                    "kind": "equals",
                    "key": "vn.route_terminal",
                    "value_hash": _json_hash(True),
                },
                "timeout_ticks": 3600,
                "continue_at_match": True,
            },
            {
                "type": "await",
                "observation": {
                    "kind": "equals",
                    "key": "vn.terminal_routes",
                    "value_hash": _json_hash(["state.tsui.ending"]),
                },
                "timeout_ticks": 3600,
                "continue_at_match": True,
            },
            {"type": "checkpoint", "id": "checkpoint.route.coverage.001"},
            {"type": "shutdown"},
        ]
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "route.coverage.001.jsonl"
            rows = [
                {
                    "schema": "astra.user_input_sequence.v1",
                    "session": "tsui.route.coverage.001",
                    "sequence": index,
                    "tick": index - 1,
                    "event": event,
                }
                for index, event in enumerate(events, start=1)
            ]
            path.write_text("\n".join(json.dumps(row) for row in rows) + "\n", encoding="utf-8")
            contract = _validate_route_input(path, route)
            self.assertEqual(contract.message_count, 5)

    def test_route_input_blocks_false_terminal_flag(self):
        route = {
            "route_id": "route.coverage.001",
            "terminal_id": "tsui.ending",
            "terminal_route_node_id": "state.tsui.ending",
            "choice_ids": [],
            "choice_sequence": [],
        }
        events = [
            {"type": "resume"},
            {
                "type": "await",
                "observation": {
                    "kind": "equals",
                    "key": "vn.route_terminal",
                    "value_hash": _json_hash(False),
                },
                "timeout_ticks": 3600,
                "continue_at_match": True,
            },
            {
                "type": "await",
                "observation": {
                    "kind": "equals",
                    "key": "vn.terminal_routes",
                    "value_hash": _json_hash(["state.tsui.ending"]),
                },
                "timeout_ticks": 3600,
                "continue_at_match": True,
            },
            {"type": "checkpoint", "id": "checkpoint.route.coverage.001"},
            {"type": "shutdown"},
        ]
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "route.coverage.001.jsonl"
            rows = [
                {
                    "schema": "astra.user_input_sequence.v1",
                    "session": "tsui.route.coverage.001",
                    "sequence": index,
                    "tick": index - 1,
                    "event": event,
                }
                for index, event in enumerate(events, start=1)
            ]
            path.write_text("\n".join(json.dumps(row) for row in rows) + "\n", encoding="utf-8")
            with self.assertRaisesRegex(RouteMatrixError, "completed terminal route state"):
                _validate_route_input(path, route)

    def test_route_input_blocks_missing_terminal_observation(self):
        route = {
            "route_id": "route.coverage.001",
            "terminal_id": "tsui.ending",
            "terminal_route_node_id": "state.tsui.ending",
            "choice_ids": [],
            "choice_sequence": [],
        }
        events = [
            {"type": "resume"},
            {"type": "checkpoint", "id": "checkpoint.route.coverage.001"},
            {"type": "shutdown"},
        ]
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "route.coverage.001.jsonl"
            rows = [
                {
                    "schema": "astra.user_input_sequence.v1",
                    "session": "tsui.route.coverage.001",
                    "sequence": index,
                    "tick": index - 1,
                    "event": event,
                }
                for index, event in enumerate(events, start=1)
            ]
            path.write_text("\n".join(json.dumps(row) for row in rows) + "\n", encoding="utf-8")
            with self.assertRaisesRegex(RouteMatrixError, "terminal route observation"):
                _validate_route_input(path, route)


if __name__ == "__main__":
    unittest.main()
