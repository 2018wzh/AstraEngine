import json
import tempfile
import unittest
from pathlib import Path

from headless_route_matrix import (
    RouteMatrixError,
    _json_hash,
    _load_resumed_routes,
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
            self.assertEqual(contract.message_count, 4)

            resume = {
                "schema": "tsuinosora.headless_route_matrix_report.v1",
                "build_fingerprint": "sha256:" + "1" * 64,
                "package_hash": "sha256:" + "2" * 64,
                "routes": [
                    {
                        "route_id": contract.route_id,
                        "terminal_id": contract.terminal_id,
                        "terminal_route_node_id": contract.terminal_route_node_id,
                        "choice_count": 1,
                        "choice_selection_count": 1,
                        "choice_signature_hash": _json_hash(["choice.safe.1"]),
                        "session_id": "tsui.route.coverage.001",
                        "build_fingerprint": "sha256:" + "1" * 64,
                        "package_hash": "sha256:" + "2" * 64,
                        "input_sequence_hash": contract.input_sequence_hash,
                        "completed_sequence": 4,
                        "status": "passed",
                    }
                ],
            }
            resume_path = Path(temp) / "resume.json"
            resume_path.write_text(json.dumps(resume), encoding="utf-8")
            self.assertEqual(
                len(
                    _load_resumed_routes(
                        resume_path,
                        [contract],
                        build_fingerprint="sha256:" + "1" * 64,
                        package_hash="sha256:" + "2" * 64,
                    )
                ),
                1,
            )

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
