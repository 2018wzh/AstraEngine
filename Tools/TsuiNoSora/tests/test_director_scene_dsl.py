import unittest

from director_scene_dsl import DirectorSceneDslError, build_scene_dsl_ir


def _story(text: str) -> dict:
    return {
        "schema": "tsuinosora.director_story_source.v1",
        "movies": [
            {
                "movie_id": "TEST",
                "labels": [
                    {
                        "frame": 1,
                        "label": "scene.one",
                        "label_sha256": "sha256:" + "1" * 64,
                        "scene_text": {
                            "resource_id": 1,
                            "source_sha256": "sha256:" + "2" * 64,
                            "text": text,
                        },
                    }
                ],
                "coverage": {"scene_text_binding_count": 1},
            }
        ],
    }


class DirectorSceneDslTests(unittest.TestCase):
    def test_parses_scene_reading_and_original_end_of_scene_termination(self):
        detailed, report = build_scene_dsl_ir(
            _story(
                "\n".join(
                    [
                        "{",
                        "<back>",
                        "background.one",
                        "</back>",
                        "}",
                        "<talk>",
                        "line one",
                        "<audio>",
                        "S",
                        "sound.one",
                        "</audio>",
                        "line two",
                        "</talk>",
                        "<char>",
                        "-",
                        "<char>",
                    ]
                )
            )
        )
        self.assertEqual(report["status"], "pass")
        self.assertEqual(report["source_scene_count"], 1)
        self.assertEqual(report["termination_counts"]["end_of_scene"], 1)
        operations = detailed["scenes"][0]["operations"]
        self.assertEqual(operations[0]["kind"], "scene_transaction")
        self.assertEqual(operations[1]["kind"], "talk")
        self.assertEqual(operations[2]["termination"], "end_of_scene")

    def test_rejects_unknown_control(self):
        with self.assertRaisesRegex(DirectorSceneDslError, "unsupported self-closing tag"):
            build_scene_dsl_ir(_story("<unknown/>"))


if __name__ == "__main__":
    unittest.main()
