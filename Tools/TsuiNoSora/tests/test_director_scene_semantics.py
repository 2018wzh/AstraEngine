import unittest

from director_scene_semantics import DirectorSceneSemanticError, build_scene_semantic_ir


def fixture(operations):
    return {
        "schema": "tsuinosora.director_scene_dsl_ir.v1",
        "scenes": [
            {
                "movie_id": "K",
                "frame": 1,
                "source_resource_id": 7,
                "source_sha256": "sha256:" + "a" * 64,
                "operations": operations,
            }
        ],
    }


class DirectorSceneSemanticTests(unittest.TestCase):
    def test_lowers_scene_controls_without_dropping_original_operations(self):
        detailed, report = build_scene_semantic_ir(
            fixture(
                [
                    {"kind": "preload", "values": ["bg"], "termination": "close"},
                    {"kind": "audio", "values": ["L+", "music", "S", "effect"], "termination": "close"},
                    {"kind": "char", "values": ["+-", "person"], "termination": "close"},
                    {
                        "kind": "talk",
                        "termination": "close",
                        "events": [{"kind": "text", "text": "private"}, {"kind": "waitse"}],
                    },
                ]
            )
        )

        self.assertEqual(report["status"], "pass")
        self.assertEqual(report["source_operation_count"], 6)
        self.assertEqual(detailed["scenes"][0]["operations"][0]["kind"], "preload_member")
        self.assertEqual(detailed["scenes"][0]["operations"][1]["bus"], "bgm")
        self.assertEqual(detailed["scenes"][0]["operations"][2]["bus"], "se")
        character = next(
            operation
            for operation in detailed["scenes"][0]["operations"]
            if operation.get("kind") == "show_member" and operation.get("layer") == "character"
        )
        self.assertEqual(character["opacity"], 50)

    def test_unknown_audio_token_is_blocking(self):
        with self.assertRaises(DirectorSceneSemanticError):
            build_scene_semantic_ir(
                fixture([{"kind": "audio", "values": ["UNKNOWN"], "termination": "close"}])
            )

    def test_missing_paired_member_is_blocking(self):
        with self.assertRaises(DirectorSceneSemanticError):
            build_scene_semantic_ir(
                fixture([{"kind": "char", "values": ["+"], "termination": "close"}])
            )

    def test_event_controls_preserve_the_exact_transition_helper(self):
        detailed, _ = build_scene_semantic_ir(
            fixture(
                [
                    {"kind": "event", "values": ["+", "event_a", "*", "event_b", "*-", "event_c", "-"], "termination": "close"}
                ]
            )
        )

        self.assertEqual(
            [operation.get("transition_helper") for operation in detailed["scenes"][0]["operations"]],
            ["ttrans3", "ttrans1", "ttrans1", "ttrans4"],
        )

    def test_scene_controllers_emit_their_authored_pre_mutation_transition(self):
        detailed, _ = build_scene_semantic_ir(
            fixture(
                [
                    {"kind": "back", "values": ["background"], "termination": "close"},
                    {"kind": "char", "values": ["+", "character"], "termination": "close"},
                    {"kind": "shade", "values": ["+-"], "termination": "close"},
                ]
            )
        )

        operations = detailed["scenes"][0]["operations"]
        self.assertEqual([operation["kind"] for operation in operations], ["transition", "show_member", "transition", "show_member", "transition", "set_shade"])
        self.assertEqual(
            [operation["transition_helper"] for operation in operations if operation["kind"] == "transition"],
            ["ttrans1", "ttrans1", "ttrans1"],
        )


if __name__ == "__main__":
    unittest.main()
