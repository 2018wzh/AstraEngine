import unittest

from director_story_graph import build_story_graph


class DirectorStoryGraphTests(unittest.TestCase):
    def test_builds_exact_terminal_graph_for_all_story_movies(self):
        source_hash = "sha256:" + "a" * 64
        movies = []
        for movie_id in ("K", "S", "T", "Y", "Z"):
            movies.append(
                {
                    "movie_id": movie_id,
                    "score": {"decoded_frame_count": 1},
                    "labels": [
                        {
                            "frame": 1,
                            "frame_status": "in_score",
                            "label": "end",
                            "label_sha256": "sha256:" + "b" * 64,
                            "scene_text": None,
                        }
                    ],
                    "text_members": [],
                    "frame_actions": [
                        {
                            "frame": 1,
                            "action": {"script_source_sha256": source_hash},
                        }
                    ],
                }
            )
        story = {"schema": "tsuinosora.director_story_source.v1", "movies": movies}
        scenes = {"schema": "tsuinosora.director_scene_dsl_ir.v1", "scenes": []}
        lingo = {
            "schema": "tsuinosora.director_lingo_ir.v1",
            "scripts": [
                {
                    "script_source_sha256": source_hash,
                    "handlers": [
                        {
                            "name": "exitFrame",
                            "statements": [
                                {
                                    "kind": "go",
                                    "expression": [
                                        {"kind": "punctuation", "value": "("},
                                        {"kind": "number", "value": "1"},
                                        {"kind": "punctuation", "value": ","},
                                        {"kind": "identifier", "value": "tgetmovietogo"},
                                        {"kind": "punctuation", "value": "("},
                                        {"kind": "punctuation", "value": ")"},
                                        {"kind": "punctuation", "value": ")"},
                                    ],
                                }
                            ],
                        }
                    ],
                }
            ],
        }
        detailed, report = build_story_graph(story, scenes, lingo)
        self.assertEqual(report["status"], "pass")
        self.assertEqual(report["node_count"], 5)
        self.assertEqual(report["terminal_count"], 5)
        self.assertEqual(detailed["movies"][0]["nodes"][0]["flow"]["kind"], "terminal_external_dispatch")


if __name__ == "__main__":
    unittest.main()
